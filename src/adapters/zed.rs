use crate::catalog::{CanonicalCommand, CanonicalRole, CanonicalTool};
use std::collections::HashMap;
use super::render::{emit_skill_files, emit_tool_files, ZED};
use super::Adapter;

/// Zed discovers skills under `.zed/skills/<name>/SKILL.md` and has no
/// subagent mechanic, so the crew becomes twelve skills and `roles` is not
/// emitted.
pub struct ZedAdapter;

impl Adapter for ZedAdapter {
    fn base_dir(&self) -> &'static str {
        "harnesses/zed/.zed"
    }

    fn build(&self, _roles: &[CanonicalRole], commands: &[CanonicalCommand]) -> anyhow::Result<HashMap<String, String>> {
        Ok(emit_skill_files(self.base_dir(), commands, &ZED))
    }

    fn build_tools(&self, tools: &[CanonicalTool]) -> HashMap<String, String> {
        // Zed skills are model-invoked from the agent's catalog; `disable-model-
        // invocation` would hide it from the agent (the opposite of a tool), so
        // it stays a plain model-invoked skill — agent-invoked but typeable.
        emit_tool_files(self.base_dir(), tools, &ZED, false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_zed_adapter_emits_skills_only() {
        let command = CanonicalCommand {
            name: "onboard".to_string(),
            description: "desc".to_string(),
            argument_hint: String::new(),
            allowed_tools: String::new(),
            disable_model_invocation: true,
            arguments: vec![],
            loop_max: 0,
            stages: vec![],
            narrative: "answer the crew's questions".to_string(),
            invocation: String::new(),
            board: String::new(),
            source: std::path::PathBuf::from(""),
        };
        let files = ZedAdapter.build(&[], &[command]).unwrap();
        assert!(files.contains_key("harnesses/zed/.zed/skills/onboard/SKILL.md"));
        assert!(!files.keys().any(|k| k.contains("agents/")));
    }
}
