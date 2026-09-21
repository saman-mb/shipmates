use crate::catalog::{CanonicalCommand, CanonicalRole, CanonicalTool, validate_role_name};
use regex::Regex;
use std::collections::HashMap;

/// Per-harness dialect settings shared by every adapter.
///
/// The canonical `commands/*.md` and `crew/*.md` bodies are harness-neutral
/// prose. They use explicit exporter tokens (`{{agents-glob}}`, `{{session-key}}`,
/// `{{project-instructions}}`), spawn roles with `{{role:name}}`, and reference
/// command arguments as `{{name}}`. Each harness resolves those tokens into
/// its own dialect — where its agents live, what its session metadata is
/// called, which project-instructions file it reads, how a role is spawned.
///
/// Adapters stay thin by declaring a `Dialect` and letting `render_body` do
/// the substitution.
pub struct Dialect {
    pub agents_glob: &'static str,
    pub session_key: &'static str,
    pub instructions_primary: &'static str,
    pub instructions_fallback: &'static str,
    pub general_purpose: &'static str,
    pub planner: &'static str,
    pub args_token: &'static str,
}

const SENTINEL: &str = "\u{00A7}agents-instructions";
const COMMAND_PREAMBLE_MARKER: &str = "<!-- shipmates:command-preamble -->";
const ACCEPTANCE_BOARD_MARKER: &str = "<!-- shipmates:acceptance-board -->";
const EPIC_INTEGRATION_BOARD_MARKER: &str = "<!-- shipmates:epic-integration-board -->";
const SUBAGENT_PREAMBLE_MARKER: &str = "<!-- shipmates:subagent-preamble -->";
const WHY_MERGE_PR_MARKER: &str = "<!-- shipmates:why-merge-pr -->";
const MODEL_ROUTING_MARKER: &str = "<!-- shipmates:model-routing -->";
const COST_DOCTRINE: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/docs/COST.md"));

fn render_token(text: &str, token: &str, value: &str) -> String {
    text.replace(token, value)
}

fn doctrine_section(start: &str, end: &str) -> &'static str {
    let start = COST_DOCTRINE
        .find(start)
        .expect("cost doctrine start marker missing")
        + start.len();
    let end = COST_DOCTRINE[start..]
        .find(end)
        .expect("cost doctrine end marker missing")
        + start;
    COST_DOCTRINE[start..end].trim()
}

fn command_preamble() -> &'static str {
    doctrine_section("<!-- command-preamble:start -->", "<!-- command-preamble:end -->")
}

fn acceptance_board() -> &'static str {
    doctrine_section("<!-- acceptance-board:start -->", "<!-- acceptance-board:end -->")
}

fn epic_integration_board() -> &'static str {
    doctrine_section(
        "<!-- epic-integration-board:start -->",
        "<!-- epic-integration-board:end -->",
    )
}

fn subagent_preamble() -> &'static str {
    doctrine_section("<!-- subagent-preamble:start -->", "<!-- subagent-preamble:end -->")
}

/// The one canonical statement of pool discovery, the resolution order and the
/// audit line. The marker that expands it lives inside the shared cost-discipline
/// preamble, so every rendered command carries it and none can drift from the
/// doctrine — no command opts in, and none can be left out.
fn model_routing() -> &'static str {
    doctrine_section("<!-- model-routing:start -->", "<!-- model-routing:end -->")
}

fn why_merge_pr() -> &'static str {
    doctrine_section("<!-- why-merge-pr:start -->", "<!-- why-merge-pr:end -->")
}

/// Resolve explicit repo-instructions tokens in neutral prose.
///
/// Primary and fallback remain separate, so literal filenames in prose are not
/// rewritten accidentally.
fn render_instructions(text: &str, primary: &str, fallback: &str) -> String {
    let mut out = text.to_string();
    out = render_token(&out, "{{project-instructions}}", primary);
    render_token(&out, "{{project-instructions-fallback}}", fallback)
}

/// Render a harness-neutral command body into a harness's dialect.
///
/// Order matters, and only here: the command preamble is expanded first, and the
/// model-routing ruleset is expanded last, because the preamble's own text carries
/// the ruleset's marker. Reordering those two substitutions silently drops the
/// ruleset from every command — `test_every_command_carries_the_model_routing_ruleset`
/// and the preamble unit test both fail loudly if it happens, so treat this list
/// as ordered rather than as a set.
pub fn render_body(text: &str, d: &Dialect) -> String {
    let mut out = text.replace(COMMAND_PREAMBLE_MARKER, command_preamble());
    out = out.replace(ACCEPTANCE_BOARD_MARKER, acceptance_board());
    out = out.replace(EPIC_INTEGRATION_BOARD_MARKER, epic_integration_board());
    out = out.replace(SUBAGENT_PREAMBLE_MARKER, subagent_preamble());
    out = out.replace(WHY_MERGE_PR_MARKER, why_merge_pr());
    out = out.replace(MODEL_ROUTING_MARKER, model_routing());
    out = render_instructions(&out, d.instructions_primary, d.instructions_fallback);
    out = render_token(&out, "{{agents-glob}}", &format!("{}/*.md", d.agents_glob));
    out = render_token(&out, "{{session-key}}", d.session_key);
    out = render_token(&out, "{{general-purpose}}", d.general_purpose);
    out = render_token(
        &out,
        "{{role:planner}}",
        &format!("subagent_type: {}", d.planner),
    );
    out = render_token(&out, "{{planner-agent}}", d.planner);
    out = render_token(
        &out,
        "{{role:senior-engineer}}",
        "subagent_type: senior-engineer",
    );
    out = render_token(&out, "{{role:sdet}}", "subagent_type: sdet");
    out = render_token(&out, "{{role-reference}}", "`subagent_type`");
    out
}

/// Render a role body through the same neutral-to-harness rules as commands,
/// including the stable return preamble shared by every subagent.
pub(crate) fn render_role_body(text: &str, d: &Dialect) -> String {
    render_body(text, d)
}

/// Replace every `{{name}}` argument placeholder with the harness's token.
fn render_args(text: &str, token: &str) -> anyhow::Result<String> {
    let re = Regex::new(r"\{\{[a-z][a-z0-9_-]*\}\}").expect("static regex");
    let mut names = Vec::new();
    for matched in re.find_iter(text) {
        let name = &matched.as_str()[2..matched.as_str().len() - 2];
        if !names.iter().any(|seen| seen == name) {
            names.push(name.to_string());
        }
    }
    if names.len() > 1 {
        anyhow::bail!(
            "command narrative uses multiple arguments ({}) but target accepts one argument token",
            names.join(", ")
        );
    }
    // `$A` is regex replacement syntax (a named-group reference); double it so
    // the token's `$` survives literally — `$ARGUMENTS` must not vanish.
    let escaped = token.replace('$', "$$");
    Ok(re.replace_all(text, escaped.as_str()).into_owned())
}

pub fn render_command_body(command: &CanonicalCommand, d: &Dialect) -> anyhow::Result<String> {
    render_args(&render_body(&command.narrative, d), d.args_token)
}

/// How a harness wraps rendered steering prose at its install path.
pub enum SteeringFormat {
    PlainMarkdown,
    CursorMdc { description: &'static str },
    CopilotInstructions { apply_to: &'static str },
    /// Devin rule files (`.devin/rules/*.md`) take the Windsurf rule
    /// frontmatter, so the steering file declares `trigger: always_on`
    /// explicitly rather than relying on an undocumented default.
    DevinRules { description: &'static str },
}

pub struct SteeringTarget {
    pub rel_path: &'static str,
    pub format: SteeringFormat,
}

/// Fallback steering path when a harness has no documented auto-load rules surface.
pub const SHIPMATES_STEERING_REL: &str = ".shipmates/contributor-steering.md";

/// Render contributor steering to a harness-native modular path (rules file,
/// instructions file, or `.shipmates/contributor-steering.md` fallback).
pub fn emit_steering_at(
    container: &str,
    target: &SteeringTarget,
    dialect: &Dialect,
    body: &str,
) -> HashMap<String, String> {
    let rendered =
        render_instructions(body, dialect.instructions_primary, dialect.instructions_fallback);
    let content = match target.format {
        SteeringFormat::PlainMarkdown => rendered,
        SteeringFormat::CursorMdc { description } => {
            format!(
                "---\ndescription: {}\nalwaysApply: true\n---\n{rendered}",
                yaml_scalar(description)
            )
        }
        SteeringFormat::CopilotInstructions { apply_to } => {
            format!("---\napplyTo: {}\n---\n{rendered}", yaml_scalar(apply_to))
        }
        SteeringFormat::DevinRules { description } => {
            format!(
                "---\ndescription: {}\ntrigger: always_on\n---\n{rendered}",
                yaml_scalar(description)
            )
        }
    };
    let path = format!("{}/{}", container, target.rel_path);
    HashMap::from([(path, content)])
}

/// Claude Code's dialect.
pub const CLAUDE_CODE: Dialect = Dialect {
    agents_glob: ".claude/agents",
    session_key: "Claude-Session",
    instructions_primary: "CLAUDE.md",
    instructions_fallback: "AGENTS.md",
    general_purpose: "general-purpose",
    planner: "Plan",
    args_token: "$ARGUMENTS",
};

/// opencode's dialect.
pub const OPENCODE: Dialect = Dialect {
    agents_glob: ".opencode/agents",
    session_key: "Opencode-Session",
    instructions_primary: "AGENTS.md",
    instructions_fallback: "CLAUDE.md",
    general_purpose: "general",
    planner: "architect",
    args_token: "$ARGUMENTS",
};

/// The neutral dialect for the shared open Agent Skills tree (`.agents/skills/`).
///
/// Codex, Antigravity, Cursor and Copilot all read skills from this one
/// open-standard location. A *per-harness* rendering would make each write
/// different bytes to the same install path — whichever ran last would win, and
/// the rest would silently get the wrong crew references (see the collision the
/// `--harness all` install exhibited before this existed). So the shared tree is
/// rendered ONCE, neutrally, and every one of those harnesses emits byte-identical
/// files: one source of truth, no duplication, no collision.
///
/// The values are the common denominator that resolves on all four:
/// - `agents_glob` = `.agents/agents` — the open-standard sibling of
///   `.agents/skills`, and Antigravity's real crew location. For Codex/Copilot,
///   whose crew live in their own trees, this is a descriptive pointer only;
///   orchestration goes through `subagent_type`, which each harness resolves
///   against its own registered crew regardless of the glob text.
/// - `planner` = `architect` — a real shipped crew member, so it resolves
///   wherever crew are installed. The old per-harness `planner` named a
///   subagent that ships nowhere; `architect` is what Antigravity already used
///   and what makes the Planner stage resolvable everywhere.
/// - `session_key` = `Agent-Session` — a neutral commit-trailer name.
pub const AGENT_SKILLS: Dialect = Dialect {
    agents_glob: ".agents/agents",
    session_key: "Agent-Session",
    instructions_primary: "AGENTS.md",
    instructions_fallback: "CLAUDE.md",
    general_purpose: "general-purpose",
    planner: "architect",
    args_token: "$ARGUMENTS",
};

/// Antigravity's project-instruction fallback is `GEMINI.md`, not the
/// Claude/Codex fallback used by the shared Agent Skills tree.
pub const ANTIGRAVITY: Dialect = Dialect {
    agents_glob: ".agents/agents",
    session_key: "Agent-Session",
    instructions_primary: "AGENTS.md",
    instructions_fallback: "GEMINI.md",
    general_purpose: "general-purpose",
    planner: "architect",
    args_token: "$ARGUMENTS",
};

// Antigravity (`agy`, the retired Gemini CLI's successor) reads skills from the
// shared `.agents/skills/` tree, while its crew uses the target dialect above.

/// Codex CLI's dialect.
pub const CODEX: Dialect = Dialect {
    agents_glob: ".codex/agents",
    session_key: "Codex-Session",
    instructions_primary: "AGENTS.md",
    instructions_fallback: "CLAUDE.md",
    general_purpose: "general-purpose",
    planner: "planner",
    args_token: "$ARGUMENTS",
};

/// Pi's crew dialect.
///
/// Pi's crew mechanic comes from the third-party `pi-subagents` extension
/// (github.com/nicobailon/pi-subagents) — core pi documents no declarative
/// subagent schema. It reads agent
/// definitions from `.pi/agents/**/*.md` as its canonical project scope and from
/// `~/.pi/agent/agents/**/*.md` as its canonical user scope. Pi also still reads
/// the legacy `.agents/**` tree "for compatibility", and that is the path
/// shipmates installs Antigravity's crew to.
///
/// Pi's *commands and tools* stay on the shared neutral `.agents/skills/` tree
/// so a sibling harness in the same repo is one copy. A global install omits
/// those skills (#513) because Pi also loads `~/.pi/agent/skills` in the same
/// session. Note what that implies: pi's command skills are rendered through
/// `AGENT_SKILLS`, so the command-only tokens below (`agents_glob`,
/// `session_key`, `general_purpose`, `planner`, `args_token`) reach *no emitted
/// pi byte* — only `instructions_primary`/`instructions_fallback` do, through the
/// crew bodies. They are set to pi's real values anyway, so the dialect is
/// correct;
/// `general_purpose` is `worker` because pi ships that builtin and would resolve
/// the neutral `general-purpose` to nothing. And `.pi/agents/` wins only within
/// the directory pi resolves as its project root — see `pi.rs` for the scope
/// caveat.
pub const PI: Dialect = Dialect {
    agents_glob: ".pi/agents",
    session_key: "Pi-Session",
    instructions_primary: "AGENTS.md",
    instructions_fallback: "CLAUDE.md",
    general_purpose: "worker",
    planner: "architect",
    args_token: "$ARGUMENTS",
};

// Cursor has no crew mechanic here, so it renders no personas of its own; its
// commands ship to the shared `.agents/skills/` tree via AGENT_SKILLS. (Cursor
// reads `.agents/skills/` natively, first-party — see cursor.rs.)

/// GitHub Copilot CLI's dialect.
pub const GITHUB_COPILOT: Dialect = Dialect {
    agents_glob: ".github/agents",
    session_key: "Copilot-Session",
    instructions_primary: "AGENTS.md",
    instructions_fallback: "CLAUDE.md",
    general_purpose: "general-purpose",
    planner: "planner",
    args_token: "$ARGUMENTS",
};

/// Grok Build (the xAI `grok` CLI)'s dialect.
///
/// Grok keeps both resources in one dotdir: crew at `.grok/agents/<name>.md` and
/// skills at `.grok/skills/<name>/SKILL.md`, with contributor steering as a rule
/// file under `.grok/rules/`. It resolves `.grok/` ahead of the shared `.agents/`
/// and `.claude/` trees, which is why its commands ship natively (see
/// [`emit_native_command_skills`]) rather than into the shared tree.
pub const GROK_BUILD: Dialect = Dialect {
    agents_glob: ".grok/agents",
    session_key: "Grok-Session",
    instructions_primary: "AGENTS.md",
    instructions_fallback: "CLAUDE.md",
    general_purpose: "general-purpose",
    planner: "plan",
    args_token: "$ARGUMENTS",
};

/// Devin CLI / Devin Desktop's dialect. Devin is what Windsurf became on
/// 2026-06-02; `.windsurf/` remains a legacy read path in the product, and the
/// installer keeps sweeping it, but nothing new is written there.
pub const DEVIN: Dialect = Dialect {
    agents_glob: ".devin/agents",
    session_key: "Devin-Session",
    instructions_primary: "AGENTS.md",
    instructions_fallback: "CLAUDE.md",
    general_purpose: "general-purpose",
    planner: "planner",
    args_token: "$ARGUMENTS",
};

pub struct CrewFormat {
    pub file_suffix: &'static str,
    pub dialect: &'static Dialect,
    pub map_tools: fn(&CanonicalRole) -> anyhow::Result<Vec<String>>,
    pub serialize: fn(&CanonicalRole, &str, &[String]) -> anyhow::Result<String>,
    /// How each role's file is laid out beneath the harness's `agents/`
    /// directory. Most harnesses take one file per role; Antigravity takes a
    /// directory per role, and a flat file installs cleanly and is never read.
    pub layout: CrewLayout,
}

/// On-disk shape of a harness's crew directory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrewLayout {
    /// `<base>/agents/<role><suffix>` — one file per role.
    Flat,
    /// `<base>/agents/<role>/agent.md` — a directory per role.
    ///
    /// The Antigravity CLI discovers agents from
    /// `{workspace}/.agents/agents/{agent_name}/` and reads the `agent.md`
    /// inside it, so a flat `<role>.md` is invisible to it. Verified against
    /// the shipped `agy` binary's own path template.
    DirPerAgent,
}

/// Quote a scalar for YAML frontmatter.
///
/// Contributors' prose routinely contains `: `, quotes and leading symbols.
/// An unquoted `description: Shipmates: take an issue…` is not a parse
/// warning: YAML reads the second colon as a nested mapping, the whole
/// frontmatter block fails in a strict loader (Cursor parses `.mdc` rules with
/// Ruby's Psych), and the skill installs cleanly and is never discovered
/// (#407). Double-quoted style is the one YAML style that can carry every
/// character, so escape control characters and quoting rather than strip,
/// reject or hope. Callers deliberately leave `name:` bare — install identity
/// and receipt matching read the bare form.
pub fn yaml_scalar(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for character in value.chars() {
        match character {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            control if control.is_control() => out.push_str(&format!("\\u{:04x}", control as u32)),
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

/// Inverse of [`yaml_scalar`]: strip the wrapping double quotes and undo
/// the escapes that function emits (`\\`, `\"`, `\n`, `\r`, `\t`, `\uXXXX`).
/// `unquote(yaml_scalar(s)) == s` for every string `yaml_scalar` can produce.
/// Does not change what `yaml_scalar` writes — only reads it.
#[cfg(test)]
fn unquote(value: &str) -> String {
    let inner = value
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .unwrap_or_else(|| panic!("yaml_scalar output is always double-quoted, got {value:?}"));
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(character) = chars.next() {
        if character != '\\' {
            out.push(character);
            continue;
        }
        match chars.next() {
            Some('\\') => out.push('\\'),
            Some('"') => out.push('"'),
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            Some('u') => {
                let mut code = 0u32;
                for _ in 0..4 {
                    let digit = chars
                        .next()
                        .expect("yaml_scalar emits four hex digits after \\u");
                    code = code * 16
                        + digit
                            .to_digit(16)
                            .expect("yaml_scalar emits lowercase hex in \\uXXXX");
                }
                out.push(char::from_u32(code).expect("yaml_scalar emits a valid code point"));
            }
            other => panic!("unknown yaml_scalar escape {other:?} in {value:?}"),
        }
    }
    out
}

/// Emit crew files with one path/body loop shared by every crew-bearing target.
/// Format-specific tool mapping and serialisation stay adapter-owned.
pub fn emit_crew_files(
    base_dir: &str,
    roles: &[CanonicalRole],
    format: &CrewFormat,
) -> anyhow::Result<HashMap<String, String>> {
    let mut files = HashMap::new();
    for role in roles {
        validate_role_name(&role.name, "role")?;
        let tools = (format.map_tools)(role)?;
        let body = render_body(&role.body, format.dialect);
        let content = (format.serialize)(role, &body, &tools)?;
        let rel = match format.layout {
            CrewLayout::Flat => {
                format!("{}/agents/{}{}", base_dir, role.name, format.file_suffix)
            }
            CrewLayout::DirPerAgent => {
                format!("{}/agents/{}/agent.md", base_dir, role.name)
            }
        };
        files.insert(rel, content);
    }
    Ok(files)
}

/// Emit a command's rendered skill for a skill-only harness.
///
/// The [Agent Skills](https://agentskills.io) standard guarantees exactly two
/// frontmatter keys — `name` and `description` — in that order. The keys the
/// canonical `SKILL.md` adds (`argument-hint`, `allowed-tools`,
/// `disable-model-invocation`) are Claude Code vendor extensions, and an
/// unknown key is rejected by a strict parser rather than ignored, so a
/// skill-only adapter ships the standard's pair plus the rendered body and
/// nothing else. These harnesses discover skills themselves; they have no
/// subagent mechanic, so the crew ships as skills and `roles` is ignored.
pub fn emit_skill_files(
    base_dir: &str,
    commands: &[CanonicalCommand],
    dialect: &Dialect,
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
        content.push_str("---\n");
        content.push_str(&render_command_body(command, dialect)?);
        files.insert(
            format!("{}/skills/{}/SKILL.md", base_dir, command.name),
            content,
        );
    }
    Ok(files)
}

/// Emit a harness's commands into its **own** skills tree, carrying the vendor
/// frontmatter keys that harness honours.
///
/// This is deliberately NOT [`emit_skill_files`]. That one is the strict
/// [Agent Skills](https://agentskills.io) pair *by construction* — `name` and
/// `description` and nothing else — because a strict parser rejects an unknown
/// key. Grok Build parses extra frontmatter keys, ignores the ones it does not
/// know, and *honours* `disable-model-invocation`; it is the one target that can
/// express the guard every canonical command carries. Rendering the strict pair
/// here would silently drop that guard, and Grok would then be free to load a
/// workflow that creates worktrees, pushes branches and opens pull requests on
/// its own — the exact decision `disable-model-invocation` exists to reserve for
/// the captain.
///
/// `allowed-tools` is deliberately dropped. Grok parses it into its skill
/// metadata, but no code path applies it as a permission filter, so emitting it
/// would advertise a boundary that does not exist.
pub fn emit_native_command_skills(
    base_dir: &str,
    commands: &[CanonicalCommand],
    dialect: &Dialect,
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
            content.push_str("disable-model-invocation: true\n");
        }
        content.push_str("---\n");
        content.push_str(&render_command_body(command, dialect)?);
        files.insert(
            format!("{}/skills/{}/SKILL.md", base_dir, command.name),
            content,
        );
    }
    Ok(files)
}

/// Emit an agent-invoked tool as a model-invoked Agent Skill (+ bundled assets).
///
/// A shipmates *tool* is the model-invoked sibling of a command: the crew reach
/// for it implicitly, never by typing a slash command. On Claude Code that is a
/// `SKILL.md` carrying the `user-invocable: false` vendor key — model-invoked,
/// hidden from the `/` menu — so pass `agent_only = true`. The other skill
/// harnesses have no documented way to hide a skill from manual mention, so they
/// get the strict two-key Agent Skills pair; the tool is model-invoked but still
/// technically typeable, which is recorded rather than faked (`agent_only =
/// false`). Bundled assets (a runnable script) ride alongside the `SKILL.md`.
pub fn emit_tool_files(
    base_dir: &str,
    tools: &[CanonicalTool],
    dialect: &Dialect,
    agent_only: bool,
) -> HashMap<String, String> {
    let mut files = HashMap::new();
    for tool in tools {
        let mut content = String::new();
        content.push_str("---\n");
        content.push_str(&format!("name: {}\n", tool.name));
        content.push_str(&format!(
            "description: {}\n",
            yaml_scalar(&tool.description)
        ));
        if agent_only {
            content.push_str("user-invocable: false\n");
        }
        content.push_str("---\n");
        content.push_str(&render_body(&tool.body, dialect));
        files.insert(
            format!("{}/skills/{}/SKILL.md", base_dir, tool.name),
            content,
        );
        for (rel, asset) in &tool.assets {
            files.insert(
                format!("{}/skills/{}/{}", base_dir, tool.name, rel),
                asset.clone(),
            );
        }
    }
    files
}

/// Emit a harness's commands into the SHARED open `.agents/skills/` tree.
///
/// `container` is the harness's payload staging root (`harnesses/<name>`); the
/// files land at `<container>/.agents/skills/<name>/SKILL.md` and, once the
/// installer strips `harnesses/<name>/`, at `.agents/skills/` in the target.
/// Rendered with the neutral [`AGENT_SKILLS`] dialect so every harness that
/// reads this location writes identical bytes — the single source of truth for
/// the shared tree. See [`AGENT_SKILLS`] for why that matters.
pub fn emit_shared_skills(
    container: &str,
    commands: &[CanonicalCommand],
) -> anyhow::Result<HashMap<String, String>> {
    emit_skill_files(&format!("{container}/.agents"), commands, &AGENT_SKILLS)
}

/// Emit a harness's opt-in tools into the shared `.agents/skills/` tree.
///
/// The neutral-tree harnesses can't hide a skill from manual mention (only
/// Claude Code's `user-invocable: false` does that), so `agent_only = false` —
/// the tool is model-invoked but still technically typeable, recorded not faked.
pub fn emit_shared_tool_skills(
    container: &str,
    tools: &[CanonicalTool],
) -> HashMap<String, String> {
    emit_tool_files(&format!("{container}/.agents"), tools, &AGENT_SKILLS, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_instructions_claude() {
        let out = render_instructions(
            "`{{project-instructions}}` if one exists, else `{{project-instructions-fallback}}` if one exists, else `{{project-instructions}}`.",
            CLAUDE_CODE.instructions_primary,
            CLAUDE_CODE.instructions_fallback,
        );
        assert_eq!(
            out,
            "`CLAUDE.md` if one exists, else `AGENTS.md` if one exists, else `CLAUDE.md`."
        );
    }

    #[test]
    fn test_render_instructions_opencode() {
        let out = render_instructions(
            "`{{project-instructions}}` if one exists, else `{{project-instructions-fallback}}` if one exists, else `{{project-instructions}}`.",
            OPENCODE.instructions_primary,
            OPENCODE.instructions_fallback,
        );
        assert_eq!(
            out,
            "`AGENTS.md` if one exists, else `CLAUDE.md` if one exists, else `AGENTS.md`."
        );
    }

    #[test]
    fn test_render_body_claude() {
        let body = render_body(
            "Ship an issue. Resolve a role via `{{agents-glob}}` or fall back to `{{general-purpose}}`. \
             Spawn `{{role:sdet}}`. Use {{question}} as input. Tag with `{{session-key}}`.",
            &CLAUDE_CODE,
        );
        assert!(body.contains(".claude/agents/*.md"));
        assert!(body.contains("general-purpose"));
        assert!(body.contains("subagent_type: sdet"));
        assert!(body.contains("Claude-Session"));
        assert!(body.contains("{{question}}"));
        assert!(!body.contains("{{role:"));
        assert!(!body.contains("{{agents-glob}}"));
    }

    #[test]
    fn test_render_body_opencode() {
        let body = render_body(
            "Resolve via `{{agents-glob}}` else `{{general-purpose}}`. Spawn `{{role:planner}}`.",
            &OPENCODE,
        );
        assert!(body.contains(".opencode/agents/*.md"));
        assert!(body.contains("general"));
        assert!(body.contains("subagent_type: architect"));
        assert!(!body.contains("general-purpose"));
        assert!(!body.contains("agent-files/"));
    }

    #[test]
    fn test_render_args_rejects_multiple_placeholders() {
        let out = render_args("a {{one}} b {{two}} c", "$ARGUMENTS");
        assert!(out.is_err());
    }

    #[test]
    fn test_render_preserves_literal_harness_names() {
        let body = render_body(
            "Literal AGENTS.md and __AGENTS__ stay unchanged.",
            &CLAUDE_CODE,
        );
        assert_eq!(body, "Literal AGENTS.md and __AGENTS__ stay unchanged.");
    }

    #[test]
    fn test_emit_steering_claude_rules_path() {
        let target = SteeringTarget {
            rel_path: ".claude/rules/shipmates-contributor.md",
            format: SteeringFormat::PlainMarkdown,
        };
        let files = emit_steering_at("harnesses/claude-code", &target, &CLAUDE_CODE, "body");
        assert_eq!(files.len(), 1);
        assert!(files.contains_key("harnesses/claude-code/.claude/rules/shipmates-contributor.md"));
    }

    #[test]
    fn test_emit_steering_cursor_mdc_wrapper() {
        let target = SteeringTarget {
            rel_path: ".cursor/rules/shipmates-contributor.mdc",
            format: SteeringFormat::CursorMdc {
                description: "Shipmates contributor checklists",
            },
        };
        let files = emit_steering_at("harnesses/cursor", &target, &AGENT_SKILLS, "steer");
        let content = &files["harnesses/cursor/.cursor/rules/shipmates-contributor.mdc"];
        assert!(content.starts_with("---\n"));
        assert!(content.contains("description: \"Shipmates contributor checklists\"\n"));
        assert!(content.contains("alwaysApply: true"));
        assert!(content.contains("steer"));
    }

    #[test]
    fn test_emit_steering_copilot_apply_to_stays_quoted() {
        let target = SteeringTarget {
            rel_path: ".github/instructions/shipmates.instructions.md",
            format: SteeringFormat::CopilotInstructions { apply_to: "**/*" },
        };
        let files = emit_steering_at(
            "harnesses/github-copilot",
            &target,
            &GITHUB_COPILOT,
            "steer",
        );
        let content =
            &files["harnesses/github-copilot/.github/instructions/shipmates.instructions.md"];
        assert!(content.starts_with("---\napplyTo: \"**/*\"\n---\n"));
        assert!(content.ends_with("steer"));
    }

    #[test]
    fn test_yaml_scalar_quotes_colon_space() {
        assert_eq!(
            yaml_scalar("Shipmates: take an issue to a PR"),
            "\"Shipmates: take an issue to a PR\""
        );
    }

    #[test]
    fn test_yaml_scalar_quotes_leading_bracket() {
        assert_eq!(yaml_scalar("[draft] ship it"), "\"[draft] ship it\"");
    }

    #[test]
    fn test_yaml_scalar_escapes_embedded_quote() {
        assert_eq!(
            yaml_scalar("the \"right\" shape"),
            r#""the \"right\" shape""#
        );
    }

    #[test]
    fn test_yaml_scalar_escapes_backslash() {
        assert_eq!(yaml_scalar(r"a\b"), r#""a\\b""#);
    }

    #[test]
    fn test_yaml_scalar_escapes_newline() {
        assert_eq!(yaml_scalar("two\nlines"), "\"two\\nlines\"");
    }

    #[test]
    fn test_yaml_scalar_escapes_control_characters() {
        assert_eq!(yaml_scalar("a\tb\rc\u{7}"), "\"a\\tb\\rc\\u0007\"");
    }

    #[test]
    fn test_unquote_inverts_yaml_scalar_quotes_backslashes_and_c0() {
        let mut cases = vec![
            String::new(),
            "plain".into(),
            "the \"right\" shape".into(),
            r"a\\b".into(),
            "quotes and \\backslashes\" mixed".into(),
        ];
        for byte in 0u8..=0x1f {
            cases.push((byte as char).to_string());
            cases.push(format!("pre{}post", byte as char));
        }
        cases.push((0u8..=0x1f).map(char::from).collect());
        for value in &cases {
            assert_eq!(
                unquote(&yaml_scalar(value)),
                *value,
                "unquote must invert yaml_scalar for {value:?}"
            );
        }
    }

    #[test]
    fn test_shared_preambles_expand_and_leave_no_markers() {
        let command = render_body(
            "<!-- shipmates:command-preamble -->\n<!-- shipmates:acceptance-board -->\n<!-- shipmates:epic-integration-board -->\n<!-- shipmates:why-merge-pr -->\nbody",
            &CLAUDE_CODE,
        );
        let role = render_role_body("<!-- shipmates:subagent-preamble -->\nrole", &CLAUDE_CODE);

        assert!(command.contains("## Cost discipline"));
        assert!(command.contains("## Argument intake"));
        assert!(command.contains("Mandatory seats"));
        assert!(command.contains("Integration questions"));
        assert!(command.contains("Why merge this"));
        // The model-routing ruleset is part of the shared cost-discipline
        // preamble, so a command carrying only that one marker still gets it —
        // which is what makes the ruleset global rather than per-command.
        assert!(command.contains("## Model routing"));
        // Agents carry the subagent preamble, not the command preamble, so the
        // ruleset reaches commands only — never every crew file.
        assert!(!role.contains("## Model routing"));
        assert!(role.contains("## Return discipline"));
        assert!(!command.contains("shipmates:command-preamble"));
        assert!(!command.contains("shipmates:acceptance-board"));
        assert!(!command.contains("shipmates:epic-integration-board"));
        assert!(!command.contains("shipmates:why-merge-pr"));
        assert!(!command.contains("shipmates:model-routing"));
        assert!(!role.contains("shipmates:subagent-preamble"));
    }

    /// The model-routing block ships into user repos, so it carries none of the
    /// tokens or shapes that only make sense inside this repo: no HTML comment,
    /// no `{{…}}` placeholder, no `$`-plus-digit (a command file is scanned for
    /// one and a fence would not protect it), and no leftover marker. The
    /// resolution order must be stated exactly once, in this block.
    ///
    /// The block's one mechanism is the orchestrator's own judgment (#531), so
    /// that is asserted both ways: the judgment rule is present in its stated
    /// order, and **no** declared-config concept survives anywhere in the block
    /// — the retired word included, because a feature that lingers in the
    /// doctrine is a captain still being told to maintain a file that no longer
    /// does anything. The block is inlined into every command on every target,
    /// so its ceiling is asserted here too, beside the table ceiling in the
    /// integration suite.
    #[test]
    fn test_model_routing_block_is_canonical_and_self_contained() {
        let out = render_body("<!-- shipmates:model-routing -->", &CLAUDE_CODE);
        assert!(
            out.contains(
                "explicit spawn value → parent/session value → the model's own effort default"
            ),
            "model routing must state the resolution order verbatim"
        );
        // The block wraps at ~100 columns, so clauses that may cross a line
        // break are asserted on a whitespace-normalised copy.
        let flat = out.split_whitespace().collect::<Vec<_>>().join(" ");

        // #531 — the judgment rule, in the order it must be exercised. Each step
        // is load-bearing: drop `Record the call` and a wrong pick becomes
        // uncorrectable; drop `inherit` and an unreadable target dead-ends.
        for step in [
            "The main agent makes the call — nothing is configured in advance.",
            "Ask the target what exists.",
            "Where there is no listing command, read what the target documents.",
            "Then judge.",
            "Record the call.",
            "`inherit` when you cannot decide.",
        ] {
            assert!(
                flat.contains(step),
                "the block must state the judgment rule's step `{step}` — the orchestrator's own \
                 call is the only mechanism left (#531)"
            );
        }
        assert!(
            flat.contains("Name only an identity the target itself offered"),
            "the block must forbid an identity the target did not offer — that restriction is what \
             makes an agent's judgment safe (#531)"
        );
        assert!(
            flat.contains("source=<observed|inherit>"),
            "the audit template must carry where the identity came from (#531)"
        );

        // #531 — the declared config is gone and must stay gone. The word itself
        // is retired here: every mention that used to be in this block was an
        // instruction to maintain a file that no longer exists, so a residue
        // would send a captain looking for it. Re-using the word for something
        // else is a deliberate act that updates this assertion.
        assert!(
            !flat.contains("pool"),
            "the block still carries the retired declared-config vocabulary — it is removed and the \
             doctrine must not describe it (#531)"
        );
        for residue in ["model-pool", "declared pool", "pool unusable", "pool out of scope"] {
            assert!(
                !flat.contains(residue),
                "the block still carries the retired declared-config concept `{residue}` (#531)"
            );
        }
        assert!(
            out.len() <= 7_500,
            "the model-routing block is {} bytes, past the 7,500-byte ceiling — it is inlined into \
             every command on every target, and the ceiling came down when the declared config was \
             removed (#531): trim a cell or a clause rather than raising it",
            out.len()
        );
        assert!(!out.contains("<!--"), "block must carry no HTML comment");
        assert!(!out.contains("{{"), "block must carry no exporter token");
        assert!(!out.contains("shipmates:model-routing"));
        assert!(
            !Regex::new(r"\$[0-9]").unwrap().is_match(&out),
            "block must carry no `$` followed by a digit — a command file is scanned whole"
        );
    }
}
