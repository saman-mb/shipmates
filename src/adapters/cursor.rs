use super::Adapter;
use super::render::{AGENT_SKILLS, emit_skill_files, emit_tool_files};
use crate::catalog::{CanonicalCommand, CanonicalRole, CanonicalTool};
use std::collections::HashMap;

/// Cursor ships no subagents, so only the fifteen commands ship (as skills) and
/// `roles` is not emitted.
///
/// Cursor is documented as reading the open Agent Skills tree
/// `.agents/skills/<name>/SKILL.md` (a first-party peer of `.cursor/skills/`,
/// since Cursor 2.4, <https://cursor.com/docs/skills>), but a field report
/// (#405) shows a global Cursor install populated only from
/// `~/.cursor/skills/` — skills installed to `~/.agents/skills/` never appeared
/// in the `/` menu. So cursor ships to the harness-native `.cursor/skills/`
/// tree instead, and `base_dir` sits at `.cursor`.
///
/// Exactly one copy, never both: Cursor reads both locations, so a mirrored
/// payload would list every command twice in the picker and double the prompt
/// cost — the duplicate-command defect (#403) this work exists to remove.
/// Symlinking the two trees was rejected too (receipts hash real files, and
/// symlinks are not portable to Windows). The bytes are still the neutral
/// [`AGENT_SKILLS`] rendering, identical to what codex/antigravity/copilot ship
/// to the shared tree; only the path differs.
pub struct CursorAdapter;

impl Adapter for CursorAdapter {
    fn base_dir(&self) -> &'static str {
        "harnesses/cursor/.cursor"
    }

    fn digest_root(&self) -> &'static str {
        self.container()
    }

    fn steering_dialect(&self) -> Option<&'static super::render::Dialect> {
        Some(&super::render::AGENT_SKILLS)
    }

    fn steering_target(&self) -> Option<super::render::SteeringTarget> {
        Some(super::render::SteeringTarget {
            rel_path: ".cursor/rules/shipmates-contributor.mdc",
            format: super::render::SteeringFormat::CursorMdc {
                description: "Shipmates contributor checklists for crew, commands, tools, and site assets",
            },
        })
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
        emit_skill_files(self.base_dir(), commands, &AGENT_SKILLS)
    }

    fn build_tools(&self, tools: &[CanonicalTool]) -> HashMap<String, String> {
        // Model-invoked skill in cursor's own skills tree. Cursor has no
        // documented way to hide a skill from manual mention (only Claude
        // Code's `user-invocable: false` does that), so `agent_only = false` —
        // technically typeable, recorded not faked.
        emit_tool_files(self.base_dir(), tools, &AGENT_SKILLS, false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cursor_adapter_emits_skills_only() {
        let command = CanonicalCommand {
            name: "shipmates-fix-bug".to_string(),
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
        // Exactly one copy, in cursor's own tree: mirroring into
        // `.agents/skills/` too would double every entry in the picker (#403).
        assert_eq!(
            files.keys().collect::<Vec<_>>(),
            vec!["harnesses/cursor/.cursor/skills/shipmates-fix-bug/SKILL.md"]
        );
    }

    #[test]
    fn test_cursor_adapter_emits_tools_into_its_own_tree_only() {
        let tool = CanonicalTool {
            name: "shipmates-termgif".to_string(),
            description: "desc".to_string(),
            body: "record a terminal".to_string(),
            assets: vec![("run.py".to_string(), "print(1)\n".to_string())],
            requires: vec![],
            source: std::path::PathBuf::from(""),
        };
        let files = CursorAdapter.build_tools(&[tool]);
        let mut paths = files.keys().cloned().collect::<Vec<_>>();
        paths.sort();
        assert_eq!(
            paths,
            vec![
                "harnesses/cursor/.cursor/skills/shipmates-termgif/SKILL.md".to_string(),
                "harnesses/cursor/.cursor/skills/shipmates-termgif/run.py".to_string(),
            ]
        );
    }
}
