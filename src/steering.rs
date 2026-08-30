//! Legacy contributor-steering migration — strip #295 install artifacts from
//! root `AGENTS.md` / `CLAUDE.md` after steering moved to harness-native paths.

use crate::catalog;
use std::path::{Path, PathBuf};

pub const START: &str = "<!-- shipmates:contributor-steering -->\n";
pub const END: &str = "\n<!-- /shipmates:contributor-steering -->\n";

const LEGACY_CLAUDE_HEADING: &str = "# Shipmates contributor steering";

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

/// True when root `CLAUDE.md` is the steering-only file #295 wrote at install.
pub fn is_legacy_claude_steering(content: &str) -> bool {
    has_section(content)
        || content.starts_with(LEGACY_CLAUDE_HEADING)
        || catalog::load_steering_embedded()
            .ok()
            .is_some_and(|embedded| content.trim() == embedded.trim())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LegacyMigration {
    Write { path: PathBuf, content: String },
    Remove { path: PathBuf },
}

/// Best-effort cleanup of #295 steering in root project-instructions files.
pub fn plan_legacy_migration(target_dir: &Path) -> std::io::Result<Vec<LegacyMigration>> {
    if !catalog::is_shipmates_contributor_tree(target_dir) {
        return Ok(Vec::new());
    }

    let mut actions = Vec::new();

    let agents = target_dir.join("AGENTS.md");
    if agents.is_file() {
        let existing = std::fs::read_to_string(&agents)?;
        if has_section(&existing) {
            let stripped = strip_section(&existing);
            if stripped.trim().is_empty() {
                actions.push(LegacyMigration::Remove { path: agents });
            } else {
                actions.push(LegacyMigration::Write {
                    path: agents,
                    content: stripped,
                });
            }
        }
    }

    let claude = target_dir.join("CLAUDE.md");
    if claude.is_file() {
        let existing = std::fs::read_to_string(&claude)?;
        if is_legacy_claude_steering(&existing) {
            actions.push(LegacyMigration::Remove { path: claude });
        }
    }

    Ok(actions)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn strip_leaves_base_intact() {
        let merged = format!("{START}steer{END}");
        let base = format!("# AGENTS\n\n{merged}");
        assert_eq!(strip_section(&base).trim(), "# AGENTS");
    }

    #[test]
    fn detects_legacy_claude_steering_file() {
        assert!(is_legacy_claude_steering(
            "# Shipmates contributor steering\n\nchecklists\n"
        ));
        assert!(!is_legacy_claude_steering("# My Project\n\nReal CLAUDE.md\n"));
    }

    #[test]
    fn plan_removes_orphan_claude_and_strips_agents() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("commands")).unwrap();
        fs::write(dir.path().join("commands/ship-issue.md"), "---\n---\n").unwrap();
        fs::create_dir_all(dir.path().join("toolbox")).unwrap();
        fs::create_dir_all(dir.path().join("tools")).unwrap();
        fs::write(dir.path().join("tools/gen_command_pages.py"), "# gen").unwrap();

        fs::write(
            dir.path().join("CLAUDE.md"),
            "# Shipmates contributor steering\n\nold install\n",
        )
        .unwrap();
        fs::write(
            dir.path().join("AGENTS.md"),
            format!("# Repo\n\n{START}steer{END}\n"),
        )
        .unwrap();

        let actions = plan_legacy_migration(dir.path()).unwrap();
        assert_eq!(actions.len(), 2);
        assert!(actions.iter().any(|a| matches!(
            a,
            LegacyMigration::Remove { path } if path.ends_with("CLAUDE.md")
        )));
        assert!(actions.iter().any(|a| matches!(
            a,
            LegacyMigration::Write { content, .. } if content.contains("# Repo")
        )));
    }
}
