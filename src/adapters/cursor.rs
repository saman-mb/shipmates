use crate::catalog::{CanonicalCommand, CanonicalRole, CanonicalTool};
use std::collections::HashMap;
use super::render::{emit_skill_files, emit_tool_files, CURSOR};
use super::Adapter;

/// Cursor ships no subagents, so the crew becomes twelve skills under
/// `.cursor/skills/<name>/SKILL.md` and `roles` is not emitted.
pub struct CursorAdapter;

impl Adapter for CursorAdapter {
    fn base_dir(&self) -> &'static str {
        "harnesses/cursor/.cursor"
    }

    fn build(&self, _roles: &[CanonicalRole], commands: &[CanonicalCommand]) -> anyhow::Result<HashMap<String, String>> {
        Ok(emit_skill_files(self.base_dir(), commands, &CURSOR))
    }

    fn build_tools(&self, tools: &[CanonicalTool]) -> HashMap<String, String> {
        // Cursor skills are model-invoked by default; no documented way to hide
        // one from manual `/`-mention, so the tool is agent-invoked but still
        // technically typeable (recorded, not faked).
        emit_tool_files(self.base_dir(), tools, &CURSOR, false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cursor_adapter_emits_skills_only() {
        let command = CanonicalCommand {
            name: "fix-bug".to_string(),
            description: "desc".to_string(),
            argument_hint: String::new(),
            allowed_tools: String::new(),
            disable_model_invocation: true,
            arguments: vec![],
            loop_max: 0,
            stages: vec![],
            narrative: "reproduce first".to_string(),
            invocation: String::new(),
            board: String::new(),
            source: std::path::PathBuf::from(""),
        };
        let files = CursorAdapter.build(&[], &[command]).unwrap();
        assert!(files.contains_key("harnesses/cursor/.cursor/skills/fix-bug/SKILL.md"));
        assert!(!files.keys().any(|k| k.contains("agents/")));
    }
}
