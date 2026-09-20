use super::Adapter;
use super::claude_code::claude_compatible_tools;
use super::render::{
    CrewFormat, CrewLayout, GROK_BUILD, emit_crew_files, emit_native_command_skills, emit_tool_files,
    yaml_scalar,
};
use crate::catalog::{CanonicalCommand, CanonicalRole, CanonicalTool};
use std::collections::HashMap;

/// Grok Build — the xAI `grok` CLI. Crew at `.grok/agents/<name>.md`, skills at
/// `.grok/skills/<name>/SKILL.md`, project rules at `.grok/rules/*.md`.
///
/// Grok is the one target that reads *all* of Claude Code's vendor frontmatter
/// keys and acts on them, so this adapter is the only one that ships the whole
/// command frontmatter natively instead of the strict Agent Skills pair — see
/// [`emit_native_command_skills`] for why the guard in particular must survive.
/// Its crew frontmatter is YAML with camelCase multi-word keys and takes
/// `name` + `description` as required, `tools` as a comma-separated scalar and
/// `effort` as one of `low|medium|high|xhigh|max`.
///
/// Crew tools are emitted **only** as names Grok's first-party alias table
/// resolves — the Claude-compatible vocabulary in
/// [`claude_compatible_tools`]. That is a least-privilege requirement, not a
/// naming preference: Grok's builder silently reverts an agent to its FULL
/// toolset (with a warn log and nothing else) when any entry in `tools` cannot
/// be resolved. An `Agent` entry is exactly such an unresolvable name, so no
/// role ever emits one — and no canonical crew role declares the agent
/// capability, so nothing is lost by it.
///
/// No `model:` key is ever emitted: which model a seat runs on is a runtime
/// decision the orchestrator makes at spawn (#205). `effort:` is the one static
/// per-role knob and is emitted when the role declares one.
pub struct GrokBuildAdapter;

fn grok_serialize(role: &CanonicalRole, body: &str, tools: &[String]) -> anyhow::Result<String> {
    let mut content = String::new();
    content.push_str("---\n");
    // Bare, like every other adapter: install identity and receipt matching read
    // the bare form.
    content.push_str(&format!("name: {}\n", role.name));
    content.push_str(&format!(
        "description: {}\n",
        yaml_scalar(&role.description)
    ));
    // A comma-separated scalar, not a YAML list. Omitting the key entirely would
    // hand the seat Grok's full default toolset, so the adapter never invents an
    // empty list either — `claude_compatible_tools` yields at least one tool for
    // every role that declares a capability.
    if !tools.is_empty() {
        content.push_str(&format!("tools: {}\n", tools.join(", ")));
    }
    if let Some(e) = &role.effort {
        content.push_str(&format!("effort: {e}\n"));
    }
    content.push_str("---\n");
    content.push_str(body);
    Ok(content)
}

const CREW_FORMAT: CrewFormat = CrewFormat {
    file_suffix: ".md",
    dialect: &GROK_BUILD,
    map_tools: claude_compatible_tools,
    serialize: grok_serialize,
    layout: CrewLayout::Flat,
};

impl Adapter for GrokBuildAdapter {
    fn base_dir(&self) -> &'static str {
        "harnesses/grok-build/.grok"
    }

    fn steering_dialect(&self) -> Option<&'static super::render::Dialect> {
        Some(&GROK_BUILD)
    }

    fn steering_target(&self) -> Option<super::render::SteeringTarget> {
        Some(super::render::SteeringTarget {
            rel_path: ".grok/rules/shipmates-contributor.md",
            format: super::render::SteeringFormat::PlainMarkdown,
        })
    }

    fn build(
        &self,
        roles: &[CanonicalRole],
        commands: &[CanonicalCommand],
    ) -> anyhow::Result<HashMap<String, String>> {
        // Crew, commands and tools all live under `.grok/`, so the default
        // `digest_root()` (the base dir) already covers the whole payload.
        let mut files = emit_crew_files(self.base_dir(), roles, &CREW_FORMAT)?;
        files.extend(emit_native_command_skills(
            self.base_dir(),
            commands,
            &GROK_BUILD,
        )?);
        Ok(files)
    }

    fn build_tools(&self, tools: &[CanonicalTool]) -> HashMap<String, String> {
        // `agent_only = false`: Grok has no key that hides a skill from the `/`
        // menu without also removing it from model invocation, so the tool stays
        // model-invoked but still technically typeable — recorded, not faked.
        emit_tool_files(self.base_dir(), tools, &GROK_BUILD, false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Grok's first-party alias table, as far as shipmates may emit from it. A
    /// name outside this set does not fail loudly at discovery — Grok warns and
    /// hands the agent its full toolset — so the assertion below is the only
    /// thing standing between a typo and a silent privilege escalation.
    const ALIAS_LEGAL: &[&str] = &[
        "Read",
        "Grep",
        "Glob",
        "Write",
        "Edit",
        "Bash",
        "WebSearch",
        "WebFetch",
    ];

    fn role(name: &str, capabilities: &[&str]) -> CanonicalRole {
        CanonicalRole {
            name: name.to_string(),
            description: "desc".to_string(),
            capabilities: capabilities.iter().map(|s| s.to_string()).collect(),
            writes: false,
            web_scopes: vec![],
            read_scopes: vec![],
            tool_order: vec![],
            effort: None,
            source: std::path::PathBuf::from(""),
            body: "body".to_string(),
        }
    }

    fn command(name: &str) -> CanonicalCommand {
        CanonicalCommand {
            name: name.to_string(),
            description: "desc".to_string(),
            argument_hint: "<issue>".to_string(),
            allowed_tools: "Bash".to_string(),
            disable_model_invocation: true,
            arguments: vec![],
            narrative: "resolve via `{{agents-glob}}`".to_string(),
            invocation: String::new(),
            board: String::new(),
            source: std::path::PathBuf::from(""),
        }
    }

    fn declared_tools(content: &str) -> Vec<String> {
        content
            .lines()
            .find_map(|line| line.strip_prefix("tools: "))
            .expect("a crew file must declare its tools")
            .split(", ")
            .map(str::to_string)
            .collect()
    }

    #[test]
    fn test_crew_lands_in_the_grok_agents_tree() {
        let files = GrokBuildAdapter
            .build(&[role("sdet", &["read"])], &[command("shipmates-issue")])
            .unwrap();
        assert!(
            files.contains_key("harnesses/grok-build/.grok/agents/sdet.md"),
            "crew must land at .grok/agents/<name>.md: {:?}",
            files.keys().collect::<Vec<_>>()
        );
        assert!(files.contains_key("harnesses/grok-build/.grok/skills/shipmates-issue/SKILL.md"));
    }

    #[test]
    fn test_command_keeps_the_guard_and_argument_hint() {
        let files = GrokBuildAdapter
            .build(&[], &[command("shipmates-issue")])
            .unwrap();
        let skill = &files["harnesses/grok-build/.grok/skills/shipmates-issue/SKILL.md"];
        assert!(skill.contains("name: shipmates-issue\n"), "{skill}");
        assert!(skill.contains("argument-hint: \"<issue>\"\n"), "{skill}");
        // The guard is the whole reason this target renders its own command
        // frontmatter instead of the strict Agent Skills pair.
        assert!(skill.contains("disable-model-invocation: true\n"), "{skill}");
        // `allowed-tools` is parsed by Grok but never applied as a permission
        // filter, so emitting it would imply a boundary that does not exist.
        assert!(!skill.contains("allowed-tools"), "{skill}");
        assert!(skill.contains(".grok/agents/*.md"), "{skill}");
        assert!(!skill.contains("{{agents-glob}}"), "{skill}");
    }

    #[test]
    fn test_command_without_the_guard_omits_it() {
        let mut c = command("shipmates-issue");
        c.disable_model_invocation = false;
        c.argument_hint = String::new();
        let files = GrokBuildAdapter.build(&[], &[c]).unwrap();
        let skill = &files["harnesses/grok-build/.grok/skills/shipmates-issue/SKILL.md"];
        assert!(!skill.contains("disable-model-invocation"), "{skill}");
        assert!(!skill.contains("argument-hint"), "{skill}");
    }

    #[test]
    fn test_tool_files_ship_with_their_bundled_assets() {
        let tool = CanonicalTool {
            name: "termgif".to_string(),
            description: "render a gif".to_string(),
            body: "instructions".to_string(),
            assets: vec![("termgif.py".to_string(), "print('hi')".to_string())],
            requires: vec![],
            source: std::path::PathBuf::from(""),
        };
        let files = GrokBuildAdapter.build_tools(&[tool]);
        let skill = &files["harnesses/grok-build/.grok/skills/termgif/SKILL.md"];
        assert!(skill.contains("name: termgif\n"), "{skill}");
        // Grok cannot hide a skill from the `/` menu without hiding it from the
        // model too, so no `user-invocable: false` is emitted here.
        assert!(!skill.contains("user-invocable"), "{skill}");
        assert_eq!(
            files["harnesses/grok-build/.grok/skills/termgif/termgif.py"],
            "print('hi')"
        );
    }

    #[test]
    fn test_steering_lands_in_the_project_rules_tree() {
        let files = GrokBuildAdapter.build_steering("steer");
        assert_eq!(
            files.keys().collect::<Vec<_>>(),
            vec!["harnesses/grok-build/.grok/rules/shipmates-contributor.md"]
        );
        assert_eq!(files["harnesses/grok-build/.grok/rules/shipmates-contributor.md"], "steer");
    }

    #[test]
    fn test_read_only_seat_gets_no_write_tools_and_no_model_line() {
        let mut architect = role("architect", &["read"]);
        architect.effort = Some("high".to_string());
        let files = GrokBuildAdapter.build(&[architect], &[]).unwrap();
        let content = &files["harnesses/grok-build/.grok/agents/architect.md"];
        assert!(content.contains("tools: Read, Grep, Glob\n"), "{content}");
        assert!(content.contains("effort: high\n"), "{content}");
        for forbidden in ["Write", "Edit", "Bash"] {
            assert!(
                !declared_tools(content).contains(&forbidden.to_string()),
                "a read-only seat must not hold {forbidden}: {content}"
            );
        }
        // A model is never stamped — it is a runtime decision (#205). Prefix
        // check so `effort:` cannot false-positive.
        assert!(
            !content.lines().any(|l| l.trim_start().starts_with("model:")),
            "{content}"
        );
    }

    #[test]
    fn test_writing_seat_gets_write_edit_and_bash() {
        let mut engineer = role("senior-engineer", &["read", "edit", "bash"]);
        engineer.effort = Some("medium".to_string());
        let files = GrokBuildAdapter.build(&[engineer], &[]).unwrap();
        let content = &files["harnesses/grok-build/.grok/agents/senior-engineer.md"];
        let tools = declared_tools(content);
        for expected in ["Write", "Edit", "Bash"] {
            assert!(
                tools.contains(&expected.to_string()),
                "senior-engineer must hold {expected}: {content}"
            );
        }
    }

    #[test]
    fn test_every_emitted_tool_name_is_alias_legal() {
        // `Agent` is the trap this test exists for: Grok cannot resolve it, and
        // one unresolvable entry makes the builder revert the agent to its full
        // toolset with only a warn log — the least-privilege failure this
        // adapter's tool mapping is shaped to avoid.
        let roles = vec![
            role("architect", &["read", "bash"]),
            role("senior-engineer", &["read", "edit", "bash"]),
            role("art-director", &["read", "bash", "web"]),
        ];
        let files = GrokBuildAdapter.build(&roles, &[]).unwrap();
        for (path, content) in &files {
            if !path.contains("/agents/") {
                continue;
            }
            let tools = declared_tools(content);
            assert!(!tools.is_empty(), "{path} must name at least one tool");
            for tool in tools {
                assert!(
                    ALIAS_LEGAL.contains(&tool.as_str()),
                    "{path} emits {tool:?}, which Grok cannot resolve from its alias table"
                );
            }
            assert!(!content.contains("Agent"), "{path} must never emit Agent");
        }
    }

    #[test]
    fn test_effort_is_omitted_when_the_role_declares_none() {
        let files = GrokBuildAdapter.build(&[role("sdet", &["read"])], &[]).unwrap();
        let content = &files["harnesses/grok-build/.grok/agents/sdet.md"];
        assert!(!content.contains("effort"), "{content}");
    }

    #[test]
    fn test_crew_body_is_rendered_into_the_grok_dialect() {
        let mut r = role("architect", &["read"]);
        r.body = "see {{agents-glob}} and {{session-key}}".to_string();
        let files = GrokBuildAdapter.build(&[r], &[]).unwrap();
        let content = &files["harnesses/grok-build/.grok/agents/architect.md"];
        assert!(content.contains(".grok/agents/*.md"), "{content}");
        assert!(content.contains("Grok-Session"), "{content}");
        assert!(!content.contains("{{"), "{content}");
    }
}
