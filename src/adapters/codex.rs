use crate::catalog::{CanonicalCommand, CanonicalRole, CanonicalTool};
use std::collections::HashMap;
use super::render::{emit_shared_skills, emit_shared_tool_skills, render_role_body, CODEX};
use super::Adapter;

/// Codex CLI — thirteen skills under `.agents/skills/<name>/SKILL.md` plus the
/// crew as project-scoped subagents under `.codex/agents/<name>.toml`.
///
/// Skills and crew land in **two different dotdirs**. Codex discovers skills at
/// the open Agent Skills location — `$CWD/.agents/skills` up to the repo root,
/// `~/.agents/skills` for the user — not `.codex/skills`; it builds on the
/// <https://agentskills.io> standard and a `.codex/skills` tree is never read.
/// Its *crew*, by contrast, are Codex-native and live at `.codex/agents/`.
/// See <https://learn.chatgpt.com/docs/build-skills>. The skills come from the
/// shared `emit_shared_skills` emitter (byte-identical across every harness that
/// reads `.agents/skills/`, so a multi-harness install doesn't collide); only
/// the crew are rendered in the Codex dialect. `digest_root()` is overridden to
/// the container so both dotdirs are covered.
///
/// Subagents reached GA in March 2026. Codex reads custom agents from
/// `~/.codex/agents/` (global) and `.codex/agents/` (project-scoped, checked
/// into the repo) — the project directory is what a repo-scoped install writes.
/// Built-in agents are `default`, `worker` and `explorer`.
/// See <https://learn.chatgpt.com/docs/agent-configuration/subagents>.
///
/// Codex is the only target so far whose agent format is **not** Markdown with
/// frontmatter: each agent is a standalone TOML file. The documented fields are
/// `name`, `description`, `developer_instructions`, and optionally `model`,
/// `model_reasoning_effort`, `sandbox_mode`, `mcp_servers` and `skills.config`.
///
/// There is no documented per-agent tool allowlist, so a role's `capabilities`
/// cannot be expressed here the way they are for Claude Code, Antigravity or
/// Copilot. That is a real capability gap, recorded rather than papered over —
/// an invented `tools` key would be ignored at best and rejected at worst.
/// Codex crew inherit the session's tools; least privilege is not enforced on
/// this target.
pub struct CodexAdapter;

/// Render a TOML multi-line *literal* string.
///
/// Literal (`'''`), not basic (`"""`): basic strings process escapes, and the
/// personas contain backslashes — the documented `\$1` escape would be consumed
/// on the way to disk and change the instruction. Literal strings take the
/// bytes verbatim. The one sequence they cannot carry is `'''`, which is an
/// error here rather than a silent corruption.
fn toml_literal(value: &str) -> anyhow::Result<String> {
    if value.contains("'''") {
        anyhow::bail!("body contains a TOML literal-string terminator (''') and cannot be emitted");
    }
    // TOML trims a newline immediately after the opening delimiter, so starting
    // the body on its own line keeps the content byte-identical.
    Ok(format!("'''\n{}'''", value))
}

/// Render a TOML basic string for a single-line value.
fn toml_basic(value: &str) -> String {
    let escaped = value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\t', "\\t")
        .replace('\r', "\\r");
    format!("\"{}\"", escaped)
}

impl Adapter for CodexAdapter {
    fn base_dir(&self) -> &'static str {
        "harnesses/codex/.codex"
    }

    // Crew (`.codex/`) and skills (`.agents/`) span two dotdirs, so digests key
    // off the install container to cover both rather than only `.codex/`.
    fn digest_root(&self) -> &'static str {
        self.container()
    }

    fn build(&self, roles: &[CanonicalRole], commands: &[CanonicalCommand]) -> anyhow::Result<HashMap<String, String>> {
        let mut files = HashMap::new();
        for role in roles {
            let body = render_role_body(&role.body, &CODEX);
            let mut content = String::new();
            content.push_str(&format!("name = {}\n", toml_basic(&role.name)));
            content.push_str(&format!("description = {}\n", toml_basic(&role.description)));
            // Codex carries reasoning effort as the documented `model_reasoning_effort`.
            if let Some(e) = &role.effort {
                content.push_str(&format!("model_reasoning_effort = {}\n", toml_basic(e)));
            }
            content.push_str(&format!("developer_instructions = {}\n", toml_literal(&body)?));
            files.insert(format!("{}/agents/{}.toml", self.base_dir(), role.name), content);
        }
        // Commands ship to the shared `.agents/skills/` tree (neutral dialect).
        files.extend(emit_shared_skills(self.container(), commands));
        Ok(files)
    }

    fn build_tools(&self, tools: &[CanonicalTool]) -> HashMap<String, String> {
        // Codex has no tool primitive outside MCP; a model-invoked skill in the
        // shared `.agents/skills/` tree is the closest native fit. `$skill` can
        // still name it, so agent-invoked but typeable (recorded, not faked).
        emit_shared_tool_skills(self.container(), tools)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn command(name: &str, narrative: &str) -> CanonicalCommand {
        CanonicalCommand {
            name: name.to_string(),
            description: "desc".to_string(),
            argument_hint: String::new(),
            allowed_tools: String::new(),
            disable_model_invocation: true,
            arguments: vec![],
            narrative: narrative.to_string(),
            invocation: String::new(),
            board: String::new(),
            source: std::path::PathBuf::from(""),
        }
    }

    fn role(name: &str, body: &str) -> CanonicalRole {
        CanonicalRole {
            name: name.to_string(),
            description: "desc".to_string(),
            capabilities: vec!["read".to_string(), "bash".to_string()],
            writes: false,
            web_scopes: vec![],
            read_scopes: vec![],
            tool_order: vec![],
            effort: None,
            source: std::path::PathBuf::from(""),
            body: body.to_string(),
        }
    }

    #[test]
    fn test_codex_adapter_emits_skills_and_crew() {
        let files = CodexAdapter
            .build(&[role("architect", "body")], &[command("migrate", "use {{arg}} via `agent-files/*.md`")])
            .unwrap();
        assert!(files.contains_key("harnesses/codex/.agents/skills/migrate/SKILL.md"));
        assert!(files.contains_key("harnesses/codex/.codex/agents/architect.toml"));
        // Skills go to the open standard tree, never `.codex/skills`.
        assert!(!files.keys().any(|k| k.contains(".codex/skills/")));
        let skill = files.get("harnesses/codex/.agents/skills/migrate/SKILL.md").unwrap();
        assert!(skill.contains("name: migrate\n"));
        // Shared neutral dialect: the crew glob is the open `.agents/agents`, not
        // the Codex-native `.codex/agents` (crew still install there; the skill's
        // glob is a descriptive pointer, orchestration goes through subagent_type).
        assert!(skill.contains(".agents/agents/*.md"));
        assert!(!skill.contains(".codex/agents/*.md"));
        assert!(skill.contains("$ARGUMENTS"));
        assert!(!skill.contains("{{arg}}"));
    }

    #[test]
    fn test_crew_is_toml_not_markdown() {
        let files = CodexAdapter.build(&[role("architect", "line one\nline two\n")], &[]).unwrap();
        let agent = files.get("harnesses/codex/.codex/agents/architect.toml").unwrap();
        assert!(agent.starts_with("name = \"architect\"\n"));
        assert!(agent.contains("description = \"desc\"\n"));
        assert!(agent.contains("developer_instructions = '''\nline one\nline two\n'''"));
        // Markdown frontmatter would be a parse error in a TOML file.
        assert!(!agent.contains("---"));
    }

    #[test]
    fn test_effort_is_emitted_as_model_reasoning_effort() {
        let mut r = role("architect", "body");
        r.effort = Some("high".to_string());
        let files = CodexAdapter.build(&[r], &[]).unwrap();
        let agent = files.get("harnesses/codex/.codex/agents/architect.toml").unwrap();
        assert!(agent.contains("model_reasoning_effort = \"high\"\n"), "{agent}");
    }

    #[test]
    fn test_no_bare_model_line_is_emitted() {
        // A model is never stamped (#205). Prefix check on `model =` so the
        // present `model_reasoning_effort =` does NOT false-positive.
        let mut r = role("architect", "body");
        r.effort = Some("high".to_string());
        let files = CodexAdapter.build(&[r], &[]).unwrap();
        let agent = files.get("harnesses/codex/.codex/agents/architect.toml").unwrap();
        assert!(!agent.lines().any(|l| l.trim_start().starts_with("model =")), "{agent}");
    }

    #[test]
    fn test_crew_body_is_rendered_into_the_codex_dialect() {
        let files = CodexAdapter
            .build(&[role("architect", "see `agent-files/*.md` and Harness-Session")], &[])
            .unwrap();
        let agent = files.get("harnesses/codex/.codex/agents/architect.toml").unwrap();
        assert!(agent.contains(".codex/agents/*.md"));
        assert!(agent.contains("Codex-Session"));
        assert!(!agent.contains("agent-files/"));
    }

    #[test]
    fn test_quotes_and_backslashes_survive() {
        // A persona containing `\$1` must reach disk byte-for-byte: a TOML basic
        // string would consume the backslash and change the instruction.
        let files = CodexAdapter
            .build(&[role("architect", "escape a literal as `\\$1` \"quoted\"\n")], &[])
            .unwrap();
        let agent = files.get("harnesses/codex/.codex/agents/architect.toml").unwrap();
        assert!(agent.contains("`\\$1`"));
        assert!(agent.contains("\"quoted\""));
    }

    #[test]
    fn test_literal_terminator_in_body_is_an_error() {
        let result = CodexAdapter.build(&[role("architect", "a ''' b")], &[]);
        assert!(result.is_err(), "a body containing ''' must fail the build, not corrupt the file");
    }
}
