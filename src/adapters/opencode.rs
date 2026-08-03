use crate::catalog::{CanonicalCommand, CanonicalRole, CanonicalTool};
use std::collections::HashMap;
use super::render::{render_body, OPENCODE};
use super::Adapter;

pub struct OpencodeAdapter;

/// opencode is the one harness with a genuine native tool: a `.ts`/`.js` file in
/// `.opencode/tools/` whose default export the model can call directly. We emit
/// a thin definition that shells out to the tool's bundled script (opencode
/// treats only `.ts`/`.js` here as tools, so the script rides alongside and is
/// otherwise ignored). Format-checked against opencode's docs, not runtime-run.
fn opencode_tool_ts(tool: &CanonicalTool) -> String {
    let desc = serde_json::to_string(&tool.description).unwrap_or_else(|_| "\"\"".to_string());
    let name = &tool.name;
    format!(
        r#"import {{ tool }} from "@opencode-ai/plugin"
import {{ spawnSync }} from "node:child_process"
import * as path from "node:path"

// {name}: an agent-invoked tool. See {name}.py alongside this file.
export default tool({{
  description: {desc},
  args: {{
    spec: tool.schema.string().describe("JSON spec passed to the tool on stdin"),
    out: tool.schema.string().describe("output file path"),
  }},
  async execute(args) {{
    const script = path.join(import.meta.dirname, "{name}.py")
    const res = spawnSync("python3", [script, "--out", args.out], {{ input: args.spec, encoding: "utf8" }})
    if (res.status !== 0) throw new Error(res.stderr || "{name} failed")
    return res.stdout.trim()
  }},
}})
"#
    )
}

impl Adapter for OpencodeAdapter {
    fn base_dir(&self) -> &'static str {
        "harnesses/opencode/.opencode"
    }

    fn build(&self, roles: &[CanonicalRole], commands: &[CanonicalCommand]) -> anyhow::Result<HashMap<String, String>> {
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
            files.insert(format!("{}/agents/{}.md", self.base_dir(), role.name), content);
        }
        for command in commands {
            let mut content = String::new();
            content.push_str("---\n");
            content.push_str(&format!("description: {}\n", command.description));
            content.push_str("---\n");
            content.push_str(&render_body(&command.narrative, &OPENCODE));
            files.insert(format!("{}/commands/{}.md", self.base_dir(), command.name), content);
        }
        Ok(files)
    }

    fn build_tools(&self, tools: &[CanonicalTool]) -> HashMap<String, String> {
        let mut files = HashMap::new();
        for tool in tools {
            files.insert(
                format!("{}/tools/{}.ts", self.base_dir(), tool.name),
                opencode_tool_ts(tool),
            );
            // Bundle the tool's scripts next to the `.ts` definition.
            for (rel, asset) in &tool.assets {
                files.insert(format!("{}/tools/{}", self.base_dir(), rel), asset.clone());
            }
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

        let result = OpencodeAdapter.build(&[role], &[]).unwrap();
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

    #[test]
    fn test_opencode_command_body_renders_dialect() {
        let command = CanonicalCommand {
            name: "ship-issue".to_string(),
            description: "desc".to_string(),
            argument_hint: "".to_string(),
            allowed_tools: "".to_string(),
            disable_model_invocation: true,
            arguments: vec![],
            loop_max: 0,
            stages: vec![],
            narrative: "Resolve via `agent-files/*.md` else `general-purpose`; spawn `@role(planner)`; use {{issue}}."
                .to_string(),
            invocation: "".to_string(),
            board: "".to_string(),
            source: std::path::PathBuf::from(""),
        };
        let files = OpencodeAdapter.build(&[], &[command]).unwrap();
        let content = files.get("harnesses/opencode/.opencode/commands/ship-issue.md").unwrap();
        assert!(content.contains(".opencode/agents/*.md"));
        assert!(content.contains("general"));
        assert!(content.contains("subagent_type: architect"));
        assert!(content.contains("$ARGUMENTS"));
        assert!(!content.contains("agent-files/"));
        assert!(!content.contains("general-purpose"));
        assert!(!content.contains("{{issue}}"));
    }

    #[test]
    fn test_tool_emits_native_ts_plus_bundled_script() {
        let tool = CanonicalTool {
            name: "termgif".to_string(),
            description: "render a gif".to_string(),
            body: "x".to_string(),
            assets: vec![("termgif.py".to_string(), "print('hi')".to_string())],
            source: std::path::PathBuf::from(""),
        };
        let files = OpencodeAdapter.build_tools(&[tool]);
        // opencode's native tool is code, not a skill.
        let ts = files.get("harnesses/opencode/.opencode/tools/termgif.ts").unwrap();
        assert!(ts.contains("export default tool("));
        assert!(ts.contains("termgif.py"));
        assert_eq!(
            files.get("harnesses/opencode/.opencode/tools/termgif.py").unwrap(),
            "print('hi')"
        );
        // No skill or command form for a tool on opencode.
        assert!(!files.keys().any(|k| k.contains("/skills/")));
    }
}
