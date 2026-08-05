//! Migrate a legacy `commands/<name>.md` layout to the skill it is now
//! superseded by (`skills/<name>/SKILL.md`).
//!
//! Old installs dropped each workflow as a flat command file; the current
//! payload ships it as a skill. A skill beats a same-named legacy command, so a
//! stale command file is dead weight at best and a shadow at worst. This module
//! plans and applies the cleanup — always backing a file up before removing it,
//! and only ever touching files Shipmates itself installed.

use crate::catalog::parse_frontmatter;
use crate::installer::atomic_write;
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// The backup tree, under the target directory, that superseded files are copied
/// into before removal. Excluded from every scan so a backup is never itself
/// mistaken for a live install.
pub const BACKUP_DIR: &str = ".shipmates-backup";

/// One legacy command file that a skill of the same name now supersedes.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct MigrationItem {
    /// The legacy `…/commands/<name>.md`, relative to the target directory.
    pub legacy_path: PathBuf,
    /// The `…/skills/<name>/SKILL.md` that replaces it, relative to the target.
    pub superseded_by: PathBuf,
}

/// What `apply` did — every path is relative to the target directory.
#[derive(Debug, Clone, Default)]
#[allow(dead_code)]
pub struct MigrationReport {
    /// Legacy files backed up and then removed.
    pub migrated: Vec<PathBuf>,
    /// The backup copy written for each migrated file (same order as `migrated`).
    pub backups: Vec<PathBuf>,
    /// Name-matching files left untouched because they are not Shipmates-owned.
    pub skipped_unmanaged: Vec<PathBuf>,
}

/// Plan the legacy → skill migration for a freshly built payload.
///
/// `built` is the adapter's payload map, keyed on `<container>/<dotdir>/…`. For
/// every `…/skills/<name>/SKILL.md` a healthy install writes, the superseded
/// sibling is `…/commands/<name>.md` under the same dotdir. An item is included
/// only when that legacy file actually exists on disk under `target_dir`, so a
/// harness that never had a `commands/` layout beside its skills (opencode ships
/// live commands but no skills; the `.agents/*` trees ship skills but no
/// commands) yields an empty plan and no live file is ever touched.
pub fn plan(
    target_dir: &Path,
    built: &HashMap<String, String>,
    container: &str,
) -> Vec<MigrationItem> {
    let prefix = format!("{}/", container);
    let mut items = Vec::new();
    for key in built.keys() {
        let Some(rel) = key.strip_prefix(&prefix) else {
            continue;
        };
        // Never scan our own backup tree.
        if rel.split('/').any(|seg| seg == BACKUP_DIR) {
            continue;
        }
        let parts: Vec<&str> = rel.split('/').collect();
        // Keep only `<dotdir…>/skills/<name>/SKILL.md`.
        if parts.len() < 4
            || parts[parts.len() - 1] != "SKILL.md"
            || parts[parts.len() - 3] != "skills"
        {
            continue;
        }
        let name = parts[parts.len() - 2];
        // The superseded sibling: `<same dotdir prefix>/commands/<name>.md`.
        let mut legacy_path = PathBuf::new();
        for seg in &parts[..parts.len() - 3] {
            legacy_path.push(seg);
        }
        legacy_path.push("commands");
        legacy_path.push(format!("{}.md", name));
        if !target_dir.join(&legacy_path).exists() {
            continue;
        }
        let superseded_by: PathBuf = parts.iter().collect();
        items.push(MigrationItem {
            legacy_path,
            superseded_by,
        });
    }
    items.sort_by(|a, b| a.legacy_path.cmp(&b.legacy_path));
    items
}

/// Whether a legacy command file is one Shipmates installed — and so safe to
/// migrate — rather than a user's own file that happens to share the name.
///
/// The filename must be `<name>.md` and the file's frontmatter `name:` must equal
/// `<name>`. A file that fails to parse, or declares a different (or no) `name:`,
/// is NEVER owned, so a user's own workflow at that path is left intact.
//
// #190: replace this name/content heuristic with receipt-manifest ownership once
// manifest_db lands.
pub fn is_shipmates_owned(legacy_path: &Path, name: &str) -> bool {
    if legacy_path.file_stem().and_then(|s| s.to_str()) != Some(name) {
        return false;
    }
    match parse_frontmatter(legacy_path) {
        Ok((fm, _)) => fm.get("name").map(|n| n == name).unwrap_or(false),
        Err(_) => false,
    }
}

/// A fresh, collision-proof backup directory under the target:
/// `<target>/.shipmates-backup/<timestamp>-<unique>/`. Each run gets its own so a
/// repeated migration never overwrites an earlier backup.
pub fn new_backup_root(target_dir: &Path) -> PathBuf {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    // Epoch-UTC seconds, plus sub-second and PID for a short unique suffix.
    let stamp = format!(
        "{}s{:09}-{}",
        now.as_secs(),
        now.subsec_nanos(),
        std::process::id()
    );
    target_dir.join(BACKUP_DIR).join(stamp)
}

/// Apply a migration plan: back up, then remove, each Shipmates-owned legacy file.
///
/// The order is strict — the backup is written and verified to exist BEFORE the
/// original is deleted; if the backup can't be written the delete is skipped, so
/// a file is never lost. Files that are not Shipmates-owned are recorded in
/// `skipped_unmanaged` and left exactly as they were. A plan entry whose legacy
/// file is already gone is a no-op, so a second run is idempotent.
pub fn apply(
    target_dir: &Path,
    items: &[MigrationItem],
    backup_root: &Path,
) -> Result<MigrationReport> {
    let mut report = MigrationReport::default();
    for item in items {
        let full_legacy = target_dir.join(&item.legacy_path);
        if !full_legacy.exists() {
            continue;
        }
        let name = item
            .legacy_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default();
        if !is_shipmates_owned(&full_legacy, name) {
            report.skipped_unmanaged.push(item.legacy_path.clone());
            continue;
        }
        // Back up first — read the bytes and write them through the same atomic
        // path the installer uses.
        let contents = fs::read_to_string(&full_legacy)
            .with_context(|| format!("reading legacy file {}", full_legacy.display()))?;
        let backup_path = backup_root.join(&item.legacy_path);
        if atomic_write(&backup_path, &contents).is_err() || !backup_path.exists() {
            // Backup failed — never delete without a verified copy.
            continue;
        }
        fs::remove_file(&full_legacy)
            .with_context(|| format!("removing superseded file {}", full_legacy.display()))?;
        report.migrated.push(item.legacy_path.clone());
        report.backups.push(backup_path);
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    const CONTAINER: &str = "harnesses/claude-code";

    fn built_with_skill(name: &str) -> HashMap<String, String> {
        let mut m = HashMap::new();
        m.insert(
            format!("{}/.claude/skills/{}/SKILL.md", CONTAINER, name),
            "skill body".to_string(),
        );
        m
    }

    fn owned_command(name: &str) -> String {
        format!("---\nname: {}\ndescription: legacy\n---\nold body\n", name)
    }

    #[test]
    fn test_owned_legacy_is_backed_up_and_removed() {
        let dir = tempdir().unwrap();
        let target = dir.path();
        // A prior install's legacy command sitting beside the current skill.
        let legacy = target.join(".claude/commands/ship-issue.md");
        atomic_write(&legacy, &owned_command("ship-issue")).unwrap();
        let skill = target.join(".claude/skills/ship-issue/SKILL.md");
        atomic_write(&skill, "current skill").unwrap();

        let built = built_with_skill("ship-issue");
        let items = plan(target, &built, CONTAINER);
        assert_eq!(items.len(), 1);
        assert_eq!(
            items[0].legacy_path,
            PathBuf::from(".claude/commands/ship-issue.md")
        );
        assert_eq!(
            items[0].superseded_by,
            PathBuf::from(".claude/skills/ship-issue/SKILL.md")
        );

        let backup_root = new_backup_root(target);
        let report = apply(target, &items, &backup_root).unwrap();

        assert_eq!(
            report.migrated,
            vec![PathBuf::from(".claude/commands/ship-issue.md")]
        );
        assert!(report.skipped_unmanaged.is_empty());
        // Legacy gone, backup present and faithful, skill untouched.
        assert!(!legacy.exists());
        assert_eq!(report.backups.len(), 1);
        assert!(report.backups[0].exists());
        assert_eq!(
            fs::read_to_string(&report.backups[0]).unwrap(),
            owned_command("ship-issue")
        );
        assert_eq!(fs::read_to_string(&skill).unwrap(), "current skill");
    }

    #[test]
    fn test_unmanaged_name_match_is_left_intact() {
        let dir = tempdir().unwrap();
        let target = dir.path();
        // A user's own file at the legacy path — foreign frontmatter name.
        let legacy = target.join(".claude/commands/ship-issue.md");
        let foreign = "---\nname: my-own-thing\n---\nkeep me\n";
        atomic_write(&legacy, foreign).unwrap();

        let items = plan(target, &built_with_skill("ship-issue"), CONTAINER);
        assert_eq!(items.len(), 1); // planned (the file exists); ownership decides its fate

        let backup_root = new_backup_root(target);
        let report = apply(target, &items, &backup_root).unwrap();

        assert!(report.migrated.is_empty());
        assert_eq!(
            report.skipped_unmanaged,
            vec![PathBuf::from(".claude/commands/ship-issue.md")]
        );
        assert!(legacy.exists());
        assert_eq!(fs::read_to_string(&legacy).unwrap(), foreign);
        assert!(!backup_root.exists()); // nothing was backed up
    }

    #[test]
    fn test_absent_frontmatter_is_not_owned() {
        let dir = tempdir().unwrap();
        let target = dir.path();
        atomic_write(
            &target.join(".claude/commands/ship-issue.md"),
            "just prose, no frontmatter\n",
        )
        .unwrap();
        let items = plan(target, &built_with_skill("ship-issue"), CONTAINER);
        let report = apply(target, &items, &new_backup_root(target)).unwrap();
        assert!(report.migrated.is_empty());
        assert_eq!(report.skipped_unmanaged.len(), 1);
    }

    #[test]
    fn test_no_legacy_yields_empty_plan() {
        let dir = tempdir().unwrap();
        let target = dir.path();
        // Only the skill exists — no legacy commands/ tree at all.
        atomic_write(&target.join(".claude/skills/ship-issue/SKILL.md"), "skill").unwrap();
        let items = plan(target, &built_with_skill("ship-issue"), CONTAINER);
        assert!(items.is_empty());
    }

    #[test]
    fn test_second_run_is_idempotent() {
        let dir = tempdir().unwrap();
        let target = dir.path();
        atomic_write(
            &target.join(".claude/commands/ship-issue.md"),
            &owned_command("ship-issue"),
        )
        .unwrap();
        let built = built_with_skill("ship-issue");

        let items = plan(target, &built, CONTAINER);
        let report = apply(target, &items, &new_backup_root(target)).unwrap();
        assert_eq!(report.migrated.len(), 1);

        // Second run: the legacy file is gone, so the plan is empty and nothing
        // new is backed up.
        let items2 = plan(target, &built, CONTAINER);
        assert!(items2.is_empty());
        let backup_root2 = new_backup_root(target);
        let report2 = apply(target, &items2, &backup_root2).unwrap();
        assert!(report2.migrated.is_empty());
        assert!(!backup_root2.exists());
    }
}
