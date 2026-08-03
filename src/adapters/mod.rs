use crate::catalog::{CanonicalCommand, CanonicalRole, CanonicalTool};
use std::collections::HashMap;

pub mod antigravity;
pub mod claude_code;
pub mod codex;
pub mod cursor;
pub mod github_copilot;
pub mod opencode;
pub mod render;
pub mod windsurf;
pub mod zed;

pub trait Adapter {
    fn build(&self, roles: &[CanonicalRole], commands: &[CanonicalCommand]) -> anyhow::Result<HashMap<String, String>>;

    /// Map the harness-agnostic tools to this harness's native tool surface.
    ///
    /// A *tool* is agent-invoked, never a slash command. opencode has a genuine
    /// code-tool directory (`.opencode/tools/*.ts`); every other harness's
    /// closest native fit is a model-invoked Agent Skill (Claude Code can pin it
    /// agent-only with `user-invocable: false`; the rest cannot hide it from
    /// manual mention, which is recorded, not faked). The default is empty —
    /// tools are opt-in and only ever written when `--with-tools` selects them.
    fn build_tools(&self, _tools: &[CanonicalTool]) -> HashMap<String, String> {
        HashMap::new()
    }

    /// Directory inside the built payload that this adapter owns, e.g.
    /// `harnesses/claude-code/.claude`. Digest checks and manifest validation
    /// resolve payload paths against it — a target's digest can no longer
    /// silently pass because the checker looked in the wrong tree.
    fn base_dir(&self) -> &'static str;
}

#[allow(dead_code)]
pub fn conformance_report() {}

/// The harnesses a user can `shipmates install --harness <name>` for.
pub fn targets() -> [&'static str; 8] {
    [
        "claude-code",
        "opencode",
        "antigravity",
        "codex",
        "cursor",
        "github-copilot",
        "windsurf",
        "zed",
    ]
}
