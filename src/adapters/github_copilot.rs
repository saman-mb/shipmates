use crate::catalog::{CanonicalCommand, CanonicalRole};
use std::collections::HashMap;
use super::render::{emit_skill_files, GITHUB_COPILOT};
use super::Adapter;

/// GitHub Copilot CLI discovers skills under `.github/skills/<name>/SKILL.md`
/// (project) or `~/.copilot/skills` (personal); the project tree is what a
/// repo-scoped install writes. Copilot has no subagent mechanic, so the crew
/// becomes twelve skills and `roles` is not emitted.
pub struct GithubCopilotAdapter;

impl Adapter for GithubCopilotAdapter {
    fn base_dir(&self) -> &'static str {
        "harnesses/github-copilot/.github"
    }

    fn build(&self, _roles: &[CanonicalRole], commands: &[CanonicalCommand]) -> anyhow::Result<HashMap<String, String>> {
        Ok(emit_skill_files(self.base_dir(), commands, &GITHUB_COPILOT))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_copilot_adapter_emits_skills_only() {
        let command = CanonicalCommand {
            name: "pr-review".to_string(),
            description: "desc".to_string(),
            argument_hint: String::new(),
            allowed_tools: String::new(),
            disable_model_invocation: true,
            arguments: vec![],
            loop_max: 0,
            stages: vec![],
            narrative: "one consolidated verdict".to_string(),
            invocation: String::new(),
            board: String::new(),
            source: std::path::PathBuf::from(""),
        };
        let files = GithubCopilotAdapter.build(&[], &[command]).unwrap();
        assert!(files.contains_key("harnesses/github-copilot/.github/skills/pr-review/SKILL.md"));
        assert!(!files.keys().any(|k| k.contains("agents/")));
    }
}
