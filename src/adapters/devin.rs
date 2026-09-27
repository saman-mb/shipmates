use super::Adapter;
use super::render::{
    CrewFormat, CrewLayout, DEVIN, emit_crew_files, emit_tool_files, render_command_body,
    yaml_scalar,
};
use crate::catalog::{CanonicalCommand, CanonicalRole, CanonicalTool};
use std::collections::HashMap;

/// Devin CLI / Devin Desktop (Cognition) — the product Windsurf became on
/// 2026-06-02. Crew at `.devin/agents/<name>.md`, commands and tools at
/// `.devin/skills/<name>/SKILL.md`, project rules at `.devin/rules/*.md`.
///
/// Why the native `.devin/` tree and not the shared `.agents/skills/` one, even
/// though Devin reads both: the eighteen commands are user-invoked only, and
/// the key that keeps them that way is `triggers: [user]`. The neutral shared
/// rendering emits the Agent Skills standard's `name`/`description` pair alone,
/// which drops the guard and hands the agent eighteen workflows to invoke on
/// its own. The same reasoning put Grok Build on its native tree.
///
/// Devin documents both trees first-party: `.agents/skills/<name>/SKILL.md` is
/// recommended for portability, `.devin/skills/` is native and takes
/// precedence, and `.windsurf/skills/` is retained as a legacy read path — so
/// pre-rename installs keep working while `update` moves them across.
///
/// The crew is real on this harness: Devin CLI spawns subagents and documents a
/// custom-profile file with `name`, `description`, `model`, `allowed-tools` and
/// `max-nesting` (docs.devin.ai/cli/subagents). No `model:` key is ever emitted
/// — which model a seat runs on is a runtime decision the orchestrator makes at
/// spawn (#205) — and Devin has no per-agent reasoning-effort field, so no
/// effort key is emitted either.
///
/// Crew tools are emitted as `allowed-tools` **only when every capability the
/// role declares has a documented Devin tool name**. On a subagent definition
/// that key is a restriction (unlike the skill-level key, which only
/// auto-approves), so guessing a name for a capability Devin does not document
/// would silently strip that capability from the seat. A role that declares a
/// web scope therefore gets no allowlist at all, and the gap is recorded in
/// `tools/harness_matrix.json` rather than papered over.
pub struct DevinAdapter;

/// Devin CLI's native tool vocabulary, as documented for `allowed-tools`:
/// `read`, `edit`, `grep`, `glob`, `exec` (docs.devin.ai/cli/extensibility/
/// skills/creating-skills, "Auto-Approved Tools"). Scope-based permissions
/// (`Read(glob)`, `Exec(prefix)`, `Fetch(pattern)`) are a different surface and
/// are not emitted here.
fn devin_tools(role: &CanonicalRole) -> anyhow::Result<Option<Vec<String>>> {
    if !role.tool_order.is_empty() {
        return Ok(Some(role.tool_order.clone()));
    }
    let mut out: Vec<String> = Vec::new();
    for capability in &role.capabilities {
        match capability.as_str() {
            "read" => {
                let scopes: Vec<&str> = if role.read_scopes.is_empty() {
                    vec!["read", "grep", "glob"]
                } else {
                    role.read_scopes.iter().map(String::as_str).collect()
                };
                for scope in scopes {
                    let tool = match scope {
                        "read" => "read",
                        "search" | "grep" => "grep",
                        "glob" => "glob",
                        // A read scope Devin has no name for is not a narrower
                        // allowlist, it is a removed capability.
                        _ => return Ok(None),
                    };
                    if !out.iter().any(|existing| existing == tool) {
                        out.push(tool.to_string());
                    }
                }
            }
            "edit" => {
                if !out.iter().any(|existing| existing == "edit") {
                    out.push("edit".to_string());
                }
            }
            "bash" => {
                if !out.iter().any(|existing| existing == "exec") {
                    out.push("exec".to_string());
                }
            }
            // Web search/fetch has no documented Devin tool name, and `agent`
            // is not a canonical crew capability. Either way the honest move is
            // to emit no allowlist rather than guess one.
            _ => return Ok(None),
        }
    }
    Ok(Some(out))
}

fn devin_serialize(role: &CanonicalRole, body: &str, tools: &[String]) -> anyhow::Result<String> {
    let mut content = String::new();
    content.push_str("---\n");
    // Bare, like every other adapter: install identity and receipt matching read
    // the bare form.
    content.push_str(&format!("name: {}\n", role.name));
    content.push_str(&format!(
        "description: {}\n",
        yaml_scalar(&role.description)
    ));
    if !tools.is_empty() {
        content.push_str("allowed-tools:\n");
        for tool in tools {
            content.push_str(&format!("  - {tool}\n"));
        }
    }
    content.push_str("---\n");
    content.push_str(body);
    Ok(content)
}

const CREW_FORMAT: CrewFormat = CrewFormat {
    file_suffix: ".md",
    dialect: &DEVIN,
    map_tools: devin_tools_ignore_absence,
    serialize: devin_serialize,
    layout: CrewLayout::Flat,
};

/// `CrewFormat::map_tools` yields a `Vec`, while the Devin mapper decides
/// between "these tools" and "no allowlist"; this collapses the `None` case to
/// an empty list, which `devin_serialize` renders as an absent key.
fn devin_tools_ignore_absence(role: &CanonicalRole) -> anyhow::Result<Vec<String>> {
    Ok(devin_tools(role)?.unwrap_or_default())
}

/// Render a command as a Devin skill: the standard's pair plus the vendor keys
/// Devin actually parses — `argument-hint` for the picker and `triggers: [user]`
/// for the user-invoked-only guard that this repo makes non-negotiable.
fn emit_devin_command_skills(
    base_dir: &str,
    commands: &[CanonicalCommand],
) -> anyhow::Result<HashMap<String, String>> {
    let mut files = HashMap::new();
    for command in commands {
        let mut content = String::new();
        content.push_str("---\n");
        content.push_str(&format!("name: {}\n", command.name));
        content.push_str(&format!(
            "description: {}\n",
            yaml_scalar(&command.description)
        ));
        if !command.argument_hint.is_empty() {
            content.push_str(&format!(
                "argument-hint: {}\n",
                yaml_scalar(&command.argument_hint)
            ));
        }
        if command.disable_model_invocation {
            content.push_str("triggers:\n  - user\n");
        } else {
            content.push_str("triggers:\n  - user\n  - model\n");
        }
        content.push_str("---\n");
        content.push_str(&render_command_body(command, &DEVIN)?);
        files.insert(
            format!("{}/skills/{}/SKILL.md", base_dir, command.name),
            content,
        );
    }
    Ok(files)
}

impl Adapter for DevinAdapter {
    fn base_dir(&self) -> &'static str {
        "harnesses/devin/.devin"
    }

    fn digest_root(&self) -> &'static str {
        self.container()
    }

    fn steering_dialect(&self) -> Option<&'static super::render::Dialect> {
        Some(&DEVIN)
    }

    fn steering_target(&self) -> Option<super::render::SteeringTarget> {
        Some(super::render::SteeringTarget {
            rel_path: ".devin/rules/shipmates-contributor.md",
            format: super::render::SteeringFormat::DevinRules {
                description: "Shipmates contributor checklists for crew, commands, tools, and site assets",
            },
        })
    }

    fn build(
        &self,
        roles: &[CanonicalRole],
        commands: &[CanonicalCommand],
    ) -> anyhow::Result<HashMap<String, String>> {
        let mut files = emit_crew_files(self.base_dir(), roles, &CREW_FORMAT)?;
        files.extend(emit_devin_command_skills(self.base_dir(), commands)?);
        Ok(files)
    }

    fn build_tools(&self, tools: &[CanonicalTool]) -> HashMap<String, String> {
        // Toolbox skills stay model-invoked. Devin's `triggers` key could hide
        // one from the `/` menu, but omitting it is also what keeps the tool
        // available to the model, so the tool ships typeable — recorded, not
        // faked.
        emit_tool_files(self.base_dir(), tools, &DEVIN, false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEVIN_TOOL_NAMES: &[&str] = &["read", "edit", "grep", "glob", "exec"];

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
            description: "Shipmates: take an issue to a reviewed PR".to_string(),
            argument_hint: "<issue>".to_string(),
            allowed_tools: String::new(),
            disable_model_invocation: true,
            arguments: vec![],
            narrative: "body".to_string(),
            invocation: String::new(),
            board: String::new(),
            source: std::path::PathBuf::from(""),
        }
    }

    #[test]
    fn test_devin_emits_crew_and_native_command_skills() {
        let files = DevinAdapter
            .build(&[role("sdet", &["read", "bash"])], &[command("shipmates-ship-issue")])
            .unwrap();
        assert!(files.contains_key("harnesses/devin/.devin/agents/sdet.md"));
        assert!(files.contains_key(
            "harnesses/devin/.devin/skills/shipmates-ship-issue/SKILL.md"
        ));
    }

    /// The guard is the whole reason this adapter is native rather than shared:
    /// losing it hands the agent eighteen workflows to invoke on its own.
    #[test]
    fn test_devin_commands_are_user_invoked_only() {
        let files = DevinAdapter
            .build(&[], &[command("shipmates-ship-issue")])
            .unwrap();
        let skill = &files["harnesses/devin/.devin/skills/shipmates-ship-issue/SKILL.md"];
        assert!(skill.contains("triggers:\n  - user\n"), "{skill}");
        assert!(!skill.contains("- model\n"), "{skill}");
    }

    #[test]
    fn test_devin_crew_tools_stay_inside_the_documented_vocabulary() {
        for capabilities in [&["read", "bash"][..], &["read", "edit", "bash"][..]] {
            let mapped = devin_tools(&role("role", capabilities)).unwrap().unwrap();
            assert!(!mapped.is_empty());
            for tool in &mapped {
                assert!(
                    DEVIN_TOOL_NAMES.contains(&tool.as_str()),
                    "undocumented Devin tool name {tool:?}"
                );
            }
        }
    }

    /// A capability Devin documents no tool name for must not narrow the seat.
    #[test]
    fn test_devin_omits_the_allowlist_rather_than_dropping_a_capability() {
        assert!(devin_tools(&role("product-manager", &["read", "bash", "web"]))
            .unwrap()
            .is_none());
        let files = DevinAdapter
            .build(&[role("product-manager", &["read", "bash", "web"])], &[])
            .unwrap();
        let agent = &files["harnesses/devin/.devin/agents/product-manager.md"];
        assert!(!agent.contains("allowed-tools"), "{agent}");
    }
}
