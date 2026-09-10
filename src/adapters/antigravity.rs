use super::Adapter;
use super::render::{emit_crew_files, emit_shared_skills, emit_shared_tool_skills, ANTIGRAVITY, CrewFormat};
use crate::catalog::{CanonicalCommand, CanonicalRole, CanonicalTool};
use std::collections::HashMap;

/// The Antigravity CLI (`agy`) — Google's successor to the retired Gemini CLI.
///
/// `agy` reads workspace customizations from `.agents/`: subagent definitions
/// as `.agents/agents/<name>.md` (YAML frontmatter with `name`, `description`,
/// `tools`, `subagent: true`, `mainAgent: false`), and skills as
/// `.agents/skills/<name>/SKILL.md`. The CLI reads project instructions from
/// `AGENTS.md` (falling back to `GEMINI.md`), scopes conversations to the
/// workspace, and authenticates through the Antigravity account rather than a
/// per-process key. See https://antigravity.google/docs/cli/plugins.
pub struct AntigravityAdapter;

fn scope_tool(scope: &str) -> Option<&'static str> {
    match scope {
        "read" => Some("view_file"),
        "write" => Some("write_to_file"),
        "edit" => Some("replace_file_content"),
        "bash" => Some("run_command"),
        "search" => Some("grep_search"),
        "glob" => Some("list_dir"),
        "web-search" => Some("search_web"),
        "web-fetch" => Some("read_url_content"),
        "agent" => Some("invoke_subagent"),
        _ => None,
    }
}

fn tools_for(role: &CanonicalRole) -> anyhow::Result<Vec<String>> {
    // agy tool names are snake_case and distinct from every other harness's;
    // a misspelled or unmapped name in `tools` can hang the subagent, so map
    // only names the CLI's own docs use (view_file, grep_search, list_dir,
    // write_to_file, replace_file_content, run_command, read_url_content,
    // search_web, ask_question, invoke_subagent).
    if !role.tool_order.is_empty() {
        let mut ordered = Vec::new();
        for scope in &role.tool_order {
            let tool = scope_tool(scope)
                .ok_or_else(|| anyhow::anyhow!("unknown tool scope {scope:?}"))?
                .to_string();
            if !ordered.contains(&tool) {
                ordered.push(tool);
            }
        }
        return Ok(ordered);
    }
    let mut tools = vec![];
    for cap in &role.capabilities {
        match cap.as_str() {
            "read" => {
                let scopes = if role.read_scopes.is_empty() {
                    vec!["read", "search", "glob"]
                } else {
                    role.read_scopes.iter().map(String::as_str).collect()
                };
                for scope in scopes {
                    tools.push(
                        scope_tool(scope)
                            .ok_or_else(|| anyhow::anyhow!("unknown read scope {scope:?}"))?
                            .to_string(),
                    );
                }
            }
            "edit" => {
                tools.extend(["write_to_file", "replace_file_content"].map(str::to_string));
            }
            "bash" => tools.push("run_command".to_string()),
            "web" => {
                let scopes: Vec<String> = if role.web_scopes.is_empty() {
                    vec!["web-search".to_string(), "web-fetch".to_string()]
                } else {
                    role.web_scopes
                        .iter()
                        .map(|scope| format!("web-{scope}"))
                        .collect()
                };
                for scope in scopes {
                    tools.push(
                        scope_tool(&scope)
                            .ok_or_else(|| anyhow::anyhow!("unknown web scope {scope:?}"))?
                            .to_string(),
                    );
                }
            }
            "agent" => tools.push("invoke_subagent".to_string()),
            other => anyhow::bail!("unmapped capability {other:?} for antigravity"),
        }
    }
    tools.sort_unstable();
    tools.dedup();
    Ok(tools)
}

fn serialize(role: &CanonicalRole, body: &str, tools: &[String]) -> anyhow::Result<String> {
    let mut content = String::new();
    content.push_str("---\n");
    content.push_str(&format!("name: {}\n", role.name));
    content.push_str(&format!("description: {}\n", role.description));
    if !tools.is_empty() {
        content.push_str("tools:\n");
        for tool in tools {
            content.push_str(&format!("  - {tool}\n"));
        }
    }
    content.push_str("subagent: true\nmainAgent: false\ncommandExecutionPolicy: sandbox\n---\n");
    content.push_str(body);
    Ok(content)
}

const CREW_FORMAT: CrewFormat = CrewFormat {
    file_suffix: ".md",
    dialect: &ANTIGRAVITY,
    map_tools: tools_for,
    serialize,
};

impl Adapter for AntigravityAdapter {
    fn base_dir(&self) -> &'static str {
        "harnesses/antigravity/.agents"
    }

    fn digest_root(&self) -> &'static str {
        self.container()
    }

    fn steering_dialect(&self) -> Option<&'static super::render::Dialect> {
        Some(&ANTIGRAVITY)
    }

    fn steering_target(&self) -> Option<super::render::SteeringTarget> {
        Some(super::render::SteeringTarget {
            rel_path: super::render::SHIPMATES_STEERING_REL,
            format: super::render::SteeringFormat::PlainMarkdown,
        })
    }

    fn build(
        &self,
        roles: &[CanonicalRole],
        commands: &[CanonicalCommand],
    ) -> anyhow::Result<HashMap<String, String>> {
        let mut files = emit_crew_files(self.base_dir(), roles, &CREW_FORMAT)?;
        // `.agents/skills/` is the open Agent Skills tree agy reads; the skills
        // come from the shared emitter (byte-identical with codex/cursor/
        // copilot). Only the crew above are agy-specific.
        files.extend(emit_shared_skills(self.container(), commands)?);
        Ok(files)
    }

    fn build_tools(&self, tools: &[CanonicalTool]) -> HashMap<String, String> {
        // agy skills are model-invoked ("it decides based on context"); they
        // also surface as slash commands, so agent-invoked but typeable
        // (recorded, not faked). They land in the shared `.agents/skills/` tree.
        emit_shared_tool_skills(self.container(), tools)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_antigravity_adapter_build() {
        let role = CanonicalRole {
            name: "test-role".to_string(),
            description: "A test role".to_string(),
            capabilities: vec!["read".to_string(), "bash".to_string()],
            writes: false,
            web_scopes: vec![],
            read_scopes: vec![],
            tool_order: vec![],
            effort: None,
            source: std::path::PathBuf::from(""),
            body: "test body {{project-instructions}} {{project-instructions-fallback}}".to_string(),
        };
        let adapter = AntigravityAdapter;
        let files = adapter.build(&[role], &[]).unwrap();
        let content = files
            .get("harnesses/antigravity/.agents/agents/test-role.md")
            .unwrap();
        assert!(content.contains("name: test-role"));
        assert!(content.contains("description: A test role"));
        assert!(
            content
                .contains("tools:\n  - grep_search\n  - list_dir\n  - run_command\n  - view_file")
        );
        assert!(content.contains("subagent: true"));
        assert!(content.contains("mainAgent: false"));
        assert!(content.contains("commandExecutionPolicy: sandbox"));
        assert!(content.ends_with("test body AGENTS.md GEMINI.md"));
        assert!(content.contains("AGENTS.md GEMINI.md"), "{content}");
        assert!(!content.contains("CLAUDE.md"), "{content}");
    }

    #[test]
    fn test_antigravity_preserves_tool_order_while_deduping() {
        let role = CanonicalRole {
            name: "ordered-role".to_string(),
            description: "A test role".to_string(),
            capabilities: vec!["read".to_string(), "edit".to_string(), "bash".to_string()],
            writes: false,
            web_scopes: vec![],
            read_scopes: vec![],
            tool_order: vec![
                "bash".to_string(),
                "read".to_string(),
                "bash".to_string(),
                "edit".to_string(),
                "read".to_string(),
            ],
            effort: None,
            source: std::path::PathBuf::from(""),
            body: "body".to_string(),
        };
        let files = AntigravityAdapter.build(&[role], &[]).unwrap();
        let content = files
            .get("harnesses/antigravity/.agents/agents/ordered-role.md")
            .unwrap();
        let tools = content
            .lines()
            .skip_while(|line| *line != "tools:")
            .skip(1)
            .take(3)
            .collect::<Vec<_>>();
        assert_eq!(tools, vec!["  - run_command", "  - view_file", "  - replace_file_content"]);
    }

    #[test]
    fn test_antigravity_skill_emits_standard_pair() {
        let command = CanonicalCommand {
            name: "ship-issue".to_string(),
            description: "desc".to_string(),
            argument_hint: "".to_string(),
            allowed_tools: "".to_string(),
            disable_model_invocation: true,
            arguments: vec![],
            narrative: "Use `{{agents-glob}}` and {{session-key}}.".to_string(),
            invocation: "".to_string(),
            board: "".to_string(),
            source: std::path::PathBuf::from(""),
        };
        let files = AntigravityAdapter.build(&[], &[command]).unwrap();
        let content = files
            .get("harnesses/antigravity/.agents/skills/ship-issue/SKILL.md")
            .unwrap();
        assert!(content.starts_with("---\nname: ship-issue\ndescription: desc\n---\n"));
        // Shared neutral dialect: `.agents/agents` glob (matches agy's real crew
        // dir) and the neutral `Agent-Session` trailer.
        assert!(content.contains(".agents/agents/*.md"));
        assert!(content.contains("Agent-Session"));
        assert!(!content.contains("disable-model-invocation"));
    }
}
