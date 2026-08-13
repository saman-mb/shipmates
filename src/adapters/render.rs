use crate::catalog::{CanonicalCommand, CanonicalTool};
use regex::Regex;
use std::collections::HashMap;

/// Per-harness dialect settings shared by every adapter.
///
/// The canonical `commands/*.md` bodies are harness-neutral prose. They name
/// the exporter's abstract locations (`agent-files/*.md`, `Harness-Session`,
/// `TARGET.md`/`AGENTS.md`), spawn roles with `@role(name)`, and reference
/// command arguments as `{{name}}`. Each harness resolves those tokens into
/// its own dialect — where its agents live, what its session metadata is
/// called, which project-instructions file it reads, how a role is spawned.
///
/// Adapters stay thin by declaring a `Dialect` and letting `render_body` do
/// the substitution. The ordering of the instruction-filename rules is
/// load-bearing (see `render_instructions`).
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
const SUBAGENT_PREAMBLE_MARKER: &str = "<!-- shipmates:subagent-preamble -->";
const COST_DOCTRINE: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/docs/COST.md"));

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

fn subagent_preamble() -> &'static str {
    doctrine_section("<!-- subagent-preamble:start -->", "<!-- subagent-preamble:end -->")
}

/// Resolve the repo-instructions filename the neutral prose refers to.
///
/// Canonical prose treats `TARGET.md` and `AGENTS.md` as two *different*
/// fallbacks ("`TARGET.md` if one exists, else `AGENTS.md` if one exists").
/// A naive sweep of both to one name would produce "`CLAUDE.md` if one
/// exists, else `CLAUDE.md`". The two specific phrases are protected with a
/// sentinel first, so the primary target maps both names while the fallback
/// arm keeps the harness's real second-choice filename.
fn render_instructions(text: &str, primary: &str, fallback: &str) -> String {
    let mut out = text.to_string();
    out = out.replace("`TARGET.md`/`AGENTS.md`", &format!("`TARGET.md`/`{SENTINEL}`"));
    out = out.replace("else `AGENTS.md`", &format!("else `{SENTINEL}`"));
    out = out.replace("TARGET.md", primary);
    out = out.replace("AGENTS.md", primary);
    out.replace(SENTINEL, fallback)
}

/// Render a harness-neutral command body into a harness's dialect.
pub fn render_body(text: &str, d: &Dialect) -> String {
    let mut out = text.replace(COMMAND_PREAMBLE_MARKER, command_preamble());
    out = out.replace(SUBAGENT_PREAMBLE_MARKER, subagent_preamble());
    out = render_instructions(&out, d.instructions_primary, d.instructions_fallback);
    out = out.replace("agent-files/*.md", &format!("{}/*.md", d.agents_glob));
    out = out.replace("Harness-Session", d.session_key);
    out = out.replace("general-purpose", d.general_purpose);
    out = out.replace("@role(planner)", &format!("subagent_type: {}", d.planner));
    out = out.replace("agent: `planner`", &format!("agent: `{}`", d.planner));
    out = out.replace("@role(senior-engineer)", "subagent_type: senior-engineer");
    out = out.replace("@role(sdet)", "subagent_type: sdet");
    out = out.replace("`@role` reference", "`subagent_type`");
    out = render_args(&out, d.args_token);
    out = out.replace(
        &format!("to an `{}/*.md`", d.agents_glob),
        &format!("to a `{}/*.md`", d.agents_glob),
    );
    out
}

/// Render a role body through the same neutral-to-harness rules as commands,
/// including the stable return preamble shared by every subagent.
pub fn render_role_body(text: &str, d: &Dialect) -> String {
    render_body(text, d)
}

/// Replace every `{{name}}` argument placeholder with the harness's token.
fn render_args(text: &str, token: &str) -> String {
    let re = Regex::new(r"\{\{[a-z][a-z0-9_-]*\}\}").expect("static regex");
    // `$A` is regex replacement syntax (a named-group reference); double it so
    // the token's `$` survives literally — `$ARGUMENTS` must not vanish.
    let escaped = token.replace('$', "$$");
    re.replace_all(text, escaped.as_str()).into_owned()
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

// Antigravity (`agy`, the retired Gemini CLI's successor) renders its crew from
// raw persona bodies and reads skills from the shared `.agents/skills/` tree, so
// it needs no dialect of its own — the neutral AGENT_SKILLS covers its skills.

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

/// Windsurf (Cascade)'s dialect.
pub const WINDSURF: Dialect = Dialect {
    agents_glob: ".windsurf/agents",
    session_key: "Windsurf-Session",
    instructions_primary: "AGENTS.md",
    instructions_fallback: "CLAUDE.md",
    general_purpose: "general-purpose",
    planner: "planner",
    args_token: "$ARGUMENTS",
};

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
) -> HashMap<String, String> {
    let mut files = HashMap::new();
    for command in commands {
        let mut content = String::new();
        content.push_str("---\n");
        content.push_str(&format!("name: {}\n", command.name));
        content.push_str(&format!("description: {}\n", command.description));
        content.push_str("---\n");
        content.push_str(&render_body(&command.narrative, dialect));
        files.insert(format!("{}/skills/{}/SKILL.md", base_dir, command.name), content);
    }
    files
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
        content.push_str(&format!("description: {}\n", tool.description));
        if agent_only {
            content.push_str("user-invocable: false\n");
        }
        content.push_str("---\n");
        content.push_str(&render_body(&tool.body, dialect));
        files.insert(format!("{}/skills/{}/SKILL.md", base_dir, tool.name), content);
        for (rel, asset) in &tool.assets {
            files.insert(format!("{}/skills/{}/{}", base_dir, tool.name, rel), asset.clone());
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
pub fn emit_shared_skills(container: &str, commands: &[CanonicalCommand]) -> HashMap<String, String> {
    emit_skill_files(&format!("{container}/.agents"), commands, &AGENT_SKILLS)
}

/// Emit a harness's opt-in tools into the shared `.agents/skills/` tree.
///
/// The neutral-tree harnesses can't hide a skill from manual mention (only
/// Claude Code's `user-invocable: false` does that), so `agent_only = false` —
/// the tool is model-invoked but still technically typeable, recorded not faked.
pub fn emit_shared_tool_skills(container: &str, tools: &[CanonicalTool]) -> HashMap<String, String> {
    emit_tool_files(&format!("{container}/.agents"), tools, &AGENT_SKILLS, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_instructions_claude() {
        let out = render_instructions(
            "`TARGET.md` if one exists, else `AGENTS.md` if one exists, else `TARGET.md`.",
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
            "`TARGET.md` if one exists, else `AGENTS.md` if one exists, else `TARGET.md`.",
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
            "Ship an issue. Resolve a role via `agent-files/*.md` or fall back to `general-purpose`. \
             Spawn `@role(sdet)`. Use {{question}} as input. Tag with `Harness-Session`.",
            &CLAUDE_CODE,
        );
        assert!(body.contains(".claude/agents/*.md"));
        assert!(body.contains("general-purpose"));
        assert!(body.contains("subagent_type: sdet"));
        assert!(body.contains("$ARGUMENTS"));
        assert!(body.contains("Claude-Session"));
        assert!(!body.contains("{{question}}"));
        assert!(!body.contains("@role("));
        assert!(!body.contains("agent-files/"));
    }

    #[test]
    fn test_render_body_opencode() {
        let body = render_body(
            "Resolve via `agent-files/*.md` else `general-purpose`. Spawn `@role(planner)`.",
            &OPENCODE,
        );
        assert!(body.contains(".opencode/agents/*.md"));
        assert!(body.contains("general"));
        assert!(body.contains("subagent_type: architect"));
        assert!(!body.contains("general-purpose"));
        assert!(!body.contains("agent-files/"));
    }

    #[test]
    fn test_render_args_replaces_all_placeholders() {
        let out = render_args("a {{one}} b {{two}} c", "$ARGUMENTS");
        assert_eq!(out, "a $ARGUMENTS b $ARGUMENTS c");
    }

    #[test]
    fn test_shared_preambles_expand_and_leave_no_markers() {
        let command = render_body("<!-- shipmates:command-preamble -->\nbody", &CLAUDE_CODE);
        let role = render_role_body("<!-- shipmates:subagent-preamble -->\nrole", &CLAUDE_CODE);

        assert!(command.contains("## Cost discipline"));
        assert!(role.contains("## Return discipline"));
        assert!(!command.contains("shipmates:command-preamble"));
        assert!(!role.contains("shipmates:subagent-preamble"));
    }
}
