use crate::catalog::{CanonicalCommand, CanonicalRole};
use std::collections::HashMap;

pub mod claude_code;
pub mod codex;
pub mod cursor;
pub mod gemini;
pub mod github_copilot;
pub mod opencode;
pub mod render;
pub mod windsurf;
pub mod zed;

pub trait Adapter {
    fn build(&self, roles: &[CanonicalRole], commands: &[CanonicalCommand]) -> anyhow::Result<HashMap<String, String>>;

    /// Directory inside the built payload that this adapter owns, e.g.
    /// `harnesses/claude-code/.claude`. Digest checks and manifest validation
    /// resolve payload paths against it — a target's digest can no longer
    /// silently pass because the checker looked in the wrong tree.
    fn base_dir(&self) -> &'static str;
}

#[allow(dead_code)]
pub fn conformance_report() {}

/// The harnesses a user can `shipmates install --harness <name>` for.
pub fn targets() -> [&'static str; 9] {
    [
        "claude-code",
        "opencode",
        "gemini",
        "antigravity",
        "codex",
        "cursor",
        "github-copilot",
        "windsurf",
        "zed",
    ]
}
