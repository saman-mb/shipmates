use crate::catalog::{CanonicalCommand, CanonicalRole};
use std::collections::HashMap;
use super::Adapter;

pub struct ClaudeCodeAdapter;

impl Adapter for ClaudeCodeAdapter {
    fn build(&self, roles: &[CanonicalRole], commands: &[CanonicalCommand]) -> anyhow::Result<HashMap<String, String>> {
        let mut files = HashMap::new();
        for role in roles {
            let mut content = String::new();
            content.push_str("---\n");
            content.push_str(&format!("name: {}\n", role.name));
            content.push_str(&format!("description: {}\n", role.description));
            if !role.capabilities.is_empty() {
                content.push_str(&format!("tools: {}\n", role.capabilities.join(",")));
            }
            content.push_str("---\n");
            content.push_str(&role.body);
            files.insert(format!("harnesses/claude-code/.claude/agents/{}.md", role.name), content);
        }
        for command in commands {
            let mut content = String::new();
            content.push_str("---\n");
            content.push_str(&format!("name: {}\n", command.name));
            content.push_str(&format!("description: {}\n", command.description));
            if !command.allowed_tools.is_empty() {
                content.push_str(&format!("allowed-tools: {}\n", command.allowed_tools));
            }
            if command.disable_model_invocation {
                content.push_str("disable-model-invocation: true\n");
            }
            content.push_str("---\n");
            content.push_str(&command.narrative);
            files.insert(format!("harnesses/claude-code/.claude/skills/{}/SKILL.md", command.name), content);
        }
        Ok(files)
    }
}
