//! Fail-closed uninstall driven solely by install receipts.

use crate::digest;
use crate::installer::{
    manifest_db::{InstallReceipt, ReceiptRepository},
    plan,
};
use anyhow::{Result, bail};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct LocatedReceipt {
    pub path: PathBuf,
    pub receipt: InstallReceipt,
}

#[derive(Debug, Clone, Default)]
pub struct UninstallReport {
    pub harness: String,
    pub removed: usize,
    pub warnings: Vec<String>,
    pub receipt_removed: bool,
}

/// Find valid receipts without guessing a harness. Invalid receipts are
/// deliberately omitted; explicit selection handles invalid receipts as an
/// error instead of attempting a legacy path scan.
pub fn discover(target_dir: &Path) -> Vec<LocatedReceipt> {
    let mut found = Vec::new();
    for harness in crate::adapters::targets() {
        let (state, receipt, _) = plan::read_receipt(target_dir, harness);
        if state == plan::ReceiptState::Valid {
            if let Some(receipt) = receipt {
                found.push(LocatedReceipt {
                    path: plan::receipt_path(target_dir, harness),
                    receipt,
                });
            }
        }
    }
    found.sort_by(|a, b| a.receipt.harness.cmp(&b.receipt.harness));
    found
}

pub fn select_receipt(target_dir: &Path, harness: Option<&str>) -> Result<Option<LocatedReceipt>> {
    match harness {
        Some(harness) => {
            let (state, receipt, error) = plan::read_receipt(target_dir, harness);
            match state {
                plan::ReceiptState::Valid => Ok(Some(LocatedReceipt {
                    path: plan::receipt_path(target_dir, harness),
                    receipt: receipt.expect("valid receipt must be present"),
                })),
                plan::ReceiptState::Missing => {
                    bail!("No install receipt found for harness {harness}; refusing to uninstall")
                }
                plan::ReceiptState::Invalid => bail!(
                    "Install receipt for harness {harness} is invalid: {}; refusing to uninstall",
                    error.unwrap_or_else(|| "unknown receipt error".into())
                ),
            }
        }
        None => {
            let found = discover(target_dir);
            match found.as_slice() {
                [] if ReceiptRepository::new(target_dir).receipts_dir().exists() => {
                    bail!("No valid install receipt found; refusing to uninstall")
                }
                [] => Ok(None),
                [one] => Ok(Some(one.clone())),
                all => {
                    let names = all
                        .iter()
                        .map(|item| item.receipt.harness.as_str())
                        .collect::<Vec<_>>();
                    bail!(
                        "multiple harness installs found ({}); specify --harness",
                        names.join(", ")
                    )
                }
            }
        }
    }
}

/// Remove only files listed by a valid receipt whose raw bytes still match its
/// recorded hash. Other valid receipts claim shared paths; those paths remain.
pub fn uninstall(target_dir: &Path, selected: LocatedReceipt) -> Result<UninstallReport> {
    let mut report = UninstallReport {
        harness: selected.receipt.harness.clone(),
        ..UninstallReport::default()
    };
    let repository = ReceiptRepository::new(target_dir);
    let mut blocked = false;
    let other_claims = discover(target_dir)
        .into_iter()
        .filter(|item| item.receipt.harness != selected.receipt.harness)
        .flat_map(|item| item.receipt.files.into_iter().map(|file| file.path))
        .collect::<std::collections::BTreeSet<_>>();
    for file in &selected.receipt.files {
        let rel = PathBuf::from(&file.path);
        let path = target_dir.join(&rel);
        if other_claims.contains(&file.path) {
            report.warnings.push(format!(
                "Warning: shared-managed file preserved: {}",
                rel.display()
            ));
            blocked = true;
            continue;
        }
        let current = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                report.warnings.push(format!(
                    "Warning: managed file preserved (cannot read {}): {}",
                    rel.display(),
                    error
                ));
                blocked = true;
                continue;
            }
        };
        if digest::hash_bytes(&current) != file.sha256 {
            report.warnings.push(format!(
                "Warning: modified managed file preserved: {}",
                rel.display()
            ));
            blocked = true;
            continue;
        }
        fs::remove_file(&path)
            .map_err(|error| anyhow::anyhow!("removing {}: {}", path.display(), error))?;
        report.removed += 1;
    }

    let managed = selected
        .receipt
        .files
        .iter()
        .map(|file| file.path.clone())
        .collect::<std::collections::BTreeSet<_>>();
    for path in plan::unmanaged_files(target_dir, &selected.receipt.roots, &managed) {
        report.warnings.push(format!(
            "Warning: unmanaged file preserved: {}",
            path.strip_prefix(target_dir).unwrap_or(&path).display()
        ));
    }

    if !blocked {
        let _ = fs::remove_file(&selected.path);
        let _ = repository.remove(&selected.receipt.harness);
        report.receipt_removed = true;
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::installer::manifest_db::{LAYOUT_SKILLS, ReceiptFile};
    use tempfile::tempdir;

    fn receipt(target: &Path, harness: &str, path: &str, content: &[u8]) {
        let receipt = InstallReceipt::new(
            "1",
            harness,
            LAYOUT_SKILLS,
            vec![
                Path::new(path)
                    .components()
                    .next()
                    .unwrap()
                    .as_os_str()
                    .to_string_lossy()
                    .into_owned(),
            ],
            vec![ReceiptFile {
                path: path.into(),
                sha256: digest::hash_bytes(content),
            }],
        )
        .unwrap();
        plan::save_receipt(target, &receipt).unwrap();
    }

    #[test]
    fn modified_file_is_preserved() {
        let dir = tempdir().unwrap();
        let path = dir.path().join(".claude/agents/a.md");
        crate::installer::atomic_write(&path, "installed").unwrap();
        receipt(
            dir.path(),
            "claude-code",
            ".claude/agents/a.md",
            b"original",
        );
        let selected = select_receipt(dir.path(), Some("claude-code"))
            .unwrap()
            .unwrap();
        let report = uninstall(dir.path(), selected).unwrap();
        assert_eq!(report.removed, 0);
        assert!(path.exists());
        assert!(!report.warnings.is_empty());
    }

    #[test]
    fn shared_file_is_preserved() {
        let dir = tempdir().unwrap();
        let path = dir.path().join(".agents/skills/x/SKILL.md");
        crate::installer::atomic_write(&path, "installed").unwrap();
        receipt(
            dir.path(),
            "codex",
            ".agents/skills/x/SKILL.md",
            b"installed",
        );
        receipt(
            dir.path(),
            "antigravity",
            ".agents/skills/x/SKILL.md",
            b"installed",
        );
        let selected = select_receipt(dir.path(), Some("codex")).unwrap().unwrap();
        let report = uninstall(dir.path(), selected).unwrap();
        assert_eq!(report.removed, 0);
        assert!(path.exists());
    }
}
