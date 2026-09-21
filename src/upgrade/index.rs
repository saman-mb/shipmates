//! On-disk index of Shipmates installs, keyed by `(root, harness)`.
//!
//! The index lets `shipmates status` / `shipmates upgrade` find installs the
//! running binary itself did not make (e.g. another harness or an older run).
//! Registration is best-effort at call sites: a failure to write the index must
//! never fail an install, update or uninstall.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::upgrade::types::PrunedRoot;

/// The index schema version this code writes.
const CURRENT_SCHEMA_VERSION: u32 = 1;

/// One install's record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InstallRecord {
    pub root: String,
    pub harness: String,
    pub version: String,
    pub layout: String,
    pub last_seen: u64,
}

/// The persisted index file.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InstallsIndex {
    pub schema_version: u32,
    pub records: Vec<InstallRecord>,
}

impl InstallsIndex {
    /// Load the index, returning an empty index when the file does not exist.
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let content = match std::fs::read_to_string(path) {
            Ok(content) => content,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::empty()),
            Err(e) => {
                eprintln!(
                    "warning: ignoring unreadable installs index {}: {e}",
                    path.display()
                );
                return Ok(Self::empty());
            }
        };
        if content.trim().is_empty() {
            return Ok(Self::empty());
        }
        let index: Self = match serde_json::from_str(&content) {
            Ok(index) => index,
            Err(e) => {
                eprintln!(
                    "warning: ignoring corrupt installs index {}: {e}",
                    path.display()
                );
                return Ok(Self::empty());
            }
        };
        if index.schema_version > CURRENT_SCHEMA_VERSION {
            eprintln!(
                "warning: ignoring installs index {} (schema_version {} is newer than this binary supports {CURRENT_SCHEMA_VERSION})",
                path.display(),
                index.schema_version
            );
            return Ok(Self::empty());
        }
        Ok(index)
    }

    /// Persist the index atomically via `installer::atomic_write`.
    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        let mut index = self.clone();
        index.schema_version = CURRENT_SCHEMA_VERSION;
        let json = serde_json::to_string_pretty(&index).context("serializing installs index")?;
        crate::installer::atomic_write(path, &json)
            .with_context(|| format!("writing installs index {}", path.display()))?;
        Ok(())
    }

    /// Upsert a `(root, harness)` record with a fresh `last_seen`, then save.
    pub fn register(
        &mut self,
        path: &Path,
        root: &Path,
        harness: &str,
        version: &str,
        layout: &str,
    ) -> anyhow::Result<()> {
        let root = root.to_string_lossy().to_string();
        let now = unix_secs();
        if let Some(record) = self
            .records
            .iter_mut()
            .find(|record| record.root == root && record.harness == harness)
        {
            record.version = version.to_string();
            record.layout = layout.to_string();
            record.last_seen = now;
        } else {
            self.records.push(InstallRecord {
                root,
                harness: harness.to_string(),
                version: version.to_string(),
                layout: layout.to_string(),
                last_seen: now,
            });
        }
        self.schema_version = CURRENT_SCHEMA_VERSION;
        self.save(path)
    }

    /// Remove a `(root, harness)` record, saving only when something changed.
    pub fn deregister(&mut self, path: &Path, root: &Path, harness: &str) -> anyhow::Result<()> {
        let root = root.to_string_lossy().to_string();
        let before = self.records.len();
        self.records
            .retain(|record| !(record.root == root && record.harness == harness));
        if self.records.len() != before {
            self.save(path)?;
        }
        Ok(())
    }

    /// All recorded roots, canonicalized where possible, de-duped, in order.
    ///
    /// Roots whose directory no longer exists are kept as-is (pruning is a
    /// separate, deliberate step); canonicalization only collapses spellings of
    /// the same live directory.
    pub fn roots(&self) -> Vec<PathBuf> {
        let mut roots: Vec<PathBuf> = Vec::new();
        for record in &self.records {
            let raw = PathBuf::from(&record.root);
            let canonical = std::fs::canonicalize(&raw).unwrap_or(raw);
            if !roots.contains(&canonical) {
                roots.push(canonical);
            }
        }
        roots
    }

    /// Drop records whose root directory no longer exists, save, and return them.
    pub fn prune_dead(&mut self, path: &Path) -> anyhow::Result<Vec<PrunedRoot>> {
        let before = self.records.len();
        let mut pruned = Vec::new();
        self.records.retain(|record| {
            let alive = Path::new(&record.root).exists();
            if !alive {
                pruned.push(PrunedRoot {
                    root: record.root.clone(),
                    reason: "missing".to_string(),
                });
            }
            alive
        });
        if self.records.len() != before {
            self.save(path)?;
        }
        Ok(pruned)
    }

    fn empty() -> Self {
        Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            records: Vec::new(),
        }
    }
}

/// Path to the installs index: `SHIPMATES_INDEX` when set and non-empty, else
/// `$HOME/.shipmates/installs.json`. Missing `HOME` is an error.
pub fn index_path() -> anyhow::Result<PathBuf> {
    index_path_from(
        std::env::var_os("SHIPMATES_INDEX").as_deref(),
        std::env::var_os("HOME").as_deref(),
    )
}

/// Testable core of [`index_path`]: precedence without touching process env.
fn index_path_from(
    shipmates_index: Option<&OsStr>,
    home: Option<&OsStr>,
) -> anyhow::Result<PathBuf> {
    if let Some(raw) = shipmates_index {
        if !raw.is_empty() {
            return Ok(PathBuf::from(raw));
        }
    }
    let home = home
        .filter(|h| !h.is_empty())
        .ok_or_else(|| anyhow::anyhow!("HOME is not set; cannot locate the installs index"))?;
    Ok(PathBuf::from(home).join(".shipmates").join("installs.json"))
}

fn unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_missing_file_returns_empty_index() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("installs.json");
        let index = InstallsIndex::load(&path).unwrap();
        assert!(index.records.is_empty());
        assert_eq!(index.schema_version, CURRENT_SCHEMA_VERSION);
    }

    #[test]
    fn load_garbage_falls_back_to_empty_index() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("installs.json");
        std::fs::write(&path, "{ not json").unwrap();

        let index = InstallsIndex::load(&path).unwrap();
        assert!(index.records.is_empty());
        assert_eq!(index.schema_version, CURRENT_SCHEMA_VERSION);
    }

    #[test]
    fn load_future_schema_falls_back_to_empty_index() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("installs.json");
        std::fs::write(
            &path,
            r#"{"schema_version":99,"records":[{"root":"/x","harness":"claude-code","version":"1.0.0","layout":"skills","last_seen":1}]}"#,
        )
        .unwrap();

        let index = InstallsIndex::load(&path).unwrap();
        assert!(index.records.is_empty());
    }

    #[test]
    fn round_trip_register_save_load() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("installs.json");
        let root = dir.path().join("project");

        let mut index = InstallsIndex::empty();
        index
            .register(&path, &root, "claude-code", "0.12.0", "skills")
            .unwrap();

        let loaded = InstallsIndex::load(&path).unwrap();
        assert_eq!(loaded.schema_version, 1);
        assert_eq!(loaded.records.len(), 1);
        assert_eq!(loaded.records[0].root, root.to_string_lossy());
        assert_eq!(loaded.records[0].harness, "claude-code");
        assert_eq!(loaded.records[0].version, "0.12.0");
        assert_eq!(loaded.records[0].layout, "skills");
        assert!(loaded.records[0].last_seen > 0);
    }

    #[test]
    fn register_upserts_same_root_and_harness() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("installs.json");
        let root = dir.path().join("project");

        let mut index = InstallsIndex::empty();
        index
            .register(&path, &root, "claude-code", "0.11.0", "skills")
            .unwrap();
        let first_seen = index.records[0].last_seen;
        index
            .register(&path, &root, "claude-code", "0.12.0", "skills")
            .unwrap();

        let loaded = InstallsIndex::load(&path).unwrap();
        assert_eq!(loaded.records.len(), 1);
        assert_eq!(loaded.records[0].version, "0.12.0");
        assert!(loaded.records[0].last_seen >= first_seen);
    }

    #[test]
    fn register_keeps_distinct_harnesses_for_same_root() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("installs.json");
        let root = dir.path().join("project");

        let mut index = InstallsIndex::empty();
        index
            .register(&path, &root, "claude-code", "0.12.0", "skills")
            .unwrap();
        index
            .register(&path, &root, "opencode", "0.12.0", "skills")
            .unwrap();

        let loaded = InstallsIndex::load(&path).unwrap();
        assert_eq!(loaded.records.len(), 2);
    }

    #[test]
    fn deregister_removes_only_matching_record() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("installs.json");
        let root = dir.path().join("project");

        let mut index = InstallsIndex::empty();
        index
            .register(&path, &root, "claude-code", "0.12.0", "skills")
            .unwrap();
        index
            .register(&path, &root, "opencode", "0.12.0", "skills")
            .unwrap();

        index.deregister(&path, &root, "claude-code").unwrap();

        let loaded = InstallsIndex::load(&path).unwrap();
        assert_eq!(loaded.records.len(), 1);
        assert_eq!(loaded.records[0].harness, "opencode");
    }

    #[test]
    fn prune_dead_removes_and_reports_missing_roots() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("installs.json");
        let live = dir.path().join("live");
        let gone = dir.path().join("gone");
        std::fs::create_dir_all(&live).unwrap();

        let mut index = InstallsIndex::empty();
        index
            .register(&path, &live, "claude-code", "0.12.0", "skills")
            .unwrap();
        index
            .register(&path, &gone, "claude-code", "0.12.0", "skills")
            .unwrap();

        let pruned = index.prune_dead(&path).unwrap();
        assert_eq!(pruned.len(), 1);
        assert_eq!(pruned[0].root, gone.to_string_lossy());
        assert_eq!(pruned[0].reason, "missing");

        let loaded = InstallsIndex::load(&path).unwrap();
        assert_eq!(loaded.records.len(), 1);
        assert_eq!(loaded.records[0].root, live.to_string_lossy());
    }

    #[test]
    fn roots_canonicalizes_dedupes_and_preserves_order() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("installs.json");
        let first = dir.path().join("first");
        let second = dir.path().join("second");
        std::fs::create_dir_all(&first).unwrap();
        std::fs::create_dir_all(&second).unwrap();

        // Two spellings of `first` (one with a trailing `.`), then `second`.
        let dot_spelling = format!("{}/.", first.display());
        let mut index = InstallsIndex::empty();
        index
            .register(&path, &first, "claude-code", "0.12.0", "skills")
            .unwrap();
        index
            .register(
                &path,
                Path::new(&dot_spelling),
                "opencode",
                "0.12.0",
                "skills",
            )
            .unwrap();
        index
            .register(&path, &second, "claude-code", "0.12.0", "skills")
            .unwrap();

        let roots = index.roots();
        assert_eq!(roots.len(), 2);
        assert_eq!(roots[0], std::fs::canonicalize(&first).unwrap());
        assert_eq!(roots[1], std::fs::canonicalize(&second).unwrap());
    }

    #[test]
    fn index_path_prefers_shipmates_index_override() {
        let dir = tempfile::tempdir().unwrap();
        let override_path = dir.path().join("custom.json");
        let got = index_path_from(Some(override_path.as_os_str()), None).unwrap();
        assert_eq!(got, override_path);
    }

    #[test]
    fn index_path_defaults_under_home() {
        let got = index_path_from(None, Some(OsStr::new("/home/me"))).unwrap();
        assert_eq!(got, PathBuf::from("/home/me/.shipmates/installs.json"));
    }

    #[test]
    fn index_path_errors_without_home() {
        assert!(index_path_from(None, None).is_err());
    }

    #[test]
    fn index_path_ignores_empty_override() {
        let got = index_path_from(Some(OsStr::new("")), Some(OsStr::new("/home/me"))).unwrap();
        assert_eq!(got, PathBuf::from("/home/me/.shipmates/installs.json"));
    }
}
