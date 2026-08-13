use crate::catalog::{CanonicalCommand, CanonicalRole, CanonicalTool};
use std::collections::HashMap;
use super::render::{emit_shared_skills, emit_shared_tool_skills};
use super::Adapter;

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

fn tools_for(capabilities: &[String]) -> Vec<&'static str> {
    // agy tool names are snake_case and distinct from every other harness's;
    // a misspelled or unmapped name in `tools` can hang the subagent, so map
    // only names the CLI's own docs use (view_file, grep_search, list_dir,
    // write_to_file, replace_file_content, run_command, read_url_content,
    // search_web, ask_question, invoke_subagent).
    let mut tools: Vec<&'static str> = vec![];
    for cap in capabilities {
        match cap.as_str() {
            "read" => {
                tools.push("view_file");
                tools.push("grep_search");
                tools.push("list_dir");
            }
            "edit" => {
                tools.push("write_to_file");
                tools.push("replace_file_content");
            }
            "bash" => tools.push("run_command"),
            "web" => {
                tools.push("read_url_content");
                tools.push("search_web");
            }
            "agent" => tools.push("invoke_subagent"),
            _ => {}
        }
    }
    tools.sort_unstable();
    tools.dedup();
    tools
}

impl Adapter for AntigravityAdapter {
    fn base_dir(&self) -> &'static str {
        "harnesses/antigravity/.agents"
    }

    fn build(&self, roles: &[CanonicalRole], commands: &[CanonicalCommand]) -> anyhow::Result<HashMap<String, String>> {
        let mut files = HashMap::new();
        for role in roles {
            let mut content = String::new();
            content.push_str("---\n");
            content.push_str(&format!("name: {}\n", role.name));
            content.push_str(&format!("description: {}\n", role.description));
            let tools = tools_for(&role.capabilities);
            if !tools.is_empty() {
                content.push_str("tools:\n");
                for tool in &tools {
                    content.push_str(&format!("  - {}\n", tool));
                }
            }
            // agy has no documented per-agent reasoning-effort field, so
            // `role.effort` is intentionally not emitted here — recorded as a gap
            // (like codex's missing tool allowlist) rather than faked with an
            // invented key (#204).
            content.push_str("subagent: true\n");
            content.push_str("mainAgent: false\n");
            content.push_str("commandExecutionPolicy: sandbox\n");
            content.push_str("---\n");
            content.push_str(&role.body);
            files.insert(format!("{}/agents/{}.md", self.base_dir(), role.name), content);
        }
        // `.agents/skills/` is the open Agent Skills tree agy reads; the skills
        // come from the shared emitter (byte-identical with codex/cursor/
        // copilot). Only the crew above are agy-specific.
        files.extend(emit_shared_skills(self.container(), commands));
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
            body: "test body".to_string(),
        };
        let adapter = AntigravityAdapter;
        let files = adapter.build(&[role], &[]).unwrap();
        let content = files.get("harnesses/antigravity/.agents/agents/test-role.md").unwrap();
        assert!(content.contains("name: test-role"));
        assert!(content.contains("description: A test role"));
        assert!(content.contains("tools:\n  - grep_search\n  - list_dir\n  - run_command\n  - view_file"));
        assert!(content.contains("subagent: true"));
        assert!(content.contains("mainAgent: false"));
        assert!(content.contains("commandExecutionPolicy: sandbox"));
        assert!(content.ends_with("test body"));
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
            narrative: "Use `agent-files/*.md` and `Harness-Session`.".to_string(),
            invocation: "".to_string(),
            board: "".to_string(),
            source: std::path::PathBuf::from(""),
        };
        let files = AntigravityAdapter.build(&[], &[command]).unwrap();
        let content = files.get("harnesses/antigravity/.agents/skills/ship-issue/SKILL.md").unwrap();
        assert!(content.starts_with("---\nname: ship-issue\ndescription: desc\n---\n"));
        // Shared neutral dialect: `.agents/agents` glob (matches agy's real crew
        // dir) and the neutral `Agent-Session` trailer.
        assert!(content.contains(".agents/agents/*.md"));
        assert!(content.contains("Agent-Session"));
        assert!(!content.contains("disable-model-invocation"));
    }
}
