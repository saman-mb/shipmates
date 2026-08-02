use crate::catalog::{CanonicalCommand, CanonicalRole};
use std::collections::HashMap;
use super::Adapter;

pub struct ClaudeCodeAdapter;

impl Adapter for ClaudeCodeAdapter {
    fn build(&self, roles: &[CanonicalRole], commands: &[CanonicalCommand]) -> anyhow::Result<HashMap<String, String>> {
        let mut files = HashMap::new();
        for role in roles {
            files.insert(format!("harnesses/claude-code/.claude/agents/{}.md", role.name), role.body.clone());
        }
        for command in commands {
            files.insert(format!("harnesses/claude-code/.claude/skills/{}/SKILL.md", command.name), command.narrative.clone());
        }
        Ok(files)
    }
}
