//! Legacy contributor-steering migration — strip the marked block from root
//! `AGENTS.md` left by the #295 install path.

use crate::catalog;
use std::path::{Path, PathBuf};

pub const START: &str = "<!-- shipmates:contributor-steering -->\n";
pub const END: &str = "\n<!-- /shipmates:contributor-steering -->\n";

/// Remove the marked steering section, if present.
pub fn strip_section(existing: &str) -> String {
    let Some(start) = existing.find(START) else {
        return existing.to_string();
    };
    let Some(end_rel) = existing[start..].find(END) else {
        return existing.to_string();
    };
    let end = start + end_rel + END.len();
    format!("{}{}", &existing[..start], &existing[end..])
        .trim_end()
        .to_string()
}

pub fn has_section(existing: &str) -> bool {
    existing.contains(START) && existing.contains(END)
}

/// Best-effort cleanup of the #295 merged steering block in root `AGENTS.md`.
pub fn migrate_legacy_agents_md(target_dir: &Path) -> std::io::Result<Option<(PathBuf, String)>> {
    if !catalog::is_shipmates_contributor_tree(target_dir) {
        return Ok(None);
    }
    let path = target_dir.join("AGENTS.md");
    if !path.is_file() {
        return Ok(None);
    }
    let existing = std::fs::read_to_string(&path)?;
    if !has_section(&existing) {
        return Ok(None);
    }
    let stripped = strip_section(&existing);
    Ok(Some((path, stripped)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_leaves_base_intact() {
        let merged = format!("{START}steer{END}");
        let base = format!("# AGENTS\n\n{merged}");
        assert_eq!(strip_section(&base).trim(), "# AGENTS");
    }
}
