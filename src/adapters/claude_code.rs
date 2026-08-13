use super::Adapter;
use super::render::{
    CLAUDE_CODE, CrewFormat, emit_crew_files, emit_tool_files, render_command_body,
};
use crate::catalog::{CanonicalCommand, CanonicalRole, CanonicalTool};
use std::collections::HashMap;

fn scope_tool(scope: &str) -> Option<&'static str> {
    match scope {
        "read" => Some("Read"),
        "write" => Some("Write"),
        "edit" => Some("Edit"),
        "bash" => Some("Bash"),
        "search" => Some("Grep"),
        "glob" => Some("Glob"),
        "web-search" => Some("WebSearch"),
        "web-fetch" => Some("WebFetch"),
        "agent" => Some("Agent"),
        _ => None,
    }
}

fn map_tools(role: &CanonicalRole) -> anyhow::Result<Vec<String>> {
    if !role.tool_order.is_empty() {
        return role
            .tool_order
            .iter()
            .map(|scope| {
                scope_tool(scope)
                    .map(str::to_string)
                    .ok_or_else(|| anyhow::anyhow!("unknown tool scope {scope:?}"))
            })
            .collect();
    }
    let mut out = Vec::new();
    for cap in &role.capabilities {
        match cap.as_str() {
            "read" => {
                let scopes = if role.read_scopes.is_empty() {
                    vec!["read", "search", "glob"]
                } else {
                    role.read_scopes.iter().map(String::as_str).collect()
                };
                for scope in scopes {
                    let tool = scope_tool(scope)
                        .ok_or_else(|| anyhow::anyhow!("unknown read scope {scope:?}"))?;
                    out.push(tool.to_string());
                }
            }
            "edit" => out.extend(["Write", "Edit"].map(str::to_string)),
            "bash" => out.push("Bash".to_string()),
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
                    let tool = scope_tool(&scope)
                        .ok_or_else(|| anyhow::anyhow!("unknown web scope {scope:?}"))?;
                    out.push(tool.to_string());
                }
            }
            "agent" => out.push("Agent".to_string()),
            other => anyhow::bail!("unmapped capability {other:?} for claude-code"),
        }
    }
    Ok(out)
}

fn serialize(role: &CanonicalRole, body: &str, tools: &[String]) -> anyhow::Result<String> {
    let mut content = String::new();
    content.push_str("---\n");
    content.push_str(&format!("name: {}\n", role.name));
    content.push_str(&format!("description: {}\n", role.description));
    if !tools.is_empty() {
        content.push_str(&format!("tools: {}\n", tools.join(", ")));
    }
    if let Some(e) = &role.effort {
        content.push_str(&format!("effort: {}\n", e));
    }
    content.push_str("---\n");
    content.push_str(body);
    Ok(content)
}

const CREW_FORMAT: CrewFormat = CrewFormat {
    file_suffix: ".md",
    dialect: &CLAUDE_CODE,
    map_tools,
    serialize,
};

pub struct ClaudeCodeAdapter;

impl Adapter for ClaudeCodeAdapter {
    fn base_dir(&self) -> &'static str {
        "harnesses/claude-code/.claude"
    }

    fn build(
        &self,
        roles: &[CanonicalRole],
        commands: &[CanonicalCommand],
    ) -> anyhow::Result<HashMap<String, String>> {
        let mut files = emit_crew_files(self.base_dir(), roles, &CREW_FORMAT)?;
        for command in commands {
            let mut content = String::new();
            content.push_str("---\n");
            content.push_str(&format!("name: {}\n", command.name));
            content.push_str(&format!("description: {}\n", command.description));
            if !command.argument_hint.is_empty() {
                content.push_str(&format!("argument-hint: {}\n", command.argument_hint));
            }
            if !command.allowed_tools.is_empty() {
                content.push_str(&format!("allowed-tools: {}\n", command.allowed_tools));
            }
            if command.disable_model_invocation {
                content.push_str("disable-model-invocation: true\n");
            }
            content.push_str("---\n");
            content.push_str(&render_command_body(command, &CLAUDE_CODE)?);
            files.insert(
                format!("{}/skills/{}/SKILL.md", self.base_dir(), command.name),
                content,
            );
        }
        Ok(files)
    }

    fn build_tools(&self, tools: &[CanonicalTool]) -> HashMap<String, String> {
        // Claude Code can pin a tool agent-only with `user-invocable: false`:
        // model-invoked, hidden from the `/` menu — exactly "never a command".
        emit_tool_files(self.base_dir(), tools, &CLAUDE_CODE, true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn role(name: &str, capabilities: &[&str]) -> CanonicalRole {
        CanonicalRole {
            name: name.to_string(),
            description: "desc".to_string(),
            capabilities: capabilities.iter().map(|s| s.to_string()).collect(),
            writes: false,
            web_scopes: vec![],
            read_scopes: vec![],
            tool_order: vec![],
            effort: None,
            source: std::path::PathBuf::from(""),
            body: "body".to_string(),
        }
    }

    #[test]
    fn test_tool_names_are_mapped() {
        let files = ClaudeCodeAdapter
            .build(&[role("architect", &["read", "bash"])], &[])
            .unwrap();
        let content = files
            .get("harnesses/claude-code/.claude/agents/architect.md")
            .unwrap();
        assert!(content.contains("tools: Read, Grep, Glob, Bash\n"));
        assert!(!content.contains("read,"));
    }

    #[test]
    fn test_scopes_and_tool_order_are_preserved() {
        let mut scoped = role("art-director", &["read", "web"]);
        scoped.read_scopes = vec!["read".to_string()];
        scoped.web_scopes = vec!["search".to_string()];
        let files = ClaudeCodeAdapter.build(&[scoped], &[]).unwrap();
        let content = files
            .get("harnesses/claude-code/.claude/agents/art-director.md")
            .unwrap();
        assert!(content.contains("tools: Read, WebSearch\n"), "{content}");
        assert!(!content.contains("Glob"));
        assert!(!content.contains("WebFetch"));

        let mut ordered = role("senior-engineer", &["read", "edit", "bash"]);
        ordered.tool_order = vec!["bash".to_string(), "read".to_string(), "edit".to_string()];
        let files = ClaudeCodeAdapter.build(&[ordered], &[]).unwrap();
        let content = files
            .get("harnesses/claude-code/.claude/agents/senior-engineer.md")
            .unwrap();
        assert!(content.contains("tools: Bash, Read, Edit\n"), "{content}");
    }

    #[test]
    fn test_multiple_command_arguments_fail_closed() {
        let command = CanonicalCommand {
            name: "migrate".to_string(),
            description: "desc".to_string(),
            argument_hint: "<from> <to>".to_string(),
            allowed_tools: String::new(),
            disable_model_invocation: true,
            arguments: vec!["from".to_string(), "to".to_string()],
            narrative: "Migrate {{from}} to {{to}}.".to_string(),
            invocation: String::new(),
            board: String::new(),
            source: std::path::PathBuf::from(""),
        };
        assert!(ClaudeCodeAdapter.build(&[], &[command]).is_err());
    }

    #[test]
    fn test_effort_is_emitted_as_the_effort_key() {
        let mut r = role("architect", &["read"]);
        r.effort = Some("high".to_string());
        let files = ClaudeCodeAdapter.build(&[r], &[]).unwrap();
        let content = files
            .get("harnesses/claude-code/.claude/agents/architect.md")
            .unwrap();
        assert!(content.contains("effort: high\n"), "{content}");
    }

    #[test]
    fn test_no_model_line_is_emitted() {
        // A model is never stamped — it is a runtime decision (#205). Prefix
        // check so `effort:` (which is present) cannot false-positive.
        let mut r = role("architect", &["read"]);
        r.effort = Some("high".to_string());
        let files = ClaudeCodeAdapter.build(&[r], &[]).unwrap();
        let content = files
            .get("harnesses/claude-code/.claude/agents/architect.md")
            .unwrap();
        assert!(
            !content
                .lines()
                .any(|l| l.trim_start().starts_with("model:")),
            "{content}"
        );
    }

    #[test]
    fn test_command_body_is_rendered() {
        let command = CanonicalCommand {
            name: "migrate".to_string(),
            description: "desc".to_string(),
            argument_hint: "<arg>".to_string(),
            allowed_tools: "Bash".to_string(),
            disable_model_invocation: true,
            arguments: vec![],
            narrative: "Resolve via `{{agents-glob}}`; use {{arg}}.".to_string(),
            invocation: "".to_string(),
            board: "".to_string(),
            source: std::path::PathBuf::from(""),
        };
        let files = ClaudeCodeAdapter.build(&[], &[command]).unwrap();
        let content = files
            .get("harnesses/claude-code/.claude/skills/migrate/SKILL.md")
            .unwrap();
        assert!(content.contains("argument-hint: <arg>\n"));
        assert!(content.contains("disable-model-invocation: true\n"));
        assert!(content.contains(".claude/agents/*.md"));
        assert!(content.contains("$ARGUMENTS"));
        assert!(!content.contains("agent-files/"));
        assert!(!content.contains("{{arg}}"));
    }

    #[test]
    fn test_tool_is_agent_only_skill_with_bundled_assets() {
        let tool = CanonicalTool {
            name: "termgif".to_string(),
            description: "render a gif".to_string(),
            body: "instructions".to_string(),
            assets: vec![("termgif.py".to_string(), "print('hi')".to_string())],
            requires: vec![],
            source: std::path::PathBuf::from(""),
        };
        let files = ClaudeCodeAdapter.build_tools(&[tool]);
        let skill = files
            .get("harnesses/claude-code/.claude/skills/termgif/SKILL.md")
            .unwrap();
        assert!(skill.contains("name: termgif\n"));
        // A tool is agent-invoked only — never a slash command.
        assert!(skill.contains("user-invocable: false\n"));
        assert!(!skill.contains("disable-model-invocation"));
        let asset = files
            .get("harnesses/claude-code/.claude/skills/termgif/termgif.py")
            .unwrap();
        assert_eq!(asset, "print('hi')");
    }
}
