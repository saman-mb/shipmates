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
pub fn apply(target_dir: &Path, install: &InstallPlan, force: bool) -> Result<ApplyReport> {
    apply_with_preserved_paths(target_dir, install, force, &BTreeSet::new())
}

/// Apply an install while retaining receipt ownership for migration items that
/// were deliberately left in place (`--no-migrate` or a skipped migration).
pub fn apply_with_preserved_paths(
    target_dir: &Path,
    install: &InstallPlan,
    force: bool,
    preserved_paths: &BTreeSet<String>,
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
    for (rel, want) in &install.files {
        let path = crate::installer::manifest_db::resolve_target_relative(target_dir, rel)?;
        let rel_string = rel.to_string_lossy().into_owned();
        let owned = old
            .as_ref()
            .and_then(|receipt| receipt.file(&rel_string))
            .is_some();
        if sibling_claims.contains(&rel_string) {
            let current = fs::read(&path).ok();
            if current.as_deref() == Some(want.as_bytes()) {
                managed.push(rel.clone());
            } else if let (Some(old_file), Some(current)) = (
                old.as_ref().and_then(|receipt| receipt.file(&rel_string)),
                current.as_deref(),
            ) {
                if crate::digest::hash_bytes(current) == old_file.sha256 {
                    report.warnings.push(format!(
                        "Warning: shared-managed file left untouched; preserving existing ownership: {}",
                        rel.display()
                    ));
                } else {
                    report.warnings.push(format!(
                        "Warning: shared-managed file left untouched; current bytes match neither desired nor existing ownership: {}",
                        rel.display()
                    ));
                }
            } else {
                report.warnings.push(format!(
                    "Warning: shared-managed file left untouched; current install does not claim it: {}",
                    rel.display()
                ));
            }
            continue;
        }
        match fs::read(&path) {
            Ok(current) if current == want.as_bytes() => {
                if !owned && !force {
                    report.warnings.push(format!(
                        "Warning: existing file left untouched (use --force to replace): {}",
                        rel.display()
                    ));
                    continue;
                }
                managed.push(rel.clone());
                report.skipped += 1;
            }
            Ok(current) => {
                if !owned && !force {
                    report.warnings.push(format!(
                        "Warning: existing file left untouched (use --force to replace): {}",
                        rel.display()
                    ));
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

    if let Some(old_receipt) = old.as_ref() {
        let old_managed = old_receipt
            .files
            .iter()
            .map(|file| file.path.clone())
            .collect::<BTreeSet<_>>();
        for path in plan::unmanaged_files(target_dir, &old_receipt.roots, &old_managed)? {
            report.warnings.push(format!(
                "Warning: unmanaged file left untouched: {}",
                path.strip_prefix(target_dir).unwrap_or(&path).display()
            ));
        }
        for old_file in &old_receipt.files {
            if install.files.contains_key(Path::new(&old_file.path)) {
                continue;
            }
            let path = crate::installer::manifest_db::resolve_target_relative(
                target_dir,
                Path::new(&old_file.path),
            )?;
            if fs::symlink_metadata(&path).is_ok() {
                report.warnings.push(format!(
                    "Warning: previous managed file left untouched (no longer in payload): {}",
                    old_file.path
                ));
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
            if (preserve || install.files.contains_key(path))
                && !receipt.files.iter().any(|file| file.path == old_file.path)
                && (preserve || unchanged_on_disk)
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

fn backup_existing(path: &Path, bytes: &[u8]) -> Result<Option<PathBuf>> {
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

    #[test]
    fn unchanged_reinstall_writes_no_backup() {
        let dir = tempdir().unwrap();
        let first = install(dir.path(), "one", &[(".claude/agents/a.md", "a")]);
        let result = apply(dir.path(), &first, false).unwrap();
        assert_eq!(result.written, 1);
        let second = apply(dir.path(), &first, false).unwrap();
        assert_eq!(second.written, 0);
        assert!(second.backups.is_empty());
    }

    #[test]
    fn changed_receipt_owned_file_is_backed_up() {
        let dir = tempdir().unwrap();
        let first = install(dir.path(), "one", &[(".claude/agents/a.md", "a")]);
        apply(dir.path(), &first, false).unwrap();
        let second = install(dir.path(), "two", &[(".claude/agents/a.md", "b")]);
        let result = apply(dir.path(), &second, false).unwrap();
        assert_eq!(result.written, 1);
        assert_eq!(fs::read_to_string(&result.backups[0]).unwrap(), "a");
        assert_eq!(
            fs::read_to_string(dir.path().join(".claude/agents/a.md")).unwrap(),
            "b"
        );
    }
}
