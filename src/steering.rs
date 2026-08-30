//! Contributor steering merge — append/update a marked block in an existing
//! project-instructions file instead of replacing the whole document.

use crate::catalog;
use std::path::Path;

pub const START: &str = "<!-- shipmates:contributor-steering -->\n";
pub const END: &str = "\n<!-- /shipmates:contributor-steering -->\n";

pub fn block(body: &str) -> String {
    format!("{START}{body}{END}")
}

/// Insert or replace the marked steering section in `existing`.
pub fn merge_into(existing: &str, steering: &str) -> String {
    let block = block(steering);
    if let Some(start) = existing.find(START) {
        if let Some(end_rel) = existing[start..].find(END) {
            let end = start + end_rel + END.len();
            return format!("{}{}{}", &existing[..start], block, &existing[end..]);
        }
    }
    if existing.trim().is_empty() {
        return block;
    }
    format!("{existing}\n\n{block}")
}

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

/// Harness-native project-instructions path steering installs to.
pub fn instructions_rel(harness: &str) -> Option<&'static str> {
    match harness {
        "claude-code" => Some("CLAUDE.md"),
        "opencode" | "antigravity" | "codex" | "cursor" | "github-copilot" | "windsurf" => {
            Some("AGENTS.md")
        }
        _ => None,
    }
}

/// Rewrite steering entries in a container-prefixed payload map to the bytes
/// that should actually be written (merged `AGENTS.md` in the contributor tree).
pub fn adjust_payload_map(
    target_dir: &Path,
    harness: &str,
    files: &mut std::collections::HashMap<String, String>,
    container: &str,
) -> std::io::Result<()> {
    let Some(rel) = instructions_rel(harness) else {
        return Ok(());
    };
    let key = format!("{container}/{rel}");
    let Some(steering_only) = files.get(&key).cloned() else {
        return Ok(());
    };
    files.insert(key, install_content(target_dir, rel, &steering_only)?);
    Ok(())
}

/// Doctor/fix expected map uses the same merge rules as install.
pub fn adjust_expected_map(
    target_dir: &Path,
    harness: &str,
    expected: &mut std::collections::BTreeMap<String, String>,
) {
    let Some(rel) = instructions_rel(harness) else {
        return;
    };
    if let Some(steering_only) = expected.get(rel).cloned() {
        expected.insert(rel.to_string(), expected_content(target_dir, rel, &steering_only));
    }
}

/// True when an existing unowned file may receive a merged steering section.
pub fn bypass_collision_guard(target_dir: &Path, rel: &str) -> bool {
    merge_agents_md_in_contributor_tree(target_dir, rel)
}

/// True when install should splice steering into an existing root `AGENTS.md`
/// rather than skip or replace the whole file.
pub fn merge_agents_md_in_contributor_tree(target_dir: &Path, rel: &str) -> bool {
    rel == "AGENTS.md"
        && catalog::is_shipmates_contributor_tree(target_dir)
        && target_dir.join("AGENTS.md").is_file()
}

/// Content to write for a steering instructions path at install time.
pub fn install_content(
    target_dir: &Path,
    rel: &str,
    steering_only: &str,
) -> std::io::Result<String> {
    let path = target_dir.join(rel);
    if merge_agents_md_in_contributor_tree(target_dir, rel) {
        let existing = std::fs::read_to_string(&path)?;
        return Ok(merge_into(&existing, steering_only));
    }
    Ok(steering_only.to_string())
}

/// Expected on-disk bytes for doctor/fix when comparing steering paths.
pub fn expected_content(
    target_dir: &Path,
    rel: &str,
    steering_only: &str,
) -> String {
    if merge_agents_md_in_contributor_tree(target_dir, rel) {
        if let Ok(existing) = std::fs::read_to_string(target_dir.join(rel)) {
            let base = strip_section(&existing);
            return merge_into(&base, steering_only);
        }
    }
    steering_only.to_string()
}

/// Uninstall: drop steering ownership without deleting a merged instructions file.
pub fn uninstall_instructions(current: &str, steering_only: &str) -> UninstallAction {
    if has_section(current) {
        let stripped = strip_section(current);
        if stripped.trim().is_empty() || stripped.trim() == steering_only.trim() {
            UninstallAction::RemoveFile
        } else {
            UninstallAction::Write(stripped)
        }
    } else if current.trim() == steering_only.trim() {
        UninstallAction::RemoveFile
    } else {
        UninstallAction::Preserve
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UninstallAction {
    RemoveFile,
    Write(String),
    Preserve,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_appends_then_replaces() {
        let base = "# Project\n\nBody.\n";
        let one = merge_into(base, "checklists");
        assert!(one.contains(START));
        assert!(one.contains("checklists"));
        let two = merge_into(&one, "updated");
        assert!(!two.contains("checklists"));
        assert!(two.contains("updated"));
        assert!(two.starts_with("# Project"));
    }

    #[test]
    fn strip_leaves_base_intact() {
        let merged = merge_into("# AGENTS\n", "steer");
        assert_eq!(strip_section(&merged).trim(), "# AGENTS");
    }

    #[test]
    fn uninstall_merged_agents_preserves_base() {
        let merged = merge_into("# Full AGENTS.md\n\nLots of rules.", "steer");
        match uninstall_instructions(&merged, "steer") {
            UninstallAction::Write(body) => {
                assert!(body.contains("# Full AGENTS.md"));
                assert!(!body.contains(START));
            }
            other => panic!("expected Write, got {other:?}"),
        }
    }

    #[test]
    fn uninstall_steering_only_removes() {
        assert_eq!(
            uninstall_instructions("steer", "steer"),
            UninstallAction::RemoveFile
        );
    }
}
