use crate::catalog::{CanonicalCommand, CanonicalRole, CanonicalTool};
use std::collections::HashMap;
use super::render::{emit_skill_files, emit_tool_files, WINDSURF};
use super::Adapter;

/// Windsurf (Cascade) discovers skills under `.windsurf/skills/<name>/SKILL.md`
/// and has no subagent mechanic, so the crew becomes twelve skills and `roles`
/// is not emitted.
pub struct WindsurfAdapter;

impl Adapter for WindsurfAdapter {
    fn base_dir(&self) -> &'static str {
        "harnesses/windsurf/.windsurf"
    }

    fn build(&self, _roles: &[CanonicalRole], commands: &[CanonicalCommand]) -> anyhow::Result<HashMap<String, String>> {
        Ok(emit_skill_files(self.base_dir(), commands, &WINDSURF))
    }

    fn build_tools(&self, tools: &[CanonicalTool]) -> HashMap<String, String> {
        // Cascade skills are model-invoked by description; no documented flag to
        // hide one from `@mention`, so agent-invoked-but-typeable (recorded).
        emit_tool_files(self.base_dir(), tools, &WINDSURF, false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_windsurf_adapter_emits_skills_only() {
        let command = CanonicalCommand {
            name: "refactor".to_string(),
            description: "desc".to_string(),
            argument_hint: String::new(),
            allowed_tools: String::new(),
            disable_model_invocation: true,
            arguments: vec![],
            loop_max: 0,
            stages: vec![],
            narrative: "characterization tests first".to_string(),
            invocation: String::new(),
            board: String::new(),
            source: std::path::PathBuf::from(""),
        };
        let files = WindsurfAdapter.build(&[], &[command]).unwrap();
        assert!(files.contains_key("harnesses/windsurf/.windsurf/skills/refactor/SKILL.md"));
        assert!(!files.keys().any(|k| k.contains("agents/")));
    }
}
