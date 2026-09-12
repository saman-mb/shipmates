//! Identity-rename colliding install names with a `shipmates-` prefix.
//!
//! Flagship commands and crew roles are absent from the table — they keep
//! their names. Generic command verbs and every tool move (`polish` →
//! `shipmates-polish`, `gh` → `shipmates-gh`). Adapters expand a row into
//! real paths by substituting the identity folder or file stem in a payload
//! map; leftover `…/commands/<old>.md` is included when the payload ships
//! `…/skills/<new>/SKILL.md`.
//!
//! Ownership is any Shipmates receipt that `claims_for_path`, or — for a tree
//! installed before receipts existed — a file at a rename-table path that
//! still declares the very identity the table moves (`name: harden` at
//! `…/skills/harden/SKILL.md`), which is reclaimed (#403). New files are
//! written from the current payload (never `mv` of a directory), then old
//! files are deleted, then every receipt that listed the old path is
//! rewritten. `--no-migrate` skips this sweep.

use crate::digest;
use crate::installer::adopt;
use crate::installer::atomic_write_bytes;
use crate::installer::manifest_db::{self, InstallReceipt, ReceiptFile, ReceiptRepository};
use crate::installer::migrate;
use anyhow::{Context, Result};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

/// Generic command verbs that pick up the `shipmates-` prefix.
/// Flagships (`ship-issue`, `ship-epic`, `plan-epics`, `pr-review`,
/// `report-bug`, `consolidate-issues`) are deliberately absent.
pub const COMMAND_RENAMES: &[(&str, &str)] = &[
    ("document", "shipmates-document"),
    ("fix-bug", "shipmates-fix-bug"),
    ("harden", "shipmates-harden"),
    ("migrate", "shipmates-migrate"),
    ("onboard", "shipmates-onboard"),
    ("polish", "shipmates-polish"),
    ("refactor", "shipmates-refactor"),
    ("release", "shipmates-release"),
    ("spike", "shipmates-spike"),
];

/// Every tool occupies the same skill tree as commands; `gh` is the collision
/// that actually bites. Identity rows only — no aliases besides the prefix.
pub const TOOL_RENAMES: &[(&str, &str)] = &[
    ("gh", "shipmates-gh"),
    ("badge", "shipmates-badge"),
    ("diagram", "shipmates-diagram"),
    ("domaincheck", "shipmates-domaincheck"),
    ("fixtures", "shipmates-fixtures"),
    ("pixelart", "shipmates-pixelart"),
    ("scrub", "shipmates-scrub"),
    ("social-card", "shipmates-social-card"),
    ("sparkline", "shipmates-sparkline"),
    ("svgflow", "shipmates-svgflow"),
    ("termgif", "shipmates-termgif"),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenameKind {
    Command,
    Tool,
}

/// One old path that an identity row maps onto a payload path.
#[derive(Debug, Clone)]
pub struct RenameItem {
    pub old_path: PathBuf,
    pub new_path: PathBuf,
    pub old_name: String,
    pub new_name: String,
    pub kind: RenameKind,
}

/// What `apply` did — paths are relative to the target directory unless noted.
#[derive(Debug, Clone, Default)]
pub struct RenameReport {
    pub renamed: Vec<RenamedFile>,
    pub skipped_unmanaged: Vec<PathBuf>,
    pub backups: Vec<PathBuf>,
    receipt_originals: Vec<(PathBuf, Vec<u8>)>,
}

#[derive(Debug, Clone)]
pub struct RenamedFile {
    pub old_path: PathBuf,
    pub new_path: PathBuf,
    pub old_name: String,
    pub new_name: String,
    pub kind: RenameKind,
    pub backup: PathBuf,
    pub wrote_new: bool,
    /// No receipt claimed the old path; ownership came from the file's own
    /// identity declaration instead (#403).
    pub reclaimed: bool,
}

/// Who the file at a rename-table old path belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ownership {
    /// A Shipmates receipt lists the old path.
    Claimed,
    /// No receipt lists it, but it declares the identity the table moves.
    Reclaimed,
    /// Somebody else's file that merely sits at a table name.
    ThirdParty,
}

/// Match `--with-tools` against a catalog name, accepting either the current
/// name or the pre-prefix alias from the rename table.
pub fn matches_requested_tool(requested: &str, available_name: &str) -> bool {
    if requested == available_name {
        return true;
    }
    TOOL_RENAMES.iter().any(|(old, new)| {
        (requested == *old && available_name == *new)
            || (requested == *new && available_name == *old)
    })
}

/// Canonical (post-prefix) tool name for errors and listings.
pub fn canonical_tool_name(name: &str) -> &str {
    TOOL_RENAMES
        .iter()
        .find(|(old, new)| *old == name || *new == name)
        .map(|(_, new)| *new)
        .unwrap_or(name)
}

/// Target-relative old paths a `--no-migrate` install must keep so apply
/// does not drop them.
pub fn preserved_old_paths(items: &[RenameItem]) -> BTreeSet<String> {
    items
        .iter()
        .map(|item| item.old_path.to_string_lossy().into_owned())
        .collect()
}

/// Plan identity renames from a payload map.
///
/// Keys may be target-relative (an `InstallPlan`'s `files` keys) or
/// container-prefixed (`<container>/.claude/…`). Identity substitution
/// rewrites a folder or file stem equal to the *new* name back to the old
/// name. When the payload ships `…/skills/<new>/SKILL.md`, a leftover
/// `…/commands/<old>.md` is also scheduled if it exists on disk.
pub fn plan(
    target_dir: &Path,
    payload: &HashMap<String, String>,
    container: &str,
) -> Result<Vec<RenameItem>> {
    let rel_keys = collect_rel_keys(payload, container);
    let payload_rels: BTreeSet<&str> = rel_keys.iter().map(String::as_str).collect();
    let mut items = Vec::new();
    let mut seen = BTreeSet::new();

    for (old_name, new_name, kind) in all_rows() {
        for rel in &rel_keys {
            if let Some(old_path) = substitute_identity(rel, new_name, old_name) {
                consider_item(
                    target_dir,
                    &old_path,
                    rel,
                    old_name,
                    new_name,
                    kind,
                    &payload_rels,
                    &mut seen,
                    &mut items,
                )?;
            }
            if let Some(leftover) = leftover_command_path(rel, new_name, old_name) {
                consider_item(
                    target_dir,
                    &leftover,
                    rel,
                    old_name,
                    new_name,
                    kind,
                    &payload_rels,
                    &mut seen,
                    &mut items,
                )?;
            }
        }
    }
    items.sort_by(|a, b| a.old_path.cmp(&b.old_path));
    Ok(items)
}

/// Apply a rename plan: write new from payload, then delete owned old files,
/// then rewrite every receipt that listed the old path.
///
/// The order is load-bearing — the new file is on disk (and verified) before
/// the old file is removed. A write failure leaves the old file untouched.
/// Files no receipt claims are left in place with a one-line notice.
pub fn apply(
    target_dir: &Path,
    items: &[RenameItem],
    payload: &HashMap<String, String>,
    container: &str,
    backup_root: &Path,
) -> Result<RenameReport> {
    let mut report = RenameReport::default();
    let backup_relative = backup_root.strip_prefix(target_dir).map_err(|error| {
        anyhow::anyhow!(
            "rename backup escaped target {}: {}",
            backup_root.display(),
            error
        )
    })?;
    manifest_db::resolve_target_relative(target_dir, backup_relative)?;
    let repository = ReceiptRepository::new(target_dir);
    let mut receipt_originals: HashMap<String, (PathBuf, Vec<u8>)> = HashMap::new();
    let mut noticed = BTreeSet::new();

    for item in items {
        let result = apply_one(
            target_dir,
            item,
            payload,
            container,
            backup_root,
            &repository,
            &mut receipt_originals,
            &mut report,
            &mut noticed,
        );
        if let Err(error) = result {
            report.receipt_originals = receipt_originals.values().cloned().collect();
            let rollback = rollback(target_dir, &report);
            return Err(combine_rollback_error(error, rollback));
        }
    }
    report.receipt_originals = receipt_originals.into_values().collect();
    Ok(report)
}

/// Restore files and receipts rewritten by a successful apply. Used when a
/// later payload write fails, so a rename never becomes an irreversible side
/// effect of an unsuccessful install.
pub fn rollback(target_dir: &Path, report: &RenameReport) -> Result<()> {
    for (receipt_path, bytes) in report.receipt_originals.iter().rev() {
        let relative = receipt_path
            .strip_prefix(target_dir)
            .map_err(|error| anyhow::anyhow!("rename receipt escaped target: {error}"))?;
        let path = manifest_db::resolve_target_relative(target_dir, relative)?;
        atomic_write_bytes(&path, bytes)
            .with_context(|| format!("restoring receipt {}", path.display()))?;
    }
    for item in report.renamed.iter().rev() {
        let backup_relative = item
            .backup
            .strip_prefix(target_dir)
            .map_err(|error| anyhow::anyhow!("rename backup escaped target: {error}"))?;
        let backup_path = manifest_db::resolve_target_relative(target_dir, backup_relative)?;
        let bytes = fs::read(&backup_path)
            .with_context(|| format!("reading rename backup {}", backup_path.display()))?;
        let old_path = manifest_db::resolve_target_relative(target_dir, &item.old_path)?;
        atomic_write_bytes(&old_path, &bytes)
            .with_context(|| format!("restoring renamed file {}", old_path.display()))?;
        let old_path = manifest_db::resolve_target_relative(target_dir, &item.old_path)?;
        if fs::read(&old_path)
            .map(|restored| restored != bytes)
            .unwrap_or(true)
        {
            anyhow::bail!(
                "rename rollback verification failed for {}",
                old_path.display()
            );
        }
        if item.wrote_new {
            let new_path = manifest_db::resolve_target_relative(target_dir, &item.new_path)?;
            match fs::remove_file(&new_path) {
                Ok(()) => remove_empty_parents(target_dir, &item.new_path)?,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(error)
                        .with_context(|| format!("removing renamed file {}", new_path.display()));
                }
            }
        }
        let backup_path = manifest_db::resolve_target_relative(target_dir, backup_relative)?;
        fs::remove_file(&backup_path)
            .with_context(|| format!("removing rename backup {}", backup_path.display()))?;
    }
    Ok(())
}

/// Print the logical rename map once, grouped by kind. Identities no receipt
/// claimed are reported separately, so a captain can see which moves rested on
/// the file's own declaration rather than on recorded ownership (#403). No-ops
/// if nothing actually moved.
pub fn print_map(report: &RenameReport) {
    for reclaimed in [false, true] {
        let mut commands = BTreeMap::new();
        let mut tools = BTreeMap::new();
        for item in report
            .renamed
            .iter()
            .filter(|item| item.reclaimed == reclaimed)
        {
            match item.kind {
                RenameKind::Command => {
                    commands.insert(item.old_name.as_str(), item.new_name.as_str());
                }
                RenameKind::Tool => {
                    tools.insert(item.old_name.as_str(), item.new_name.as_str());
                }
            }
        }
        let verb = if reclaimed { "Reclaimed" } else { "Renamed" };
        if !commands.is_empty() {
            println!(
                "{verb} {} command(s) (autocomplete /shipmates-):",
                commands.len()
            );
            for (old, new) in commands {
                println!("  /{old} → /{new}");
            }
        }
        if !tools.is_empty() {
            println!("{verb} {} tool(s):", tools.len());
            for (old, new) in tools {
                println!("  {old} → {new}");
            }
        }
    }
}

fn apply_one(
    target_dir: &Path,
    item: &RenameItem,
    payload: &HashMap<String, String>,
    container: &str,
    backup_root: &Path,
    repository: &ReceiptRepository,
    receipt_originals: &mut HashMap<String, (PathBuf, Vec<u8>)>,
    report: &mut RenameReport,
    noticed: &mut BTreeSet<String>,
) -> Result<()> {
    let full_old = manifest_db::resolve_target_relative(target_dir, &item.old_path)?;
    let metadata = match fs::symlink_metadata(&full_old) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(error).with_context(|| format!("inspecting {}", full_old.display()));
        }
    };
    if !metadata.file_type().is_file() {
        report.skipped_unmanaged.push(item.old_path.clone());
        return Ok(());
    }
    let contents =
        fs::read(&full_old).with_context(|| format!("reading {}", full_old.display()))?;
    let ownership = ownership(repository, item, &contents)?;
    if ownership == Ownership::ThirdParty {
        report.skipped_unmanaged.push(item.old_path.clone());
        notice_left(item, noticed);
        return Ok(());
    }

    let Some(want) = lookup_payload(payload, &item.new_path.to_string_lossy(), container) else {
        anyhow::bail!(
            "rename payload is missing {}; refusing to delete {}",
            item.new_path.display(),
            item.old_path.display()
        );
    };

    // Settle the new path before a single byte moves, so a refusal leaves no
    // orphaned backup behind and the old file stays exactly as found.
    let full_new = manifest_db::resolve_target_relative(target_dir, &item.new_path)?;
    let new_exists = match fs::symlink_metadata(&full_new) {
        Ok(meta) if meta.file_type().is_file() => true,
        Ok(_) => {
            anyhow::bail!("refusing to overwrite non-file at {}", full_new.display());
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(error) => {
            return Err(error).with_context(|| format!("inspecting {}", full_new.display()));
        }
    };
    let write_new = if new_exists {
        let current =
            fs::read(&full_new).with_context(|| format!("reading {}", full_new.display()))?;
        if current == want.as_bytes() {
            false
        } else if !repository.claims_for_path(&item.new_path)?.is_empty()
            || adopt::classify(&item.new_path, &current) == adopt::Collision::Adoptable
        {
            true
        } else {
            // Somebody else already occupies the new name. Leaving the old file
            // in place keeps one working copy rather than two half-migrated
            // ones; the notice says so out loud (#403).
            report.skipped_unmanaged.push(item.old_path.clone());
            notice_left(item, noticed);
            return Ok(());
        }
    } else {
        true
    };

    let backup_path = backup_root.join(&item.old_path);
    let backup_relative = backup_path
        .strip_prefix(target_dir)
        .map_err(|error| anyhow::anyhow!("rename backup escaped target: {error}"))?;
    let backup_path = manifest_db::resolve_target_relative(target_dir, backup_relative)?;
    atomic_write_bytes(&backup_path, &contents)
        .with_context(|| format!("backing up {}", full_old.display()))?;
    let backup_relative = backup_path
        .strip_prefix(target_dir)
        .map_err(|error| anyhow::anyhow!("rename backup escaped target: {error}"))?;
    let verified_backup = manifest_db::resolve_target_relative(target_dir, backup_relative)?;
    if fs::read(&verified_backup)
        .map(|backup| backup != contents)
        .unwrap_or(true)
    {
        let _ = fs::remove_file(&verified_backup);
        anyhow::bail!("backup verification failed for {}", full_old.display());
    }

    if write_new {
        atomic_write_bytes(&full_new, want.as_bytes())
            .with_context(|| format!("writing {}", full_new.display()))?;
        let verified = manifest_db::resolve_target_relative(target_dir, &item.new_path)?;
        if fs::read(&verified)
            .map(|bytes| bytes != want.as_bytes())
            .unwrap_or(true)
        {
            anyhow::bail!("write verification failed for {}", full_new.display());
        }
    }

    // New is valid on disk. Record before delete so a delete error still
    // rolls back.
    report.renamed.push(RenamedFile {
        old_path: item.old_path.clone(),
        new_path: item.new_path.clone(),
        old_name: item.old_name.clone(),
        new_name: item.new_name.clone(),
        kind: item.kind,
        backup: backup_path.clone(),
        wrote_new: write_new,
        reclaimed: ownership == Ownership::Reclaimed,
    });
    report.backups.push(backup_path);

    let full_old = manifest_db::resolve_target_relative(target_dir, &item.old_path)?;
    fs::remove_file(&full_old)
        .with_context(|| format!("removing superseded file {}", full_old.display()))?;
    remove_empty_parents(target_dir, &item.old_path)?;

    rewrite_receipts(
        repository,
        &item.old_path.to_string_lossy(),
        &item.new_path.to_string_lossy(),
        &digest::hash_bytes(want.as_bytes()),
        receipt_originals,
    )?;
    Ok(())
}

/// Who owns the file sitting at a rename-table old path.
///
/// A receipt claim settles it. Failing that — a tree installed before receipts
/// existed, or one restored from a backup — the file may still be reclaimed,
/// but only when it declares the very identity this table row moves: `name:
/// harden` in `…/skills/harden/SKILL.md` (#403). The path must imply that same
/// name, so reclaiming stays confined to the table's own names and a
/// third-party artifact squatting one (`name: my-own-polish`) fails closed, as
/// does anything `adopt` cannot read or recognise.
fn ownership(
    repository: &ReceiptRepository,
    item: &RenameItem,
    contents: &[u8],
) -> Result<Ownership> {
    if !repository.claims_for_path(&item.old_path)?.is_empty() {
        return Ok(Ownership::Claimed);
    }
    if adopt::artifact_name(&item.old_path).as_deref() == Some(item.old_name.as_str())
        && adopt::classify(&item.old_path, contents) == adopt::Collision::Adoptable
    {
        return Ok(Ownership::Reclaimed);
    }
    Ok(Ownership::ThirdParty)
}

/// One line per old identity for anything the sweep declined to move.
fn notice_left(item: &RenameItem, noticed: &mut BTreeSet<String>) {
    if !noticed.insert(item.old_name.clone()) {
        return;
    }
    let kind = match item.kind {
        RenameKind::Command => "skill",
        RenameKind::Tool => "tool",
    };
    println!(
        "left {} (not ours); new {kind} is {}",
        display_old_identity(&item.old_path),
        item.new_name
    );
}

fn rewrite_receipts(
    repository: &ReceiptRepository,
    old_path: &str,
    new_path: &str,
    new_sha256: &str,
    originals: &mut HashMap<String, (PathBuf, Vec<u8>)>,
) -> Result<()> {
    for receipt in repository.load_all()? {
        if receipt.file(old_path).is_none() {
            continue;
        }
        if !originals.contains_key(&receipt.harness) {
            let path = repository.receipt_path(&receipt.harness)?;
            let bytes = fs::read(&path)
                .with_context(|| format!("snapshotting receipt {}", path.display()))?;
            originals.insert(receipt.harness.clone(), (path, bytes));
        }
        let mut files = receipt.files.clone();
        let already = files.iter().any(|file| file.path == new_path);
        files.retain(|file| file.path != old_path);
        if already {
            if let Some(file) = files.iter_mut().find(|file| file.path == new_path) {
                file.sha256 = new_sha256.to_string();
            }
        } else {
            files.push(ReceiptFile {
                path: new_path.to_string(),
                sha256: new_sha256.to_string(),
            });
        }
        files.sort_by(|left, right| left.path.cmp(&right.path));
        let updated = InstallReceipt::new(
            receipt.version,
            receipt.harness,
            receipt.layout,
            receipt.roots,
            files,
        )?;
        repository.write(&updated)?;
    }
    Ok(())
}

fn consider_item(
    target_dir: &Path,
    old_path: &str,
    new_path: &str,
    old_name: &str,
    new_name: &str,
    kind: RenameKind,
    payload_rels: &BTreeSet<&str>,
    seen: &mut BTreeSet<String>,
    items: &mut Vec<RenameItem>,
) -> Result<()> {
    if old_path == new_path || !seen.insert(old_path.to_string()) {
        return Ok(());
    }
    if payload_rels.contains(old_path) {
        // Current payload still ships the old path — it is live, not leftover.
        return Ok(());
    }
    let rel = PathBuf::from(old_path);
    manifest_db::resolve_target_relative(target_dir, &rel)?;
    match fs::symlink_metadata(target_dir.join(&rel)) {
        Ok(meta) if meta.file_type().is_file() => {}
        Ok(_) | Err(_) => return Ok(()),
    }
    items.push(RenameItem {
        old_path: rel,
        new_path: PathBuf::from(new_path),
        old_name: old_name.to_string(),
        new_name: new_name.to_string(),
        kind,
    });
    Ok(())
}

fn collect_rel_keys(payload: &HashMap<String, String>, container: &str) -> Vec<String> {
    let mut keys = BTreeSet::new();
    let prefix = if container.is_empty() {
        String::new()
    } else {
        format!("{container}/")
    };
    for key in payload.keys() {
        let rel = if !prefix.is_empty() {
            key.strip_prefix(&prefix)
                .map(str::to_string)
                .or_else(|| key.starts_with('.').then(|| key.clone()))
        } else if key.starts_with('.') {
            Some(key.clone())
        } else {
            None
        };
        let Some(rel) = rel else {
            continue;
        };
        if rel.split('/').any(|seg| seg == migrate::BACKUP_DIR) {
            continue;
        }
        keys.insert(rel);
    }
    keys.into_iter().collect()
}

fn lookup_payload<'a>(
    payload: &'a HashMap<String, String>,
    rel: &str,
    container: &str,
) -> Option<&'a str> {
    if let Some(value) = payload.get(rel) {
        return Some(value.as_str());
    }
    if !container.is_empty() {
        if let Some(value) = payload.get(&format!("{container}/{rel}")) {
            return Some(value.as_str());
        }
    }
    None
}

/// Rewrite a path by replacing a folder or file stem equal to `from` with `to`.
fn substitute_identity(rel: &str, from: &str, to: &str) -> Option<String> {
    let parts: Vec<&str> = rel.split('/').collect();
    if parts.iter().any(|seg| *seg == migrate::BACKUP_DIR) {
        return None;
    }
    let mut changed = false;
    let mut out = Vec::with_capacity(parts.len());
    for (index, part) in parts.iter().enumerate() {
        let is_last = index + 1 == parts.len();
        if !is_last && *part == from {
            out.push(to.to_string());
            changed = true;
            continue;
        }
        if is_last {
            if let Some((stem, ext)) = split_stem_ext(part) {
                if stem == from {
                    out.push(format!("{to}{ext}"));
                    changed = true;
                    continue;
                }
            }
        }
        out.push((*part).to_string());
    }
    if changed { Some(out.join("/")) } else { None }
}

fn leftover_command_path(rel: &str, new_name: &str, old_name: &str) -> Option<String> {
    let parts: Vec<&str> = rel.split('/').collect();
    if parts.len() < 4
        || parts[parts.len() - 1] != "SKILL.md"
        || parts[parts.len() - 3] != "skills"
        || parts[parts.len() - 2] != new_name
    {
        return None;
    }
    let mut leftover = String::new();
    for seg in &parts[..parts.len() - 3] {
        leftover.push_str(seg);
        leftover.push('/');
    }
    leftover.push_str("commands/");
    leftover.push_str(old_name);
    leftover.push_str(".md");
    Some(leftover)
}

fn split_stem_ext(filename: &str) -> Option<(&str, &str)> {
    let dot = filename.rfind('.')?;
    if dot == 0 {
        return None;
    }
    Some((&filename[..dot], &filename[dot..]))
}

fn display_old_identity(old_path: &Path) -> String {
    let name = old_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    if name == "SKILL.md" || name.ends_with(".py") {
        old_path
            .parent()
            .map(|parent| parent.display().to_string())
            .unwrap_or_else(|| old_path.display().to_string())
    } else {
        old_path.display().to_string()
    }
}

fn remove_empty_parents(target_dir: &Path, file_rel: &Path) -> Result<()> {
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
        let full = manifest_db::resolve_target_relative(target_dir, parent)?;
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

fn all_rows() -> impl Iterator<Item = (&'static str, &'static str, RenameKind)> {
    COMMAND_RENAMES
        .iter()
        .map(|(old, new)| (*old, *new, RenameKind::Command))
        .chain(
            TOOL_RENAMES
                .iter()
                .map(|(old, new)| (*old, *new, RenameKind::Tool)),
        )
}

fn combine_rollback_error(error: anyhow::Error, rollback: Result<()>) -> anyhow::Error {
    match rollback {
        Ok(()) => error,
        Err(rollback) => error.context(rollback.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::installer::apply::{self, apply_with_preserved_paths};
    use crate::installer::atomic_write;
    use crate::installer::plan::InstallPlan;
    use std::collections::BTreeMap;
    use tempfile::tempdir;

    fn payload_skill(rel_new: &str, body: &str) -> HashMap<String, String> {
        let mut payload = HashMap::new();
        payload.insert(rel_new.to_string(), body.to_string());
        payload
    }

    fn save_receipt(target: &Path, harness: &str, roots: &[&str], files: &[(&str, &str)]) {
        let entries = files
            .iter()
            .map(|(path, body)| ReceiptFile {
                path: (*path).to_string(),
                sha256: digest::hash_bytes(body.as_bytes()),
            })
            .collect::<Vec<_>>();
        let receipt = InstallReceipt::new(
            "0.1.3",
            harness,
            manifest_db::LAYOUT_SKILLS,
            roots.iter().map(|root| (*root).to_string()).collect(),
            entries,
        )
        .unwrap();
        ReceiptRepository::new(target).save(&receipt).unwrap();
    }

    fn install_plan(files: &[(&str, &str)]) -> InstallPlan {
        InstallPlan {
            harness: "claude-code".into(),
            version: "0.1.3".into(),
            layout: manifest_db::LAYOUT_SKILLS.into(),
            roots: vec![".claude".into()],
            files: files
                .iter()
                .map(|(path, body)| (PathBuf::from(path), (*body).to_string()))
                .collect::<BTreeMap<_, _>>(),
        }
    }

    #[test]
    fn table_omits_flagships_and_crew() {
        for forbidden in [
            "ship-issue",
            "ship-epic",
            "plan-epics",
            "pr-review",
            "report-bug",
            "consolidate-issues",
            "architect",
        ] {
            assert!(
                all_rows().all(|(old, new, _)| old != forbidden && new != forbidden),
                "{forbidden} must not be in the rename table"
            );
        }
    }

    #[test]
    fn with_tools_matches_old_and_new_names() {
        assert!(matches_requested_tool("scrub", "shipmates-scrub"));
        assert!(matches_requested_tool("shipmates-scrub", "scrub"));
        assert!(matches_requested_tool("scrub", "scrub"));
        assert!(matches_requested_tool("shipmates-scrub", "shipmates-scrub"));
        assert!(!matches_requested_tool("scrub", "badge"));
        assert_eq!(canonical_tool_name("scrub"), "shipmates-scrub");
        assert_eq!(canonical_tool_name("shipmates-scrub"), "shipmates-scrub");
    }

    #[test]
    fn plan_expands_skill_folder_and_leftover_command() {
        let dir = tempdir().unwrap();
        let target = dir.path();
        atomic_write(&target.join(".claude/skills/polish/SKILL.md"), "old skill").unwrap();
        atomic_write(
            &target.join(".claude/commands/polish.md"),
            "---\nname: polish\n---\nold\n",
        )
        .unwrap();
        let payload = payload_skill(".claude/skills/shipmates-polish/SKILL.md", "new polish");
        let items = plan(target, &payload, "").unwrap();
        let olds: Vec<_> = items
            .iter()
            .map(|item| item.old_path.to_string_lossy().into_owned())
            .collect();
        assert!(olds.contains(&".claude/skills/polish/SKILL.md".to_string()));
        assert!(olds.contains(&".claude/commands/polish.md".to_string()));
        assert!(items.iter().all(|item| item.new_name == "shipmates-polish"));
    }

    #[test]
    fn plan_accepts_container_prefixed_keys() {
        let dir = tempdir().unwrap();
        let target = dir.path();
        atomic_write(&target.join(".claude/skills/polish/SKILL.md"), "old").unwrap();
        let mut payload = HashMap::new();
        payload.insert(
            "harnesses/claude-code/.claude/skills/shipmates-polish/SKILL.md".into(),
            "new".into(),
        );
        let items = plan(target, &payload, "harnesses/claude-code").unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(
            items[0].old_path,
            PathBuf::from(".claude/skills/polish/SKILL.md")
        );
        assert_eq!(
            items[0].new_path,
            PathBuf::from(".claude/skills/shipmates-polish/SKILL.md")
        );
    }

    #[test]
    fn plan_expands_opencode_command_stem() {
        let dir = tempdir().unwrap();
        let target = dir.path();
        atomic_write(&target.join(".opencode/commands/polish.md"), "old").unwrap();
        let payload = payload_skill(".opencode/commands/shipmates-polish.md", "new");
        let items = plan(target, &payload, "").unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(
            items[0].old_path,
            PathBuf::from(".opencode/commands/polish.md")
        );
        assert_eq!(
            items[0].new_path,
            PathBuf::from(".opencode/commands/shipmates-polish.md")
        );
    }

    #[test]
    fn owned_rename_writes_payload_deletes_old_and_rewrites_receipt() {
        let dir = tempdir().unwrap();
        let target = dir.path();
        let old = ".claude/skills/polish/SKILL.md";
        let new = ".claude/skills/shipmates-polish/SKILL.md";
        atomic_write(&target.join(old), "old polish").unwrap();
        save_receipt(target, "claude-code", &[".claude"], &[(old, "old polish")]);

        let payload = payload_skill(new, "new polish");
        let items = plan(target, &payload, "").unwrap();
        let report = apply(
            target,
            &items,
            &payload,
            "",
            &migrate::new_backup_root(target),
        )
        .unwrap();

        assert_eq!(report.renamed.len(), 1);
        assert!(!target.join(old).exists());
        assert!(!target.join(".claude/skills/polish").exists());
        assert_eq!(fs::read_to_string(target.join(new)).unwrap(), "new polish");
        let receipt = ReceiptRepository::new(target)
            .load("claude-code")
            .unwrap()
            .unwrap();
        assert!(receipt.file(old).is_none());
        assert_eq!(
            receipt.file(new).map(|file| file.sha256.as_str()),
            Some(digest::hash_bytes(b"new polish").as_str())
        );
    }

    #[test]
    fn unowned_old_is_left_in_place() {
        let dir = tempdir().unwrap();
        let target = dir.path();
        let old = ".claude/skills/polish/SKILL.md";
        atomic_write(&target.join(old), "user polish").unwrap();
        let payload = payload_skill(".claude/skills/shipmates-polish/SKILL.md", "new polish");
        let items = plan(target, &payload, "").unwrap();
        let report = apply(
            target,
            &items,
            &payload,
            "",
            &migrate::new_backup_root(target),
        )
        .unwrap();

        assert!(report.renamed.is_empty());
        assert_eq!(report.skipped_unmanaged.len(), 1);
        assert_eq!(fs::read_to_string(target.join(old)).unwrap(), "user polish");
        assert!(
            !target
                .join(".claude/skills/shipmates-polish/SKILL.md")
                .exists()
        );
    }

    fn skill_declaring(name: &str) -> String {
        format!("---\nname: {name}\ndescription: d\n---\nbody\n")
    }

    #[test]
    fn unowned_but_self_declaring_old_skill_is_reclaimed() {
        let dir = tempdir().unwrap();
        let target = dir.path();
        let old = ".agents/skills/polish/SKILL.md";
        let new = ".agents/skills/shipmates-polish/SKILL.md";
        // A pre-receipt v0.1.4 tree: nothing claims the path, but the file
        // still declares the identity the table moves (#403).
        atomic_write(&target.join(old), &skill_declaring("polish")).unwrap();

        let payload = payload_skill(new, "new polish");
        let items = plan(target, &payload, "").unwrap();
        let report = apply(
            target,
            &items,
            &payload,
            "",
            &migrate::new_backup_root(target),
        )
        .unwrap();

        assert_eq!(report.renamed.len(), 1);
        assert!(
            report.renamed[0].reclaimed,
            "a move with no receipt claim must be reported as reclaimed"
        );
        assert!(report.skipped_unmanaged.is_empty());
        assert!(!target.join(old).exists());
        assert!(!target.join(".agents/skills/polish").exists());
        assert_eq!(fs::read_to_string(target.join(new)).unwrap(), "new polish");
        assert_eq!(
            fs::read_to_string(&report.backups[0]).unwrap(),
            skill_declaring("polish"),
            "the reclaimed bytes must be recoverable"
        );
    }

    #[test]
    fn third_party_skill_at_a_table_name_is_still_left() {
        let dir = tempdir().unwrap();
        let target = dir.path();
        let old = ".agents/skills/polish/SKILL.md";
        // Somebody else's skill that merely squats a rename-table name.
        atomic_write(&target.join(old), &skill_declaring("my-own-polish")).unwrap();

        let payload = payload_skill(".agents/skills/shipmates-polish/SKILL.md", "new polish");
        let items = plan(target, &payload, "").unwrap();
        let report = apply(
            target,
            &items,
            &payload,
            "",
            &migrate::new_backup_root(target),
        )
        .unwrap();

        assert!(report.renamed.is_empty());
        assert_eq!(report.skipped_unmanaged, vec![PathBuf::from(old)]);
        assert_eq!(
            fs::read_to_string(target.join(old)).unwrap(),
            skill_declaring("my-own-polish")
        );
        assert!(report.backups.is_empty());
    }

    #[test]
    fn foreign_file_at_the_new_path_leaves_old_and_backs_up_nothing() {
        let dir = tempdir().unwrap();
        let target = dir.path();
        let old = ".claude/skills/polish/SKILL.md";
        let new = ".claude/skills/shipmates-polish/SKILL.md";
        atomic_write(&target.join(old), "old polish").unwrap();
        save_receipt(target, "claude-code", &[".claude"], &[(old, "old polish")]);
        // Unclaimed foreign bytes already occupy the new name.
        atomic_write(&target.join(new), &skill_declaring("somebody-else")).unwrap();

        let payload = payload_skill(new, "new polish");
        let items = plan(target, &payload, "").unwrap();
        let backup_root = migrate::new_backup_root(target);
        let report = apply(target, &items, &payload, "", &backup_root).unwrap();

        assert!(report.renamed.is_empty());
        assert_eq!(report.skipped_unmanaged, vec![PathBuf::from(old)]);
        assert_eq!(fs::read_to_string(target.join(old)).unwrap(), "old polish");
        assert_eq!(
            fs::read_to_string(target.join(new)).unwrap(),
            skill_declaring("somebody-else"),
            "a refusal must not overwrite the foreign file"
        );
        assert!(
            !backup_root.exists(),
            "a skipped item must not leave an orphaned backup behind"
        );
    }

    #[test]
    fn reclaim_rolls_back_on_later_write_failure() {
        let dir = tempdir().unwrap();
        let target = dir.path();
        let polish_old = ".claude/skills/polish/SKILL.md";
        let polish_new = ".claude/skills/shipmates-polish/SKILL.md";
        let spike_old = ".claude/skills/spike/SKILL.md";
        atomic_write(&target.join(polish_old), &skill_declaring("polish")).unwrap();
        atomic_write(&target.join(spike_old), &skill_declaring("spike")).unwrap();
        // Block the second item's destination: a regular file where the folder
        // must go, so the sweep fails after the first reclaim succeeded.
        atomic_write(&target.join(".claude/skills/shipmates-spike"), "blocker").unwrap();

        let mut payload = payload_skill(polish_new, "new polish");
        payload.insert(
            ".claude/skills/shipmates-spike/SKILL.md".into(),
            "new spike".into(),
        );
        let items = plan(target, &payload, "").unwrap();
        assert_eq!(items.len(), 2, "both reclaimable skills must be planned");

        let error = apply(
            target,
            &items,
            &payload,
            "",
            &migrate::new_backup_root(target),
        )
        .unwrap_err();

        assert!(!error.to_string().is_empty());
        assert_eq!(
            fs::read_to_string(target.join(polish_old)).unwrap(),
            skill_declaring("polish"),
            "a reclaimed file must be restored when a later item fails"
        );
        assert_eq!(
            fs::read_to_string(target.join(spike_old)).unwrap(),
            skill_declaring("spike")
        );
        assert!(
            !target.join(polish_new).exists(),
            "the half-written new path must be gone after rollback"
        );
    }

    #[test]
    fn two_receipts_sharing_agents_skill_update_together() {
        let dir = tempdir().unwrap();
        let target = dir.path();
        let old = ".agents/skills/polish/SKILL.md";
        let new = ".agents/skills/shipmates-polish/SKILL.md";
        atomic_write(&target.join(old), "shared polish").unwrap();
        save_receipt(target, "cursor", &[".agents"], &[(old, "shared polish")]);
        save_receipt(
            target,
            "github-copilot",
            &[".agents"],
            &[(old, "shared polish")],
        );

        let payload = payload_skill(new, "new shared");
        let items = plan(target, &payload, "").unwrap();
        apply(
            target,
            &items,
            &payload,
            "",
            &migrate::new_backup_root(target),
        )
        .unwrap();

        assert!(!target.join(old).exists());
        assert_eq!(fs::read_to_string(target.join(new)).unwrap(), "new shared");
        for harness in ["cursor", "github-copilot"] {
            let receipt = ReceiptRepository::new(target)
                .load(harness)
                .unwrap()
                .unwrap();
            assert!(
                receipt.file(old).is_none(),
                "{harness} still lists old path"
            );
            assert!(receipt.file(new).is_some(), "{harness} missing new path");
        }
        let claims = ReceiptRepository::new(target)
            .claims_for_path(Path::new(new))
            .unwrap();
        assert_eq!(claims, vec!["cursor", "github-copilot"]);
    }

    #[test]
    fn second_run_is_idempotent() {
        let dir = tempdir().unwrap();
        let target = dir.path();
        let old = ".claude/skills/polish/SKILL.md";
        let new = ".claude/skills/shipmates-polish/SKILL.md";
        atomic_write(&target.join(old), "old").unwrap();
        save_receipt(target, "claude-code", &[".claude"], &[(old, "old")]);
        let payload = payload_skill(new, "new");
        let items = plan(target, &payload, "").unwrap();
        apply(
            target,
            &items,
            &payload,
            "",
            &migrate::new_backup_root(target),
        )
        .unwrap();

        let items2 = plan(target, &payload, "").unwrap();
        assert!(items2.is_empty());
        let backup_root2 = migrate::new_backup_root(target);
        let report2 = apply(target, &items2, &payload, "", &backup_root2).unwrap();
        assert!(report2.renamed.is_empty());
        assert_eq!(fs::read_to_string(target.join(new)).unwrap(), "new");
    }

    #[test]
    fn no_migrate_preserve_keeps_old_when_apply_writes_new() {
        let dir = tempdir().unwrap();
        let target = dir.path();
        let old = ".claude/skills/polish/SKILL.md";
        let new = ".claude/skills/shipmates-polish/SKILL.md";
        let first = install_plan(&[(old, "old polish")]);
        apply::apply(target, &first, false).unwrap();

        let payload = payload_skill(new, "new polish");
        let items = plan(target, &payload, "").unwrap();
        assert!(!items.is_empty());
        let preserved = preserved_old_paths(&items);
        let second = install_plan(&[(new, "new polish")]);
        apply_with_preserved_paths(target, &second, false, &preserved).unwrap();

        assert_eq!(
            fs::read_to_string(target.join(old)).unwrap(),
            "old polish",
            "preserved old path must survive apply drop"
        );
        assert_eq!(fs::read_to_string(target.join(new)).unwrap(), "new polish");
        let receipt = ReceiptRepository::new(target)
            .load("claude-code")
            .unwrap()
            .unwrap();
        assert!(receipt.file(old).is_some());
        assert!(receipt.file(new).is_some());
    }

    #[test]
    fn write_fail_does_not_delete_old() {
        let dir = tempdir().unwrap();
        let target = dir.path();
        let old = ".claude/skills/polish/SKILL.md";
        atomic_write(&target.join(old), "old polish").unwrap();
        save_receipt(target, "claude-code", &[".claude"], &[(old, "old polish")]);
        // Block the new skill directory: a regular file where the folder must go.
        atomic_write(&target.join(".claude/skills/shipmates-polish"), "blocker").unwrap();

        let payload = payload_skill(".claude/skills/shipmates-polish/SKILL.md", "new polish");
        let items = plan(target, &payload, "").unwrap();
        let error = apply(
            target,
            &items,
            &payload,
            "",
            &migrate::new_backup_root(target),
        )
        .unwrap_err();
        assert!(!error.to_string().is_empty());
        assert_eq!(
            fs::read_to_string(target.join(old)).unwrap(),
            "old polish",
            "old file must remain when the new write fails"
        );
        let receipt = ReceiptRepository::new(target)
            .load("claude-code")
            .unwrap()
            .unwrap();
        assert!(receipt.file(old).is_some());
    }

    #[test]
    fn rollback_restores_old_and_receipt() {
        let dir = tempdir().unwrap();
        let target = dir.path();
        let old = ".claude/skills/polish/SKILL.md";
        let new = ".claude/skills/shipmates-polish/SKILL.md";
        atomic_write(&target.join(old), "old polish").unwrap();
        save_receipt(target, "claude-code", &[".claude"], &[(old, "old polish")]);
        let payload = payload_skill(new, "new polish");
        let items = plan(target, &payload, "").unwrap();
        let report = apply(
            target,
            &items,
            &payload,
            "",
            &migrate::new_backup_root(target),
        )
        .unwrap();

        rollback(target, &report).unwrap();

        assert_eq!(fs::read_to_string(target.join(old)).unwrap(), "old polish");
        assert!(!target.join(new).exists());
        let receipt = ReceiptRepository::new(target)
            .load("claude-code")
            .unwrap()
            .unwrap();
        assert!(receipt.file(old).is_some());
        assert!(receipt.file(new).is_none());
        assert!(report.backups.iter().all(|backup| !backup.exists()));
    }
}
