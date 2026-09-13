use super::Adapter;
use super::render::{AGENT_SKILLS, emit_shared_skills, emit_shared_tool_skills};
use crate::catalog::{CanonicalCommand, CanonicalRole, CanonicalTool};
use std::collections::HashMap;

/// Pi ships no subagents, so the adapter emits the crew as fifteen skills and
/// ignores `roles`. Pi reads the open `.agents/skills/` tree natively.
pub struct PiAdapter;

impl Adapter for PiAdapter {
    fn base_dir(&self) -> &'static str {
        "harnesses/pi/.agents"
    }

    fn digest_root(&self) -> &'static str {
        self.container()
    }

    fn steering_dialect(&self) -> Option<&'static super::render::Dialect> {
        Some(&AGENT_SKILLS)
    }

    fn steering_target(&self) -> Option<super::render::SteeringTarget> {
        None
    }

    fn build(
        &self,
        _roles: &[CanonicalRole],
        commands: &[CanonicalCommand],
    ) -> anyhow::Result<HashMap<String, String>> {
        emit_shared_skills(self.container(), commands)
    }

    fn build_tools(&self, tools: &[CanonicalTool]) -> HashMap<String, String> {
        emit_shared_tool_skills(self.container(), tools)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pi_adapter_emits_skills_only() {
        let command = CanonicalCommand {
            name: "ship-fix-bug".to_string(),
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
        let files = PiAdapter.build(&[], &[command]).unwrap();
        assert_eq!(
            files.keys().collect::<Vec<_>>(),
            vec!["harnesses/pi/.agents/skills/ship-fix-bug/SKILL.md"]
        );
    }
}
