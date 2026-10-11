//! Adopt the files a retired target name left behind.
//!
//! A product gets renamed, its tree moves, and the old target id stops being a
//! target. That is not a reason to strand a captain: the files the retired
//! install wrote are still on disk, and the harness may still read them
//! (Devin CLI keeps reading `.windsurf/skills/` after the Windsurf → Devin
//! Desktop rename), so leaving them there means two copies of every command in
//! the picker — the exact drift `doctor` exists to catch.
//!
//! Ownership is decided from the retired install's **own receipt**, not from a
//! filename heuristic: a file is adopted only when its bytes still hash to what
//! the receipt recorded. A file the captain edited is left exactly where it is
//! and reported, because it is no longer ours to delete. Every adopted file is
//! copied into a per-run backup tree before it is removed, so the move is
//! reversible and idempotent — a second run finds nothing left to do.

use crate::catalog::{CanonicalCommand, CanonicalTool};
use crate::digest;
use crate::installer::manifest_db::{ReceiptRepository, is_global_target, resolve_target_relative};
use anyhow::{Context, Result};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

/// Retired target names that resolve to `harness`, newest first.
///
/// A retired name is listed here only while the product still reads its tree —
/// that is why the files must be actively reclaimed rather than ignored.
pub fn retired_names_for(harness: &str) -> &'static [&'static str] {
    match harness {
        "devin" => &["windsurf"],
        _ => &[],
    }
}

#[derive(Debug, Clone, Default)]
pub struct RetiredReport {
    /// The retired target name whose install was adopted.
    pub name: String,
    /// Files removed after their backup was written, relative to the target.
    pub adopted: Vec<PathBuf>,
    /// Backup copy written for each adopted file (same order as `adopted`).
    pub backups: Vec<PathBuf>,
    /// The per-run backup tree the copies landed under.
    pub backup_root: Option<PathBuf>,
    /// Receipt-owned files left in place because their bytes no longer match
    /// what the retired install wrote — the captain edited them.
    pub kept_modified: Vec<PathBuf>,
    /// Whether the retired receipt itself was removed.
    pub receipt_removed: bool,
}

impl RetiredReport {
    pub fn changed(&self) -> bool {
        !self.adopted.is_empty() || self.receipt_removed
    }
}

/// Adopt any retired-name install sitting in `target_dir` for `harness`.
///
/// Returns `None` when there is nothing to adopt, so callers can print nothing.
pub fn adopt(target_dir: &Path, harness: &str) -> Result<Option<RetiredReport>> {
    let repository = ReceiptRepository::new(target_dir);
    for name in retired_names_for(harness) {
        let Some(receipt) = repository.load(name)? else {
            continue;
        };
        let mut report = RetiredReport {
            name: (*name).to_string(),
            ..RetiredReport::default()
        };
        let backup_root = crate::installer::migrate::new_backup_root(target_dir);
        let mut backups_written = false;
        for file in &receipt.files {
            let relative = PathBuf::from(&file.path);
            let path = resolve_target_relative(target_dir, &relative)?;
            let Ok(bytes) = fs::read(&path) else {
                // Already gone: nothing to adopt, nothing to report.
                continue;
            };
            if digest::hash_bytes(&bytes) != file.sha256 {
                report.kept_modified.push(relative);
                continue;
            }
            let backup = backup_root.join(&relative);
            if let Some(parent) = backup.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("creating backup dir {}", parent.display()))?;
            }
            crate::installer::atomic_write_bytes(&backup, &bytes)
                .with_context(|| format!("backing up {}", path.display()))?;
            // The backup is on disk before the original goes: the same ordering
            // rule the command migration uses, so a file is never lost.
            fs::remove_file(&path).with_context(|| format!("removing {}", path.display()))?;
            backups_written = true;
            report.adopted.push(relative);
            report.backups.push(backup);
        }
        prune_empty_dirs(target_dir, &report.adopted);
        if backups_written {
            // Keep the run's backup tree even when only some files moved.
        } else if backup_root.exists() {
            let _ = fs::remove_dir_all(&backup_root);
        }
        report.receipt_removed = repository.remove(name)?;
        report.adopted.sort();
        report.kept_modified.sort();
        return Ok(Some(report));
    }
    Ok(None)
}

/// The home-scope skill tree the pre-`global_relocate` Shipmates wrote and no
/// current payload writes: `<home>/.agents/skills/`.
///
/// Every harness's current global tree is its own private one (`.pi/agent/`,
/// `.codex/`, `.gemini/config/`, `.copilot/`, …) or the workspace dotdir joined
/// to home, so this tree is superseded at every target. At project scope it is
/// NOT superseded — the shared-tree harnesses legitimately write it — which is
/// why the checks below are gated on a home/global target.
pub const SUPERSEDED_HOME_SKILLS: &str = ".agents/skills";

/// Payload-named entries of a superseded `.agents/skills/` tree, split by
/// whether Shipmates can prove ownership.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SupersededSkills {
    /// Entries whose frontmatter `name:` equals the directory name — provably
    /// Shipmates' own, safe to retire.
    pub ours: Vec<String>,
    /// Entries at a payload name that declare a different identity (or none),
    /// so they are not ours to delete.
    pub foreign: Vec<String>,
}

/// Classify `<root>/<name>/SKILL.md` for every payload skill name (`cmds` +
/// `tools`), where `root` is a `.agents/skills/` directory.
///
/// Ownership is decided from the file's own declaration, exactly as
/// `adopt::classify` does: a skill is ours only when its frontmatter `name:`
/// equals the name its directory implies. A directory of the captain's own that
/// happens to share a payload name, or an unreadable/missing `SKILL.md`, is at
/// worst reported as foreign and never deleted — this is a path under `$HOME`.
/// A name outside the payload is ignored entirely, so a foreign harness's own
/// skills in that tree are invisible to us.
pub fn classify_superseded_skills(
    root: &Path,
    cmds: &[CanonicalCommand],
    tools: &[CanonicalTool],
) -> SupersededSkills {
    let payload: BTreeSet<&str> = cmds
        .iter()
        .map(|command| command.name.as_str())
        .chain(tools.iter().map(|tool| tool.name.as_str()))
        .collect();
    let mut out = SupersededSkills::default();
    let Ok(entries) = fs::read_dir(root) else {
        return out;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !payload.contains(name.as_str()) {
            continue;
        }
        let Ok(content) = fs::read_to_string(entry.path().join("SKILL.md")) else {
            continue;
        };
        if crate::installer::adopt::frontmatter_name_matches(&content, &name) {
            out.ours.push(name);
        } else {
            out.foreign.push(name);
        }
    }
    out.ours.sort();
    out.foreign.sort();
    out
}

/// Retire the provably-ours files of a superseded home-scope `.agents/skills/`
/// tree (`<home>/.agents/skills/`), with the same backup semantics as
/// [`adopt`] — every file is copied into a per-run backup tree before it is
/// removed, so the move is reversible.
///
/// Returns `None` when the target is not the user's home, when there is no such
/// tree, or when nothing in it is provably ours; a foreign entry is left exactly
/// where it is. Only the home/global target is in scope: at project scope this
/// path is the shared tree the codex/antigravity/github-copilot payloads write.
pub fn retire_superseded_home_tree(
    target_dir: &Path,
    cmds: &[CanonicalCommand],
    tools: &[CanonicalTool],
) -> Result<Option<RetiredReport>> {
    if !is_global_target(target_dir) {
        return Ok(None);
    }
    let root = target_dir.join(SUPERSEDED_HOME_SKILLS);
    let tree = classify_superseded_skills(&root, cmds, tools);
    if tree.ours.is_empty() {
        return Ok(None);
    }
    let backup_root = crate::installer::migrate::new_backup_root(target_dir);
    let mut report = RetiredReport {
        name: SUPERSEDED_HOME_SKILLS.to_string(),
        ..RetiredReport::default()
    };
    let mut backups_written = false;
    for name in &tree.ours {
        let path = root.join(name).join("SKILL.md");
        let Ok(bytes) = fs::read(&path) else {
            // Already gone: nothing to retire, nothing to report.
            continue;
        };
        let relative = path
            .strip_prefix(target_dir)
            .map_err(|error| anyhow::anyhow!("superseded path escaped target: {error}"))?
            .to_path_buf();
        let backup = backup_root.join(&relative);
        if let Some(parent) = backup.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("creating backup dir {}", parent.display()))?;
        }
        crate::installer::atomic_write_bytes(&backup, &bytes)
            .with_context(|| format!("backing up {}", path.display()))?;
        // The backup is on disk before the original goes, the same ordering
        // rule the retired-name adoption uses, so a file is never lost.
        fs::remove_file(&path).with_context(|| format!("removing {}", path.display()))?;
        backups_written = true;
        report.adopted.push(relative);
        report.backups.push(backup);
    }
    prune_empty_dirs(target_dir, &report.adopted);
    if !backups_written && backup_root.exists() {
        let _ = fs::remove_dir_all(&backup_root);
    }
    report.backup_root = backups_written.then_some(backup_root);
    Ok(Some(report))
}

/// Remove the directories an adopted file left behind, deepest first, stopping
/// at the first non-empty one. Never walks above the target directory.
fn prune_empty_dirs(target_dir: &Path, adopted: &[PathBuf]) {
    let mut parents: Vec<PathBuf> = Vec::new();
    for relative in adopted {
        let mut current = relative.parent();
        while let Some(dir) = current {
            if dir.as_os_str().is_empty() {
                break;
            }
            parents.push(dir.to_path_buf());
            current = dir.parent();
        }
    }
    // Deepest first so a nested empty tree collapses fully.
    parents.sort_by_key(|dir| std::cmp::Reverse(dir.components().count()));
    parents.dedup();
    for relative in parents {
        let absolute = match resolve_target_relative(target_dir, &relative) {
            Ok(path) => path,
            Err(_) => continue,
        };
        if absolute == target_dir {
            continue;
        }
        let _ = fs::remove_dir(&absolute);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::installer::manifest_db::{InstallReceipt, ReceiptFile};
    use tempfile::tempdir;

    fn write(path: &Path, content: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }

    /// A retired install is adopted: files backed up, originals removed, receipt
    /// gone, and the run is idempotent.
    #[test]
    fn adopts_an_untouched_retired_install_and_is_idempotent() {
        let dir = tempdir().unwrap();
        let target = dir.path();
        let skill = Path::new(".windsurf/skills/shipmates-ship-issue/SKILL.md");
        write(&target.join(skill), "---\nname: shipmates-ship-issue\n---\nbody\n");
        let receipt = InstallReceipt::new(
            "0.11.0",
            "windsurf",
            "skills",
            vec![".shipmates".to_string(), ".windsurf".to_string()],
            vec![ReceiptFile {
                path: skill.to_string_lossy().into_owned(),
                sha256: digest::compute_sha256(&target.join(skill)).unwrap(),
            }],
        )
        .unwrap();
        ReceiptRepository::new(target).save(&receipt).unwrap();

        let report = adopt(target, "devin").unwrap().unwrap();
        assert_eq!(report.name, "windsurf");
        assert_eq!(report.adopted.len(), 1);
        assert!(report.receipt_removed);
        assert!(!target.join(skill).exists());
        assert!(
            !target.join(".windsurf").exists(),
            "an emptied legacy tree is pruned"
        );
        assert!(report.backups[0].exists(), "the file survives in the backup");
        assert!(!target.join(".shipmates/receipts/windsurf.json").exists());

        // Second run: nothing left to adopt.
        assert!(adopt(target, "devin").unwrap().is_none());
    }

    /// A file the captain edited after installing is not ours to delete.
    #[test]
    fn keeps_a_retired_file_the_captain_edited() {
        let dir = tempdir().unwrap();
        let target = dir.path();
        let skill = Path::new(".windsurf/skills/shipmates-ship-issue/SKILL.md");
        write(&target.join(skill), "---\nname: shipmates-ship-issue\n---\nbody\n");
        let installed_hash = digest::compute_sha256(&target.join(skill)).unwrap();
        fs::write(target.join(skill), "my own notes\n").unwrap();
        let receipt = InstallReceipt::new(
            "0.11.0",
            "windsurf",
            "skills",
            vec![".shipmates".to_string(), ".windsurf".to_string()],
            vec![ReceiptFile {
                path: skill.to_string_lossy().into_owned(),
                sha256: installed_hash,
            }],
        )
        .unwrap();
        ReceiptRepository::new(target).save(&receipt).unwrap();

        let report = adopt(target, "devin").unwrap().unwrap();
        assert!(report.adopted.is_empty());
        assert_eq!(report.kept_modified.len(), 1);
        assert_eq!(fs::read_to_string(target.join(skill)).unwrap(), "my own notes\n");
    }

    /// Targets with no retired ancestor never look for one.
    #[test]
    fn a_target_without_a_retired_ancestor_adopts_nothing() {
        let dir = tempdir().unwrap();
        assert!(retired_names_for("claude-code").is_empty());
        assert!(adopt(dir.path(), "claude-code").unwrap().is_none());
    }

    fn cmd(name: &str) -> CanonicalCommand {
        CanonicalCommand {
            name: name.into(),
            description: "d".into(),
            argument_hint: String::new(),
            allowed_tools: String::new(),
            disable_model_invocation: true,
            arguments: vec![],
            narrative: "n".into(),
            invocation: String::new(),
            board: String::new(),
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

    /// #561: a superseded tree is split into the files whose own frontmatter
    /// proves they are ours and the ones it does not — the deleting decision
    /// reads the file's declaration, never the directory name alone, and a
    /// name outside the payload is invisible.
    #[test]
    fn classifies_the_ours_half_of_a_superseded_tree_and_leaves_the_rest() {
        let dir = tempdir().unwrap();
        let root = dir.path().join(SUPERSEDED_HOME_SKILLS);
        write(
            &root.join("shipmates-ship-issue/SKILL.md"),
            "---\nname: shipmates-ship-issue\ndescription: d\n---\nbody\n",
        );
        // A payload name whose frontmatter claims a different identity.
        write(
            &root.join("shipmates-termgif/SKILL.md"),
            "---\nname: my-own-thing\n---\nkeep me\n",
        );
        // Not a payload name at all: ignored, even though its frontmatter
        // declares a Shipmates name.
        write(
            &root.join("someone-elses/SKILL.md"),
            "---\nname: shipmates-ship-issue\n---\n",
        );

        let tree = classify_superseded_skills(
            &root,
            &[cmd("shipmates-ship-issue")],
            &[tool("shipmates-termgif")],
        );
        assert_eq!(tree.ours, vec!["shipmates-ship-issue"]);
        assert_eq!(tree.foreign, vec!["shipmates-termgif"]);
    }

    /// The retirement is home-scope only: a project-scope `.agents/skills/` tree
    /// is the shared tree three harnesses legitimately write, and is never in
    /// scope — even when it carries provably-Shipmates files.
    #[test]
    fn never_retires_a_project_scope_agents_tree() {
        let dir = tempdir().unwrap();
        let root = dir.path().join(SUPERSEDED_HOME_SKILLS);
        write(
            &root.join("shipmates-ship-issue/SKILL.md"),
            "---\nname: shipmates-ship-issue\n---\nbody\n",
        );
        assert!(
            retire_superseded_home_tree(dir.path(), &[cmd("shipmates-ship-issue")], &[])
                .unwrap()
                .is_none()
        );
        assert!(root.join("shipmates-ship-issue/SKILL.md").is_file());
    }
}
