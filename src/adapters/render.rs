use crate::catalog::CanonicalCommand;
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
    let mut out = render_instructions(text, d.instructions_primary, d.instructions_fallback);
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

/// Gemini CLI / Antigravity's dialect.
pub const GEMINI: Dialect = Dialect {
    agents_glob: ".gemini/agents",
    session_key: "Gemini-Session",
    instructions_primary: "AGENTS.md",
    instructions_fallback: "CLAUDE.md",
    general_purpose: "general-purpose",
    planner: "planner",
    args_token: "$ARGUMENTS",
};

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

/// Cursor's dialect.
pub const CURSOR: Dialect = Dialect {
    agents_glob: ".cursor/agents",
    session_key: "Cursor-Session",
    instructions_primary: "AGENTS.md",
    instructions_fallback: "CLAUDE.md",
    general_purpose: "general-purpose",
    planner: "planner",
    args_token: "$ARGUMENTS",
};

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

/// Zed's dialect.
pub const ZED: Dialect = Dialect {
    agents_glob: ".zed/agents",
    session_key: "Zed-Session",
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
}
