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
pub(crate) fn receipt_path(target_dir: &Path, harness: &str) -> Result<PathBuf> {
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

/// Whether a filename is one of `installer::apply`'s in-place sibling backups,
/// `{original}.bak-<secs>-<pid>-<n>`.
///
/// Matched by shape alone — all three trailing fields must be numeric — so a
/// user's own `notes.md.bak-mine` is still a file Shipmates does not own and is
/// still reported. Listing siblings uses the same shape via `parse_backup_key`
/// and sorts newest-first by the numeric triple (not lexicographic paths).
pub fn is_install_backup_name(name: &str) -> bool {
    let Some((original, _)) = name.rsplit_once(".bak-") else {
        return false;
    };
    !original.is_empty() && parse_backup_key(name, original).is_some()
}

/// Parse `{original}.bak-<secs>-<pid>-<n>` into its numeric key, or `None`.
fn parse_backup_key(filename: &str, original: &str) -> Option<(u64, u32, u32)> {
    let rest = filename.strip_prefix(&format!("{original}.bak-"))?;
    let mut parts = rest.split('-');
    let secs: u64 = parts.next()?.parse().ok()?;
    let pid: u32 = parts.next()?.parse().ok()?;
    let n: u32 = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some((secs, pid, n))
}

/// Sibling `{name}.bak-<secs>-<pid>-<n>` files next to `path`, newest first
/// (numeric secs/pid/n — not lexicographic path sort, which mis-orders across
/// digit widths).
pub fn sibling_install_backups(path: &Path) -> Vec<PathBuf> {
    let Some(parent) = path.parent() else {
        return Vec::new();
    };
    let Some(original) = path.file_name().and_then(|n| n.to_str()) else {
        return Vec::new();
    };
    let Ok(entries) = fs::read_dir(parent) else {
        return Vec::new();
    };
    let mut found: Vec<(u64, u32, u32, PathBuf)> = Vec::new();
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if !file_type.is_file() {
            continue;
        }
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if let Some(key) = parse_backup_key(name, original) {
            found.push((key.0, key.1, key.2, entry.path()));
        }
    }
    found.sort_by(|a, b| (b.0, b.1, b.2).cmp(&(a.0, a.1, a.2)));
    found.into_iter().map(|(_, _, _, p)| p).collect()
}

/// Return regular files inside the payload's own subtrees that a receipt does
/// not claim.
///
/// The scan starts at the parent directory of each managed file — never the
/// harness root itself — because a harness root doubles as the user's runtime
/// (`.pi/agent/sessions`, `.claude/skills/synced`, `node_modules/.bin`). A
/// directory is descended only when some managed path lives under it, so an
/// unmanaged sibling folder is left unwalked. Extra *files* next to a managed
/// file are still reported. Symlinks are skipped outright: never resolved,
/// never descended, never reported. Reporting unmanaged files is advisory, so a
/// root that cannot be resolved is skipped rather than failing the install or
/// uninstall around it.
///
/// Shipmates' own sibling backups are not reported: an install that just wrote
/// `SKILL.md.bak-…` must not then warn about the file it created itself (#404).
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

/// Parent directory of each managed file, as a slash-separated relative path.
///
/// Empty and single-component parents are dropped: that parent *is* the harness
/// root, which must not be walked. A managed path whose parent contains a
/// `NEVER_SCANNED` component contributes nothing either.
fn scan_prefixes(managed: &std::collections::BTreeSet<String>) -> BTreeSet<String> {
    managed
        .iter()
        .filter_map(|path| {
            let parent = Path::new(path).parent()?;
            let parts = slash_components(parent);
            if parts.len() < 2 {
                return None;
            }
            if parts.iter().any(|part| NEVER_SCANNED.contains(part)) {
                return None;
            }
            Some(parts.join("/"))
        })
        .collect()
}

fn slash_components(path: &Path) -> Vec<&str> {
    path.components()
        .filter_map(|component| match component {
            Component::Normal(value) => value.to_str(),
            _ => None,
        })
        .collect()
}

fn relative_to_managed(relative: &Path) -> String {
    slash_components(relative).join("/")
}

fn managed_under(managed: &std::collections::BTreeSet<String>, relative_dir: &str) -> bool {
    let prefix = format!("{relative_dir}/");
    managed.iter().any(|path| path.starts_with(&prefix))
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
        if name.to_str().is_some_and(is_install_backup_name) {
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
            let relative = relative_to_managed(relative);
            if managed_under(managed, &relative) {
                collect_unmanaged(&resolved, target_dir, managed, result);
            }
        } else if file_type.is_file() {
            let relative = relative_to_managed(relative);
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

    #[test]
    fn unmanaged_scan_stays_inside_directories_that_hold_managed_files() {
        struct Case {
            managed: &'static str,
            planted: &'static [&'static str],
            warned: &'static [&'static str],
        }
        let cases = [
            Case {
                managed: ".pi/agent/agents/architect.md",
                planted: &[
                    ".pi/agent/agents/architect.md",
                    ".pi/agent/sessions/foo.jsonl",
                    ".pi/agent/missions/x",
                    ".pi/agent/npm/y",
                ],
                warned: &[],
            },
            Case {
                managed: ".claude/skills/shipmates-issue/SKILL.md",
                planted: &[
                    ".claude/skills/shipmates-issue/SKILL.md",
                    ".claude/skills/synced/x",
                    ".claude/plugins/x",
                    ".claude/skills/shipmates-issue/extra.md",
                ],
                warned: &[".claude/skills/shipmates-issue/extra.md"],
            },
            Case {
                managed: ".gemini/config/skills/foo/SKILL.md",
                planted: &[
                    ".gemini/config/skills/foo/SKILL.md",
                    ".gemini/config/plugins/x",
                ],
                warned: &[],
            },
            Case {
                managed: ".codex/agents/architect.toml",
                planted: &[
                    ".codex/agents/architect.toml",
                    ".codex/agents/agency-agents/nested.md",
                    ".codex/skills/.system/hidden.md",
                ],
                warned: &[],
            },
        ];

        for case in cases {
            let dir = tempfile::tempdir().unwrap();
            let target = dir.path();
            for rel in case.planted {
                crate::installer::atomic_write(&target.join(rel), "x").unwrap();
            }
            let found = unmanaged_files(target, &managed(&[case.managed]));
            let expected: Vec<PathBuf> = case.warned.iter().map(|rel| target.join(rel)).collect();
            assert_eq!(
                found, expected,
                "managed {} must not walk unmanaged sibling trees",
                case.managed
            );
        }
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
    fn install_backups_are_not_reported_but_user_bak_files_are() {
        assert!(is_install_backup_name("SKILL.md.bak-1700000000-4242-0"));
        for not_ours in [
            "notes.md.bak-mine",
            "SKILL.md.bak-1700000000-4242",
            "SKILL.md.bak-1700000000-4242-0-1",
            "SKILL.md.bak-1700000000-4242-x",
            ".bak-1-2-3",
            "SKILL.md",
        ] {
            assert!(!is_install_backup_name(not_ours), "{not_ours}");
        }

        let dir = tempfile::tempdir().unwrap();
        let target = dir.path();
        crate::installer::atomic_write(&target.join(".claude/skills/polish/SKILL.md"), "a")
            .unwrap();
        crate::installer::atomic_write(
            &target.join(".claude/skills/polish/SKILL.md.bak-1700000000-4242-0"),
            "old",
        )
        .unwrap();
        crate::installer::atomic_write(
            &target.join(".claude/skills/polish/notes.md.bak-mine"),
            "u",
        )
        .unwrap();

        let found = unmanaged_files(target, &managed(&[".claude/skills/polish/SKILL.md"]));

        assert_eq!(
            found,
            vec![target.join(".claude/skills/polish/notes.md.bak-mine")],
            "only Shipmates' own `.bak-<secs>-<pid>-<n>` siblings are hidden"
        );
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
