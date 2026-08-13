//! Migrate a legacy `commands/<name>.md` layout to the skill it is now
//! superseded by (`skills/<name>/SKILL.md`).
//!
//! Old installs dropped each workflow as a flat command file; the current
//! payload ships it as a skill. A skill beats a same-named legacy command, so a
//! stale command file is dead weight at best and a shadow at worst. This module
//! plans and applies the cleanup — always backing a file up before removing it,
//! and only ever touching files Shipmates itself installed.

use crate::digest;
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
///
/// The invariant is structural: a skill supersedes a legacy command only when
/// the current payload no longer ships that command. If the built payload itself
/// still ships `…/commands/<name>.md`, that command is live — it is kept, never
/// scheduled for removal — even when a same-named skill is written beside it.
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
        // Structural self-limiting: if the CURRENT payload still ships this
        // command, the skill does not supersede it — never schedule a live file
        // for deletion, even when a same-named skill is written beside it.
        if built.contains_key(&format!("{}/{}", container, legacy_path.display())) {
            continue;
        }
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

/// SHA-256 signatures for the last canonical command payload before migration
/// receipts existed. A receipt-free file is removable only when its complete
/// bytes match one of these historical payloads; name/frontmatter matches alone
/// remain ambiguous.
fn historical_signature(name: &str) -> Option<&'static str> {
    match name {
        "document" => Some("8f61d2991fc5a655faa92b2a1b8bef9245d54ed5a77131b8da4569b688b83caf"),
        "fix-bug" => Some("bff39504a32db690a9247c6f62bc47ef0308884f680a83f9c9afadc236a2e2df"),
        "harden" => Some("d99a1ebb84c84fe599e52b79833ba54b2e0d1ac4bd5f7c35354c36ff7c45f951"),
        "migrate" => Some("52e3a74566e89b1362f226151ac506d203bcb86376ca2c68f47b9a7743dc269e"),
        "onboard" => Some("634389700141dd533b2c9a2a42b4789251a9519b0bb5c860d4d3cdb62cbed4a9"),
        "plan-epics" => Some("7ddc27b01597cdbaa3b11d7ce8467cc31a53c4ecc08675bb83fab706eb033fe4"),
        "polish" => Some("71df68ffb3ddc02636874fe3a2055d0870222d90be46bf4dd46a3018fc70e2ba"),
        "pr-review" => Some("0dc39884a425108b729e028bc2cc7957fc68728ef43420b50a61109752be5351"),
        "refactor" => Some("77dc95d7d20f099a6f030d1c66a540f58073589196dc9ce1e2f480037bc50304"),
        "release" => Some("95d380f72b7f1a77212d730ac2f262f0d401268decb7a500932cce5d729aa4dd"),
        "ship-issue" => Some("d967b980f4747790ec278f1093b0ede3ec933e259211fb1c6121588df8a6f9fc"),
        "spike" => Some("160080959a06cf7f2f78de3cba07ff47fdb5ccfdb407583059e68cb486bf3437"),
        _ => None,
    }
}

/// Whether a legacy command file has verified Shipmates ownership.
///
/// Receipt-free installs cannot prove ownership from a filename or frontmatter:
/// users can create the same command name and metadata. Exact bytes from the
/// historical canonical payload are accepted; every other match is left intact.
pub fn is_shipmates_owned(legacy_path: &Path, name: &str) -> bool {
    if legacy_path.file_stem().and_then(|s| s.to_str()) != Some(name) {
        return false;
    }
    let Some(signature) = historical_signature(name) else {
        return false;
    };
    fs::read_to_string(legacy_path)
        .ok()
        .is_some_and(|contents| digest::hash(&contents) == signature)
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
    fn test_receipt_free_match_is_skipped() {
        let dir = tempdir().unwrap();
        let target = dir.path();
        // A same-named receipt-free command sitting beside the current skill.
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

        assert!(report.migrated.is_empty());
        assert!(report.backups.is_empty());
        assert_eq!(
            report.skipped_unmanaged,
            vec![PathBuf::from(".claude/commands/ship-issue.md")]
        );
        // Matching name/frontmatter is ambiguous without receipt ownership.
        assert!(legacy.exists());
        assert_eq!(
            fs::read_to_string(&legacy).unwrap(),
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
        let items = plan(target, &built, CONTAINER);
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
        assert!(report.migrated.is_empty());
        assert_eq!(report.skipped_unmanaged.len(), 1);
        assert!(target.join(".claude/commands/ship-issue.md").exists());

        // Second run remains safe and idempotent: no receipt-free deletion.
        let items2 = plan(target, &built, CONTAINER);
        let backup_root2 = new_backup_root(target);
        let report2 = apply(target, &items2, &backup_root2).unwrap();
        assert!(report2.migrated.is_empty());
        assert_eq!(report2.skipped_unmanaged.len(), 1);
        assert!(!backup_root2.exists());
    }
}
