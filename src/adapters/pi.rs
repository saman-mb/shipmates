use super::Adapter;
use super::render::{
    CrewFormat, CrewLayout, PI, emit_crew_files, emit_shared_skills, emit_shared_tool_skills, yaml_scalar,
};
use crate::catalog::{CanonicalCommand, CanonicalRole, CanonicalTool};
use std::collections::HashMap;

/// The Pi coding agent, whose crew mechanic comes from the third-party
/// `pi-subagents` extension (github.com/nicobailon/pi-subagents) — core pi
/// documents no declarative subagent schema.
///
/// Pi discovers agents from `.pi/agents/**/*.md` and `~/.pi/agent/agents/**/*.md`, and still reads
/// the legacy `.agents/**` tree for compatibility — the path Antigravity's crew installs to. So the
/// crew ship to `.pi/agents/`: they deliberately do NOT go to the shared `.agents/agents/` tree,
/// because the two harnesses need incompatible `tools` shapes (Antigravity a YAML list, pi a
/// comma-separated scalar), and one file cannot serve both. A shipmates install of Antigravity used
/// to hand pi a crew whose tool names pi could not resolve.
///
/// Scope caveat, and it is a real limit rather than a detail: `.pi/agents/` is a *project-scope*
/// path, so it wins within whichever directory pi resolves as its project root — the nearest ancestor
/// carrying `.pi/` or `.agents/`. A project-local install therefore resolves correctly. A `--global`
/// install lands in the home directory, which is that root only when no ancestor between the working
/// directory and `~` carries `.pi/` or `.agents/`. And pi's user scope cannot compensate:
/// `~/.pi/agent/agents/` is loaded before legacy `~/.agents/`, so no user-scope path outranks a
/// foreign `~/.agents/agents/`. Tracked separately; see `tools/capability_registry.json`.
///
/// Skills and tools are unaffected: they stay on the shared neutral
/// `.agents/skills/` tree, byte-identical with codex / antigravity /
/// github-copilot / cursor.
///
/// See https://github.com/earendil-works/pi and the `pi-subagents` package's
/// `Agents and chains` documentation for the discovery paths and the frontmatter
/// schema.
pub struct PiAdapter;

fn scope_tool(scope: &str) -> Option<&'static str> {
    match scope {
        "read" => Some("read"),
        "write" => Some("write"),
        "edit" => Some("edit"),
        "bash" => Some("bash"),
        "search" => Some("grep"),
        "glob" => Some("find"),
        "web-search" => Some("web_search"),
        "web-fetch" => Some("fetch_content"),
        "agent" => Some("subagent"),
        _ => None,
    }
}

fn tools_for(role: &CanonicalRole) -> anyhow::Result<Vec<String>> {
    // Pi's tool vocabulary is its own lowercase set (read, grep, find, ls, bash,
    // edit, write, web_search, fetch_content, subagent). A name pi cannot
    // resolve is not ignored — the list is passed straight through to pi's own
    // `--tools` and filtered per name — so a list pi can match *none* of leaves
    // the seat with no tools at all, silently, with no error at discovery time.
    // That is exactly what #437 was: Antigravity's YAML-list shape parsed into a
    // single unmatchable token, so nothing matched. A partially-unmatched list
    // would merely be narrower than intended, which is why this vocabulary stays
    // limited to names pi actually ships (`web_search` / `fetch_content` come
    // from the separate `pi-web-access` extension — recorded in the registry).
    let mut tools = if !role.tool_order.is_empty() {
        let mut ordered = Vec::new();
        for scope in &role.tool_order {
            let tool = scope_tool(scope)
                .ok_or_else(|| anyhow::anyhow!("unknown tool scope {scope:?}"))?
                .to_string();
            if !ordered.contains(&tool) {
                ordered.push(tool);
            }
        }
        ordered
    } else {
        let mut out = Vec::new();
        for cap in &role.capabilities {
            match cap.as_str() {
                "read" => {
                    let scopes = if role.read_scopes.is_empty() {
                        vec!["read", "search", "glob"]
                    } else {
                        role.read_scopes.iter().map(String::as_str).collect()
                    };
                    for scope in scopes {
                        out.push(
                            scope_tool(scope)
                                .ok_or_else(|| anyhow::anyhow!("unknown read scope {scope:?}"))?
                                .to_string(),
                        );
                    }
                }
                "edit" => out.extend(["write", "edit"].map(str::to_string)),
                "bash" => out.push("bash".to_string()),
                "web" => {
                    let scopes: Vec<String> = if role.web_scopes.is_empty() {
                        vec!["web-search".to_string(), "web-fetch".to_string()]
                    } else {
                        role.web_scopes
                            .iter()
                            .map(|scope| format!("web-{scope}"))
                            .collect()
                    };
                    for scope in scopes {
                        out.push(
                            scope_tool(&scope)
                                .ok_or_else(|| anyhow::anyhow!("unknown web scope {scope:?}"))?
                                .to_string(),
                        );
                    }
                }
                "agent" => out.push("subagent".to_string()),
                other => anyhow::bail!("unmapped capability {other:?} for pi"),
            }
        }
        // Order-preserving dedupe, so an explicit `tool-order` intent survives
        // and a scoped write/edit pair does not name the same tool twice.
        let mut seen = Vec::new();
        for tool in out {
            if !seen.contains(&tool) {
                seen.push(tool);
            }
        }
        seen
    };
    // Floor, and it is a privilege floor rather than a convenience one: pi
    // pushes `--tools` only when the list is non-empty, so an empty list would
    // hand the seat pi's FULL default toolset instead of none. Never emit an
    // empty tool list — `read` is the narrowest useful seat.
    if tools.is_empty() {
        tools.push("read".to_string());
    }
    Ok(tools)
}

fn serialize(role: &CanonicalRole, body: &str, tools: &[String]) -> anyhow::Result<String> {
    // pi's frontmatter reader is a line-based parser, not a YAML loader. Two of
    // its behaviours constrain this serializer:
    //   1. `tools` is read as a comma-separated scalar (`tools.split(",")`), NOT
    //      as a YAML list — a list is read as one unmatchable name.
    //   2. a key with an EMPTY value opens a block and swallows the following
    //      more-indented lines, so an empty `tools:` would eat the rest of the
    //      frontmatter.
    // Hence the line below is always emitted and is never empty (`tools_for`
    // guarantees at least one tool), and no value is ever blank.
    if role.description.trim().is_empty() {
        anyhow::bail!(
            "role {:?} has an empty description; pi silently drops an agent whose \
             frontmatter carries no description, so it would install and never resolve",
            role.name
        );
    }
    let mut content = String::new();
    content.push_str("---\n");
    // Bare, like every other adapter: install identity and receipt matching read
    // the bare form.
    content.push_str(&format!("name: {}\n", role.name));
    content.push_str(&format!(
        "description: {}\n",
        yaml_scalar(&role.description)
    ));
    content.push_str(&format!("tools: {}\n", tools.join(", ")));
    if let Some(e) = &role.effort {
        // pi appends this as a `:<level>` model suffix at spawn. The accepted set
        // is first-party: off, minimal, low, medium, high, xhigh — so the neutral
        // `low|medium|high` passes through unclamped and unmapped.
        content.push_str(&format!("thinking: {e}\n"));
    }
    // Explicit rather than relied upon, and load-bearing:
    // `inheritProjectContext` defaults to `true` only for pi's own `delegate`
    // agent, so every other agent would otherwise spawn without the project's
    // AGENTS.md — which these role bodies explicitly instruct it to read.
    content.push_str("systemPromptMode: replace\n");
    content.push_str("inheritProjectContext: true\n");
    // Explicit, and pi's own default is already `false`: a crew seat sees the
    // project context but not pi's installed skills — the same choice pi's
    // builtin agents make, and the least-privilege one.
    content.push_str("inheritSkills: false\n");
    content.push_str("---\n");
    content.push_str(body);
    Ok(content)
}

const CREW_FORMAT: CrewFormat = CrewFormat {
    file_suffix: ".md",
    dialect: &PI,
    map_tools: tools_for,
    serialize,
    layout: CrewLayout::Flat,
};

impl Adapter for PiAdapter {
    fn base_dir(&self) -> &'static str {
        "harnesses/pi/.agents"
    }

    fn digest_root(&self) -> &'static str {
        // Pi writes into two dotdirs (`.pi/agents/` for the crew, `.agents/`
        // for skills and tools), so the digest root is the container, not
        // `base_dir` — otherwise the crew tree would fall outside the digest.
        self.container()
    }

    fn steering_dialect(&self) -> Option<&'static super::render::Dialect> {
        Some(&PI)
    }

    fn steering_target(&self) -> Option<super::render::SteeringTarget> {
        None
    }

    fn build(
        &self,
        roles: &[CanonicalRole],
        commands: &[CanonicalCommand],
    ) -> anyhow::Result<HashMap<String, String>> {
        let mut files = emit_crew_files(&format!("{}/.pi", self.container()), roles, &CREW_FORMAT)?;
        files.extend(emit_shared_skills(self.container(), commands)?);
        Ok(files)
    }

    fn build_tools(&self, tools: &[CanonicalTool]) -> HashMap<String, String> {
        emit_shared_tool_skills(self.container(), tools)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn role(name: &str, description: &str) -> CanonicalRole {
        CanonicalRole {
            name: name.to_string(),
            description: description.to_string(),
            capabilities: vec!["read".to_string(), "bash".to_string()],
            writes: false,
            web_scopes: vec![],
            read_scopes: vec![],
            tool_order: vec![],
            effort: None,
            source: std::path::PathBuf::from(""),
            body: "reproduce first".to_string(),
        }
    }

    fn command() -> CanonicalCommand {
        CanonicalCommand {
            name: "ship-fix-bug".to_string(),
            description: "desc".to_string(),
            argument_hint: String::new(),
            allowed_tools: String::new(),
            disable_model_invocation: true,
            arguments: vec![],
            narrative: "reproduce first".to_string(),
            invocation: String::new(),
            board: String::new(),
            source: std::path::PathBuf::from(""),
        }
    }

    #[test]
    fn test_pi_adapter_emits_crew_outside_the_shared_agent_tree() {
        let files = PiAdapter.build(&[role("sdet", "QA")], &[command()]).unwrap();
        assert_eq!(
            {
                let mut keys = files.keys().collect::<Vec<_>>();
                keys.sort();
                keys
            },
            vec![
                "harnesses/pi/.agents/skills/ship-fix-bug/SKILL.md",
                "harnesses/pi/.pi/agents/sdet.md",
            ]
        );
        // The shared crew tree belongs to Antigravity. Pi must never write there:
        // its `tools` shape and Antigravity's are mutually unreadable.
        assert!(
            !files.keys().any(|path| path.contains(".agents/agents/")),
            "pi must not emit crew into the shared .agents/agents tree"
        );
    }

    #[test]
    fn test_pi_tools_are_a_comma_scalar_that_pi_can_resolve() {
        let files = PiAdapter.build(&[role("sdet", "QA")], &[]).unwrap();
        let content = &files["harnesses/pi/.pi/agents/sdet.md"];
        // A YAML list here is the bug this adapter exists to avoid: pi's parser
        // reads the whole block as one unmatchable tool name.
        assert!(
            content.contains("tools: read, grep, find, bash\n"),
            "tools must be a single comma-separated line: {content}"
        );
        assert!(!content.contains("tools:\n"), "{content}");
        let declared = content
            .lines()
            .find_map(|line| line.strip_prefix("tools: "))
            .expect("tools line present");
        assert!(!declared.trim().is_empty(), "{content}");
        for tool in declared.split(", ") {
            assert!(
                [
                    "read",
                    "grep",
                    "find",
                    "ls",
                    "bash",
                    "edit",
                    "write",
                    "web_search",
                    "fetch_content",
                    "subagent"
                ]
                .contains(&tool),
                "unresolvable pi tool name {tool:?} in {content}"
            );
        }
    }

    /// The frontmatter text between the opening and closing `---` delimiters.
    /// Written as a helper because the earlier version of this file used
    /// `content.split("---\n").next()`, which is always `""` — the assertion
    /// below silently iterated nothing and could never fail.
    fn frontmatter_of(content: &str) -> &str {
        let rest = content
            .strip_prefix("---\n")
            .expect("frontmatter must open the file");
        let end = rest.find("\n---").expect("frontmatter must close");
        &rest[..end]
    }

    #[test]
    fn test_pi_frontmatter_is_never_empty_valued() {
        // Every other key is a literal; `tools` is the one that could go blank,
        // and in pi's line-based reader a blank value opens a block that swallows
        // the following more-indented lines.
        let files = PiAdapter
            .build(&[role("architect", "Design reviewer")], &[])
            .unwrap();
        let content = &files["harnesses/pi/.pi/agents/architect.md"];
        let frontmatter = frontmatter_of(content);
        assert!(
            !frontmatter.is_empty(),
            "the extractor must actually yield the block, or this test proves nothing: {content}"
        );
        let mut keys = Vec::new();
        for line in frontmatter.lines() {
            let (key, value) = line
                .split_once(':')
                .unwrap_or_else(|| panic!("frontmatter line is not `key: value`: {line:?}"));
            assert!(
                !key.starts_with(' '),
                "frontmatter lines must not be indented: {line:?}"
            );
            assert!(
                !value.trim().is_empty(),
                "frontmatter key {key:?} has an empty value, which pi reads as a block opener: {content}"
            );
            keys.push(key.to_string());
        }
        for expected in ["name", "description", "tools", "systemPromptMode"] {
            assert!(keys.iter().any(|k| k == expected), "missing {expected}: {content}");
        }
        assert!(!content.contains("model:"), "no model may be baked in: {content}");
    }

    #[test]
    fn test_pi_effort_is_emitted_as_thinking_only_when_set() {
        let mut r = role("architect", "Design reviewer");
        r.effort = Some("high".to_string());
        let files = PiAdapter.build(&[r], &[]).unwrap();
        assert!(
            files["harnesses/pi/.pi/agents/architect.md"].contains("thinking: high\n")
        );
        let files = PiAdapter.build(&[role("architect", "Design reviewer")], &[]).unwrap();
        assert!(!files["harnesses/pi/.pi/agents/architect.md"].contains("thinking"));
    }

    #[test]
    fn test_pi_empty_description_fails_closed() {
        let err = PiAdapter.build(&[role("sdet", "  ")], &[]).unwrap_err();
        assert!(
            err.to_string().contains("empty description"),
            "an empty description must fail loudly, not install an unreachable agent: {err}"
        );
    }

    #[test]
    fn test_pi_tools_preserve_tool_order_while_deduping() {
        let mut r = role("ordered", "Ordered");
        r.tool_order = vec![
            "bash".to_string(),
            "read".to_string(),
            "bash".to_string(),
            "edit".to_string(),
        ];
        let files = PiAdapter.build(&[r], &[]).unwrap();
        assert!(
            files["harnesses/pi/.pi/agents/ordered.md"].contains("tools: bash, read, edit\n")
        );
    }

    #[test]
    fn test_pi_never_emits_an_empty_tool_list() {
        // A role with no capabilities must still name a tool: pi grants its full
        // default toolset when `--tools` is absent.
        let mut r = role("bare", "Bare");
        r.capabilities.clear();
        let files = PiAdapter.build(&[r], &[]).unwrap();
        assert!(files["harnesses/pi/.pi/agents/bare.md"].contains("tools: read\n"));
    }
}
