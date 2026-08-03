use crate::catalog::{CanonicalCommand, CanonicalRole, CanonicalTool};
use std::collections::HashMap;
use super::render::{emit_skill_files, emit_tool_files, ANTIGRAVITY};
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
            content.push_str("subagent: true\n");
            content.push_str("mainAgent: false\n");
            content.push_str("commandExecutionPolicy: sandbox\n");
            content.push_str("---\n");
            content.push_str(&role.body);
            files.insert(format!("{}/agents/{}.md", self.base_dir(), role.name), content);
        }
        for (path, content) in emit_skill_files(self.base_dir(), commands, &ANTIGRAVITY) {
            files.insert(path, content);
        }
        Ok(files)
    }

    fn build_tools(&self, tools: &[CanonicalTool]) -> HashMap<String, String> {
        // agy skills are model-invoked ("it decides based on context"); they
        // also surface as slash commands, so agent-invoked but typeable
        // (recorded, not faked).
        emit_tool_files(self.base_dir(), tools, &ANTIGRAVITY, false)
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
            loop_max: 0,
            stages: vec![],
            narrative: "Use `agent-files/*.md` and `Harness-Session`.".to_string(),
            invocation: "".to_string(),
            board: "".to_string(),
            source: std::path::PathBuf::from(""),
        };
        let files = AntigravityAdapter.build(&[], &[command]).unwrap();
        let content = files.get("harnesses/antigravity/.agents/skills/ship-issue/SKILL.md").unwrap();
        assert!(content.starts_with("---\nname: ship-issue\ndescription: desc\n---\n"));
        assert!(content.contains(".agents/agents/*.md"));
        assert!(content.contains("Antigravity-Session"));
        assert!(!content.contains("disable-model-invocation"));
    }
}
