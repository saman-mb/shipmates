use crate::catalog::{CanonicalCommand, CanonicalRole, CanonicalTool};
use std::collections::HashMap;
use super::render::{emit_skill_files, emit_tool_files, render_body, GITHUB_COPILOT};
use super::Adapter;

/// GitHub Copilot — twelve skills under `.github/skills/<name>/SKILL.md` plus
/// the crew as custom agents under `.github/agents/<name>.agent.md`.
///
/// Custom agents are Markdown with YAML frontmatter. Documented fields:
/// `name`, `description`, `target`, `tools`, `model`,
/// `disable-model-invocation`, `user-invocable`, `mcp-servers`, `metadata`.
/// Invoked as `copilot --agent <name>` (the filename minus `.agent.md`), or
/// `@name` in Visual Studio. Note the double extension — a plain `<name>.md`
/// in this directory is not discovered.
/// See <https://docs.github.com/en/copilot/reference/custom-agents-configuration>.
pub struct GithubCopilotAdapter;

/// Frontmatter plus body must stay under this, per Copilot's docs. Exceeding it
/// is a hard error rather than a truncation: a persona silently cut in half
/// still loads and still answers, just without whatever discipline lived in the
/// part that was dropped.
const MAX_AGENT_CHARS: usize = 30_000;

/// Map semantic capabilities onto Copilot's canonical tool names.
///
/// Copilot accepts aliases (`Read`, `Bash`, `Grep`…) but the canonical names
/// are what its reference documents, so they are what we emit. Unrecognised
/// names are *silently skipped* rather than rejected, so a typo here would
/// quietly narrow an agent with no error — hence an explicit match rather than
/// a pass-through of whatever the catalog happens to hold.
fn tools_for(capabilities: &[String]) -> anyhow::Result<Vec<&'static str>> {
    let mut tools: Vec<&'static str> = Vec::new();
    for cap in capabilities {
        match cap.as_str() {
            // `read` covers file contents; `search` is Grep/Glob, which our
            // `read` capability implies on every other target too.
            "read" => {
                tools.push("read");
                tools.push("search");
            }
            "edit" => tools.push("edit"),
            "bash" => tools.push("execute"),
            "web" => tools.push("web"),
            "agent" => tools.push("agent"),
            // Dropping an unknown capability would silently narrow the agent,
            // and — if it were the only one — omitting `tools` entirely would
            // silently *widen* it to everything. Neither is acceptable for a
            // privilege boundary, so an unmapped name fails the build.
            other => anyhow::bail!("unmapped capability {other:?} for github-copilot"),
        }
    }
    // Sort before dedup: dedup only removes *adjacent* duplicates, so
    // ["read", "web", "read"] would otherwise keep both copies of read/search.
    tools.sort_unstable();
    tools.dedup();
    Ok(tools)
}

/// Quote a scalar for YAML frontmatter.
///
/// Descriptions are prose written by contributors and routinely contain `:`,
/// `#`, quotes and leading symbols. An unquoted `description: foo: bar` is not
/// a parse warning — it is a frontmatter block Copilot cannot read, so the
/// agent installs cleanly and is never loaded. Always quote; never hope.
fn yaml_scalar(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

impl Adapter for GithubCopilotAdapter {
    fn base_dir(&self) -> &'static str {
        "harnesses/github-copilot/.github"
    }

    fn build(&self, roles: &[CanonicalRole], commands: &[CanonicalCommand]) -> anyhow::Result<HashMap<String, String>> {
        let mut files = HashMap::new();
        for role in roles {
            let mut content = String::new();
            content.push_str("---\n");
            content.push_str(&format!("name: {}\n", yaml_scalar(&role.name)));
            content.push_str(&format!("description: {}\n", yaml_scalar(&role.description)));
            let tools = tools_for(&role.capabilities)?;
            // Omitting `tools` enables *every* tool, so an empty mapping must
            // fail rather than ship an unrestricted agent. This is the fail-open
            // direction and the only one that matters here.
            if tools.is_empty() {
                anyhow::bail!(
                    "github-copilot agent {} maps to no tools; omitting the key would grant every tool",
                    role.name
                );
            }
            content.push_str(&format!(
                "tools: [{}]\n",
                tools.iter().map(|t| format!("\"{}\"", t)).collect::<Vec<_>>().join(", ")
            ));
            content.push_str("---\n");
            content.push_str(&render_body(&role.body, &GITHUB_COPILOT));

            if content.chars().count() > MAX_AGENT_CHARS {
                anyhow::bail!(
                    "github-copilot agent {} is {} characters, over the documented {} limit",
                    role.name,
                    content.chars().count(),
                    MAX_AGENT_CHARS
                );
            }
            files.insert(format!("{}/agents/{}.agent.md", self.base_dir(), role.name), content);
        }
        for (path, content) in emit_skill_files(self.base_dir(), commands, &GITHUB_COPILOT) {
            files.insert(path, content);
        }
        Ok(files)
    }

    fn build_tools(&self, tools: &[CanonicalTool]) -> HashMap<String, String> {
        // Copilot Agent Skills are model-invoked by description; the `/name`
        // override still exists and skills don't document a hide flag, so
        // agent-invoked but typeable (recorded, not faked).
        emit_tool_files(self.base_dir(), tools, &GITHUB_COPILOT, false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn role(name: &str, capabilities: &[&str], body: &str) -> CanonicalRole {
        CanonicalRole {
            name: name.to_string(),
            description: "desc".to_string(),
            capabilities: capabilities.iter().map(|s| s.to_string()).collect(),
            writes: false,
            web_scopes: vec![],
            read_scopes: vec![],
            tool_order: vec![],
            source: std::path::PathBuf::from(""),
            body: body.to_string(),
        }
    }

    fn role_with_desc(name: &str, capabilities: &[&str], description: &str) -> CanonicalRole {
        let mut r = role(name, capabilities, "body");
        r.description = description.to_string();
        r
    }

    fn command(name: &str) -> CanonicalCommand {
        CanonicalCommand {
            name: name.to_string(),
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
        }
    }

    #[test]
    fn test_copilot_adapter_emits_skills_and_crew() {
        let files = GithubCopilotAdapter
            .build(&[role("architect", &["read", "bash"], "body")], &[command("pr-review")])
            .unwrap();
        assert!(files.contains_key("harnesses/github-copilot/.github/skills/pr-review/SKILL.md"));
        assert!(files.contains_key("harnesses/github-copilot/.github/agents/architect.agent.md"));
    }

    #[test]
    fn test_agent_uses_the_double_extension() {
        // `<name>.md` in this directory is not discovered by Copilot.
        let files = GithubCopilotAdapter.build(&[role("sdet", &["read"], "body")], &[]).unwrap();
        assert!(files.contains_key("harnesses/github-copilot/.github/agents/sdet.agent.md"));
        assert!(!files.contains_key("harnesses/github-copilot/.github/agents/sdet.md"));
    }

    #[test]
    fn test_capabilities_map_to_copilot_tool_names() {
        let files = GithubCopilotAdapter
            .build(&[role("architect", &["read", "bash"], "body")], &[])
            .unwrap();
        let agent = files.get("harnesses/github-copilot/.github/agents/architect.agent.md").unwrap();
        assert!(agent.contains("tools: [\"execute\", \"read\", \"search\"]\n"));
        assert!(!agent.contains("\"edit\""));
    }

    #[test]
    fn test_least_privilege_is_preserved_per_role() {
        let files = GithubCopilotAdapter
            .build(
                &[
                    role("architect", &["read", "bash"], "body"),
                    role("senior-engineer", &["read", "edit", "bash"], "body"),
                ],
                &[],
            )
            .unwrap();
        let architect = files.get("harnesses/github-copilot/.github/agents/architect.agent.md").unwrap();
        let engineer = files.get("harnesses/github-copilot/.github/agents/senior-engineer.agent.md").unwrap();
        assert!(!architect.contains("\"edit\""), "architect must not receive edit");
        assert!(engineer.contains("\"edit\""), "senior-engineer must receive edit");
    }

    #[test]
    fn test_body_is_rendered_into_the_copilot_dialect() {
        let files = GithubCopilotAdapter
            .build(&[role("architect", &["read"], "see `agent-files/*.md` and Harness-Session")], &[])
            .unwrap();
        let agent = files.get("harnesses/github-copilot/.github/agents/architect.agent.md").unwrap();
        assert!(agent.contains(".github/agents/*.md"));
        assert!(agent.contains("Copilot-Session"));
        assert!(!agent.contains("agent-files/"));
    }

    #[test]
    fn test_description_is_yaml_quoted() {
        // A description containing ": " is prose a contributor will write, and
        // unquoted it produces frontmatter Copilot cannot parse — the agent
        // installs cleanly and is never loaded.
        let files = GithubCopilotAdapter
            .build(&[role_with_desc("architect", &["read"], "reviews: structure, not style")], &[])
            .unwrap();
        let agent = files.get("harnesses/github-copilot/.github/agents/architect.agent.md").unwrap();
        assert!(agent.contains("description: \"reviews: structure, not style\"\n"), "{agent}");
    }

    #[test]
    fn test_quotes_in_description_are_escaped() {
        let files = GithubCopilotAdapter
            .build(&[role_with_desc("architect", &["read"], "the \"right\" shape")], &[])
            .unwrap();
        let agent = files.get("harnesses/github-copilot/.github/agents/architect.agent.md").unwrap();
        assert!(agent.contains(r#"description: "the \"right\" shape""#), "{agent}");
    }

    #[test]
    fn test_unmapped_capability_fails_rather_than_narrowing() {
        let result = GithubCopilotAdapter.build(&[role("architect", &["read", "telepathy"], "body")], &[]);
        assert!(result.is_err(), "an unmapped capability must fail the build");
    }

    #[test]
    fn test_role_with_no_mappable_tools_fails_rather_than_granting_everything() {
        // Omitting `tools` enables every tool — the fail-open direction.
        let result = GithubCopilotAdapter.build(&[role("architect", &[], "body")], &[]);
        assert!(result.is_err(), "an empty tool mapping must fail, not ship an unrestricted agent");
    }

    #[test]
    fn test_duplicate_capabilities_do_not_repeat_tools() {
        let files = GithubCopilotAdapter
            .build(&[role("architect", &["read", "bash", "read"], "body")], &[])
            .unwrap();
        let agent = files.get("harnesses/github-copilot/.github/agents/architect.agent.md").unwrap();
        assert!(agent.contains("tools: [\"execute\", \"read\", \"search\"]\n"), "{agent}");
    }

    #[test]
    fn test_oversized_agent_is_an_error_not_a_truncation() {
        let huge = "x".repeat(MAX_AGENT_CHARS + 1);
        let result = GithubCopilotAdapter.build(&[role("architect", &["read"], &huge)], &[]);
        assert!(result.is_err(), "an agent over the documented limit must fail the build");
    }
}
