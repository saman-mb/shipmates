//! Normalized install payloads and the adapter-to-receipt integration point.

use crate::adapters::Adapter;
use crate::installer::manifest_db::{self, InstallReceipt, ReceiptFile, ReceiptRepository};
use anyhow::{bail, Result};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::{Component, Path, PathBuf};

pub type Receipt = InstallReceipt;
#[derive(Debug, Clone)]
pub struct InstallPlan {
    pub harness: String,
    pub version: String,
    pub layout: String,
    pub roots: Vec<String>,
    pub files: BTreeMap<PathBuf, String>,
}

impl InstallPlan {
    pub fn from_payload(
        adapter: &dyn Adapter,
        harness: &str,
        mut payload: HashMap<String, String>,
        tools: HashMap<String, String>,
    ) -> Result<Self> {
        for (key, content) in tools {
            if payload.insert(key.clone(), content).is_some() {
                bail!("duplicate install payload path: {key}");
            }
        }
        let prefix = format!("{}/", adapter.container());
        let mut files = BTreeMap::new();
        for (key, content) in payload {
            let rel = key
                .strip_prefix(&prefix)
                .ok_or_else(|| anyhow::anyhow!("payload path outside install container: {key}"))?;
            let rel = validate_relative_path(rel)?;
            if files.insert(rel.clone(), content).is_some() {
                bail!("duplicate install payload path: {}", rel.display());
            }
        }
        let roots = files
            .keys()
            .filter_map(|path| path.components().next())
            .filter_map(|component| match component {
                Component::Normal(value) => value.to_str().map(str::to_string),
                _ => None,
            })
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        Ok(Self {
            harness: harness.to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            layout: layout_for(&files),
            roots,
            files,
        })
    }

    pub fn receipt_for<I>(&self, managed: I) -> Result<Receipt>
    where
        I: IntoIterator<Item = PathBuf>,
    {
        let files = managed
            .into_iter()
            .filter_map(|path| self.files.get(&path).map(|content| (path, content)))
            .map(|(path, content)| ReceiptFile {
                path: path.to_string_lossy().into_owned(),
                sha256: crate::digest::hash_bytes(content.as_bytes()),
            })
            .collect::<Vec<_>>();
        Receipt::new(
            self.version.clone(),
            self.harness.clone(),
            self.layout.clone(),
            self.roots.clone(),
            files,
        )
    }
}

/// Canonical receipt location for one harness install.
pub fn receipt_path(target_dir: &Path, harness: &str) -> Result<PathBuf> {
    ReceiptRepository::new(target_dir).receipt_path(harness)
}

pub fn save_receipt(target_dir: &Path, receipt: &Receipt) -> Result<()> {
    ReceiptRepository::new(target_dir).save(receipt)
}

pub fn read_receipt(
    target_dir: &Path,
    harness: &str,
) -> (ReceiptState, Option<Receipt>, Option<String>) {
    match ReceiptRepository::new(target_dir).read(harness) {
        Ok(Some(receipt)) => (ReceiptState::Valid, Some(receipt), None),
        Ok(None) => (ReceiptState::Missing, None, None),
        Err(error) => (ReceiptState::Invalid, None, Some(error.to_string())),
    }
}

/// Return regular files under install roots not present in a receipt. The
/// scan is bounded to roots recorded by the adapter and never enters receipt or
/// backup state.
pub fn unmanaged_files(
    target_dir: &Path,
    roots: &[String],
    managed: &std::collections::BTreeSet<String>,
) -> Result<Vec<PathBuf>> {
    let mut result = Vec::new();
    for root in roots {
        let root = manifest_db::resolve_target_relative(target_dir, Path::new(root))?;
        collect_unmanaged(&root, target_dir, managed, &mut result)?;
    }
    result.sort();
    Ok(result)
}

fn collect_unmanaged(
    path: &Path,
    target_dir: &Path,
    managed: &std::collections::BTreeSet<String>,
    result: &mut Vec<PathBuf>,
) -> Result<()> {
    let Ok(entries) = fs::read_dir(path) else {
        return Ok(());
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let relative = path.strip_prefix(target_dir).map_err(|error| {
            anyhow::anyhow!(
                "unmanaged path escaped target: {} ({error})",
                path.display()
            )
        })?;
        let path = manifest_db::resolve_target_relative(target_dir, relative)?;
        let name = entry.file_name();
        if name == ".shipmates" || name == ".shipmates-backup" {
            continue;
        }
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            collect_unmanaged(&path, target_dir, managed, result)?;
        } else if file_type.is_file() {
            let Ok(relative) = path.strip_prefix(target_dir) else {
                continue;
            };
            let relative = relative.to_string_lossy().into_owned();
            if !managed.contains(&relative) {
                result.push(path);
            }
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReceiptState {
    Missing,
    Valid,
    Invalid,
}

fn validate_relative_path(raw: &str) -> Result<PathBuf> {
    let path = Path::new(raw);
    if raw.is_empty() || path.is_absolute() {
        bail!("unsafe install path: {raw}");
    }
    for component in path.components() {
        match component {
            Component::Normal(_) => {}
            _ => bail!("unsafe install path: {raw}"),
        }
    }
    Ok(path.to_path_buf())
}

fn layout_for(files: &BTreeMap<PathBuf, String>) -> String {
    if files
        .keys()
        .any(|path| path.components().any(|c| c.as_os_str() == "commands"))
        && !files
            .keys()
            .any(|path| path.components().any(|c| c.as_os_str() == "skills"))
    {
        manifest_db::LAYOUT_COMMANDS.into()
    } else {
        manifest_db::LAYOUT_SKILLS.into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::installer::manifest_db::ReceiptFile;

    #[test]
    fn receipt_rejects_traversal() {
        let receipt = Receipt::new(
            "1",
            "claude-code",
            "skills",
            vec![".claude".into()],
            vec![ReceiptFile {
                path: "../outside".into(),
                sha256: "0".repeat(64),
            }],
        );
        assert!(receipt.is_err());
    }
}
