//! Receipt-aware payload application.

use crate::installer::{
    atomic_write,
    plan::{self, InstallPlan, Receipt, ReceiptState},
};
use anyhow::{Context, Result, bail};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static BACKUP_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Default)]
pub struct UpgradeSummary {
    pub changed: usize,
    pub new: usize,
    pub removed: usize,
}

#[derive(Debug, Clone, Default)]
pub struct ApplyReport {
    pub written: usize,
    pub skipped: usize,
    pub backups: Vec<PathBuf>,
    pub warnings: Vec<String>,
    pub summary: UpgradeSummary,
    pub previous_version: Option<String>,
    pub receipt: Option<Receipt>,
}

/// Apply normalized payload. Existing receipts make ownership explicit: a new
/// payload never overwrites an unrelated file at a colliding path unless
/// `force` is set. Missing receipts still permit new files, but preserve
/// existing collisions.
///
/// `force_hint` is the exact `shipmates install … --force` invocation the
/// captain should re-run on a third-party refusal (#392) — never a bare
/// `shipmates install --force`.
pub fn apply(
    target_dir: &Path,
    install: &InstallPlan,
    force: bool,
    force_hint: &str,
) -> Result<ApplyReport> {
    apply_with_preserved_paths(target_dir, install, force, &BTreeSet::new(), force_hint)
}

/// Apply an install while retaining receipt ownership for migration items that
/// were deliberately left in place (`--no-migrate` or a skipped migration).
pub fn apply_with_preserved_paths(
    target_dir: &Path,
    install: &InstallPlan,
    force: bool,
    preserved_paths: &BTreeSet<String>,
    force_hint: &str,
) -> Result<ApplyReport> {
    let repository = crate::installer::manifest_db::ReceiptRepository::new(target_dir);
    // Validate complete receipt set before inspecting or changing payload
    // files. A sibling receipt is part of ownership state, even when it is
    // unrelated to this harness.
    let all_receipts = repository
        .load_all()
        .context("validating install receipt set")?;
    let (state, old, receipt_error) = plan::read_receipt(target_dir, &install.harness);
    if state == ReceiptState::Invalid {
        bail!(
            "install receipt for harness {} is invalid; refusing to install: {}",
            install.harness,
            receipt_error.unwrap_or_else(|| "unknown receipt error".into())
        );
    }
    let mut report = ApplyReport::default();
    report.previous_version = old.as_ref().map(|receipt| receipt.version.clone());

    if let Some(old_receipt) = old.as_ref() {
        report.summary = compare_receipts(
            old_receipt,
            &install.receipt_for(install.files.keys().cloned())?,
        );
    } else if state == ReceiptState::Missing {
        report.summary.new = install.files.len();
    }

    let sibling_claims: BTreeSet<String> = all_receipts
        .iter()
        .filter(|receipt| receipt.harness != install.harness)
        .flat_map(|receipt| receipt.files.iter().map(|file| file.path.clone()))
        .collect();
    let mut managed = Vec::new();
    let mut pending = Vec::new();
    let mut third_party: Vec<PathBuf> = Vec::new();
    for (rel, want) in &install.files {
        let path = crate::installer::manifest_db::resolve_target_relative(target_dir, rel)?;
        let rel_string = rel.to_string_lossy().into_owned();
        let owned = old
            .as_ref()
            .and_then(|receipt| receipt.file(&rel_string))
            .is_some();
        if sibling_claims.contains(&rel_string) {
            // A shared payload tree is co-owned, not mutex-guarded: any harness
            // may advance bytes it can attribute to a recorded receipt digest.
            // Bytes no receipt vouches for are the user's own edit and stay
            // byte-identical, `--force` or not.
            let known = recorded_digests(&all_receipts, &rel_string);
            match fs::read(&path) {
                Ok(current) if current == want.as_bytes() => {
                    managed.push(rel.clone());
                }
                Ok(current) if known.contains(&crate::digest::hash_bytes(&current)) => {
                    pending.push(PendingWrite {
                        rel: rel.clone(),
                        path,
                        content: want.as_bytes().to_vec(),
                        previous: Some(current),
                    });
                }
                Ok(_) => {
                    report.warnings.push(format!(
                        "Warning: shared-managed file left untouched; current bytes match neither desired nor any recorded ownership: {}",
                        rel.display()
                    ));
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    pending.push(PendingWrite {
                        rel: rel.clone(),
                        path,
                        content: want.as_bytes().to_vec(),
                        previous: None,
                    });
                }
                Err(error) => {
                    return Err(error)
                        .with_context(|| format!("preflighting installed file {}", path.display()));
                }
            }
            continue;
        }
        match fs::read(&path) {
            Ok(current) if current == want.as_bytes() => {
                // The bytes are already ours whoever wrote them. Claim the path
                // so the receipt stops omitting a file the payload owns; there
                // is nothing to back up.
                managed.push(rel.clone());
                report.skipped += 1;
            }
            Ok(current) => {
                if !owned && !force {
                    // Adopt a file that declares itself to be this artifact;
                    // refuse the whole install for anything else, rather than
                    // publishing a receipt that quietly omits it (#386).
                    match crate::installer::adopt::classify(rel, &current) {
                        crate::installer::adopt::Collision::Adoptable => {
                            pending.push(PendingWrite {
                                rel: rel.clone(),
                                path,
                                content: want.as_bytes().to_vec(),
                                previous: Some(current),
                            });
                        }
                        crate::installer::adopt::Collision::ThirdParty => {
                            third_party.push(rel.clone());
                        }
                    }
                    continue;
                }
                if std::str::from_utf8(&current).is_err() && !force {
                    report.warnings.push(format!(
                        "Warning: non-text file left untouched: {}",
                        rel.display()
                    ));
                    continue;
                }
                pending.push(PendingWrite {
                    rel: rel.clone(),
                    path,
                    content: want.as_bytes().to_vec(),
                    previous: Some(current),
                });
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                pending.push(PendingWrite {
                    rel: rel.clone(),
                    path,
                    content: want.as_bytes().to_vec(),
                    previous: None,
                });
            }
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("preflighting installed file {}", path.display()));
            }
        }
    }

    // Every collision is decided before the first byte moves, so a refusal
    // leaves the tree exactly as it was found.
    if !third_party.is_empty() {
        let paths: Vec<String> = third_party
            .iter()
            .map(|rel| rel.display().to_string())
            .collect();
        bail!(
            "refusing to install over {} file(s) shipmates does not own at payload path(s): {}. \
             Re-run with `{force_hint}` to back each one up and overwrite it, or \
             move them aside first.",
            paths.len(),
            paths.join(", ")
        );
    }

    if let Some(old_receipt) = old.as_ref() {
        for old_file in &old_receipt.files {
            if install.files.contains_key(Path::new(&old_file.path)) {
                continue;
            }
            if preserved_paths.contains(&old_file.path) {
                continue;
            }
            if sibling_claims.contains(&old_file.path) {
                report.warnings.push(format!(
                    "Warning: shared-managed file preserved (no longer in payload): {}",
                    old_file.path
                ));
                continue;
            }
            let path = crate::installer::manifest_db::resolve_target_relative(
                target_dir,
                Path::new(&old_file.path),
            )?;
            if fs::symlink_metadata(&path).is_ok() {
                // Intentional drop from the payload (e.g. `update --with-tools none`):
                // remove the live file and any installer bak sidecars, then prune
                // emptied husk dirs. Do NOT write a new bak — that would leave
                // doctor treating an intentional removal as an interrupted update
                // forever (#418). Mid-write overwrite backups still use
                // `backup_existing` on the write path below.
                for bak in plan::sibling_install_backups(&path) {
                    let _ = fs::remove_file(&bak);
                }
                fs::remove_file(&path)
                    .with_context(|| format!("removing dropped file {}", path.display()))?;
                prune_empty_parents(target_dir, Path::new(&old_file.path))?;
                report
                    .warnings
                    .push(format!("Removed dropped file: {}", old_file.path));
            } else {
                // Live file already gone — still clear leftover bak husks from a
                // prior drop that left sidecars behind (#418).
                for bak in plan::sibling_install_backups(&path) {
                    let _ = fs::remove_file(&bak);
                }
                prune_empty_parents(target_dir, Path::new(&old_file.path))?;
            }
        }
    }

    let mut changed: Vec<(PathBuf, Option<Vec<u8>>)> = Vec::new();
    let mut created_backups = Vec::new();
    for action in &pending {
        if let Some(previous) = &action.previous {
            let backup = match backup_existing(&action.path, previous) {
                Ok(backup) => backup,
                Err(error) => {
                    rollback_files(&changed, &created_backups);
                    return Err(error);
                }
            };
            if let Some(backup) = backup {
                report.backups.push(backup.clone());
                created_backups.push(backup);
            }
        }
        if let Err(error) = crate::installer::atomic_write_bytes(&action.path, &action.content)
            .with_context(|| format!("writing installed file {}", action.path.display()))
        {
            rollback_files(&changed, &created_backups);
            return Err(error);
        }
        changed.push((action.path.clone(), action.previous.clone()));
        managed.push(action.rel.clone());
        report.written += 1;
    }

    managed.sort();
    // Publish only what this run actually owns. This preserves a user's file at
    // a new colliding path and makes a later uninstall fail closed for it.
    let mut receipt = install.receipt_for(managed)?;
    if let Some(old_receipt) = old.as_ref() {
        for old_file in &old_receipt.files {
            let path = Path::new(&old_file.path);
            let preserve = preserved_paths.contains(&old_file.path);
            let unchanged_on_disk = fs::read(
                crate::installer::manifest_db::resolve_target_relative(target_dir, path)?,
            )
            .map(|bytes| crate::digest::hash_bytes(&bytes) == old_file.sha256)
            .unwrap_or(false);
            // A shared path this receipt claimed before stays claimed even when
            // its unattributable bytes were preserved rather than overwritten:
            // ownership lapses silently otherwise. Uninstall stays fail-closed
            // for the mismatched bytes, so the claim is not deletion authority.
            let shared_claim_before =
                sibling_claims.contains(&old_file.path) && install.files.contains_key(path);
            if (preserve || install.files.contains_key(path))
                && !receipt.files.iter().any(|file| file.path == old_file.path)
                && (preserve || unchanged_on_disk || shared_claim_before)
            {
                receipt.files.push(old_file.clone());
            }
        }
        receipt
            .files
            .sort_by(|left, right| left.path.cmp(&right.path));
        receipt = Receipt::new(
            receipt.version.clone(),
            receipt.harness.clone(),
            receipt.layout.clone(),
            receipt.roots.clone(),
            receipt.files,
        )?;
    }
    // Unmanaged files are reported against what this run actually published,
    // not against the receipt it superseded: a path `--force` just overwrote
    // and claimed is managed, and saying otherwise in the same breath is the
    // contradiction #404 reported. Only an upgrade reports — a first install
    // has no prior state to account for.
    if old.is_some() {
        let owned: BTreeSet<String> = receipt
            .files
            .iter()
            .map(|file| file.path.clone())
            .collect::<BTreeSet<_>>();
        for path in plan::unmanaged_files(target_dir, &owned) {
            let relative = path.strip_prefix(target_dir).unwrap_or(&path);
            // A sibling harness's claim is ownership too, just not ours.
            if sibling_claims.contains(&relative.to_string_lossy().into_owned()) {
                continue;
            }
            report.warnings.push(format!(
                "Warning: unmanaged file left untouched: {}",
                relative.display()
            ));
        }
    }

    let receipt_path = repository.receipt_path(&receipt.harness)?;
    let previous_receipt = fs::read(&receipt_path).ok();
    if let Err(error) = plan::save_receipt(target_dir, &receipt) {
        rollback_files(&changed, &created_backups);
        if let Some(bytes) = previous_receipt {
            if let Ok(contents) = String::from_utf8(bytes) {
                let _ = atomic_write(&receipt_path, &contents);
            }
        } else {
            let _ = fs::remove_file(&receipt_path);
        }
        return Err(error).context("publishing install receipt");
    }
    report.receipt = Some(receipt);
    Ok(report)
}

/// Every digest any receipt has recorded for `rel`. Bytes matching one are
/// known payload bytes — a stale copy a sibling harness wrote — not a user
/// edit, so a co-owner may advance them.
fn recorded_digests(receipts: &[Receipt], rel: &str) -> BTreeSet<String> {
    receipts
        .iter()
        .filter_map(|receipt| receipt.file(rel))
        .map(|file| file.sha256.clone())
        .collect()
}

fn compare_receipts(old: &Receipt, new: &Receipt) -> UpgradeSummary {
    let mut summary = UpgradeSummary::default();
    for file in &new.files {
        match old
            .files
            .iter()
            .find(|candidate| candidate.path == file.path)
        {
            Some(previous) if previous.sha256 != file.sha256 => summary.changed += 1,
            Some(_) => {}
            None => summary.new += 1,
        }
    }
    summary.removed = old
        .files
        .iter()
        .filter(|previous| !new.files.iter().any(|file| file.path == previous.path))
        .count();
    summary
}

struct PendingWrite {
    rel: PathBuf,
    path: PathBuf,
    content: Vec<u8>,
    previous: Option<Vec<u8>>,
}

fn rollback_files(changed: &[(PathBuf, Option<Vec<u8>>)], backups: &[PathBuf]) {
    for (path, previous) in changed.iter().rev() {
        match previous {
            Some(bytes) => {
                let _ = crate::installer::atomic_write_bytes(path, bytes);
            }
            None => {
                let _ = fs::remove_file(path);
            }
        }
    }
    for backup in backups {
        let _ = fs::remove_file(backup);
    }
}

/// Remove emptied identity directories above a dropped file, stopping at the
/// install tree (`skills` / `tools` / …) or a harness root (`.claude`, …).
fn prune_empty_parents(target_dir: &Path, file_rel: &Path) -> Result<()> {
    let Some(mut parent) = file_rel.parent() else {
        return Ok(());
    };
    loop {
        let Some(name) = parent.file_name().and_then(|name| name.to_str()) else {
            break;
        };
        if matches!(name, "skills" | "commands" | "tools" | "agents") || name.starts_with('.') {
            break;
        }
        let full = crate::installer::manifest_db::resolve_target_relative(target_dir, parent)?;
        match fs::remove_dir(&full) {
            Ok(()) => {}
            Err(_) => break,
        }
        parent = match parent.parent() {
            Some(next) if !next.as_os_str().is_empty() => next,
            _ => break,
        };
    }
    Ok(())
}

pub(crate) fn backup_existing(path: &Path, bytes: &[u8]) -> Result<Option<PathBuf>> {
    let Some(parent) = path.parent() else {
        return Ok(None);
    };
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("file");
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let counter = BACKUP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let backup = parent.join(format!(
        "{}.bak-{}-{}-{}",
        name,
        now.as_secs(),
        std::process::id(),
        counter
    ));
    if fs::symlink_metadata(&backup).is_ok() {
        bail!("refusing existing backup path {}", backup.display());
    }
    // Backups are raw bytes. `--force` must not destroy a binary file merely
    // because the payload itself happens to be text.
    crate::installer::atomic_write_bytes(&backup, bytes)
        .with_context(|| format!("backing up {}", path.display()))?;
    if fs::read(&backup)? != bytes {
        anyhow::bail!("backup verification failed for {}", path.display());
    }
    Ok(Some(backup))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use tempfile::tempdir;

    const FORCE_HINT: &str = "shipmates install --harness claude-code --dir /tmp --force";

    fn install(_target: &Path, version: &str, files: &[(&str, &str)]) -> InstallPlan {
        InstallPlan {
            harness: "claude-code".into(),
            version: version.into(),
            layout: "skills".into(),
            roots: vec![".claude".into()],
            files: files
                .iter()
                .map(|(p, c)| (PathBuf::from(p), (*c).into()))
                .collect::<BTreeMap<_, _>>(),
        }
    }

    /// A plan for one of the harnesses that co-own the shared `.agents/skills`
    /// tree (codex, antigravity, github-copilot).
    fn shared_install(harness: &str, version: &str, files: &[(&str, &str)]) -> InstallPlan {
        InstallPlan {
            harness: harness.into(),
            version: version.into(),
            layout: "skills".into(),
            roots: vec![".agents".into()],
            files: files
                .iter()
                .map(|(p, c)| (PathBuf::from(p), (*c).into()))
                .collect::<BTreeMap<_, _>>(),
        }
    }

    fn deferral_warnings(report: &ApplyReport) -> Vec<&str> {
        report
            .warnings
            .iter()
            .filter(|warning| warning.contains("shared-managed file left untouched"))
            .map(String::as_str)
            .collect()
    }

    #[test]
    fn unchanged_reinstall_writes_no_backup() {
        let dir = tempdir().unwrap();
        let first = install(dir.path(), "one", &[(".claude/agents/a.md", "a")]);
        let result = apply(dir.path(), &first, false, FORCE_HINT).unwrap();
        assert_eq!(result.written, 1);
        let second = apply(dir.path(), &first, false, FORCE_HINT).unwrap();
        assert_eq!(second.written, 0);
        assert!(second.backups.is_empty());
    }

    #[test]
    fn changed_receipt_owned_file_is_backed_up() {
        let dir = tempdir().unwrap();
        let first = install(dir.path(), "one", &[(".claude/agents/a.md", "a")]);
        apply(dir.path(), &first, false, FORCE_HINT).unwrap();
        let second = install(dir.path(), "two", &[(".claude/agents/a.md", "b")]);
        let result = apply(dir.path(), &second, false, FORCE_HINT).unwrap();
        assert_eq!(result.written, 1);
        assert_eq!(fs::read_to_string(&result.backups[0]).unwrap(), "a");
        assert_eq!(
            fs::read_to_string(dir.path().join(".claude/agents/a.md")).unwrap(),
            "b"
        );
    }

    fn unmanaged_warnings(report: &ApplyReport) -> Vec<&str> {
        report
            .warnings
            .iter()
            .filter(|warning| warning.contains("unmanaged file left untouched"))
            .map(String::as_str)
            .collect()
    }

    #[test]
    fn force_written_file_is_not_reported_unmanaged() {
        let dir = tempdir().unwrap();
        let skill = ".claude/skills/polish/SKILL.md";
        let first = install(dir.path(), "one", &[(".claude/agents/a.md", "a")]);
        apply(dir.path(), &first, false, FORCE_HINT).unwrap();
        // A file the old receipt never claimed, at a path the new payload owns.
        crate::installer::atomic_write(&dir.path().join(skill), "theirs").unwrap();

        let second = install(
            dir.path(),
            "two",
            &[(".claude/agents/a.md", "a"), (skill, "ours")],
        );
        let report = apply(dir.path(), &second, true, FORCE_HINT).unwrap();

        assert_eq!(report.written, 1);
        assert_eq!(
            fs::read_to_string(dir.path().join(skill)).unwrap(),
            "ours",
            "--force must overwrite the collision"
        );
        assert!(
            unmanaged_warnings(&report).is_empty(),
            "a path this run claimed is not unmanaged: {:?}",
            report.warnings
        );
        assert!(
            report.receipt.as_ref().unwrap().file(skill).is_some(),
            "the force-written path must keep its receipt claim"
        );
    }

    #[test]
    fn genuinely_unmanaged_file_is_still_reported() {
        let dir = tempdir().unwrap();
        let skill = ".claude/skills/polish/SKILL.md";
        let plan_one = install(dir.path(), "one", &[(skill, "ours")]);
        apply(dir.path(), &plan_one, false, FORCE_HINT).unwrap();
        crate::installer::atomic_write(&dir.path().join(".claude/skills/mine/SKILL.md"), "mine")
            .unwrap();

        let plan_two = install(dir.path(), "two", &[(skill, "ours v2")]);
        let report = apply(dir.path(), &plan_two, false, FORCE_HINT).unwrap();

        assert_eq!(
            unmanaged_warnings(&report),
            vec!["Warning: unmanaged file left untouched: .claude/skills/mine/SKILL.md"]
        );
        assert!(
            !report.backups.is_empty(),
            "the upgrade backs the changed file up"
        );
    }

    #[test]
    fn preserved_path_is_not_deleted_when_dropped_from_payload() {
        let dir = tempdir().unwrap();
        let first = install(
            dir.path(),
            "one",
            &[
                (".claude/agents/a.md", "a"),
                (".claude/skills/polish/SKILL.md", "old polish"),
            ],
        );
        apply(dir.path(), &first, false, FORCE_HINT).unwrap();

        let second = install(
            dir.path(),
            "two",
            &[
                (".claude/agents/a.md", "a"),
                (".claude/skills/ship-polish/SKILL.md", "new polish"),
            ],
        );
        let mut preserved = BTreeSet::new();
        preserved.insert(".claude/skills/polish/SKILL.md".into());
        apply_with_preserved_paths(dir.path(), &second, false, &preserved, FORCE_HINT).unwrap();

        assert_eq!(
            fs::read_to_string(dir.path().join(".claude/skills/polish/SKILL.md")).unwrap(),
            "old polish",
            "preserved_paths must keep the file on disk, not only the receipt claim"
        );
        assert_eq!(
            fs::read_to_string(dir.path().join(".claude/skills/ship-polish/SKILL.md"))
                .unwrap(),
            "new polish"
        );
        let receipt = crate::installer::plan::read_receipt(dir.path(), "claude-code")
            .1
            .unwrap();
        assert!(receipt.file(".claude/skills/polish/SKILL.md").is_some());
        assert!(
            receipt
                .file(".claude/skills/ship-polish/SKILL.md")
                .is_some()
        );
    }

    const SHARED_SKILL: &str = ".agents/skills/ship-polish/SKILL.md";

    /// Two shared-tree receipts at the same bytes, then a codex update.
    fn two_receipt_shared_fixture(dir: &Path) {
        apply(
            dir,
            &shared_install("codex", "one", &[(SHARED_SKILL, "v1")]),
            false,
            FORCE_HINT,
        )
        .unwrap();
        apply(
            dir,
            &shared_install("github-copilot", "one", &[(SHARED_SKILL, "v1")]),
            false,
            FORCE_HINT,
        )
        .unwrap();
    }

    #[test]
    fn shared_bytes_attributable_to_a_receipt_are_advanced() {
        let dir = tempdir().unwrap();
        let target = dir.path();
        two_receipt_shared_fixture(target);

        // `update` passes force=true; stale v1 bytes belong to codex's own
        // receipt and the sibling's, so they are ours to refresh.
        let report = apply(
            target,
            &shared_install("codex", "two", &[(SHARED_SKILL, "v2")]),
            true,
            FORCE_HINT,
        )
        .unwrap();

        assert_eq!(report.written, 1, "stale attributable bytes must refresh");
        assert!(
            deferral_warnings(&report).is_empty(),
            "attributable shared bytes must not defer: {:?}",
            report.warnings
        );
        assert_eq!(fs::read_to_string(target.join(SHARED_SKILL)).unwrap(), "v2");
        assert_eq!(
            fs::read_to_string(report.backups.first().expect("superseded bytes backed up"))
                .unwrap(),
            "v1"
        );
        let codex = plan::read_receipt(target, "codex").1.unwrap();
        assert_eq!(
            codex.file(SHARED_SKILL).map(|file| file.sha256.as_str()),
            Some(crate::digest::hash("v2").as_str()),
            "codex claims the bytes it just wrote"
        );
        let copilot = plan::read_receipt(target, "github-copilot").1.unwrap();
        assert!(
            copilot.file(SHARED_SKILL).is_some(),
            "the sibling's ownership survives codex's write"
        );
    }

    #[test]
    fn shared_bytes_attributable_to_no_receipt_are_preserved_even_under_force() {
        let dir = tempdir().unwrap();
        let target = dir.path();
        two_receipt_shared_fixture(target);
        crate::installer::atomic_write(&target.join(SHARED_SKILL), "user edit").unwrap();

        let report = apply(
            target,
            &shared_install("codex", "two", &[(SHARED_SKILL, "v2")]),
            true,
            FORCE_HINT,
        )
        .unwrap();

        assert_eq!(
            fs::read_to_string(target.join(SHARED_SKILL)).unwrap(),
            "user edit",
            "unattributable shared bytes are never overwritten"
        );
        assert_eq!(report.written, 0);
        assert_eq!(
            deferral_warnings(&report).len(),
            1,
            "the preserved path warns once: {:?}",
            report.warnings
        );
        let codex = plan::read_receipt(target, "codex").1.unwrap();
        assert_eq!(
            codex.file(SHARED_SKILL).map(|file| file.sha256.as_str()),
            Some(crate::digest::hash("v1").as_str()),
            "the receipt keeps the claim it had before the update"
        );
    }

    #[test]
    fn identical_shared_bytes_stay_managed_without_backup() {
        let dir = tempdir().unwrap();
        let target = dir.path();
        apply(
            target,
            &shared_install("codex", "one", &[(SHARED_SKILL, "v1")]),
            false,
            FORCE_HINT,
        )
        .unwrap();

        let report = apply(
            target,
            &shared_install("github-copilot", "one", &[(SHARED_SKILL, "v1")]),
            false,
            FORCE_HINT,
        )
        .unwrap();

        assert_eq!(report.written, 0);
        assert!(
            report.backups.is_empty(),
            "identical bytes need no backup: {:?}",
            report.backups
        );
        assert!(
            deferral_warnings(&report).is_empty(),
            "{:?}",
            report.warnings
        );
        let copilot = plan::read_receipt(target, "github-copilot").1.unwrap();
        assert!(
            copilot.file(SHARED_SKILL).is_some(),
            "the second harness manages the shared path it found already correct"
        );
    }
}
