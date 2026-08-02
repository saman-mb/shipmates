use crate::catalog::{CanonicalCommand, CanonicalRole};
use std::collections::HashMap;

pub struct ClaudeCodeAdapter;

impl ClaudeCodeAdapter {
    pub fn build(roles: &[CanonicalRole], commands: &[CanonicalCommand]) -> HashMap<String, String> {
        let mut files = HashMap::new();
        for role in roles {
            files.insert(format!("harnesses/claude-code/.claude/agents/{}.md", role.name), role.body.clone());
        }
        for command in commands {
            files.insert(format!("harnesses/claude-code/.claude/skills/{}/SKILL.md", command.name), command.narrative.clone());
        }
        files
    }
}
