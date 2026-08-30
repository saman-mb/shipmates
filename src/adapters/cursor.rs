use super::Adapter;
use super::render::{emit_shared_skills, emit_shared_tool_skills};
use crate::catalog::{CanonicalCommand, CanonicalRole, CanonicalTool};
use std::collections::HashMap;

/// Cursor ships no subagents, so only the fourteen commands ship (as skills) and
/// `roles` is not emitted. Cursor reads the open Agent Skills tree
/// `.agents/skills/<name>/SKILL.md` natively (a first-party peer of
/// `.cursor/skills/`, since Cursor 2.4), so its skills come from the shared
/// `.agents/skills/` emitter — byte-identical with codex/antigravity/copilot,
/// no per-harness duplicate. `base_dir` therefore sits at `.agents`.
/// See <https://cursor.com/docs/skills>.
pub struct CursorAdapter;

impl Adapter for CursorAdapter {
    fn base_dir(&self) -> &'static str {
        "harnesses/cursor/.agents"
    }

    fn build(
        &self,
        _roles: &[CanonicalRole],
        commands: &[CanonicalCommand],
    ) -> anyhow::Result<HashMap<String, String>> {
        // Reasoning effort is DEFERRED on Cursor. Cursor folds effort into the
        // model string rather than a standalone key, and Cursor is skills-only
        // today — no crew/role emission and no per-role model string (a model is
        // never stamped, #205). So there is nowhere to carry effort until Cursor
        // grows a subagent emitter; blocked on that (relates #15/#205). Emit
        // nothing rather than fake a key.
        emit_shared_skills(self.container(), commands)
    }

    fn build_tools(&self, tools: &[CanonicalTool]) -> HashMap<String, String> {
        // Model-invoked skill in the shared `.agents/skills/` tree — agent-invoked
        // but still technically typeable (recorded, not faked).
        emit_shared_tool_skills(self.container(), tools)
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
            narrative: "reproduce first".to_string(),
            invocation: String::new(),
            board: String::new(),
            source: std::path::PathBuf::from(""),
        };
        let files = CursorAdapter.build(&[], &[command]).unwrap();
        assert!(files.contains_key("harnesses/cursor/.agents/skills/fix-bug/SKILL.md"));
        // No crew dir or private skills tree.
        assert!(!files.keys().any(|k| k.contains("/agents/")));
        assert!(!files.keys().any(|k| k.contains(".cursor/skills/")));
    }
}
