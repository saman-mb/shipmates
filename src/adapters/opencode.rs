use crate::catalog::{CanonicalCommand, CanonicalRole};
use std::collections::HashMap;

pub struct OpencodeAdapter;

impl OpencodeAdapter {
    pub fn build(roles: &[CanonicalRole], commands: &[CanonicalCommand]) -> HashMap<String, String> {
        let mut files = HashMap::new();
        for role in roles {
            let mut content = String::new();
            // Opencode's "*": deny first permission logic
            content.push_str("\"*\": deny\n");
            for cap in &role.capabilities {
                content.push_str(&format!("\"{}\": allow\n", cap));
            }
            content.push_str(&role.body);
            files.insert(format!("harnesses/opencode/.opencode/agents/{}.md", role.name), content);
        }
        for command in commands {
            files.insert(format!("harnesses/opencode/.opencode/commands/{}.md", command.name), command.narrative.clone());
        }
        files
    }
}
