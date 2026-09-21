//! Adopt the files a retired target name left behind.
//!
//! A product gets renamed, its tree moves, and the old target id stops being a
//! target. That is not a reason to strand a captain: the files the retired
//! install wrote are still on disk, and the harness may still read them
//! (Devin CLI keeps reading `.windsurf/skills/` after the Windsurf → Devin
//! Desktop rename), so leaving them there means two copies of every command in
//! the picker — the exact drift `doctor` exists to catch.
//!
//! Ownership is decided from the retired install's **own receipt**, not from a
//! filename heuristic: a file is adopted only when its bytes still hash to what
//! the receipt recorded. A file the captain edited is left exactly where it is
//! and reported, because it is no longer ours to delete. Every adopted file is
//! copied into a per-run backup tree before it is removed, so the move is
//! reversible and idempotent — a second run finds nothing left to do.

use crate::digest;
use crate::installer::manifest_db::{ReceiptRepository, resolve_target_relative};
use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

/// Retired target names that resolve to `harness`, newest first.
///
/// A retired name is listed here only while the product still reads its tree —
/// that is why the files must be actively reclaimed rather than ignored.
pub fn retired_names_for(harness: &str) -> &'static [&'static str] {
    match harness {
        "devin" => &["windsurf"],
        _ => &[],
    }
}

#[derive(Debug, Clone, Default)]
pub struct RetiredReport {
    /// The retired target name whose install was adopted.
    pub name: String,
    /// Files removed after their backup was written, relative to the target.
    pub adopted: Vec<PathBuf>,
    /// Backup copy written for each adopted file (same order as `adopted`).
    pub backups: Vec<PathBuf>,
    /// The per-run backup tree the copies landed under.
    pub backup_root: Option<PathBuf>,
    /// Receipt-owned files left in place because their bytes no longer match
    /// what the retired install wrote — the captain edited them.
    pub kept_modified: Vec<PathBuf>,
    /// Whether the retired receipt itself was removed.
    pub receipt_removed: bool,
}

impl RetiredReport {
    pub fn changed(&self) -> bool {
        !self.adopted.is_empty() || self.receipt_removed
    }
}

/// Adopt any retired-name install sitting in `target_dir` for `harness`.
///
/// Returns `None` when there is nothing to adopt, so callers can print nothing.
pub fn adopt(target_dir: &Path, harness: &str) -> Result<Option<RetiredReport>> {
    let repository = ReceiptRepository::new(target_dir);
    for name in retired_names_for(harness) {
        let Some(receipt) = repository.load(name)? else {
            continue;
        };
        let mut report = RetiredReport {
            name: (*name).to_string(),
            ..RetiredReport::default()
        };
        let backup_root = crate::installer::migrate::new_backup_root(target_dir);
        let mut backups_written = false;
        for file in &receipt.files {
            let relative = PathBuf::from(&file.path);
            let path = resolve_target_relative(target_dir, &relative)?;
            let Ok(bytes) = fs::read(&path) else {
                // Already gone: nothing to adopt, nothing to report.
                continue;
            };
            if digest::hash_bytes(&bytes) != file.sha256 {
                report.kept_modified.push(relative);
                continue;
            }
            let backup = backup_root.join(&relative);
            if let Some(parent) = backup.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("creating backup dir {}", parent.display()))?;
            }
            crate::installer::atomic_write_bytes(&backup, &bytes)
                .with_context(|| format!("backing up {}", path.display()))?;
            // The backup is on disk before the original goes: the same ordering
            // rule the command migration uses, so a file is never lost.
            fs::remove_file(&path).with_context(|| format!("removing {}", path.display()))?;
            backups_written = true;
            report.adopted.push(relative);
            report.backups.push(backup);
        }
        prune_empty_dirs(target_dir, &report.adopted);
        if backups_written {
            // Keep the run's backup tree even when only some files moved.
        } else if backup_root.exists() {
            let _ = fs::remove_dir_all(&backup_root);
        }
        report.receipt_removed = repository.remove(name)?;
        report.adopted.sort();
        report.kept_modified.sort();
        return Ok(Some(report));
    }
    Ok(None)
}

/// Remove the directories an adopted file left behind, deepest first, stopping
/// at the first non-empty one. Never walks above the target directory.
fn prune_empty_dirs(target_dir: &Path, adopted: &[PathBuf]) {
    let mut parents: Vec<PathBuf> = Vec::new();
    for relative in adopted {
        let mut current = relative.parent();
        while let Some(dir) = current {
            if dir.as_os_str().is_empty() {
                break;
            }
            parents.push(dir.to_path_buf());
            current = dir.parent();
        }
    }
    // Deepest first so a nested empty tree collapses fully.
    parents.sort_by_key(|dir| std::cmp::Reverse(dir.components().count()));
    parents.dedup();
    for relative in parents {
        let absolute = match resolve_target_relative(target_dir, &relative) {
            Ok(path) => path,
            Err(_) => continue,
        };
        if absolute == target_dir {
            continue;
        }
        let _ = fs::remove_dir(&absolute);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::installer::manifest_db::{InstallReceipt, ReceiptFile};
    use tempfile::tempdir;

    fn write(path: &Path, content: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }

    /// A retired install is adopted: files backed up, originals removed, receipt
    /// gone, and the run is idempotent.
    #[test]
    fn adopts_an_untouched_retired_install_and_is_idempotent() {
        let dir = tempdir().unwrap();
        let target = dir.path();
        let skill = Path::new(".windsurf/skills/shipmates-ship-issue/SKILL.md");
        write(&target.join(skill), "---\nname: shipmates-ship-issue\n---\nbody\n");
        let receipt = InstallReceipt::new(
            "0.11.0",
            "windsurf",
            "skills",
            vec![".shipmates".to_string(), ".windsurf".to_string()],
            vec![ReceiptFile {
                path: skill.to_string_lossy().into_owned(),
                sha256: digest::compute_sha256(&target.join(skill)).unwrap(),
            }],
        )
        .unwrap();
        ReceiptRepository::new(target).save(&receipt).unwrap();

        let report = adopt(target, "devin").unwrap().unwrap();
        assert_eq!(report.name, "windsurf");
        assert_eq!(report.adopted.len(), 1);
        assert!(report.receipt_removed);
        assert!(!target.join(skill).exists());
        assert!(
            !target.join(".windsurf").exists(),
            "an emptied legacy tree is pruned"
        );
        assert!(report.backups[0].exists(), "the file survives in the backup");
        assert!(!target.join(".shipmates/receipts/windsurf.json").exists());

        // Second run: nothing left to adopt.
        assert!(adopt(target, "devin").unwrap().is_none());
    }

    /// A file the captain edited after installing is not ours to delete.
    #[test]
    fn keeps_a_retired_file_the_captain_edited() {
        let dir = tempdir().unwrap();
        let target = dir.path();
        let skill = Path::new(".windsurf/skills/shipmates-ship-issue/SKILL.md");
        write(&target.join(skill), "---\nname: shipmates-ship-issue\n---\nbody\n");
        let installed_hash = digest::compute_sha256(&target.join(skill)).unwrap();
        fs::write(target.join(skill), "my own notes\n").unwrap();
        let receipt = InstallReceipt::new(
            "0.11.0",
            "windsurf",
            "skills",
            vec![".shipmates".to_string(), ".windsurf".to_string()],
            vec![ReceiptFile {
                path: skill.to_string_lossy().into_owned(),
                sha256: installed_hash,
            }],
        )
        .unwrap();
        ReceiptRepository::new(target).save(&receipt).unwrap();

        let report = adopt(target, "devin").unwrap().unwrap();
        assert!(report.adopted.is_empty());
        assert_eq!(report.kept_modified.len(), 1);
        assert_eq!(fs::read_to_string(target.join(skill)).unwrap(), "my own notes\n");
    }

    /// Targets with no retired ancestor never look for one.
    #[test]
    fn a_target_without_a_retired_ancestor_adopts_nothing() {
        let dir = tempdir().unwrap();
        assert!(retired_names_for("claude-code").is_empty());
        assert!(adopt(dir.path(), "claude-code").unwrap().is_none());
    }
}
