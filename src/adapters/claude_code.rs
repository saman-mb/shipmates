use crate::catalog::{CanonicalCommand, CanonicalRole, CanonicalTool};
use std::collections::HashMap;
use super::render::{emit_hook_shim, emit_tool_files, render_body, CLAUDE_CODE};
use super::Adapter;

const TOOL_MAP: [(&str, &[&str]); 5] = [
    ("read", &["Read", "Grep", "Glob"]),
    ("edit", &["Write", "Edit"]),
    ("bash", &["Bash"]),
    ("web", &["WebSearch", "WebFetch"]),
    ("agent", &["Agent"]),
];

fn map_tools(capabilities: &[String]) -> String {
    let mut out: Vec<String> = Vec::new();
    for cap in capabilities {
        if let Some((_, tools)) = TOOL_MAP.iter().find(|(c, _)| c == cap) {
            out.extend(tools.iter().map(|t| t.to_string()));
        }
    }
    out.join(", ")
}

pub struct ClaudeCodeAdapter;

impl Adapter for ClaudeCodeAdapter {
    fn base_dir(&self) -> &'static str {
        "harnesses/claude-code/.claude"
    }

    fn build(&self, roles: &[CanonicalRole], commands: &[CanonicalCommand]) -> anyhow::Result<HashMap<String, String>> {
        let mut files = HashMap::new();
        for role in roles {
            let mut content = String::new();
            content.push_str("---\n");
            content.push_str(&format!("name: {}\n", role.name));
            content.push_str(&format!("description: {}\n", role.description));
            if !role.capabilities.is_empty() {
                content.push_str(&format!("tools: {}\n", map_tools(&role.capabilities)));
            }
            // Claude Code carries reasoning effort as a per-agent `effort` key.
            if let Some(e) = &role.effort {
                content.push_str(&format!("effort: {}\n", e));
            }
            content.push_str("---\n");
            content.push_str(&role.body);
            files.insert(format!("{}/agents/{}.md", self.base_dir(), role.name), content);
        }
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
            content.push_str(&render_body(&command.narrative, &CLAUDE_CODE));
            files.insert(format!("{}/skills/{}/SKILL.md", self.base_dir(), command.name), content);
        }
        // The FSM tool-gate PreToolUse shim (`.claude/hooks/fsm-gate.sh`).
        files.extend(emit_hook_shim(self.container(), "claude-code"));
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
        let files = ClaudeCodeAdapter.build(&[role("architect", &["read", "bash"])], &[]).unwrap();
        let content = files.get("harnesses/claude-code/.claude/agents/architect.md").unwrap();
        assert!(content.contains("tools: Read, Grep, Glob, Bash\n"));
        assert!(!content.contains("read,"));
    }

    #[test]
    fn test_effort_is_emitted_as_the_effort_key() {
        let mut r = role("architect", &["read"]);
        r.effort = Some("high".to_string());
        let files = ClaudeCodeAdapter.build(&[r], &[]).unwrap();
        let content = files.get("harnesses/claude-code/.claude/agents/architect.md").unwrap();
        assert!(content.contains("effort: high\n"), "{content}");
    }

    #[test]
    fn test_no_model_line_is_emitted() {
        // A model is never stamped — it is a runtime decision (#205). Prefix
        // check so `effort:` (which is present) cannot false-positive.
        let mut r = role("architect", &["read"]);
        r.effort = Some("high".to_string());
        let files = ClaudeCodeAdapter.build(&[r], &[]).unwrap();
        let content = files.get("harnesses/claude-code/.claude/agents/architect.md").unwrap();
        assert!(!content.lines().any(|l| l.trim_start().starts_with("model:")), "{content}");
    }

    #[test]
    fn test_fsm_gate_shim_is_emitted() {
        let files = ClaudeCodeAdapter.build(&[], &[]).unwrap();
        assert!(files.contains_key("harnesses/claude-code/.claude/hooks/fsm-gate.sh"));
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
            loop_max: 0,
            stages: vec![],
            tool_gates: vec![],
            narrative: "Resolve via `agent-files/*.md`; use {{arg}}.".to_string(),
            invocation: "".to_string(),
            board: "".to_string(),
            source: std::path::PathBuf::from(""),
        };
        let files = ClaudeCodeAdapter.build(&[], &[command]).unwrap();
        let content = files.get("harnesses/claude-code/.claude/skills/migrate/SKILL.md").unwrap();
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
        let skill = files.get("harnesses/claude-code/.claude/skills/termgif/SKILL.md").unwrap();
        assert!(skill.contains("name: termgif\n"));
        // A tool is agent-invoked only — never a slash command.
        assert!(skill.contains("user-invocable: false\n"));
        assert!(!skill.contains("disable-model-invocation"));
        let asset = files.get("harnesses/claude-code/.claude/skills/termgif/termgif.py").unwrap();
        assert_eq!(asset, "print('hi')");
    }
}
