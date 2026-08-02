use crate::catalog::{CanonicalCommand, CanonicalRole};
use std::collections::HashMap;
use super::Adapter;

pub struct GeminiAdapter;

impl Adapter for GeminiAdapter {
    fn build(&self, roles: &[CanonicalRole], commands: &[CanonicalCommand]) -> anyhow::Result<HashMap<String, String>> {
        let mut files = HashMap::new();
        for role in roles {
            let mut content = String::new();
            content.push_str("---\n");
            content.push_str(&format!("name: {}\n", role.name));
            content.push_str(&format!("description: {}\n", role.description));
            content.push_str("---\n");
            content.push_str(&role.body);
            files.insert(format!("harnesses/gemini/.gemini/agents/{}.md", role.name), content);
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
            files.insert(format!("harnesses/gemini/.gemini/skills/{}/SKILL.md", command.name), content);
        }
        Ok(files)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gemini_adapter_build() {
        let role = CanonicalRole {
            name: "test-role".to_string(),
            description: "A test role".to_string(),
            capabilities: vec!["read".to_string()],
            writes: false,
            web_scopes: vec![],
            read_scopes: vec![],
            tool_order: vec![],
            source: std::path::PathBuf::from(""),
            body: "test body".to_string(),
        };
        let adapter = GeminiAdapter;
        let files = adapter.build(&[role], &[]).unwrap();
        let content = files.get("harnesses/gemini/.gemini/agents/test-role.md").unwrap();
        assert!(content.contains("name: test-role"));
        assert!(content.contains("description: A test role"));
    }
}
