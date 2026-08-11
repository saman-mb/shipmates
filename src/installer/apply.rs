//! Receipt-aware payload application.

use crate::installer::{
    atomic_write,
    plan::{self, InstallPlan, Receipt, ReceiptState},
};
use anyhow::{Context, Result};
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
/// `force` is set. Missing receipts retain legacy install behaviour.
pub fn apply(target_dir: &Path, install: &InstallPlan, force: bool) -> Result<ApplyReport> {
    let (state, old, receipt_error) = plan::read_receipt(target_dir, &install.harness);
    let mut report = ApplyReport::default();
    report.previous_version = old.as_ref().map(|receipt| receipt.version.clone());
    if let Some(error) = receipt_error {
        report.warnings.push(format!(
            "Warning: install receipt is invalid; using receipt-less compatibility mode: {}",
            error
        ));
    }

    if let Some(old_receipt) = old.as_ref() {
        report.summary = compare_receipts(
            old_receipt,
            &install.receipt_for(install.files.keys().cloned())?,
        );
    } else if state == ReceiptState::Missing || state == ReceiptState::Invalid {
        report.summary.new = install.files.len();
    }

    let mut managed = Vec::new();
    for (rel, want) in &install.files {
        let path = target_dir.join(rel);
        match fs::read(&path) {
            Ok(current) if current == want.as_bytes() => {
                let already_owned = old
                    .as_ref()
                    .and_then(|receipt| receipt.file(rel.to_string_lossy().as_ref()))
                    .is_some();
                if old.is_some() && !already_owned && !force {
                    report.warnings.push(format!(
                        "Warning: unmanaged file left untouched: {}",
                        rel.display()
                    ));
                    continue;
                }
                managed.push(rel.clone());
                report.skipped += 1;
            }
            Ok(current) => {
                let owned = old
                    .as_ref()
                    .and_then(|receipt| receipt.file(rel.to_string_lossy().as_ref()))
                    .is_some();
                if old.is_some() && !owned && !force {
                    report.warnings.push(format!(
                        "Warning: unmanaged file left untouched: {}",
                        rel.display()
                    ));
                    continue;
                }
                if std::str::from_utf8(&current).is_err() {
                    report.warnings.push(format!(
                        "Warning: non-text file left untouched: {}",
                        rel.display()
                    ));
                    continue;
                }
                if let Some(backup) = backup_existing(&path, &current)? {
                    report.backups.push(backup);
                }
                atomic_write(&path, want)
                    .with_context(|| format!("writing installed file {}", path.display()))?;
                managed.push(rel.clone());
                report.written += 1;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                atomic_write(&path, want)
                    .with_context(|| format!("writing installed file {}", path.display()))?;
                managed.push(rel.clone());
                report.written += 1;
            }
            Err(error) => {
                report.warnings.push(format!(
                    "Warning: file {} could not be read and was left untouched: {}",
                    rel.display(),
                    error
                ));
            }
        }
    }

    if let Some(old_receipt) = old.as_ref() {
        let managed = old_receipt
            .files
            .iter()
            .map(|file| file.path.clone())
            .collect::<std::collections::BTreeSet<_>>();
        for path in plan::unmanaged_files(target_dir, &old_receipt.roots, &managed) {
            report.warnings.push(format!(
                "Warning: unmanaged file left untouched: {}",
                path.strip_prefix(target_dir).unwrap_or(&path).display()
            ));
        }
        for old_file in &old_receipt.files {
            if install.files.contains_key(Path::new(&old_file.path)) {
                continue;
            }
            let path = target_dir.join(&old_file.path);
            if path.exists() {
                report.warnings.push(format!(
                    "Warning: previous managed file left untouched (no longer in payload): {}",
                    old_file.path
                ));
            }
        }
    }

    // Publish only what this run actually owns. This preserves a user's file at
    // a new colliding path and makes a later uninstall fail closed for it.
    let mut receipt = install.receipt_for(managed)?;
    if let Some(old_receipt) = old.as_ref() {
        for old_file in &old_receipt.files {
            if install.files.contains_key(Path::new(&old_file.path))
                && !receipt.files.iter().any(|file| file.path == old_file.path)
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
    plan::save_receipt(target_dir, &receipt)?;
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
    // Payloads are UTF-8. Refuse to overwrite a non-UTF-8 existing file: there
    // is no byte-preserving atomic_write API in the legacy installer surface.
    let contents = match std::str::from_utf8(bytes) {
        Ok(contents) => contents,
        Err(_) => return Ok(None),
    };
    atomic_write(&backup, contents).with_context(|| format!("backing up {}", path.display()))?;
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
