//! Normalized install payloads and the adapter-to-receipt integration point.

use crate::adapters::Adapter;
use crate::installer::manifest_db::{self, InstallReceipt, ReceiptFile, ReceiptRepository};
use anyhow::{Result, bail};
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

/// Directory names never descended into, whatever a harness root holds. A
/// lived-in harness root is also the user's runtime (`.opencode/node_modules`),
/// and Shipmates never installs below one of these.
const NEVER_SCANNED: &[&str] = &[".shipmates", ".shipmates-backup", "node_modules"];

/// Return regular files inside the payload's own subtrees that a receipt does
/// not claim.
///
/// The scan is bounded to the two-component prefixes the managed set itself
/// occupies (`.opencode/commands`, `.claude/skills`, …) rather than the whole
/// harness root, because a harness root doubles as the user's environment — an
/// opencode tree holds `node_modules/.bin` shims that are none of our business.
/// Symlinks are skipped outright: never resolved, never descended, never
/// reported. Reporting unmanaged files is advisory, so a root that cannot be
/// resolved is skipped rather than failing the install or uninstall around it.
pub fn unmanaged_files(
    target_dir: &Path,
    managed: &std::collections::BTreeSet<String>,
) -> Vec<PathBuf> {
    let mut result = Vec::new();
    for prefix in scan_prefixes(managed) {
        let Ok(root) = manifest_db::resolve_target_relative(target_dir, Path::new(&prefix)) else {
            continue;
        };
        collect_unmanaged(&root, target_dir, managed, &mut result);
    }
    result.sort();
    result.dedup();
    result
}

/// The `<first>/<second>` component prefixes the managed paths occupy. A
/// managed path shallower than two components contributes nothing: its parent
/// is the harness root itself, which is exactly what must not be walked.
fn scan_prefixes(managed: &std::collections::BTreeSet<String>) -> BTreeSet<String> {
    managed
        .iter()
        .filter_map(|path| {
            let mut components = Path::new(path).components().filter_map(|c| match c {
                Component::Normal(value) => value.to_str(),
                _ => None,
            });
            let first = components.next()?;
            let second = components.next()?;
            // A two-component managed path is a file, not a subtree.
            components.next()?;
            if NEVER_SCANNED.contains(&first) || NEVER_SCANNED.contains(&second) {
                return None;
            }
            Some(format!("{first}/{second}"))
        })
        .collect()
}

fn collect_unmanaged(
    path: &Path,
    target_dir: &Path,
    managed: &std::collections::BTreeSet<String>,
    result: &mut Vec<PathBuf>,
) {
    let Ok(entries) = fs::read_dir(path) else {
        return;
    };
    for entry in entries.flatten() {
        // File type first, and symlinks are dropped before anything resolves a
        // path: a package manager's `.bin` shim must never abort a scan (#384).
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_symlink() {
            continue;
        }
        let name = entry.file_name();
        if NEVER_SCANNED.iter().any(|skipped| name == *skipped) {
            continue;
        }
        let entry_path = entry.path();
        let Ok(relative) = entry_path.strip_prefix(target_dir) else {
            continue;
        };
        let Ok(resolved) = manifest_db::resolve_target_relative(target_dir, relative) else {
            continue;
        };
        if file_type.is_dir() {
            collect_unmanaged(&resolved, target_dir, managed, result);
        } else if file_type.is_file() {
            let relative = relative.to_string_lossy().into_owned();
            if !managed.contains(&relative) {
                result.push(resolved);
            }
        }
    }
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

    fn managed(paths: &[&str]) -> BTreeSet<String> {
        paths.iter().map(|path| (*path).to_string()).collect()
    }

    #[test]
    fn scan_is_bounded_to_payload_subtrees() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path();
        crate::installer::atomic_write(&target.join(".opencode/commands/ship-issue.md"), "a")
            .unwrap();
        crate::installer::atomic_write(&target.join(".opencode/commands/mine.md"), "b").unwrap();
        crate::installer::atomic_write(&target.join(".opencode/opencode.json"), "{}").unwrap();
        crate::installer::atomic_write(&target.join(".opencode/node_modules/pkg/index.js"), "x")
            .unwrap();

        let found = unmanaged_files(target, &managed(&[".opencode/commands/ship-issue.md"]));

        assert_eq!(found, vec![target.join(".opencode/commands/mine.md")]);
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_entries_are_skipped_not_fatal() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let target = dir.path();
        crate::installer::atomic_write(&target.join(".opencode/tools/shipmates-gh.ts"), "a")
            .unwrap();
        std::fs::write(outside.path().join("payload.js"), "outside").unwrap();
        symlink(
            outside.path().join("payload.js"),
            target.join(".opencode/tools/link.ts"),
        )
        .unwrap();
        symlink(outside.path(), target.join(".opencode/tools/linked-dir")).unwrap();

        let found = unmanaged_files(target, &managed(&[".opencode/tools/shipmates-gh.ts"]));

        assert!(found.is_empty(), "symlinks must be skipped, not reported");
    }

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
