//! Migrate a legacy `commands/<name>.md` layout to the skill it is now
//! superseded by (`skills/<name>/SKILL.md`).
//!
//! Old installs dropped each workflow as a flat command file; the current
//! payload ships it as a skill. A skill beats a same-named legacy command, so a
//! stale command file is dead weight at best and a shadow at worst. This module
//! plans and applies the cleanup — always backing a file up before removing it,
//! and only ever touching files Shipmates itself installed.

use crate::catalog::parse_frontmatter;
use crate::installer::atomic_write_bytes;
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
///
/// The invariant is structural: a skill supersedes a legacy command only when
/// the current payload no longer ships that command. If the built payload itself
/// still ships `…/commands/<name>.md`, that command is live — it is kept, never
/// scheduled for removal — even when a same-named skill is written beside it.
/// Plan migration, rejecting symlink components before inspecting whether a
/// legacy file exists.
pub fn plan(
    target_dir: &Path,
    built: &HashMap<String, String>,
    container: &str,
) -> Result<Vec<MigrationItem>> {
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
        crate::installer::manifest_db::resolve_target_relative(target_dir, &legacy_path)?;
        // Structural self-limiting: if the CURRENT payload still ships this
        // command, the skill does not supersede it — never schedule a live file
        // for deletion, even when a same-named skill is written beside it.
        if built.contains_key(&format!("{}/{}", container, legacy_path.display())) {
            continue;
        }
        if fs::symlink_metadata(target_dir.join(&legacy_path)).is_err() {
            continue;
        }
        let superseded_by: PathBuf = parts.iter().collect();
        items.push(MigrationItem {
            legacy_path,
            superseded_by,
        });
    }
    items.sort_by(|a, b| a.legacy_path.cmp(&b.legacy_path));
    Ok(items)
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
    let backup_relative = backup_root.strip_prefix(target_dir).map_err(|error| {
        anyhow::anyhow!(
            "migration backup escaped target {}: {}",
            backup_root.display(),
            error
        )
    })?;
    crate::installer::manifest_db::resolve_target_relative(target_dir, backup_relative)?;
    for item in items {
        let result = (|| -> Result<()> {
            // Resolve immediately before every read/delete. Do not let a
            // symlink inserted after planning redirect migration elsewhere.
            let full_legacy = crate::installer::manifest_db::resolve_target_relative(
                target_dir,
                &item.legacy_path,
            )?;
            let metadata = match fs::symlink_metadata(&full_legacy) {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
                Err(error) => {
                    return Err(error).with_context(|| {
                        format!("inspecting legacy file {}", full_legacy.display())
                    })
                }
            };
            if !metadata.file_type().is_file() {
                report.skipped_unmanaged.push(item.legacy_path.clone());
                return Ok(());
            }
            let name = item
                .legacy_path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or_default();
            if !is_shipmates_owned(&full_legacy, name) {
                report.skipped_unmanaged.push(item.legacy_path.clone());
                return Ok(());
            }
            // Back up raw bytes first. Legacy files are normally text, but receipt
            // ownership and rollback must not make UTF-8 a safety requirement.
            let contents = fs::read(&full_legacy)
                .with_context(|| format!("reading legacy file {}", full_legacy.display()))?;
            let backup_path = backup_root.join(&item.legacy_path);
            let backup_relative = backup_path.strip_prefix(target_dir).map_err(|error| {
                anyhow::anyhow!("migration backup escaped target: {}", error)
            })?;
            let backup_path =
                crate::installer::manifest_db::resolve_target_relative(target_dir, backup_relative)?;
            atomic_write_bytes(&backup_path, &contents)
                .with_context(|| format!("backing up legacy file {}", full_legacy.display()))?;
            let backup_relative = backup_path
                .strip_prefix(target_dir)
                .map_err(|error| anyhow::anyhow!("migration backup escaped target: {}", error))?;
            let verified_backup = crate::installer::manifest_db::resolve_target_relative(
                target_dir,
                backup_relative,
            )?;
            if fs::read(&verified_backup)
                .map(|backup| backup != contents)
                .unwrap_or(true)
            {
                let _ = fs::remove_file(&verified_backup);
                anyhow::bail!("backup verification failed for {}", full_legacy.display());
            }
            // Record backup before delete so an error in the delete itself is
            // still covered by the transaction rollback.
            report.migrated.push(item.legacy_path.clone());
            report.backups.push(backup_path.clone());
            let full_legacy = crate::installer::manifest_db::resolve_target_relative(
                target_dir,
                &item.legacy_path,
            )?;
            fs::remove_file(&full_legacy)
                .with_context(|| format!("removing superseded file {}", full_legacy.display()))?;
            Ok(())
        })();
        if let Err(error) = result {
            let rollback = rollback(target_dir, &report);
            return Err(combine_rollback_error(error, rollback));
        }
    }
    Ok(report)
}

/// Restore files migrated by a successful apply. Used when a later payload or
/// receipt operation fails, so migration never becomes an irreversible side
/// effect of an unsuccessful install.
pub fn rollback(target_dir: &Path, report: &MigrationReport) -> Result<()> {
    for (legacy, backup) in report.migrated.iter().zip(&report.backups).rev() {
        let backup_relative = backup
            .strip_prefix(target_dir)
            .map_err(|error| anyhow::anyhow!("migration backup escaped target: {}", error))?;
        let backup_path =
            crate::installer::manifest_db::resolve_target_relative(target_dir, backup_relative)?;
        let bytes = fs::read(&backup_path)
            .with_context(|| format!("reading migration backup {}", backup_path.display()))?;
        let legacy_path =
            crate::installer::manifest_db::resolve_target_relative(target_dir, legacy)?;
        atomic_write_bytes(&legacy_path, &bytes)
            .with_context(|| format!("restoring migrated file {}", legacy_path.display()))?;
        let legacy_path =
            crate::installer::manifest_db::resolve_target_relative(target_dir, legacy)?;
        if fs::read(&legacy_path).map(|restored| restored != bytes).unwrap_or(true) {
            anyhow::bail!("migration rollback verification failed for {}", legacy_path.display());
        }
        let backup_path =
            crate::installer::manifest_db::resolve_target_relative(target_dir, backup_relative)?;
        fs::remove_file(&backup_path)
            .with_context(|| format!("removing migration backup {}", backup_path.display()))?;
    }
    Ok(())
}

fn combine_rollback_error(error: anyhow::Error, rollback: Result<()>) -> anyhow::Error {
    match rollback {
        Ok(()) => error,
        Err(rollback) => error.context(rollback.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::installer::atomic_write;
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
        let items = plan(target, &built, CONTAINER).unwrap();
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

        let items = plan(target, &built_with_skill("ship-issue"), CONTAINER).unwrap();
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
        let items = plan(target, &built_with_skill("ship-issue"), CONTAINER).unwrap();
        let report = apply(target, &items, &new_backup_root(target)).unwrap();
        assert!(report.migrated.is_empty());
        assert_eq!(report.skipped_unmanaged.len(), 1);
    }

    #[test]
    fn test_payload_shipping_both_skill_and_command_keeps_the_command() {
        let dir = tempdir().unwrap();
        let target = dir.path();
        // A legacy command sits on disk...
        let legacy = target.join(".claude/commands/x.md");
        atomic_write(&legacy, &owned_command("x")).unwrap();
        // ...but the CURRENT payload ships BOTH the skill AND the command for x.
        let mut built = built_with_skill("x");
        built.insert(
            format!("{}/.claude/commands/x.md", CONTAINER),
            "current command".to_string(),
        );
        // The command is live, so it must never be scheduled for removal.
        let items = plan(target, &built, CONTAINER).unwrap();
        assert!(
            items.is_empty(),
            "a command the current payload still ships must not be scheduled for deletion"
        );
        assert!(legacy.exists());
    }

    #[test]
    fn test_no_legacy_yields_empty_plan() {
        let dir = tempdir().unwrap();
        let target = dir.path();
        // Only the skill exists — no legacy commands/ tree at all.
        atomic_write(&target.join(".claude/skills/ship-issue/SKILL.md"), "skill").unwrap();
        let items = plan(target, &built_with_skill("ship-issue"), CONTAINER).unwrap();
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

        let items = plan(target, &built, CONTAINER).unwrap();
        let report = apply(target, &items, &new_backup_root(target)).unwrap();
        assert_eq!(report.migrated.len(), 1);

        // Second run: the legacy file is gone, so the plan is empty and nothing
        // new is backed up.
        let items2 = plan(target, &built, CONTAINER).unwrap();
        assert!(items2.is_empty());
        let backup_root2 = new_backup_root(target);
        let report2 = apply(target, &items2, &backup_root2).unwrap();
        assert!(report2.migrated.is_empty());
        assert!(!backup_root2.exists());
    }

    #[test]
    fn rollback_restores_migrated_bytes_and_removes_backup() {
        let dir = tempdir().unwrap();
        let target = dir.path();
        let legacy = target.join(".claude/commands/ship-issue.md");
        let bytes = owned_command("ship-issue").into_bytes();
        atomic_write_bytes(&legacy, &bytes).unwrap();
        let items = plan(target, &built_with_skill("ship-issue"), CONTAINER).unwrap();
        let backup_root = new_backup_root(target);
        let report = apply(target, &items, &backup_root).unwrap();

        rollback(target, &report).unwrap();

        assert_eq!(fs::read(&legacy).unwrap(), bytes);
        assert!(report.backups.iter().all(|backup| !backup.exists()));
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_legacy_component_is_rejected_before_read_or_delete() {
        use std::os::unix::fs::symlink;

        let dir = tempdir().unwrap();
        let target = dir.path();
        let outside = tempdir().unwrap();
        let outside_file = outside.path().join("ship-issue.md");
        fs::write(&outside_file, owned_command("ship-issue")).unwrap();
        fs::create_dir_all(target.join(".claude")).unwrap();
        symlink(outside.path(), target.join(".claude/commands")).unwrap();

        let error = plan(target, &built_with_skill("ship-issue"), CONTAINER).unwrap_err();

        assert!(error.to_string().contains("symlink component"));
        assert!(outside_file.exists());
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_backup_root_is_rejected_before_migration() {
        use std::os::unix::fs::symlink;

        let dir = tempdir().unwrap();
        let target = dir.path();
        let outside = tempdir().unwrap();
        let legacy = target.join(".claude/commands/ship-issue.md");
        atomic_write(&legacy, &owned_command("ship-issue")).unwrap();
        symlink(outside.path(), target.join(BACKUP_DIR)).unwrap();

        let items = plan(target, &built_with_skill("ship-issue"), CONTAINER).unwrap();
        let error = apply(target, &items, &new_backup_root(target)).unwrap_err();

        assert!(error.to_string().contains("symlink component"));
        assert!(legacy.exists());
        assert!(outside.path().read_dir().unwrap().next().is_none());
    }
}
