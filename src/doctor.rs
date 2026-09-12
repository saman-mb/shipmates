//! `shipmates doctor` — diagnose an install's health and, with `--fix`, repair it.
//!
//! Read-only by default: `diagnose` inspects the on-disk tree against the payload
//! the running binary would install and reports what is healthy, stale, missing or
//! superseded. `fix` repairs only paths claimed by a valid receipt; without one,
//! it may restore missing files but never overwrites existing content. It then
//! re-diagnoses and hands back the fresh report.

use crate::adapters::{self, Adapter};
use crate::catalog::{CanonicalCommand, CanonicalRole, CanonicalTool, CatalogSource};
use crate::digest;
use crate::installer::{adopt, manifest_db, migrate, plan, rename};
use anyhow::{Context, Result};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Ok,
    Warn,
    Problem,
}

#[derive(Debug, Clone)]
pub struct Check {
    pub name: String,
    pub severity: Severity,
    pub detail: String,
    /// Whether `shipmates doctor --fix` can repair this on its own. Reported to
    /// callers and asserted in tests; the printer keys off severity, not this.
    #[allow(dead_code)]
    pub fixable: bool,
}

#[derive(Debug, Clone, Default)]
pub struct Report {
    pub checks: Vec<Check>,
}

impl Report {
    /// True when any check is a hard Problem — the caller exits non-zero.
    pub fn has_problems(&self) -> bool {
        self.checks.iter().any(|c| c.severity == Severity::Problem)
    }
}

/// Strip the `<container>/` prefix from a built payload map, yielding the on-disk
/// paths relative to the target directory — exactly as the installer writes them.
///
/// This is the sole transform from a single `adapter.build()` to the "expected
/// files" both `diagnose` and `fix` compare against, so the payload is built once
/// and this cheap map-strip feeds every check (avoids ~4 `build()` calls per
/// `--fix`).
fn strip_container(built: &HashMap<String, String>, container: &str) -> BTreeMap<String, String> {
    let prefix = format!("{}/", container);
    built
        .iter()
        .filter_map(|(k, v)| {
            k.strip_prefix(&prefix)
                .map(|rel| (rel.to_string(), v.clone()))
        })
        .collect()
}

/// Installer's in-place sibling backups: `{filename}.bak-<secs>-<pid>-<n>`
/// (see `installer::apply`).
fn parse_install_backup_name(filename: &str, original: &str) -> Option<(u64, u32, u32)> {
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

/// Sibling `{name}.bak-<secs>-<pid>-<n>` files next to `path`, newest first.
fn sibling_install_backups(path: &Path) -> Vec<PathBuf> {
    let Some(parent) = path.parent() else {
        return Vec::new();
    };
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(parent) else {
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
        let fname = entry.file_name();
        let Some(s) = fname.to_str() else {
            continue;
        };
        if let Some(key) = parse_install_backup_name(s, name) {
            found.push((key.0, key.1, key.2, entry.path()));
        }
    }
    found.sort_by(|a, b| (b.0, b.1, b.2).cmp(&(a.0, a.1, a.2)));
    found.into_iter().map(|(_, _, _, p)| p).collect()
}

fn backup_matches_payload(bak: &Path, want: &[u8]) -> bool {
    std::fs::read(bak)
        .map(|bytes| digest::hash_bytes(&bytes) == digest::hash_bytes(want))
        .unwrap_or(false)
}

/// True when `filename` has the installer's own sidecar shape,
/// `{original}.bak-<secs>-<pid>-<n>`. Shape is the ownership signal: a captain's
/// hand-made `notes.md.bak-mine` does not match, so hygiene never sees it.
fn is_install_backup_name(filename: &str) -> bool {
    filename.rsplit_once(".bak-").is_some_and(|(original, _)| {
        !original.is_empty() && parse_install_backup_name(filename, original).is_some()
    })
}

/// Split `name.ext` into stem and extension, the dot kept on the extension.
fn split_stem_ext(filename: &str) -> Option<(&str, &str)> {
    let dot = filename.rfind('.')?;
    if dot == 0 {
        return None;
    }
    Some((&filename[..dot], &filename[dot..]))
}

/// Trees a harness install writes into, under one of its roots. A husk can only
/// be one of these directories' children.
const INSTALL_TREES: &[&str] = &["skills", "commands", "tools", "agents"];

/// The pre-prefix aliases of a current name — `harden` for `shipmates-harden`,
/// from `installer::rename`'s public table. The same artifact under the name it
/// used to have, so a husk left at the old name is recognised as ours.
fn pre_prefix_aliases(name: &str) -> Vec<String> {
    rename::COMMAND_RENAMES
        .iter()
        .chain(rename::TOOL_RENAMES.iter())
        .filter(|(_, new)| *new == name)
        .map(|(old, _)| (*old).to_string())
        .collect()
}

/// The install identity a payload path belongs to — `shipmates-harden` for
/// `.cursor/skills/shipmates-harden/SKILL.md`, `architect` for
/// `.claude/agents/architect.md`. The name the harness shows a user, and the
/// name a leftover directory in an older tree still carries.
fn install_identity(rel: &str) -> Option<String> {
    let segments: Vec<&str> = rel.split('/').collect();
    let tree = segments.iter().position(|s| INSTALL_TREES.contains(s))?;
    let name = segments.get(tree + 1)?;
    if tree + 2 == segments.len() {
        // A file directly in the tree: the identity is its stem.
        return Some(
            split_stem_ext(name)
                .map(|(stem, _)| stem)
                .unwrap_or(name)
                .to_string(),
        );
    }
    Some((*name).to_string())
}

/// Installer sidecar backups Shipmates left behind, in two kinds:
///
/// * a **husk** — a directory in one of the harness's trees holding nothing but
///   `.bak-…` files, its live file gone. Two ways to get one, and the same
///   remedy for both: a pre-prefix name (`skills/harden/`) whose file was
///   renamed away, and an install that moved a tree wholesale — #405 moved
///   cursor's skills from the shared `.agents/` tree to `.cursor/`, and left 13
///   husks in `.agents/skills/`. Neither is visible to `rename::plan`, which
///   sees only live leftovers, so doctor called the emptied tree shipshape
///   (#406).
/// * a **superseded** sidecar — one beside a live payload file that already
///   matches the running version, so the undo it offers is a copy of what is
///   already installed.
///
/// Both are collected only where the artifact is installed *and* current at its
/// path in the harness's **current** tree: that is the signal the move or
/// rewrite completed and the backup is spent. While the live file is missing or
/// drifted, the backup is the undo for an interrupted install, and stays.
#[derive(Debug, Default)]
struct Hygiene {
    /// Backup files per husk, keyed by the path shown in the report.
    husks: BTreeMap<String, BTreeSet<PathBuf>>,
    /// Husk directories, removed once emptied.
    husk_dirs: BTreeSet<PathBuf>,
    /// Their `skills/` tree and root, removed if nothing else lives there.
    husk_parents: BTreeSet<PathBuf>,
    superseded: BTreeSet<PathBuf>,
}

impl Hygiene {
    fn is_empty(&self) -> bool {
        self.husks.is_empty() && self.superseded.is_empty()
    }
}

/// Classify leftover installer backups under `target_dir`. Read-only.
///
/// The sweep is bounded by `manifest_db::allowed_roots(harness)` — every root
/// the harness may own, including one it has stopped writing to, which is
/// exactly where a migration leaves its litter — and within those, by the
/// identities the current payload actually has installed and current. A tree
/// no harness owns, or a skill of the captain's own (`caveman`), is never
/// reached.
fn scan_hygiene(
    target_dir: &Path,
    harness: &str,
    payload: &BTreeMap<String, String>,
) -> Result<Hygiene> {
    let mut hygiene = Hygiene::default();
    let mut live: BTreeSet<String> = BTreeSet::new();
    for (rel, want) in payload {
        let path = manifest_db::resolve_target_relative(target_dir, Path::new(rel))?;
        let Ok(on_disk) = std::fs::read(&path) else {
            continue; // missing or unreadable — its backups are still the undo
        };
        if digest::hash_bytes(&on_disk) != digest::hash_bytes(want.as_bytes()) {
            continue; // drifted — the sidecar may be the only copy of v-current
        }
        hygiene.superseded.extend(sibling_install_backups(&path));
        if let Some(identity) = install_identity(rel) {
            live.extend(pre_prefix_aliases(&identity));
            live.insert(identity);
        }
    }
    for root in manifest_db::allowed_roots(harness) {
        for tree in INSTALL_TREES {
            let tree_rel = Path::new(root).join(tree);
            collect_husks(target_dir, &tree_rel, &live, &mut hygiene)?;
        }
    }
    Ok(hygiene)
}

/// Collect the husks directly under one install tree (`.agents/skills`).
///
/// A directory qualifies only when its name is a live identity *and* every
/// entry in it is an installer backup file: one README of the captain's and the
/// directory is theirs, left whole. A loose file qualifies only when it is an
/// installer backup whose original is gone. Symlinks are skipped rather than
/// followed, and a tree that resolves outside the target is skipped rather than
/// failing the run — it is not ours to walk either way.
fn collect_husks(
    target_dir: &Path,
    tree_rel: &Path,
    live: &BTreeSet<String>,
    hygiene: &mut Hygiene,
) -> Result<()> {
    let Ok(tree) = manifest_db::resolve_target_relative(target_dir, tree_rel) else {
        return Ok(());
    };
    let Ok(entries) = std::fs::read_dir(&tree) else {
        return Ok(());
    };
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            if !live.contains(name) {
                continue; // not an artifact we install — not ours to judge
            }
            let Some(backups) = bak_only_entries(&entry.path())? else {
                continue;
            };
            hygiene
                .husks
                .insert(tree_rel.join(name).display().to_string(), backups);
            hygiene.husk_dirs.insert(entry.path());
            hygiene.husk_parents.insert(tree.clone());
            if let Some(root) = tree.parent() {
                hygiene.husk_parents.insert(root.to_path_buf());
            }
        } else if file_type.is_file() && is_install_backup_name(name) {
            let Some((original, _)) = name.rsplit_once(".bak-") else {
                continue;
            };
            if tree.join(original).exists() {
                continue; // a sidecar of a live file, not a husk
            }
            let identity = split_stem_ext(original)
                .map(|(stem, _)| stem)
                .unwrap_or(original);
            if !live.contains(identity) {
                continue;
            }
            hygiene
                .husks
                .entry(tree_rel.join(original).display().to_string())
                .or_default()
                .insert(entry.path());
        }
    }
    Ok(())
}

/// The installer backups in `dir`, or `None` if anything else lives there.
fn bak_only_entries(dir: &Path) -> Result<Option<BTreeSet<PathBuf>>> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Ok(None);
    };
    let mut backups = BTreeSet::new();
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name();
        let is_backup = name.to_str().is_some_and(is_install_backup_name);
        if !is_backup || !entry.file_type()?.is_file() {
            return Ok(None);
        }
        backups.insert(entry.path());
    }
    Ok((!backups.is_empty()).then_some(backups))
}

fn hygiene_check(hygiene: &Hygiene) -> Check {
    if !hygiene.husks.is_empty() {
        let names: Vec<&str> = hygiene.husks.keys().map(String::as_str).collect();
        let mut detail = format!(
            "{} leftover path(s) hold install backups and no live file: {}",
            hygiene.husks.len(),
            names.join(", ")
        );
        if !hygiene.superseded.is_empty() {
            detail.push_str(&format!(
                "; {} superseded backup(s) beside current file(s)",
                hygiene.superseded.len()
            ));
        }
        detail.push_str(". `shipmates doctor --fix` prunes them");
        Check {
            name: "Hygiene".into(),
            severity: Severity::Problem,
            detail,
            fixable: true,
        }
    } else if !hygiene.superseded.is_empty() {
        Check {
            name: "Hygiene".into(),
            severity: Severity::Warn,
            detail: format!(
                "{} install backup(s) sit beside a file that already matches shipmates v{} — \
                 `shipmates doctor --fix` prunes them",
                hygiene.superseded.len(),
                env!("CARGO_PKG_VERSION")
            ),
            fixable: true,
        }
    } else {
        Check {
            name: "Hygiene".into(),
            severity: Severity::Ok,
            detail: "no leftover install backups".into(),
            fixable: false,
        }
    }
}

/// Delete the backups `scan_hygiene` classified. Returns (removed, failed); a
/// failure leaves that file for the next run rather than aborting the repair.
fn prune_hygiene(hygiene: &Hygiene) -> (usize, usize) {
    let mut removed = 0usize;
    let mut failed = 0usize;
    for path in hygiene
        .husks
        .values()
        .flatten()
        .chain(hygiene.superseded.iter())
    {
        match std::fs::remove_file(path) {
            Ok(()) => removed += 1,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => failed += 1,
        }
    }
    // `remove_dir` refuses a non-empty directory, so anything left behind —
    // a backup that would not delete, a file of the captain's — keeps its
    // folder, and the emptied tree and root only go when nothing else is there.
    for dir in &hygiene.husk_dirs {
        let _ = std::fs::remove_dir(dir);
    }
    let mut parents: Vec<&PathBuf> = hygiene.husk_parents.iter().collect();
    parents.sort_by_key(|path| std::cmp::Reverse(path.components().count()));
    for dir in parents {
        let _ = std::fs::remove_dir(dir);
    }
    (removed, failed)
}

/// The files a healthy install must contain, keyed by their on-disk path relative
/// to the target directory (the `<container>/` prefix stripped, exactly as the
/// installer writes them). Only the test harness materialises a healthy tree from
/// this now; production paths build once and pass the map via `strip_container`.
#[cfg(test)]
fn expected_files(
    adapter: &dyn Adapter,
    roles: &[CanonicalRole],
    cmds: &[CanonicalCommand],
) -> Result<BTreeMap<String, String>> {
    Ok(strip_container(
        &adapter.build(roles, cmds)?,
        adapter.container(),
    ))
}

fn agent_name(rel: &str) -> String {
    Path::new(rel)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(rel)
        .to_string()
}

/// Diagnose the health of a harness install under `target_dir`. Read-only.
pub fn diagnose(
    target_dir: &Path,
    harness: &str,
    roles: &[CanonicalRole],
    cmds: &[CanonicalCommand],
    tools: &[CanonicalTool],
    source: &CatalogSource,
) -> Result<Report> {
    let adapter = adapters::select(harness)?;
    let steering = source.steering_for_target(target_dir)?;
    let built = adapters::build_payload(adapter.as_ref(), roles, cmds, steering.as_deref())?;
    diagnose_built(target_dir, harness, adapter.as_ref(), &built, tools)
}

/// The body of `diagnose`, taking an already-built payload so `fix` can reuse the
/// single `build()` it made rather than paying for two more. `built` is the
/// container-prefixed map (as `adapter.build` returns, for `migrate::plan`);
/// `expected` is derived from it once here.
fn diagnose_built(
    target_dir: &Path,
    harness: &str,
    adapter: &dyn Adapter,
    built: &HashMap<String, String>,
    tools: &[CanonicalTool],
) -> Result<Report> {
    let expected = strip_container(built, adapter.container());
    let version = env!("CARGO_PKG_VERSION");
    let mut checks = Vec::new();

    let (receipt_state, receipt, receipt_error) = plan::read_receipt(target_dir, harness);
    match (receipt_state, receipt_error.as_deref()) {
        (plan::ReceiptState::Valid, _) => checks.push(Check {
            name: "Ownership".into(),
            severity: Severity::Ok,
            detail: "install receipt is valid".into(),
            fixable: true,
        }),
        (plan::ReceiptState::Missing, _) => checks.push(Check {
            name: "Ownership".into(),
            severity: Severity::Warn,
            detail: "install receipt missing; ownership is unknown, existing files will be left untouched".into(),
            fixable: false,
        }),
        (plan::ReceiptState::Invalid, error) => checks.push(Check {
            name: "Ownership".into(),
            severity: Severity::Problem,
            detail: format!(
                "install receipt is invalid; refusing ownership-based repair: {}",
                error.unwrap_or("unknown receipt error").to_string()
            ),
            fixable: false,
        }),
    }

    for rel in expected.keys() {
        manifest_db::resolve_target_relative(target_dir, Path::new(rel))?;
    }

    // 1. Install present — the harness's expected dotdir(s) exist.
    let dotdirs: BTreeSet<&str> = expected
        .keys()
        .filter_map(|rel| rel.split('/').next())
        .collect();
    let missing_dotdirs: Vec<&str> = dotdirs
        .iter()
        .copied()
        .filter(|d| !target_dir.join(d).exists())
        .collect();
    if dotdirs.is_empty() {
        checks.push(Check {
            name: "Install present".into(),
            severity: Severity::Ok,
            detail: "nothing expected for this harness".into(),
            fixable: false,
        });
    } else if missing_dotdirs.is_empty() {
        checks.push(Check {
            name: "Install present".into(),
            severity: Severity::Ok,
            detail: format!(
                "found {}",
                dotdirs.iter().copied().collect::<Vec<_>>().join(", ")
            ),
            fixable: false,
        });
    } else {
        checks.push(Check {
            name: "Install present".into(),
            severity: Severity::Problem,
            detail: format!(
                "no install found — missing {}. Run `shipmates install --harness {}`",
                missing_dotdirs.join(", "),
                harness
            ),
            fixable: false,
        });
    }

    // 2. Legacy/duplicate layout — a superseded `commands/<name>.md` beside a
    // skill. Only Shipmates-owned files are a fixable Problem (`--fix` migrates
    // them); a user's own file sharing a skill name is theirs to keep, so it is
    // an informational note rather than a Problem `--fix` could never clear.
    let migration_items = migrate::plan(target_dir, built, adapter.container())?;
    let mut owned = Vec::new();
    let mut unmanaged = Vec::new();
    for item in &migration_items {
        let path = manifest_db::resolve_target_relative(target_dir, &item.legacy_path)?;
        let name = item
            .legacy_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default();
        if migrate::is_shipmates_owned(&path, name) {
            owned.push(item);
        } else {
            unmanaged.push(item);
        }
    }
    if owned.is_empty() {
        checks.push(Check {
            name: "Layout".into(),
            severity: Severity::Ok,
            detail: "no superseded command files shadow a skill".into(),
            fixable: false,
        });
    } else {
        let names: Vec<String> = owned
            .iter()
            .map(|i| i.legacy_path.display().to_string())
            .collect();
        checks.push(Check {
            name: "Layout".into(),
            severity: Severity::Problem,
            detail: format!(
                "{} superseded command file(s) shadow a skill: {}",
                owned.len(),
                names.join(", ")
            ),
            fixable: true,
        });
    }
    if !unmanaged.is_empty() {
        let names: Vec<String> = unmanaged
            .iter()
            .map(|i| i.legacy_path.display().to_string())
            .collect();
        checks.push(Check {
            name: "Shadowed commands".into(),
            severity: Severity::Ok,
            detail: format!(
                "{} of your own command file(s) share a skill name and are shadowed by it — left untouched: {}",
                unmanaged.len(),
                names.join(", ")
            ),
            fixable: false,
        });
    }

    // 2b. Identity — leftover pre-prefix names (`polish` beside `shipmates-polish`)
    // that a Shipmates receipt still claims. `--fix` runs the rename sweep.
    let mut rename_payload = built.clone();
    for (key, content) in adapter.build_tools(tools) {
        rename_payload.insert(key, content);
    }
    let rename_items = rename::plan(target_dir, &rename_payload, adapter.container())?;
    let repository = manifest_db::ReceiptRepository::new(target_dir);
    let mut owned_renames = Vec::new();
    let mut unmanaged_renames = Vec::new();
    for item in &rename_items {
        let this_claim = receipt
            .as_ref()
            .and_then(|current| current.file(&item.old_path.to_string_lossy()))
            .is_some();
        let any_claim = repository.is_claimed(&item.old_path).unwrap_or(false);
        if this_claim || any_claim {
            owned_renames.push(item);
        } else {
            unmanaged_renames.push(item);
        }
    }
    if owned_renames.is_empty() {
        checks.push(Check {
            name: "Identity".into(),
            severity: Severity::Ok,
            detail: "no leftover pre-prefix skill or tool names".into(),
            fixable: false,
        });
    } else {
        let names: Vec<String> = owned_renames
            .iter()
            .map(|item| item.old_path.display().to_string())
            .collect();
        checks.push(Check {
            name: "Identity".into(),
            severity: Severity::Problem,
            detail: format!(
                "{} leftover pre-prefix name(s) still installed: {}",
                owned_renames.len(),
                names.join(", ")
            ),
            fixable: true,
        });
    }
    if !unmanaged_renames.is_empty() {
        let names: Vec<String> = unmanaged_renames
            .iter()
            .map(|item| item.old_path.display().to_string())
            .collect();
        checks.push(Check {
            name: "Foreign names".into(),
            severity: Severity::Ok,
            detail: format!(
                "{} of your own file(s) share a pre-prefix name and were left untouched: {}",
                unmanaged_renames.len(),
                names.join(", ")
            ),
            fixable: false,
        });
    }

    // 3. Missing crew agents.
    let expected_agents: Vec<&String> = expected
        .keys()
        .filter(|rel| rel.split('/').any(|s| s == "agents"))
        .collect();
    if expected_agents.is_empty() {
        checks.push(Check {
            name: "Crew agents".into(),
            severity: Severity::Ok,
            detail: "this harness ships no crew agents".into(),
            fixable: false,
        });
    } else {
        let mut missing: Vec<String> = expected_agents
            .iter()
            .filter(|rel| !target_dir.join(rel).exists())
            .map(|rel| agent_name(rel))
            .collect();
        missing.sort();
        if missing.is_empty() {
            checks.push(Check {
                name: "Crew agents".into(),
                severity: Severity::Ok,
                detail: format!("all {} present", expected_agents.len()),
                fixable: false,
            });
        } else {
            checks.push(Check {
                name: "Crew agents".into(),
                severity: Severity::Problem,
                detail: format!(
                    "missing {} of {}: {}",
                    missing.len(),
                    expected_agents.len(),
                    missing.join(", ")
                ),
                fixable: true,
            });
        }
    }

    // 4. Content drift — present files whose bytes differ from what we'd install.
    // #190: receipt manifest enables true installed-vs-running semantic version compare
    let mut missing: Vec<String> = Vec::new();
    let mut drifted: Vec<String> = Vec::new();
    let mut unreadable: Vec<String> = Vec::new();
    for (rel, want) in &expected {
        match std::fs::read(target_dir.join(rel)) {
            Ok(on_disk) => {
                if std::str::from_utf8(&on_disk).is_err() {
                    unreadable.push(rel.clone());
                } else if digest::hash_bytes(&on_disk) != digest::hash_bytes(want.as_bytes()) {
                    drifted.push(rel.clone());
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => missing.push(rel.clone()),
            Err(_) => unreadable.push(rel.clone()),
        }
    }
    if !missing.is_empty() {
        missing.sort();
        drifted.sort();
        unreadable.sort();
        let mut interrupted: Vec<String> = Vec::new();
        let mut gone: Vec<String> = Vec::new();
        for rel in &missing {
            if sibling_install_backups(&target_dir.join(rel)).is_empty() {
                gone.push(rel.clone());
            } else {
                interrupted.push(rel.clone());
            }
        }
        let mut parts: Vec<String> = Vec::new();
        if !interrupted.is_empty() {
            parts.push(format!(
                "{} interrupted-update (backup present, main file missing): {}",
                interrupted.len(),
                interrupted.join(", ")
            ));
        }
        if !gone.is_empty() {
            parts.push(format!(
                "{} core file(s) missing: {}",
                gone.len(),
                gone.join(", ")
            ));
        }
        let mut detail = parts.join("; ");
        if !drifted.is_empty() {
            detail.push_str(&format!("; drifted: {}", drifted.join(", ")));
        }
        if !unreadable.is_empty() {
            detail.push_str(&format!("; unreadable: {}", unreadable.join(", ")));
        }
        checks.push(Check {
            name: "Content".into(),
            severity: Severity::Problem,
            detail,
            fixable: true,
        });
    } else if !unreadable.is_empty() {
        unreadable.sort();
        drifted.sort();
        let mut details = format!(
            "{} file(s) are present but unreadable: {}",
            unreadable.len(),
            unreadable.join(", ")
        );
        if !drifted.is_empty() {
            details.push_str(&format!(
                "; {} file(s) differ from shipmates v{}: {}",
                drifted.len(),
                version,
                drifted.join(", ")
            ));
        }
        checks.push(Check {
            name: "Content".into(),
            severity: Severity::Problem,
            detail: details,
            fixable: false,
        });
    } else if drifted.is_empty() {
        checks.push(Check {
            name: "Content".into(),
            severity: Severity::Ok,
            detail: format!("every installed file matches shipmates v{}", version),
            fixable: false,
        });
    } else {
        drifted.sort();
        checks.push(Check {
            name: "Content".into(),
            severity: Severity::Warn,
            detail: format!(
                "{} file(s) differ from shipmates v{}: {}",
                drifted.len(),
                version,
                drifted.join(", ")
            ),
            fixable: true,
        });
    }

    // 4b. Payload paths held by files the receipt does not claim. One that
    // declares itself to be the artifact installed there is a Shipmates file
    // that fell out of ownership: `--fix` adopts it. Anything else is somebody
    // else's and only `install --force` may replace it (#386). Assessed only
    // against a valid receipt — without one, ownership of everything is unknown
    // and the Ownership check already says so.
    let mut adoptable: Vec<String> = Vec::new();
    let mut foreign: Vec<String> = Vec::new();
    if let (plan::ReceiptState::Valid, Some(current)) = (receipt_state, receipt.as_ref()) {
        for (rel, want) in &expected {
            if current.file(rel).is_some() || repository.is_claimed(Path::new(rel)).unwrap_or(false)
            {
                continue;
            }
            let Ok(on_disk) = std::fs::read(target_dir.join(rel)) else {
                continue;
            };
            if digest::hash_bytes(&on_disk) == digest::hash_bytes(want.as_bytes()) {
                continue;
            }
            match adopt::classify(Path::new(rel), &on_disk) {
                adopt::Collision::Adoptable => adoptable.push(rel.clone()),
                adopt::Collision::ThirdParty => foreign.push(rel.clone()),
            }
        }
    }
    adoptable.sort();
    foreign.sort();
    if adoptable.is_empty() && foreign.is_empty() {
        checks.push(Check {
            name: "Collisions".into(),
            severity: Severity::Ok,
            detail: if receipt_state == plan::ReceiptState::Valid {
                "no unclaimed files hold a payload path".into()
            } else {
                "ownership is unknown without a valid receipt; payload paths are left as found"
                    .into()
            },
            fixable: false,
        });
    }
    if !adoptable.is_empty() {
        checks.push(Check {
            name: "Collisions".into(),
            severity: Severity::Problem,
            detail: format!(
                "{} shipmates file(s) at payload paths are not receipt-owned and stale: {}. \
                 `shipmates doctor --fix` backs each up, restores v{} and claims it",
                adoptable.len(),
                adoptable.join(", "),
                version
            ),
            fixable: true,
        });
    }
    if !foreign.is_empty() {
        checks.push(Check {
            name: "Foreign collisions".into(),
            severity: Severity::Problem,
            detail: format!(
                "{} file(s) shipmates does not own hold payload path(s): {}. They are left \
                 untouched — run `shipmates install --force` to back each up and install v{} \
                 over it, or move them aside",
                foreign.len(),
                foreign.join(", "),
                version
            ),
            fixable: false,
        });
    }

    // 5. Tool status — optional tools are healthy only when every selected
    // file is present and its raw bytes match. A partially present tool is not
    // the same as no tool installed.
    let prefix = format!("{}/", adapter.container());
    let tool_expected: BTreeMap<String, String> = adapter
        .build_tools(tools)
        .into_iter()
        .filter_map(|(k, v)| k.strip_prefix(&prefix).map(|r| (r.to_string(), v)))
        .collect();
    for rel in tool_expected.keys() {
        manifest_db::resolve_target_relative(target_dir, Path::new(rel))?;
    }
    let mut installed: Vec<String> = Vec::new();
    let mut tool_missing: Vec<String> = Vec::new();
    let mut tool_drift: Vec<String> = Vec::new();
    let mut tool_unreadable: Vec<String> = Vec::new();
    let mut tool_unfixable: Vec<String> = Vec::new();
    let mut tool_orphaned: Vec<String> = Vec::new();
    for t in tools {
        let files: Vec<(&String, &String)> = tool_expected
            .iter()
            .filter(|(k, _)| {
                k.split('/').any(|s| s == t.name)
                    || Path::new(k).file_stem().and_then(|s| s.to_str()) == Some(t.name.as_str())
            })
            .collect();
        if files.is_empty() {
            continue;
        }
        let any_on_disk = files.iter().any(|(k, _)| target_dir.join(k).exists());
        let claimed = |k: &str| {
            receipt
                .as_ref()
                .and_then(|current| current.file(k))
                .is_some()
        };
        if !any_on_disk && !files.iter().any(|(k, _)| claimed(k)) {
            continue;
        }
        if any_on_disk && receipt.is_some() && !files.iter().any(|(k, _)| claimed(k)) {
            tool_orphaned.push(t.name.clone());
            continue;
        }
        let mut complete = true;
        let mut has_issue = false;
        let mut issues_owned = receipt_state == plan::ReceiptState::Valid;
        for (k, want) in &files {
            match std::fs::read(target_dir.join(k)) {
                Ok(on_disk) => {
                    if std::str::from_utf8(&on_disk).is_err() {
                        complete = false;
                        has_issue = true;
                        tool_unreadable.push(t.name.clone());
                        issues_owned = false;
                    } else if digest::hash_bytes(&on_disk) != digest::hash_bytes(want.as_bytes()) {
                        has_issue = true;
                        tool_drift.push(t.name.clone());
                        issues_owned &= claimed(k);
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    complete = false;
                    has_issue = true;
                    tool_missing.push(t.name.clone());
                    issues_owned &= claimed(k);
                }
                Err(_) => {
                    complete = false;
                    has_issue = true;
                    tool_unreadable.push(t.name.clone());
                    issues_owned = false;
                }
            }
        }
        if complete {
            installed.push(t.name.clone());
        }
        if has_issue && !issues_owned {
            tool_unfixable.push(t.name.clone());
        }
    }
    installed.sort();
    tool_missing.sort();
    tool_missing.dedup();
    tool_drift.sort();
    tool_drift.dedup();
    tool_unreadable.sort();
    tool_unreadable.dedup();
    tool_unfixable.sort();
    tool_unfixable.dedup();
    tool_orphaned.sort();
    tool_orphaned.dedup();
    let (severity, detail) =
        if !tool_missing.is_empty() || !tool_unreadable.is_empty() || !tool_orphaned.is_empty() {
            let mut detail = format!(
                "installed: {}; missing: {}",
                installed.join(", "),
                tool_missing.join(", ")
            );
            if !tool_unreadable.is_empty() {
                detail.push_str(&format!("; unreadable: {}", tool_unreadable.join(", ")));
            }
            if !tool_drift.is_empty() {
                detail.push_str(&format!("; drifted: {}", tool_drift.join(", ")));
            }
            if !tool_unfixable.is_empty() {
                detail.push_str(&format!(
                    "; cannot repair without receipt ownership: {}",
                    tool_unfixable.join(", ")
                ));
            }
            if !tool_orphaned.is_empty() {
                detail.push_str(&format!("; orphaned: {}", tool_orphaned.join(", ")));
            }
            (Severity::Problem, detail)
        } else if installed.is_empty() && tool_drift.is_empty() {
            (
                Severity::Ok,
                "no optional tools installed — use `--with-tools none` for crew-only".to_string(),
            )
        } else if tool_drift.is_empty() {
            (
                Severity::Ok,
                format!("installed and current: {}", installed.join(", ")),
            )
        } else {
            (
                Severity::Warn,
                format!(
                    "installed: {}; drifted: {}{}",
                    installed.join(", "),
                    tool_drift.join(", "),
                    if tool_unfixable.is_empty() {
                        String::new()
                    } else {
                        format!(
                            "; cannot repair without receipt ownership: {}",
                            tool_unfixable.join(", ")
                        )
                    }
                ),
            )
        };
    checks.push(Check {
        name: "Tools".into(),
        severity,
        detail,
        fixable: (!tool_missing.is_empty() || !tool_drift.is_empty()) && tool_unfixable.is_empty(),
    });

    // 5b. Install backups, swept across every root the harness may own — the
    // tree a migration moved *out* of is where its litter sits. `rename::plan`
    // sees only live leftovers, so a directory holding nothing but `.bak-…`
    // husks read as clean and doctor called the whole tree shipshape (#406).
    // Reporting is all `doctor` does here — pruning is `--fix`'s opt-in.
    let mut hygiene_payload = expected.clone();
    hygiene_payload.extend(tool_expected.clone());
    checks.push(hygiene_check(&scan_hygiene(
        target_dir,
        harness,
        &hygiene_payload,
    )?));

    // 6. Receipt ownership. The receipt is the authority for repair; files not
    // listed there remain user-owned from doctor's perspective and are never
    // changed automatically.
    let ownership_detail = match plan::read_receipt(target_dir, harness).0 {
        plan::ReceiptState::Valid => {
            "receipt tracks Shipmates-owned files; unlisted files are preserved"
        }
        plan::ReceiptState::Missing => {
            "receipt missing; ownership is unknown and existing files are preserved"
        }
        plan::ReceiptState::Invalid => {
            "receipt invalid; ownership checks fail closed and existing files are preserved"
        }
    };
    checks.push(Check {
        name: "Unmanaged files".into(),
        severity: Severity::Ok,
        detail: ownership_detail.into(),
        fixable: false,
    });

    Ok(Report { checks })
}

/// Repair an install: identity-rename leftover pre-prefix names, migrate
/// superseded commands, then restore any missing or drifted crew/skill files,
/// backing up everything it touches. Re-diagnoses and returns the fresh report.
///
/// With `no_migrate`, the identity-rename and legacy-command sweeps are skipped
/// — parity with `install --no-migrate`: missing/drifted files are still
/// restored, but a superseded `commands/<name>.md` or pre-prefix name is left
/// in place.
pub fn fix(
    target_dir: &Path,
    harness: &str,
    roles: &[CanonicalRole],
    cmds: &[CanonicalCommand],
    tools: &[CanonicalTool],
    no_migrate: bool,
    source: &CatalogSource,
) -> Result<Report> {
    let adapter = adapters::select(harness)?;
    let steering = source.steering_for_target(target_dir)?;
    let built = adapters::build_payload(adapter.as_ref(), roles, cmds, steering.as_deref())?;
    let expected = strip_container(&built, adapter.container());
    let repository = manifest_db::ReceiptRepository::new(target_dir);
    repository.load_all()?;
    let (mut receipt_state, mut receipt, receipt_error) = plan::read_receipt(target_dir, harness);
    if receipt_state == plan::ReceiptState::Invalid {
        // Invalid receipt — treat as missing. Skip migration (unknown ownership)
        // and ownership-based drift repair; only restore genuinely missing core
        // files so --fix makes progress instead of hard-bailing (#272).
        println!(
            "Warning: install receipt for harness {} is invalid — {} (migrate and ownership-based drift repair skipped)",
            harness,
            receipt_error.unwrap_or_else(|| "unknown receipt error".into())
        );
        receipt_state = plan::ReceiptState::Missing;
        receipt = None;
    }
    for rel in expected.keys() {
        manifest_db::resolve_target_relative(target_dir, Path::new(rel))?;
    }
    manifest_db::resolve_target_relative(target_dir, Path::new(migrate::BACKUP_DIR))?;
    let backup_root = migrate::new_backup_root(target_dir);
    let mut migrated_paths = BTreeSet::new();
    let mut migration_report = None;
    let mut rename_report = None;
    let tool_built = adapter.build_tools(tools);
    let mut rename_payload = built.clone();
    for (key, content) in &tool_built {
        rename_payload.insert(key.clone(), content.clone());
    }

    // 0. Identity-rename leftover pre-prefix names before layout migrate,
    // unless the caller opted out with `--no-migrate`. Reload this harness's
    // receipt afterwards so repair sees the new paths.
    if !no_migrate {
        let items = rename::plan(target_dir, &rename_payload, adapter.container())?;
        if !items.is_empty() {
            let report = rename::apply(
                target_dir,
                &items,
                &rename_payload,
                adapter.container(),
                &backup_root,
            )?;
            if !report.renamed.is_empty() {
                rename::print_map(&report);
            }
            rename_report = Some(report);
            let reloaded = plan::read_receipt(target_dir, harness);
            receipt_state = reloaded.0;
            receipt = reloaded.1;
            if receipt_state == plan::ReceiptState::Invalid {
                receipt_state = plan::ReceiptState::Missing;
                receipt = None;
            }
        }
    }

    let tool_prefix = format!("{}/", adapter.container());
    let tool_expected: BTreeMap<String, String> = tool_built
        .into_iter()
        .filter_map(|(k, v)| k.strip_prefix(&tool_prefix).map(|r| (r.to_string(), v)))
        .collect();
    for rel in tool_expected.keys() {
        manifest_db::resolve_target_relative(target_dir, Path::new(rel))?;
    }
    // Classify leftover install backups before anything is repaired, so `--fix`
    // prunes exactly what the preceding report named: a file that is drifted
    // now keeps its sidecar even though the repair below makes it current.
    let mut hygiene_payload = expected.clone();
    hygiene_payload.extend(tool_expected.clone());
    let hygiene = scan_hygiene(target_dir, harness, &hygiene_payload)?;

    let mut repair_expected = expected.clone();
    // Only pull optional-tool files into the repair set when the receipt
    // actually claims them. A no-tools install has no tool files to restore,
    // and listing every uninstalled tool as "skipped" is alarming noise (#267).
    // When some tools are installed, only their files are included so that
    // uninstalled tools do not appear in the skipped report either.
    if let Some(receipt) = receipt.as_ref() {
        for (k, v) in &tool_expected {
            if receipt.file(k).is_some() {
                repair_expected.insert(k.clone(), v.clone());
            }
        }
    }

    // 1. Migrate any superseded command files (backed up before removal), unless
    // the caller opted out with `--no-migrate`.
    if !no_migrate {
        let mut items = if receipt_state == plan::ReceiptState::Valid {
            migrate::plan(target_dir, &built, adapter.container())?
        } else {
            Vec::new()
        };
        if receipt_state == plan::ReceiptState::Valid {
            let owned = receipt.as_ref().expect("valid receipt must be present");
            items.retain(|item| owned.file(&item.legacy_path.to_string_lossy()).is_some());
        } else {
            // Without a receipt, existing files have unknown ownership. Do not
            // migrate or delete them; only genuinely missing payload files may
            // be restored below.
            items.clear();
        }
        if !items.is_empty() {
            let report = migrate::apply(target_dir, &items, &backup_root)?;
            migrated_paths.extend(
                report
                    .migrated
                    .iter()
                    .map(|path| path.to_string_lossy().into_owned()),
            );
            migration_report = Some(report);
            if let Some(report) = migration_report.as_ref()
                && !report.migrated.is_empty()
            {
                println!(
                    "Migrated {} superseded command(s) → skills (backup: {})",
                    report.migrated.len(),
                    backup_root.display()
                );
            }
        }
    }

    // 2. Write any missing or drifted core or optional-tool files. Receipt
    // ownership remains the authority for overwrites; a missing receipt only
    // permits restoring genuinely missing core files, never replacing content.
    let mut restored = 0usize;
    let mut backed_up = 0usize;
    let mut skipped: Vec<String> = Vec::new();
    let mut force_needed: Vec<String> = Vec::new();
    let mut adopted: BTreeSet<String> = BTreeSet::new();
    let mut repaired: BTreeSet<String> = BTreeSet::new();
    let mut changed: Vec<(String, PathBuf, Option<Vec<u8>>)> = Vec::new();
    let mut repair_backups = Vec::new();
    let repair_result: Result<()> = (|| {
        for (rel, want) in &repair_expected {
            let path = manifest_db::resolve_target_relative(target_dir, Path::new(rel))?;
            let owned = receipt
                .as_ref()
                .and_then(|current| current.file(rel))
                .is_some();
            let previous = match std::fs::read(&path) {
                Ok(on_disk) => {
                    if digest::hash_bytes(&on_disk) == digest::hash_bytes(want.as_bytes()) {
                        continue; // already current — nothing to restore
                    }
                    if std::str::from_utf8(&on_disk).is_err() {
                        // Doctor has no --force mode. Leave binary drift untouched;
                        // install --force uses the byte-verified backup path below.
                        skipped.push(rel.clone());
                        continue;
                    }
                    if receipt_state != plan::ReceiptState::Valid {
                        skipped.push(rel.clone());
                        continue;
                    }
                    if !owned {
                        // Unowned but at a payload path: adopt it when it
                        // declares itself to be this artifact, otherwise leave
                        // it and name the flag that can replace it (#386).
                        match adopt::classify(Path::new(rel), &on_disk) {
                            adopt::Collision::Adoptable => {
                                adopted.insert(rel.clone());
                            }
                            adopt::Collision::ThirdParty => {
                                force_needed.push(rel.clone());
                                continue;
                            }
                        }
                    }
                    // Preserve arbitrary bytes before replacing drift, then verify
                    // the backup byte-for-byte. This is required for --fix too:
                    // payload files are text, user files need not be.
                    let backup_path = backup_root.join(rel);
                    let backup_relative =
                        backup_path.strip_prefix(target_dir).map_err(|error| {
                            anyhow::anyhow!("doctor backup escaped target: {}", error)
                        })?;
                    let backup_path =
                        manifest_db::resolve_target_relative(target_dir, backup_relative)?;
                    let backup_ok = crate::installer::atomic_write_bytes(&backup_path, &on_disk)
                        .is_ok()
                        && std::fs::read(&backup_path)
                            .map(|backup| backup == on_disk)
                            .unwrap_or(false);
                    if !backup_ok {
                        skipped.push(rel.clone());
                        continue;
                    }
                    backed_up += 1;
                    repair_backups.push(backup_path);
                    Some(on_disk)
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    let siblings = sibling_install_backups(&path);
                    let matching = siblings
                        .iter()
                        .any(|bak| backup_matches_payload(bak, want.as_bytes()));
                    // A payload-matching sibling backup is an interrupted install
                    // rewrite: restore even when the receipt does not list the
                    // path (the receipt may predate the file, or the update
                    // died before rewriting it).
                    if !matching && receipt_state == plan::ReceiptState::Valid && !owned {
                        // Nothing on disk to protect: write the payload and
                        // claim the path rather than leaving a flagship absent
                        // because an old receipt never listed it (#386).
                        adopted.insert(rel.clone());
                    }
                    None
                }
                Err(_) => {
                    // Present but unreadable: no verified byte backup is possible.
                    skipped.push(rel.clone());
                    continue;
                }
            };
            crate::installer::atomic_write_bytes(&path, want.as_bytes())
                .map_err(anyhow::Error::from)?;
            changed.push((rel.clone(), path, previous));
            restored += 1;
            repaired.insert(rel.clone());
        }
        Ok(())
    })();
    if let Err(error) = repair_result {
        let repair_rollback = rollback_repairs(target_dir, &changed, &repair_backups);
        let migration_rollback = match migration_report.as_ref() {
            Some(report) => migrate::rollback(target_dir, report),
            None => Ok(()),
        };
        let rename_rollback = match rename_report.as_ref() {
            Some(report) => rename::rollback(target_dir, report),
            None => Ok(()),
        };
        return Err(combine_rollback_error(
            combine_rollback_error(
                combine_rollback_error(error, repair_rollback),
                migration_rollback,
            ),
            rename_rollback,
        ));
    }
    if restored > 0 {
        // A backup dir is only created for drifted overwrites; restoring only
        // missing files writes no backup, so don't advertise one that isn't there.
        if backed_up > 0 {
            println!(
                "Restored {} payload file(s) to shipmates v{} (backup: {})",
                restored,
                env!("CARGO_PKG_VERSION"),
                backup_root.display()
            );
        } else {
            println!(
                "Restored {} payload file(s) to shipmates v{}",
                restored,
                env!("CARGO_PKG_VERSION")
            );
        }
    }
    if !skipped.is_empty() {
        println!(
            "Skipped {} file(s) shipmates could not safely repair (no verified backup, \
             or present but unreadable) — left them untouched: {}",
            skipped.len(),
            skipped.join(", ")
        );
    }
    if !force_needed.is_empty() {
        println!(
            "Left {} file(s) shipmates does not own untouched at payload path(s): {} — run \
             `shipmates install --force` to back each up and install v{} over it.",
            force_needed.len(),
            force_needed.join(", "),
            env!("CARGO_PKG_VERSION")
        );
    }

    let publication_result: Result<()> = (|| {
        let Some(current) = receipt.as_mut() else {
            return Ok(());
        };
        if restored == 0 && migrated_paths.is_empty() {
            return Ok(());
        }
        current
            .files
            .retain(|file| !migrated_paths.contains(&file.path));
        for file in &mut current.files {
            if repaired.contains(&file.path) {
                let path = manifest_db::resolve_target_relative(target_dir, Path::new(&file.path))?;
                file.sha256 = digest::compute_sha256(&path)?;
            }
        }
        // Adopted paths join the receipt, with the root they sit under, so the
        // next upgrade owns them instead of warning about them forever.
        for rel in &adopted {
            if current.file(rel).is_some() {
                continue;
            }
            let path = manifest_db::resolve_target_relative(target_dir, Path::new(rel))?;
            if let Some(root) = Path::new(rel)
                .components()
                .next()
                .and_then(|component| component.as_os_str().to_str())
                && !current.roots.iter().any(|known| known == root)
            {
                current.roots.push(root.to_string());
                current.roots.sort();
            }
            current.files.push(manifest_db::ReceiptFile {
                path: rel.clone(),
                sha256: digest::compute_sha256(&path)?,
            });
        }
        current
            .files
            .sort_by(|left, right| left.path.cmp(&right.path));
        current.version = env!("CARGO_PKG_VERSION").into();
        current.validate()?;
        let receipt_path = repository.receipt_path(harness)?;
        let previous_receipt = std::fs::read(&receipt_path).ok();
        if let Err(error) = repository.save(current) {
            if let Some(bytes) = previous_receipt {
                let _ = crate::installer::atomic_write_bytes(&receipt_path, &bytes);
            } else {
                let _ = std::fs::remove_file(&receipt_path);
            }
            return Err(error);
        }
        Ok(())
    })();
    if let Err(error) = publication_result {
        let repair_rollback = rollback_repairs(target_dir, &changed, &repair_backups);
        let migration_rollback = match migration_report.as_ref() {
            Some(report) => migrate::rollback(target_dir, report),
            None => Ok(()),
        };
        let rename_rollback = match rename_report.as_ref() {
            Some(report) => rename::rollback(target_dir, report),
            None => Ok(()),
        };
        return Err(combine_rollback_error(
            combine_rollback_error(
                combine_rollback_error(error, repair_rollback),
                migration_rollback,
            ),
            rename_rollback,
        ));
    }

    // 3. Prune the leftover install backups. Deliberately last and never rolled
    // back: it removes only copies of bytes that are already on disk, and a
    // per-file failure leaves that file for the next run.
    if !hygiene.is_empty() {
        let (removed, failed) = prune_hygiene(&hygiene);
        if removed > 0 {
            println!("Pruned {} leftover install backup(s)", removed);
        }
        if failed > 0 {
            println!(
                "Could not prune {} install backup(s) — left them in place",
                failed
            );
        }
    }

    // 4. Re-diagnose and hand back the fresh report — reusing the single built
    // payload rather than rebuilding it.
    diagnose_built(target_dir, harness, adapter.as_ref(), &built, tools)
}

fn rollback_repairs(
    target_dir: &Path,
    changed: &[(String, PathBuf, Option<Vec<u8>>)],
    backups: &[PathBuf],
) -> Result<()> {
    for (rel, _path, previous) in changed.iter().rev() {
        let path = manifest_db::resolve_target_relative(target_dir, Path::new(rel))?;
        match previous {
            Some(bytes) => crate::installer::atomic_write_bytes(&path, bytes)
                .map_err(anyhow::Error::from)
                .with_context(|| format!("restoring doctor repair {}", path.display()))?,
            None => match std::fs::remove_file(&path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            },
        }
    }
    for backup in backups {
        let relative = backup
            .strip_prefix(target_dir)
            .map_err(|error| anyhow::anyhow!("doctor backup escaped target: {}", error))?;
        let backup = manifest_db::resolve_target_relative(target_dir, relative)?;
        let _ = std::fs::remove_file(backup);
    }
    Ok(())
}

fn combine_rollback_error(error: anyhow::Error, rollback: Result<()>) -> anyhow::Error {
    match rollback {
        Ok(()) => error,
        Err(rollback) => error.context(rollback.to_string()),
    }
}

/// Print a report in a plain, positive voice — OKs included, so a healthy
/// install is affirmed rather than silent.
pub fn print_report(report: &Report) {
    println!("shipmates doctor · v{}\n", env!("CARGO_PKG_VERSION"));
    for c in &report.checks {
        let tag = match c.severity {
            Severity::Ok => "ok  ",
            Severity::Warn => "warn",
            Severity::Problem => "fix ",
        };
        println!("  [{}] {} — {}", tag, c.name, c.detail);
    }
    println!();
    if report.has_problems() {
        println!(
            "Some checks need attention. Run `shipmates doctor --fix` to repair what shipmates can."
        );
    } else if report.checks.iter().any(|c| c.severity == Severity::Warn) {
        println!(
            "Mostly shipshape — `shipmates doctor --fix` brings the flagged files back in line."
        );
    } else {
        println!("All shipshape. Your crew is aboard and current.");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::installer::atomic_write;
    use std::path::PathBuf;
    use tempfile::tempdir;

    // Source-agnostic shims: these tests build every payload from the passed-in
    // catalogs, so the source only decides steering, which a tempdir target
    // never receives.
    fn diagnose(
        target_dir: &Path,
        harness: &str,
        roles: &[CanonicalRole],
        cmds: &[CanonicalCommand],
        tools: &[CanonicalTool],
    ) -> Result<Report> {
        super::diagnose(
            target_dir,
            harness,
            roles,
            cmds,
            tools,
            &CatalogSource::Embedded,
        )
    }

    fn fix(
        target_dir: &Path,
        harness: &str,
        roles: &[CanonicalRole],
        cmds: &[CanonicalCommand],
        tools: &[CanonicalTool],
        no_migrate: bool,
    ) -> Result<Report> {
        super::fix(
            target_dir,
            harness,
            roles,
            cmds,
            tools,
            no_migrate,
            &CatalogSource::Embedded,
        )
    }

    fn role(name: &str) -> CanonicalRole {
        CanonicalRole {
            name: name.into(),
            description: "d".into(),
            capabilities: vec![],
            writes: false,
            web_scopes: vec![],
            read_scopes: vec![],
            tool_order: vec![],
            effort: None,
            source: PathBuf::from(""),
            body: "b".into(),
        }
    }

    fn cmd(name: &str) -> CanonicalCommand {
        CanonicalCommand {
            name: name.into(),
            description: "d".into(),
            argument_hint: "".into(),
            allowed_tools: "".into(),
            disable_model_invocation: true,
            arguments: vec![],
            narrative: "n".into(),
            invocation: "".into(),
            board: "".into(),
            source: PathBuf::from(""),
        }
    }

    fn tool(name: &str) -> CanonicalTool {
        CanonicalTool {
            name: name.into(),
            description: "d".into(),
            body: "b".into(),
            assets: vec![],
            requires: vec![],
            source: PathBuf::from(""),
        }
    }

    fn install_healthy(target: &Path, roles: &[CanonicalRole], cmds: &[CanonicalCommand]) {
        let adapter = adapters::select("claude-code").unwrap();
        for (rel, content) in expected_files(adapter.as_ref(), roles, cmds).unwrap() {
            atomic_write(&target.join(&rel), &content).unwrap();
        }
    }

    fn install_tools(target: &Path, tools: &[CanonicalTool]) {
        let adapter = adapters::select("claude-code").unwrap();
        let built = adapter.build_tools(tools);
        for (rel, content) in strip_container(&built, adapter.container()) {
            atomic_write(&target.join(&rel), &content).unwrap();
        }
    }

    fn write_receipt(
        target: &Path,
        roles: &[CanonicalRole],
        cmds: &[CanonicalCommand],
        tools: &[CanonicalTool],
    ) {
        let adapter = adapters::select("claude-code").unwrap();
        let install = crate::installer::plan::InstallPlan::from_payload(
            adapter.as_ref(),
            "claude-code",
            adapter.build(roles, cmds).unwrap(),
            adapter.build_tools(tools),
        )
        .unwrap();
        let receipt = install.receipt_for(install.files.keys().cloned()).unwrap();
        crate::installer::plan::save_receipt(target, &receipt).unwrap();
    }

    fn sev(report: &Report, name: &str) -> Severity {
        report
            .checks
            .iter()
            .find(|c| c.name == name)
            .unwrap()
            .severity
    }

    #[test]
    fn test_fix_restores_missing_skill_from_sibling_install_backup() {
        // Interrupted install: main file gone, `{name}.bak-<secs>-<pid>-<n>`
        // sibling still there and matches the current payload. Receipt may not
        // list the path (update died before rewriting ownership) — --fix must
        // still restore, and diagnose must name interrupted-update (#352).
        let dir = tempdir().unwrap();
        let target = dir.path();
        let roles = [role("architect")];
        let cmds = [cmd("ship-issue")];
        install_healthy(target, &roles, &cmds);

        let adapter = adapters::select("claude-code").unwrap();
        let files = expected_files(adapter.as_ref(), &roles, &cmds).unwrap();
        let skill_rel = files
            .keys()
            .find(|k| k.ends_with("SKILL.md") && k.contains("ship-issue"))
            .cloned()
            .expect("ship-issue skill in payload");
        let skill_path = target.join(&skill_rel);
        let want = files.get(&skill_rel).unwrap().clone();
        let bak_path = skill_path.with_file_name("SKILL.md.bak-1788191317-3827013-0");
        std::fs::copy(&skill_path, &bak_path).unwrap();
        std::fs::remove_file(&skill_path).unwrap();

        // Receipt claims crew agents only — the skill is unowned, matching the
        // production skip ("valid receipt + missing + not in receipt").
        let install = crate::installer::plan::InstallPlan::from_payload(
            adapter.as_ref(),
            "claude-code",
            adapter.build(&roles, &cmds).unwrap(),
            adapter.build_tools(&[]),
        )
        .unwrap();
        let agent_keys: Vec<PathBuf> = install
            .files
            .keys()
            .filter(|p| p.to_string_lossy().contains("/agents/"))
            .cloned()
            .collect();
        let receipt = install.receipt_for(agent_keys).unwrap();
        crate::installer::plan::save_receipt(target, &receipt).unwrap();

        let before = diagnose(target, "claude-code", &roles, &cmds, &[]).unwrap();
        let content = before.checks.iter().find(|c| c.name == "Content").unwrap();
        assert_eq!(content.severity, Severity::Problem);
        assert!(
            content.detail.contains("interrupted-update"),
            "diagnose must name interrupted-update: {}",
            content.detail
        );
        assert!(content.detail.contains(&skill_rel), "{}", content.detail);

        let after = fix(target, "claude-code", &roles, &cmds, &[], false).unwrap();
        assert_eq!(
            std::fs::read_to_string(&skill_path).unwrap(),
            want,
            "must restore payload bytes so Content matches the running version"
        );
        assert_eq!(sev(&after, "Content"), Severity::Ok);
        assert!(
            after
                .checks
                .iter()
                .find(|c| c.name == "Content")
                .unwrap()
                .detail
                .contains(&format!("shipmates v{}", env!("CARGO_PKG_VERSION"))),
            "Content ok must name the running version"
        );
    }

    /// The installer's own sidecar shape, for husks the tests plant.
    const BAK: &str = ".bak-1788191317-3827013-0";

    #[test]
    fn test_install_backup_name_shape_is_the_ownership_signal() {
        assert!(is_install_backup_name("SKILL.md.bak-1788191317-3827013-0"));
        assert!(is_install_backup_name("gh.py.bak-1-2-3"));
        // A captain's own backup: never the installer's, never touched.
        assert!(!is_install_backup_name("notes.md.bak-mine"));
        assert!(!is_install_backup_name("notes.md.bak"));
        assert!(!is_install_backup_name("SKILL.md.bak-1-2"));
        assert!(!is_install_backup_name(".bak-1-2-3"));
        assert!(!is_install_backup_name("SKILL.md"));
    }

    #[test]
    fn test_install_identity_and_pre_prefix_aliases() {
        assert_eq!(
            install_identity(".cursor/skills/shipmates-harden/SKILL.md").as_deref(),
            Some("shipmates-harden")
        );
        assert_eq!(
            install_identity(".agents/skills/ship-issue/SKILL.md").as_deref(),
            Some("ship-issue")
        );
        assert_eq!(
            install_identity(".claude/agents/architect.md").as_deref(),
            Some("architect")
        );
        assert_eq!(install_identity(".shipmates/receipts/cursor.json"), None);

        assert_eq!(pre_prefix_aliases("shipmates-harden"), vec!["harden"]);
        assert_eq!(pre_prefix_aliases("shipmates-gh"), vec!["gh"]);
        // Flagships and third-party skills are not in the rename table.
        assert!(pre_prefix_aliases("ship-issue").is_empty());
        assert!(pre_prefix_aliases("caveman").is_empty());
    }

    #[test]
    fn test_diagnose_reports_bak_only_husk_and_fix_prunes_it() {
        // #406: `skills/harden/` holding nothing but `SKILL.md.bak-…` is
        // invisible to the live-file rename sweep, so doctor called the tree
        // shipshape. It is a Problem, and --fix prunes the husk (and the
        // superseded sidecar beside the current skill) without touching the
        // live payload.
        let dir = tempdir().unwrap();
        let target = dir.path();
        let roles = [role("architect")];
        let cmds = [cmd("shipmates-harden")];
        install_healthy(target, &roles, &cmds);
        write_receipt(target, &roles, &cmds, &[]);

        let live = target.join(".claude/skills/shipmates-harden/SKILL.md");
        let live_bytes = std::fs::read_to_string(&live).unwrap();
        let husk = target.join(format!(".claude/skills/harden/SKILL.md{BAK}"));
        atomic_write(&husk, "pre-prefix payload\n").unwrap();
        let sidecar = target.join(format!(".claude/skills/shipmates-harden/SKILL.md{BAK}"));
        atomic_write(&sidecar, "shipmates v0.1.4\n").unwrap();

        let before = diagnose(target, "claude-code", &roles, &cmds, &[]).unwrap();
        let check = before.checks.iter().find(|c| c.name == "Hygiene").unwrap();
        assert_eq!(check.severity, Severity::Problem);
        assert!(check.fixable);
        assert!(
            check.detail.contains(".claude/skills/harden"),
            "must name the husk: {}",
            check.detail
        );
        assert!(
            before.has_problems(),
            "a tree full of husks must not exit zero"
        );
        assert!(husk.exists(), "diagnose is read-only");

        let after = fix(target, "claude-code", &roles, &cmds, &[], false).unwrap();
        assert!(!husk.exists(), "husk backup must be pruned");
        assert!(
            !target.join(".claude/skills/harden").exists(),
            "the emptied identity directory goes with it"
        );
        assert!(!sidecar.exists(), "superseded sidecar must be pruned");
        assert_eq!(
            std::fs::read_to_string(&live).unwrap(),
            live_bytes,
            "the live skill is untouched"
        );
        assert_eq!(sev(&after, "Hygiene"), Severity::Ok);
        assert!(!after.has_problems());
    }

    #[test]
    fn test_diagnose_reports_husks_left_in_a_migrated_tree_and_fix_prunes_them() {
        // #405 moved cursor's skills off the shared `.agents/` tree onto
        // `.cursor/`, leaving a `SKILL.md.bak-…` alone in each emptied
        // `.agents/skills/<name>/`. `.agents` is still a cursor root, so the
        // sweep reaches it — and the names are flagships, which no rename-table
        // row can reach (#406).
        let dir = tempdir().unwrap();
        let target = dir.path();
        let cmds = [cmd("ship-issue"), cmd("plan-epics")];
        let adapter = adapters::select("cursor").unwrap();
        for (rel, content) in
            strip_container(&adapter.build(&[], &cmds).unwrap(), adapter.container())
        {
            atomic_write(&target.join(&rel), &content).unwrap();
        }
        let husks: Vec<PathBuf> = cmds
            .iter()
            .map(|command| {
                let husk = target.join(format!(".agents/skills/{}/SKILL.md{BAK}", command.name));
                atomic_write(&husk, "pre-#405 payload\n").unwrap();
                husk
            })
            .collect();
        // A skill of the captain's own, in the same emptied tree.
        let third_party = target.join(".agents/skills/caveman/SKILL.md");
        atomic_write(&third_party, "name: caveman\n").unwrap();

        let before = diagnose(target, "cursor", &[], &cmds, &[]).unwrap();
        let check = before.checks.iter().find(|c| c.name == "Hygiene").unwrap();
        assert_eq!(check.severity, Severity::Problem);
        assert!(
            check.detail.contains(".agents/skills/ship-issue")
                && check.detail.contains(".agents/skills/plan-epics"),
            "must name the husks in the tree the install moved out of: {}",
            check.detail
        );
        assert!(
            before.has_problems(),
            "a migrated-away tree is not shipshape"
        );

        let after = fix(target, "cursor", &[], &cmds, &[], false).unwrap();
        for husk in &husks {
            assert!(!husk.exists(), "{} must be pruned", husk.display());
            assert!(
                !husk.parent().unwrap().exists(),
                "the emptied identity directory goes with it"
            );
        }
        assert!(
            third_party.exists(),
            "a skill shipmates does not install keeps its tree"
        );
        assert!(
            target.join(".agents/skills").exists(),
            "and so the tree itself stays"
        );
        assert!(
            target.join(".cursor/skills/ship-issue/SKILL.md").exists(),
            "the live payload is untouched"
        );
        assert_eq!(sev(&after, "Hygiene"), Severity::Ok);
        assert!(!after.has_problems());
    }

    #[test]
    fn test_hygiene_leaves_hand_named_and_interrupted_backups_alone() {
        // Two files hygiene must never claim: a captain's own `*.bak-mine`,
        // which does not have the installer's shape, and a sidecar beside a
        // drifted file, which may be the only copy of the current version.
        let dir = tempdir().unwrap();
        let target = dir.path();
        let roles = [role("architect")];
        let cmds = [cmd("ship-issue")];
        install_healthy(target, &roles, &cmds);
        write_receipt(target, &roles, &cmds, &[]);

        let mine = target.join(".claude/agents/notes.md.bak-mine");
        atomic_write(&mine, "my own backup\n").unwrap();
        let drifted = target.join(".claude/agents/architect.md");
        atomic_write(&drifted, "hand-edited\n").unwrap();
        let interrupted = target.join(format!(".claude/agents/architect.md{BAK}"));
        atomic_write(&interrupted, "shipmates v0.1.4\n").unwrap();

        let before = diagnose(target, "claude-code", &roles, &cmds, &[]).unwrap();
        assert_eq!(sev(&before, "Hygiene"), Severity::Ok);
        assert_eq!(
            sev(&before, "Content"),
            Severity::Warn,
            "drift is Content's"
        );

        fix(target, "claude-code", &roles, &cmds, &[], false).unwrap();
        assert!(
            mine.exists(),
            "a hand-named backup is not shipmates' to prune"
        );
        assert!(
            interrupted.exists(),
            "the sidecar of a file that was drifted at report time stays"
        );
    }

    #[test]
    fn test_hygiene_leaves_third_party_and_occupied_husk_alone() {
        // A skill the rename table never names is out of scope entirely, and a
        // pre-prefix directory the captain keeps a file in is theirs, not a husk.
        let dir = tempdir().unwrap();
        let target = dir.path();
        let roles = [role("architect")];
        let cmds = [cmd("shipmates-polish")];
        install_healthy(target, &roles, &cmds);
        write_receipt(target, &roles, &cmds, &[]);

        let third_party = target.join(".claude/skills/caveman/SKILL.md");
        atomic_write(&third_party, "name: caveman\n").unwrap();
        let third_party_bak = target.join(format!(".claude/skills/caveman/SKILL.md{BAK}"));
        atomic_write(&third_party_bak, "older caveman\n").unwrap();
        let readme = target.join(".claude/skills/polish/README.md");
        atomic_write(&readme, "why I kept this\n").unwrap();
        let occupied = target.join(format!(".claude/skills/polish/SKILL.md{BAK}"));
        atomic_write(&occupied, "pre-prefix payload\n").unwrap();

        let before = diagnose(target, "claude-code", &roles, &cmds, &[]).unwrap();
        assert_eq!(sev(&before, "Hygiene"), Severity::Ok);

        fix(target, "claude-code", &roles, &cmds, &[], false).unwrap();
        assert!(third_party.exists() && third_party_bak.exists());
        assert!(readme.exists(), "the captain's file keeps its directory");
        assert!(occupied.exists(), "so the husk beside it stays too");
    }

    #[test]
    fn test_parse_install_backup_name() {
        assert_eq!(
            parse_install_backup_name("SKILL.md.bak-1788191317-3827013-0", "SKILL.md"),
            Some((1788191317, 3827013, 0))
        );
        assert!(parse_install_backup_name("SKILL.md.bak", "SKILL.md").is_none());
        assert!(parse_install_backup_name("SKILL.md.bak-1-2", "SKILL.md").is_none());
        assert!(parse_install_backup_name("other.md.bak-1-2-3", "SKILL.md").is_none());
    }

    #[test]
    fn test_fix_adopts_missing_unowned_payload_path_from_the_payload() {
        // A payload path that is absent and unclaimed has nothing to protect:
        // --fix writes the payload bytes and claims it, and never trusts a
        // sibling backup whose contents are not the payload (#386).
        let dir = tempdir().unwrap();
        let target = dir.path();
        let roles = [role("architect")];
        let cmds = [cmd("ship-issue")];
        install_healthy(target, &roles, &cmds);

        let adapter = adapters::select("claude-code").unwrap();
        let files = expected_files(adapter.as_ref(), &roles, &cmds).unwrap();
        let skill_rel = files
            .keys()
            .find(|k| k.ends_with("SKILL.md") && k.contains("ship-issue"))
            .cloned()
            .expect("ship-issue skill in payload");
        let skill_path = target.join(&skill_rel);
        let want = files.get(&skill_rel).unwrap().clone();
        let bak_path = skill_path.with_file_name("SKILL.md.bak-1788191317-3827013-0");
        std::fs::write(&bak_path, "not the payload").unwrap();
        std::fs::remove_file(&skill_path).unwrap();

        let install = crate::installer::plan::InstallPlan::from_payload(
            adapter.as_ref(),
            "claude-code",
            adapter.build(&roles, &cmds).unwrap(),
            adapter.build_tools(&[]),
        )
        .unwrap();
        let agent_keys: Vec<PathBuf> = install
            .files
            .keys()
            .filter(|p| p.to_string_lossy().contains("/agents/"))
            .cloned()
            .collect();
        let receipt = install.receipt_for(agent_keys).unwrap();
        crate::installer::plan::save_receipt(target, &receipt).unwrap();

        let after = fix(target, "claude-code", &roles, &cmds, &[], false).unwrap();

        assert_eq!(
            std::fs::read_to_string(&skill_path).unwrap(),
            want,
            "restore must come from the payload, not the non-matching backup"
        );
        assert_eq!(sev(&after, "Content"), Severity::Ok);
        let receipt = crate::installer::plan::read_receipt(target, "claude-code")
            .1
            .unwrap();
        assert!(
            receipt.file(&skill_rel).is_some(),
            "an adopted path must be claimed"
        );
    }

    #[test]
    fn test_fix_adopts_stale_unowned_shipmates_file_but_not_a_foreign_one() {
        let adapter = adapters::select("claude-code").unwrap();
        let roles = [role("architect")];
        let cmds = [cmd("ship-issue")];
        let files = expected_files(adapter.as_ref(), &roles, &cmds).unwrap();
        let skill_rel = files
            .keys()
            .find(|k| k.ends_with("SKILL.md") && k.contains("ship-issue"))
            .cloned()
            .expect("ship-issue skill in payload");
        let want = files.get(&skill_rel).unwrap().clone();

        for (planted, adoptable) in [
            ("---\nname: ship-issue\n---\nstale shipmates copy\n", true),
            ("---\nname: someone-elses\n---\nmine\n", false),
        ] {
            let dir = tempdir().unwrap();
            let target = dir.path();
            install_healthy(target, &roles, &cmds);
            atomic_write(&target.join(&skill_rel), planted).unwrap();

            // Receipt claims the crew only — the skill is a live payload path
            // nobody owns.
            let install = crate::installer::plan::InstallPlan::from_payload(
                adapter.as_ref(),
                "claude-code",
                adapter.build(&roles, &cmds).unwrap(),
                adapter.build_tools(&[]),
            )
            .unwrap();
            let agent_keys: Vec<PathBuf> = install
                .files
                .keys()
                .filter(|p| p.to_string_lossy().contains("/agents/"))
                .cloned()
                .collect();
            crate::installer::plan::save_receipt(target, &install.receipt_for(agent_keys).unwrap())
                .unwrap();

            let before = diagnose(target, "claude-code", &roles, &cmds, &[]).unwrap();
            let check_name = if adoptable {
                "Collisions"
            } else {
                "Foreign collisions"
            };
            assert_eq!(sev(&before, check_name), Severity::Problem);
            if !adoptable {
                let detail = &before
                    .checks
                    .iter()
                    .find(|check| check.name == check_name)
                    .unwrap()
                    .detail;
                assert!(
                    detail.contains("shipmates install --force"),
                    "a foreign collision must name the flag that can replace it: {detail}"
                );
            }

            fix(target, "claude-code", &roles, &cmds, &[], false).unwrap();

            let on_disk = std::fs::read_to_string(target.join(&skill_rel)).unwrap();
            let receipt = crate::installer::plan::read_receipt(target, "claude-code")
                .1
                .unwrap();
            if adoptable {
                assert_eq!(on_disk, want, "a stale shipmates file must be adopted");
                assert!(receipt.file(&skill_rel).is_some());
            } else {
                assert_eq!(on_disk, planted, "a foreign file must be untouched");
                assert!(receipt.file(&skill_rel).is_none());
            }
        }
    }

    #[test]
    fn test_diagnose_missing_install_reports_problem() {
        let dir = tempdir().unwrap();
        let roles = [role("architect")];
        let cmds = [cmd("ship-issue")];
        let report = diagnose(dir.path(), "claude-code", &roles, &cmds, &[]).unwrap();
        assert!(report.has_problems());
        assert_eq!(sev(&report, "Install present"), Severity::Problem);
        // No panic on an empty dir; the migration plan is empty.
        assert_eq!(sev(&report, "Layout"), Severity::Ok);
    }

    #[test]
    fn test_diagnose_does_not_write() {
        let dir = tempdir().unwrap();
        let roles = [role("architect")];
        let cmds = [cmd("ship-issue")];
        let _ = diagnose(dir.path(), "claude-code", &roles, &cmds, &[]).unwrap();
        assert!(
            std::fs::read_dir(dir.path()).unwrap().next().is_none(),
            "diagnose must be read-only"
        );
    }

    #[test]
    fn test_diagnose_clean_install_is_healthy() {
        let dir = tempdir().unwrap();
        let roles = [role("architect"), role("sdet")];
        let cmds = [cmd("ship-issue")];
        install_healthy(dir.path(), &roles, &cmds);
        let report = diagnose(dir.path(), "claude-code", &roles, &cmds, &[]).unwrap();
        assert!(!report.has_problems());
        assert_eq!(sev(&report, "Crew agents"), Severity::Ok);
        assert_eq!(sev(&report, "Content"), Severity::Ok);
    }

    #[test]
    fn test_diagnose_detects_missing_agent() {
        let dir = tempdir().unwrap();
        let roles = [role("architect"), role("devops-engineer")];
        let cmds = [cmd("ship-issue")];
        install_healthy(dir.path(), &roles, &cmds);
        std::fs::remove_file(dir.path().join(".claude/agents/devops-engineer.md")).unwrap();
        let report = diagnose(dir.path(), "claude-code", &roles, &cmds, &[]).unwrap();
        assert!(report.has_problems());
        let crew = report
            .checks
            .iter()
            .find(|c| c.name == "Crew agents")
            .unwrap();
        assert_eq!(crew.severity, Severity::Problem);
        assert!(crew.detail.contains("devops-engineer"));
        assert!(crew.fixable);
    }

    #[test]
    fn test_diagnose_detects_content_drift() {
        let dir = tempdir().unwrap();
        let roles = [role("architect")];
        let cmds = [cmd("ship-issue")];
        install_healthy(dir.path(), &roles, &cmds);
        atomic_write(
            &dir.path().join(".claude/agents/architect.md"),
            "hand-edited\n",
        )
        .unwrap();
        let report = diagnose(dir.path(), "claude-code", &roles, &cmds, &[]).unwrap();
        let content = report.checks.iter().find(|c| c.name == "Content").unwrap();
        assert_eq!(content.severity, Severity::Warn);
        assert!(content.fixable);
    }

    #[test]
    fn test_fix_restores_missing_but_leaves_unowned_legacy() {
        let dir = tempdir().unwrap();
        let target = dir.path();
        let roles = [role("architect"), role("devops-engineer")];
        let cmds = [cmd("ship-issue")];
        install_healthy(target, &roles, &cmds);
        // Break it: remove an agent, plant a superseded (owned) legacy command.
        std::fs::remove_file(target.join(".claude/agents/devops-engineer.md")).unwrap();
        atomic_write(
            &target.join(".claude/commands/ship-issue.md"),
            "---\nname: ship-issue\n---\nold\n",
        )
        .unwrap();

        let before = diagnose(target, "claude-code", &roles, &cmds, &[]).unwrap();
        assert!(before.has_problems());

        let after = fix(target, "claude-code", &roles, &cmds, &[], false).unwrap();
        assert!(after.has_problems());
        assert!(target.join(".claude/agents/devops-engineer.md").exists());
        assert!(target.join(".claude/commands/ship-issue.md").exists());
    }

    #[test]
    fn test_fix_leaves_unreadable_file_untouched_and_skips_it() {
        let dir = tempdir().unwrap();
        let target = dir.path();
        let roles = [role("architect")];
        let cmds = [cmd("ship-issue")];
        install_healthy(target, &roles, &cmds);

        // Corrupt a managed file with invalid UTF-8 so `read_to_string` fails —
        // present-but-unreadable, the case that used to be conflated with
        // "missing" and overwritten with no backup.
        let victim = target.join(".claude/agents/architect.md");
        let bad_bytes = [0xffu8, 0xfe, 0x00, 0x9c];
        std::fs::write(&victim, bad_bytes).unwrap();

        let report = fix(target, "claude-code", &roles, &cmds, &[], false).unwrap();

        // Byte-for-byte untouched — never overwritten via atomic_write.
        assert_eq!(std::fs::read(&victim).unwrap(), bad_bytes);
        // Still unreadable as text, proving it was skipped rather than restored.
        assert!(std::fs::read_to_string(&victim).is_err());
        assert!(report.has_problems());
        assert_eq!(sev(&report, "Content"), Severity::Problem);
    }

    #[test]
    fn test_diagnose_reports_unreadable_crew_skill_and_tool() {
        let cases = [
            (".claude/agents/architect.md", "Content", false),
            (".claude/skills/ship-issue/SKILL.md", "Content", false),
            (".claude/skills/termgif/SKILL.md", "Tools", true),
        ];

        for (relative, check_name, is_tool) in cases {
            let dir = tempdir().unwrap();
            let roles = [role("architect")];
            let cmds = [cmd("ship-issue")];
            let tools = if is_tool {
                vec![tool("termgif")]
            } else {
                vec![]
            };
            install_healthy(dir.path(), &roles, &cmds);
            if is_tool {
                install_tools(dir.path(), &tools);
            }

            std::fs::write(dir.path().join(relative), [0xffu8, 0xfe, 0x00]).unwrap();

            let report = diagnose(dir.path(), "claude-code", &roles, &cmds, &tools).unwrap();
            assert_eq!(sev(&report, check_name), Severity::Problem, "{relative}");
            assert!(report.has_problems(), "{relative}: {report:?}");
        }
    }

    #[test]
    fn test_fix_no_migrate_keeps_legacy_but_restores_missing() {
        let dir = tempdir().unwrap();
        let target = dir.path();
        let roles = [role("architect"), role("devops-engineer")];
        let cmds = [cmd("ship-issue")];
        install_healthy(target, &roles, &cmds);
        // Break it: remove an agent, plant a superseded (owned) legacy command.
        std::fs::remove_file(target.join(".claude/agents/devops-engineer.md")).unwrap();
        atomic_write(
            &target.join(".claude/commands/ship-issue.md"),
            "---\nname: ship-issue\n---\nold\n",
        )
        .unwrap();

        let after = fix(target, "claude-code", &roles, &cmds, &[], true).unwrap();

        // Missing agent still restored...
        assert!(target.join(".claude/agents/devops-engineer.md").exists());
        // ...but the owned legacy command is left in place — no migration sweep.
        assert!(target.join(".claude/commands/ship-issue.md").exists());
        // And the report still flags the un-migrated legacy layout as a Problem.
        assert_eq!(sev(&after, "Layout"), Severity::Problem);
    }

    #[test]
    fn test_fix_repairs_owned_tool_drift_and_missing_file() {
        let dir = tempdir().unwrap();
        let target = dir.path();
        let roles = [role("architect")];
        let cmds = [cmd("ship-issue")];
        let mut termgif = tool("termgif");
        termgif
            .assets
            .push(("termgif.py".into(), "print('termgif')".into()));
        let tools = [termgif];
        install_healthy(target, &roles, &cmds);
        install_tools(target, &tools);
        write_receipt(target, &roles, &cmds, &tools);

        let adapter = adapters::select("claude-code").unwrap();
        let tool_files = strip_container(&adapter.build_tools(&tools), adapter.container());
        let mut paths = tool_files.keys();
        let drifted = paths.next().unwrap();
        atomic_write(&target.join(drifted), "drifted").unwrap();
        let missing = paths.next();
        if let Some(missing) = missing {
            std::fs::remove_file(target.join(missing)).unwrap();
        }

        let report = fix(target, "claude-code", &roles, &cmds, &tools, false).unwrap();

        for (rel, expected) in tool_files {
            assert_eq!(std::fs::read_to_string(target.join(rel)).unwrap(), expected);
        }
        assert_eq!(sev(&report, "Tools"), Severity::Ok);
    }

    #[test]
    fn test_diagnose_reports_unowned_tool_drift_as_unrepairable() {
        let dir = tempdir().unwrap();
        let target = dir.path();
        let roles = [role("architect")];
        let cmds = [cmd("ship-issue")];
        let tools = [tool("termgif")];
        install_healthy(target, &roles, &cmds);
        install_tools(target, &tools);

        let adapter = adapters::select("claude-code").unwrap();
        let path = strip_container(&adapter.build_tools(&tools), adapter.container())
            .into_keys()
            .next()
            .unwrap();
        atomic_write(&target.join(path), "user drift").unwrap();

        let report = diagnose(target, "claude-code", &roles, &cmds, &tools).unwrap();
        let tools_check = report
            .checks
            .iter()
            .find(|check| check.name == "Tools")
            .unwrap();
        assert_eq!(tools_check.severity, Severity::Warn);
        assert!(
            tools_check
                .detail
                .contains("cannot repair without receipt ownership")
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_diagnose_rejects_symlinked_legacy_migration_component() {
        use std::os::unix::fs::symlink;

        let dir = tempdir().unwrap();
        let outside = tempdir().unwrap();
        let roles = [role("architect")];
        let cmds = [cmd("ship-issue")];
        install_healthy(dir.path(), &roles, &cmds);
        let outside_file = outside.path().join("ship-issue.md");
        atomic_write(&outside_file, "---\nname: ship-issue\n---\nold\n").unwrap();
        symlink(outside.path(), dir.path().join(".claude/commands")).unwrap();

        let error = diagnose(dir.path(), "claude-code", &roles, &cmds, &[]).unwrap_err();

        assert!(error.to_string().contains("symlink component"));
        assert!(outside_file.exists());
    }
    #[test]
    fn test_fix_corrupted_receipt_does_not_bail() {
        // Corrupted receipt must degrade gracefully — restore missing files,
        // skip migration and ownership-based drift repair (#272).
        let dir = tempdir().unwrap();
        let target = dir.path();
        let roles = [role("architect")];
        let cmds = [cmd("ship-issue")];
        install_healthy(target, &roles, &cmds);
        write_receipt(target, &roles, &cmds, &[]);

        // Corrupt the receipt by overwriting with invalid bytes.
        let adapter = adapters::select("claude-code").unwrap();
        let files = expected_files(adapter.as_ref(), &roles, &cmds).unwrap();
        let mut receipt_rel = files.keys().find(|k| k.ends_with(".sha256")).cloned();
        if let Some(ref mut rel) = receipt_rel {
            *rel = rel.replace(".sha256", "");
        }
        if let Some(receipt_path) = receipt_rel {
            let receipt_path = target.join(&receipt_path);
            std::fs::write(&receipt_path, "CORRUPTED_BYTES_NOT_VALID_JSON").unwrap();
        }

        // Remove a crew agent to create a missing-file scenario.
        let agent_rel = files
            .keys()
            .find(|k| k.contains("agents") && k.ends_with(".md"))
            .cloned();
        if let Some(ref rel) = agent_rel {
            std::fs::remove_file(target.join(rel)).unwrap();
        }

        // fix() must succeed (not bail) and restore the missing agent.
        let report = fix(target, "claude-code", &roles, &cmds, &[], false).unwrap();
        assert_eq!(sev(&report, "Crew agents"), Severity::Ok);
        if let Some(ref rel) = agent_rel {
            assert!(target.join(rel).exists(), "missing agent must be restored");
        }
    }

    #[test]
    fn test_diagnose_opencode_tool_by_file_stem() {
        // Opencode stores tools as `tools/<name>.ts` (flat file per tool), not
        // `skills/<name>/SKILL.md` (directory per tool). The doctor must match
        // by file stem, not just path segment (#271).
        let dir = tempdir().unwrap();
        let target = dir.path();
        let roles = [role("architect")];
        let cmds = [cmd("ship-issue")];
        let tools = [tool("badge")];

        let adapter = adapters::select("opencode").unwrap();
        let built = adapter.build(&roles, &cmds).unwrap();
        let expected = strip_container(&built, adapter.container());
        for (rel, content) in &expected {
            atomic_write(&target.join(rel), content).unwrap();
        }
        // Write the tool file at the opencode-native flat path.
        let tool_built = adapter.build_tools(&tools);
        for (rel, content) in strip_container(&tool_built, adapter.container()) {
            atomic_write(&target.join(&rel), &content).unwrap();
        }
        // Write a valid receipt that claims the tool file.
        let install = crate::installer::plan::InstallPlan::from_payload(
            adapter.as_ref(),
            "opencode",
            built,
            tool_built,
        )
        .unwrap();
        let receipt = install.receipt_for(install.files.keys().cloned()).unwrap();
        crate::installer::plan::save_receipt(target, &receipt).unwrap();

        let report = diagnose(target, "opencode", &roles, &cmds, &tools).unwrap();
        let tools_check = report.checks.iter().find(|c| c.name == "Tools").unwrap();
        assert_eq!(tools_check.severity, Severity::Ok);
        assert!(
            tools_check.detail.contains("badge"),
            "doctor must detect opencode tool by file stem: {}",
            tools_check.detail
        );
    }

    #[test]
    fn test_diagnose_orphaned_tool_not_marked_ok() {
        // Tools with files on disk but no receipt ownership must not be
        // reported as "installed and current" (#270).
        let dir = tempdir().unwrap();
        let target = dir.path();
        let roles = [role("architect")];
        let cmds = [cmd("ship-issue")];
        let tools = [tool("badge"), tool("scrub")];

        install_healthy(target, &roles, &cmds);
        install_tools(target, &tools);

        let adapter = adapters::select("claude-code").unwrap();
        let tool_built = adapter.build_tools(&tools);
        let all_keys: Vec<String> = tool_built.keys().cloned().collect();
        let badge_keys: Vec<PathBuf> = all_keys
            .iter()
            .filter(|k| k.contains("badge"))
            .map(|k| PathBuf::from(k))
            .collect();
        let install = crate::installer::plan::InstallPlan::from_payload(
            adapter.as_ref(),
            "claude-code",
            adapter.build(&roles, &cmds).unwrap(),
            tool_built,
        )
        .unwrap();
        let receipt = install.receipt_for(badge_keys).unwrap();
        crate::installer::plan::save_receipt(target, &receipt).unwrap();

        let report = diagnose(target, "claude-code", &roles, &cmds, &tools).unwrap();
        let tools_check = report.checks.iter().find(|c| c.name == "Tools").unwrap();
        // Scrub is on disk but unclaimed — must not be OK.
        assert_ne!(
            tools_check.severity,
            Severity::Ok,
            "orphaned tool must not be reported as OK: {}",
            tools_check.detail
        );
        assert!(
            tools_check.detail.contains("orphaned"),
            "doctor must report orphaned tool: {}",
            tools_check.detail
        );
    }
}
