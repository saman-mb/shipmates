use super::Adapter;
use super::render::{
    CrewFormat, CrewLayout, PI, emit_crew_files, emit_tool_files, render_command_body,
    yaml_scalar,
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
/// Commands and tools ship to Pi's native `.pi/skills/` tree (relocating to
/// `~/.pi/agent/skills/` on global install). Pi exposes skills as `/skill:<name>`,
/// so `cmd_prefix` is `"/skill:"`. Emitting native skills allows Pi to retain
/// `disable-model-invocation: true`, tool scoping via `allowed-tools`, and
/// `argument-hint`, which the neutral shared tree drops.
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
    // limited to names a bare pi install can resolve. `web_search` and
    // `fetch_content` come from the separate `pi-web-access` extension, which
    // this install does not provision, and a declared-but-missing tool aborts
    // the seat before a model turn (#583). The `web` capability is therefore
    // not emitted. `bash` remains for a fetch.
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
                    // Not emitted. See the comment on `tools_for`. A role that
                    // declares `web` keeps its other tools; bash covers a fetch.
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
    // #583: never declare a tool a bare pi install cannot resolve.
    tools.retain(|tool| tool != "web_search" && tool != "fetch_content");
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

fn map_pi_command_tools(allowed_tools: &str) -> Vec<String> {
    let mut out = Vec::new();
    for token in allowed_tools.split(',') {
        let trimmed = token.trim();
        if trimmed.is_empty() {
            continue;
        }
        let mapped = match trimmed {
            "Bash" => "bash",
            "Read" => "read",
            "Write" => "write",
            "Edit" => "edit",
            "Agent" => "subagent",
            "Grep" => "grep",
            "Glob" => "find",
            "WebSearch" => "web_search",
            "WebFetch" => "fetch_content",
            other => other,
        };
        if !out.iter().any(|t| t == mapped) {
            out.push(mapped.to_string());
        }
    }
    out
}

fn emit_pi_command_skills(
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
        if !command.allowed_tools.is_empty() {
            let tools = map_pi_command_tools(&command.allowed_tools);
            if !tools.is_empty() {
                content.push_str(&format!("allowed-tools: {}\n", tools.join(" ")));
            }
        }
        if command.disable_model_invocation {
            content.push_str("disable-model-invocation: true\n");
        }
        content.push_str("---\n");
        content.push_str(&render_command_body(command, &PI)?);
        files.insert(
            format!("{}/skills/{}/SKILL.md", base_dir, command.name),
            content,
        );
    }
    Ok(files)
}

const CREW_FORMAT: CrewFormat = CrewFormat {
    file_suffix: ".md",
    dialect: &PI,
    map_tools: tools_for,
    serialize,
    layout: CrewLayout::Flat,
};

fn strip_module_imports(source: &str) -> String {
    let mut out = String::new();
    for line in source.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("import ") && trimmed.ends_with(';') {
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

fn exported_names(source: &str) -> Vec<String> {
    let mut names = Vec::new();
    for line in source.lines() {
        let trimmed = line.trim();
        let rest = trimmed
            .strip_prefix("export async function ")
            .or_else(|| trimmed.strip_prefix("export function "))
            .or_else(|| trimmed.strip_prefix("export const "));
        let Some(rest) = rest else { continue };
        let name: String = rest
            .chars()
            .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '_')
            .collect();
        if !name.is_empty() {
            names.push(name);
        }
    }
    names
}

fn inline_sibling(source: &str) -> String {
    let body = strip_module_imports(source).replace("export ", "");
    let names = exported_names(source);
    format!(
        "const {{ {} }} = (() => {{\n{body}  return {{ {} }};\n}})();\n",
        names.join(", "),
        names.join(", ")
    )
}

fn bundle_pi_crew_extension() -> String {
    const POLICY: &str = include_str!("pi_crew/policy.mjs");
    const CAPACITY: &str = include_str!("pi_crew/capacity.mjs");
    const PROTOCOL: &str = include_str!("pi_crew/protocol.mjs");
    const TELEMETRY: &str = include_str!("pi_crew/telemetry.mjs");
    const BROKER: &str = include_str!("pi_crew/broker.mjs");
    const HOOKS: &str = include_str!("pi_crew/index.ts");
    let mut bundled = String::from(
        "import net from \"node:net\";\n\
import os, { freemem, homedir } from \"node:os\";\n\
import path, { dirname, join } from \"node:path\";\n\
import { unlinkSync, mkdirSync, readFileSync, renameSync, writeFileSync } from \"node:fs\";\n\
import { PassThrough } from \"node:stream\";\n\
import { spawn } from \"node:child_process\";\n\
import { randomUUID } from \"node:crypto\";\n\
import { fileURLToPath, pathToFileURL } from \"node:url\";\n\n",
    );
    bundled.push_str(POLICY);
    bundled.push('\n');
    bundled.push_str(&inline_sibling(CAPACITY));
    bundled.push_str(&inline_sibling(PROTOCOL));
    bundled.push_str(&inline_sibling(TELEMETRY));
    bundled.push_str(&inline_sibling(BROKER));
    bundled.push_str(&strip_module_imports(HOOKS));
    if bundled.contains("from \"./") || bundled.contains(".mjs\"") {
        panic!("pi crew extension still imports a sibling; emit must inline every module");
    }
    bundled
}

impl Adapter for PiAdapter {
    fn base_dir(&self) -> &'static str {
        "harnesses/pi/.pi"
    }

    fn digest_root(&self) -> &'static str {
        // Pi writes crew and skills into `.pi/` and steering into `.shipmates/`, so the
        // digest root is the container, not `base_dir`.
        self.container()
    }

    fn steering_dialect(&self) -> Option<&'static super::render::Dialect> {
        Some(&PI)
    }

    fn steering_target(&self) -> Option<super::render::SteeringTarget> {
        Some(super::render::SteeringTarget {
            rel_path: super::render::SHIPMATES_STEERING_REL,
            format: super::render::SteeringFormat::PlainMarkdown,
        })
    }

    fn build(
        &self,
        roles: &[CanonicalRole],
        commands: &[CanonicalCommand],
    ) -> anyhow::Result<HashMap<String, String>> {
        let mut files = emit_crew_files(self.base_dir(), roles, &CREW_FORMAT)?;
        files.extend(emit_pi_command_skills(self.base_dir(), commands)?);
        files.insert(
            format!("{}/extensions/shipmates-crew/index.ts", self.base_dir()),
            bundle_pi_crew_extension(),
        );
        Ok(files)
    }

    fn build_tools(&self, tools: &[CanonicalTool]) -> HashMap<String, String> {
        emit_tool_files(self.base_dir(), tools, &PI, false)
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
    fn test_pi_emits_one_inlined_crew_extension() {
        let files = PiAdapter.build(&[role("sdet", "Tests")], &[command()]).unwrap();
        let bundled = files
            .get("harnesses/pi/.pi/extensions/shipmates-crew/index.ts")
            .expect("pi payload must include the crew extension");
        assert!(bundled.contains("registerCommand(\"shipmates-crew\""));
        assert!(bundled.contains("export function decideSpawn"));
        assert!(bundled.contains("function startBroker"));
        assert!(bundled.contains("function probe"));
        assert!(!bundled.contains("policy.mjs"));
        assert!(!bundled.contains("from \"./"));
        assert!(!bundled.contains("login"));
        assert!(!files.keys().any(|path| path.contains("claude") && path.contains("shipmates-crew")));
        let check_path = std::env::temp_dir().join("shipmates-pi-crew-bundle-check.mjs");
        std::fs::write(&check_path, bundled).expect("write bundle check");
        if let Ok(node) = std::process::Command::new("node")
            .arg("--check")
            .arg(&check_path)
            .output()
        {
            if !node.status.success() {
                let _ = std::fs::remove_file(&check_path);
                panic!(
                    "bundled pi crew extension failed node --check:\n{}",
                    String::from_utf8_lossy(&node.stderr)
                );
            }
        }
        let mut import_path = check_path.display().to_string().replace('\\', "/");
        if !import_path.starts_with('/') {
            import_path.insert(0, '/');
        }
        let import_url = format!("file://{import_path}");
        if let Ok(imported) = std::process::Command::new("node")
            .arg("--input-type=module")
            .arg("-e")
            .arg(format!(
                "const mod = await import({import_url:?}); if (typeof mod.default !== 'function') process.exit(2);"
            ))
            .output()
        {
            if !imported.status.success() {
                let _ = std::fs::remove_file(&check_path);
                panic!(
                    "bundled pi crew extension failed to evaluate:\n{}{}",
                    String::from_utf8_lossy(&imported.stderr),
                    String::from_utf8_lossy(&imported.stdout)
                );
            }
        }
        let _ = std::fs::remove_file(&check_path);
    }

    #[test]
    fn test_pi_steering_target_is_shared_plain_markdown() {
        let target = PiAdapter.steering_target().expect("pi must emit steering");
        assert_eq!(
            target.rel_path,
            crate::adapters::render::SHIPMATES_STEERING_REL
        );
        assert!(matches!(
            target.format,
            crate::adapters::render::SteeringFormat::PlainMarkdown
        ));
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
                "harnesses/pi/.pi/agents/sdet.md",
                "harnesses/pi/.pi/extensions/shipmates-crew/index.ts",
                "harnesses/pi/.pi/skills/ship-fix-bug/SKILL.md",
            ]
        );
        // The shared crew tree belongs to Antigravity. Pi must never write there:
        // its `tools` shape and Antigravity's are mutually unreadable.
        assert!(
            !files.keys().any(|path| path.contains(".agents/agents/")),
            "pi must not emit crew into the shared .agents/agents tree"
        );
    }

    /// Native `.pi/skills` tree (#548, #553, #554): Pi command skills ship to
    /// `.pi/skills/`, retaining `allowed-tools`, `argument-hint`, and
    /// `disable-model-invocation: true`, with command references rendered with
    /// `/skill:` prefix.
    #[test]
    fn test_pi_command_skills_land_in_native_tree_with_scoping() {
        let mut cmd = command();
        cmd.argument_hint = "<issue>".to_string();
        cmd.allowed_tools = "Bash, Read, Edit, Agent".to_string();
        cmd.disable_model_invocation = true;
        cmd.narrative = "Run `/shipmates-ship-issue` to begin.".to_string();

        let files = PiAdapter.build(&[], &[cmd]).unwrap();
        let path = "harnesses/pi/.pi/skills/ship-fix-bug/SKILL.md";
        assert!(
            files.contains_key(path),
            "command skill must land at .pi/skills/: {:?}",
            files.keys().collect::<Vec<_>>()
        );
        let content = &files[path];
        assert!(content.contains("name: ship-fix-bug\n"), "{content}");
        assert!(content.contains("argument-hint: \"<issue>\"\n"), "{content}");
        assert!(
            content.contains("allowed-tools: bash read edit subagent\n"),
            "{content}"
        );
        assert!(
            content.contains("disable-model-invocation: true\n"),
            "{content}"
        );
        // Dialect check: command routing rewritten to /skill:
        assert!(
            content.contains("`/skill:shipmates-ship-issue`"),
            "{content}"
        );
    }

    #[test]
    fn test_pi_command_tools_mapping_covers_all_catalogue_tools() {
        let canonical = "Bash, Read, Write, Edit, Agent, Grep, Glob, WebSearch, WebFetch";
        let mapped = map_pi_command_tools(canonical);
        assert_eq!(
            mapped,
            vec![
                "bash",
                "read",
                "write",
                "edit",
                "subagent",
                "grep",
                "find",
                "web_search",
                "fetch_content",
            ]
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
                    "subagent"
                ]
                .contains(&tool),
                "unresolvable pi tool name {tool:?} in {content}"
            );
        }
    }

    #[test]
    fn test_pi_does_not_emit_web_tools_a_bare_install_cannot_resolve() {
        let mut manager = role("product-manager", "Accepts the work");
        manager.capabilities = vec!["read".to_string(), "bash".to_string(), "web".to_string()];
        manager.web_scopes = vec!["search".to_string(), "fetch".to_string()];
        let mut director = role("art-director", "Directs the picture");
        director.capabilities = vec!["read".to_string(), "bash".to_string(), "web".to_string()];
        director.web_scopes = vec!["search".to_string()];
        let files = PiAdapter.build(&[manager, director], &[]).unwrap();
        for name in ["product-manager", "art-director"] {
            let content = &files[&format!("harnesses/pi/.pi/agents/{name}.md")];
            assert!(!content.contains("web_search"), "{content}");
            assert!(!content.contains("fetch_content"), "{content}");
            assert!(content.contains("bash"), "{content}");
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
