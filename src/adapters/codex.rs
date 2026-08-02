use crate::catalog::{CanonicalCommand, CanonicalRole};
use std::collections::HashMap;
use super::render::{emit_skill_files, CODEX};
use super::Adapter;

/// Codex CLI ships no subagents, so the crew becomes twelve skills under
/// `.codex/skills/<name>/SKILL.md` and `roles` is not emitted. Codex reads
/// project instructions from `AGENTS.md`, which the dialect honours.
pub struct CodexAdapter;

impl Adapter for CodexAdapter {
    fn base_dir(&self) -> &'static str {
        "harnesses/codex/.codex"
    }

    fn build(&self, _roles: &[CanonicalRole], commands: &[CanonicalCommand]) -> anyhow::Result<HashMap<String, String>> {
        Ok(emit_skill_files(self.base_dir(), commands, &CODEX))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn command(name: &str, narrative: &str) -> CanonicalCommand {
        CanonicalCommand {
            name: name.to_string(),
            description: "desc".to_string(),
            argument_hint: String::new(),
            allowed_tools: String::new(),
            disable_model_invocation: true,
            arguments: vec![],
            loop_max: 0,
            stages: vec![],
            narrative: narrative.to_string(),
            invocation: String::new(),
            board: String::new(),
            source: std::path::PathBuf::from(""),
        }
    }

    #[test]
    fn test_codex_adapter_emits_skills_only() {
        let files = CodexAdapter.build(&[], &[command("migrate", "use {{arg}} via `agent-files/*.md`")]).unwrap();
        assert!(files.contains_key("harnesses/codex/.codex/skills/migrate/SKILL.md"));
        assert!(!files.keys().any(|k| k.contains("agents/")));
        let content = files.values().next().unwrap();
        assert!(content.contains("name: migrate\n"));
        assert!(content.contains("description: desc\n"));
        assert!(content.contains(".codex/agents/*.md"));
        assert!(content.contains("$ARGUMENTS"));
        assert!(!content.contains("{{arg}}"));
    }
}
