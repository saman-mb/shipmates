use crate::catalog::{CanonicalCommand, CanonicalRole};
use std::collections::HashMap;

pub struct OpencodeAdapter;

impl OpencodeAdapter {
    pub fn build(roles: &[CanonicalRole], commands: &[CanonicalCommand]) -> HashMap<String, String> {
        let mut files = HashMap::new();
        for role in roles {
            let mut content = String::new();
            content.push_str("---\n");
            content.push_str(&format!("description: {}\n", role.description));
            content.push_str("mode: subagent\n");
            content.push_str("permission:\n");
            // Opencode's "*": deny first permission logic
            content.push_str("  \"*\": deny\n");
            for cap in &role.capabilities {
                content.push_str(&format!("  {}: allow\n", cap));
            }
            content.push_str("---\n");
            content.push_str(&role.body);
            files.insert(format!("harnesses/opencode/.opencode/agents/{}.md", role.name), content);
        }
        for command in commands {
            let mut content = String::new();
            content.push_str("---\n");
            content.push_str(&format!("description: {}\n", command.description));
            content.push_str("---\n");
            content.push_str(&command.narrative);
            files.insert(format!("harnesses/opencode/.opencode/commands/{}.md", command.name), content);
        }
        files
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_opencode_adapter_frontmatter() {
        let role = CanonicalRole {
            name: "test-role".to_string(),
            description: "A test role".to_string(),
            capabilities: vec!["read".to_string(), "bash".to_string()],
            writes: false,
            web_scopes: vec![],
            read_scopes: vec![],
            tool_order: vec![],
            source: std::path::PathBuf::from(""),
            body: "test body".to_string(),
        };

        let result = OpencodeAdapter::build(&[role], &[]);
        let content = result.get("harnesses/opencode/.opencode/agents/test-role.md").unwrap();
        
        // Assert the frontmatter
        assert!(content.starts_with("---\n"));
        assert!(content.contains("description: A test role\n"));
        assert!(content.contains("mode: subagent\n"));
        assert!(content.contains("permission:\n"));
        assert!(content.contains("  \"*\": deny\n"));
        assert!(content.contains("  read: allow\n"));
        assert!(content.contains("  bash: allow\n"));
        assert!(content.contains("---\n"));
        assert!(content.ends_with("test body"));
    }
}
