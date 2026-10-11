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

use crate::catalog::{CanonicalCommand, CanonicalTool, parse_frontmatter_from};
use crate::digest;
use crate::installer::manifest_db::{ReceiptRepository, is_global_target, resolve_target_relative};
use anyhow::{Context, Result};
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
    /// The retired entry: a target id for a retired-target adoption, or the
    /// tree's relative path for a superseded home-scope tree.
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

/// Every rendered payload description opens with this. It is the second half of
/// the ownership proof: a captain's own skill can reuse a payload name, but it
/// will not carry our brand prefix — and the prefix survives renames and
/// re-renders, so a superseded copy from an older release still proves ours.
const SHIPMATES_DESCRIPTION_PREFIX: &str = "Shipmates:";

/// Whether the bytes at a payload path are provably a rendered Shipmates
/// payload: the frontmatter declares the name its directory implies **and**
/// carries the brand-prefixed description every rendered payload is stamped
/// with.
///
/// The name alone is not proof. A captain's own skill that reuses a payload
/// name declares `name: shipmates-…` too, so deleting it on the name would be
/// exactly the mistake this module exists to avoid — and it would delete the
/// target of a symlink farm's link.
fn declares_our_payload(content: &str, name: &str) -> bool {
    match parse_frontmatter_from(content, name) {
        Ok((frontmatter, _)) => {
            frontmatter
                .get("name")
                .map(|value| value == name)
                .unwrap_or(false)
                && frontmatter.get("description").is_some_and(|value| {
                    unquoted(value).starts_with(SHIPMATES_DESCRIPTION_PREFIX)
                })
        }
        Err(_) => false,
    }
}

/// A frontmatter value with any surrounding quotes removed.
///
/// Rendered payloads quote the description (`description: "Shipmates: …"`) and
/// the frontmatter reader hands it back verbatim, quotes included; a
/// pre-quoting release left it bare. Both have to classify as ours.
fn unquoted(value: &str) -> &str {
    let value = value.trim();
    let value = value
        .strip_prefix('"')
        .or_else(|| value.strip_prefix('\''))
        .unwrap_or(value);
    value
        .strip_suffix('"')
        .or_else(|| value.strip_suffix('\''))
        .unwrap_or(value)
}

/// Whether every component of `path` down from `base` — `base` itself
/// included — is free of symlinks.
///
/// The containment rule the installer already applies to the paths it writes
/// (#462): a symlinked skills tree is a normal thing to want (a dotfiles repo,
/// a skills manager), so it is skipped rather than followed. Missing trailing
/// components are allowed; a symlink anywhere in the chain is not.
fn path_is_symlink_free(base: &Path, path: &Path) -> bool {
    let Ok(relative) = path.strip_prefix(base) else {
        return false;
    };
    if fs::symlink_metadata(base)
        .map(|metadata| metadata.file_type().is_symlink())
        .unwrap_or(false)
    {
        return false;
    }
    let mut current = base.to_path_buf();
    for component in relative.components() {
        current.push(component.as_os_str());
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => return false,
            Ok(_) => {}
            Err(_) => return true,
        }
    }
    true
}

/// The files a rendered payload installs under its own skill directory: the
/// manifest `SKILL.md` first, then the installed filenames of a canonical
/// tool's bundled assets (a command bundles none). `None` when `name` is not a
/// payload name at all.
///
/// This is the set a superseded tree can orphan one file at a time: a retire
/// that keys on `SKILL.md` alone leaves a tool's script behind, and a leftover
/// script proves the payload name as surely as the manifest does.
fn payload_file_names(
    name: &str,
    cmds: &[CanonicalCommand],
    tools: &[CanonicalTool],
) -> Option<Vec<String>> {
    let tool = tools.iter().find(|tool| tool.name == name);
    if tool.is_none() && !cmds.iter().any(|command| command.name == name) {
        return None;
    }
    let mut files = vec!["SKILL.md".to_string()];
    if let Some(tool) = tool {
        files.extend(tool.assets.iter().map(|(rel, _)| rel.clone()));
    }
    Some(files)
}

/// Payload-named entries of a superseded `.agents/skills/` tree, split by
/// whether Shipmates can prove ownership.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SupersededSkills {
    /// Entries whose own frontmatter declares a rendered Shipmates payload at
    /// this name — provably ours, safe to retire.
    pub ours: Vec<String>,
    /// Entries at a payload name that declare a different identity or origin
    /// (or none), so they are not ours to delete.
    pub foreign: Vec<String>,
    /// Entries reached through a symlink: left exactly as found, because a
    /// symlinked skills tree is the captain's own arrangement.
    pub linked: Vec<String>,
}

/// Classify the payload-named skill directories of
/// `<target_dir>/.agents/skills/`.
///
/// `target_dir` — not the tree root — is the base for the containment walk. A
/// symlinked `.agents` (a dotfiles repo's usual shape) has to be caught, and a
/// base of `<target_dir>/.agents/skills` would never inspect it: the walk would
/// start below the link, classify a real file behind it as ours, and then the
/// retire step — which *does* walk from the target — would refuse it, leaving a
/// problem `--fix` could never clear.
///
/// Ownership is proved from the file's own declaration, exactly as
/// `adopt::classify` does: a skill is ours only when its frontmatter `name:`
/// equals the name its directory implies **and** its description carries the
/// brand prefix every rendered payload is stamped with. A captain's own skill
/// that merely reuses a payload name, a `SKILL.md` that is missing or
/// unreadable, and anything reached through a symlink are each at worst
/// reported as foreign and never deleted — this is a path under `$HOME`. A name
/// outside the payload is ignored entirely, so a foreign harness's own skills in
/// that tree are invisible to us. An entry is a candidate while any file of its
/// name's payload set is present, so an orphan script left by a partial retire
/// is still reported rather than read as an empty tree.
pub fn classify_superseded_skills(
    target_dir: &Path,
    cmds: &[CanonicalCommand],
    tools: &[CanonicalTool],
) -> SupersededSkills {
    let root = target_dir.join(SUPERSEDED_HOME_SKILLS);
    let mut out = SupersededSkills::default();
    let Ok(entries) = fs::read_dir(&root) else {
        return out;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(files) = payload_file_names(&name, cmds, tools) else {
            continue;
        };
        let dir = entry.path();
        // A candidate holds a file of the name's payload set — the manifest
        // *or* a bundled script a partial retire left behind. A directory
        // holding only the captain's own files is not ours to report, let alone
        // remove, and an empty one is not a tree at all.
        let present: Vec<PathBuf> = files
            .iter()
            .map(|file| dir.join(file))
            .filter(|path| fs::symlink_metadata(path).is_ok())
            .collect();
        if present.is_empty() {
            continue;
        }
        if present
            .iter()
            .any(|path| !path_is_symlink_free(target_dir, path))
        {
            out.linked.push(name);
            continue;
        }
        match fs::read_to_string(dir.join("SKILL.md")) {
            Ok(content) if declares_our_payload(&content, &name) => out.ours.push(name),
            // No readable `SKILL.md`, or one whose declaration is not ours:
            // nothing proves ownership, so it is reported and never deleted.
            _ => out.foreign.push(name),
        }
    }
    out.ours.sort();
    out.foreign.sort();
    out.linked.sort();
    out
}

/// Retire the provably-ours files of a superseded home-scope `.agents/skills/`
/// tree (`<home>/.agents/skills/`), with the same backup semantics as
/// [`adopt`] — every file is copied into a per-run backup tree before it is
/// removed, so the move is reversible.
///
/// "Every file" is the whole payload set for the name, not only `SKILL.md`: a
/// rendered tool skill is its manifest **plus** the bundled script, so retiring
/// the manifest alone would leave the script orphaned and the tree unclaimable.
/// Each file is symlink-checked immediately before it is acted on, and a file
/// the payload does not name — the captain's own — is left exactly where it is.
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
    let tree = classify_superseded_skills(target_dir, cmds, tools);
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
        let Some(files) = payload_file_names(name, cmds, tools) else {
            continue;
        };
        for file in files {
            let path = root.join(name).join(&file);
            // Belt and braces: the classifier already put symlinked entries in
            // `linked`, and a delete under `$HOME` is worth re-checking.
            if !path_is_symlink_free(target_dir, &path) {
                continue;
            }
            let Ok(bytes) = fs::read(&path) else {
                // Not part of this entry, or already gone: nothing to retire.
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
        tool_with_assets(name, &[])
    }

    fn tool_with_assets(name: &str, assets: &[&str]) -> CanonicalTool {
        CanonicalTool {
            name: name.into(),
            description: "d".into(),
            body: "b".into(),
            assets: assets
                .iter()
                .map(|asset| ((*asset).to_string(), String::new()))
                .collect(),
            requires: vec![],
            source: PathBuf::from(""),
        }
    }

    /// #561: a tool's rendered skill is a `SKILL.md` *and* its bundled script,
    /// so a name is a candidate while the manifest, the script, or both are
    /// present — an orphan script a partial retire left behind is still
    /// reported, not read as an empty tree. A directory holding only the
    /// captain's own files is not in the payload set and is invisible.
    #[test]
    fn classifies_a_tool_by_any_file_of_its_payload_set() {
        let dir = tempdir().unwrap();
        let root = dir.path().join(SUPERSEDED_HOME_SKILLS);
        write(
            &root.join("shipmates-termgif/SKILL.md"),
            "---\nname: shipmates-termgif\ndescription: \"Shipmates: superseded\"\n---\nbody\n",
        );
        write(&root.join("shipmates-termgif/termgif.py"), "print('hi')\n");
        // An orphan script whose `SKILL.md` a partial retire already removed.
        write(&root.join("shipmates-badge/badge.py"), "print('orphan')\n");
        // A payload name holding only the captain's own file: not a file of the
        // payload set, so it is never a candidate — not reported, not removed.
        write(&root.join("shipmates-harden/notes.txt"), "mine\n");

        let tree = classify_superseded_skills(
            dir.path(),
            &[cmd("shipmates-harden")],
            &[
                tool_with_assets("shipmates-termgif", &["termgif.py"]),
                tool_with_assets("shipmates-badge", &["badge.py"]),
            ],
        );
        assert_eq!(tree.ours, vec!["shipmates-termgif"]);
        assert_eq!(tree.foreign, vec!["shipmates-badge"]);
        assert!(tree.linked.is_empty(), "linked: {:?}", tree.linked);
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
            "---\nname: shipmates-ship-issue\ndescription: \"Shipmates: superseded fixture\"\n---\nbody\n",
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
            dir.path(),
            &[cmd("shipmates-ship-issue")],
            &[tool("shipmates-termgif")],
        );
        assert_eq!(tree.ours, vec!["shipmates-ship-issue"]);
        assert_eq!(tree.foreign, vec!["shipmates-termgif"]);
    }

    /// A captain's own skill that reuses a payload name is not ours. The
    /// frontmatter name is not proof on its own — their file declares it too —
    /// so it must fail closed to foreign and never be deleted.
    #[test]
    fn a_payload_named_skill_with_a_foreign_description_is_never_retired() {
        let dir = tempdir().unwrap();
        let root = dir.path().join(SUPERSEDED_HOME_SKILLS);
        write(
            &root.join("shipmates-ship-issue/SKILL.md"),
            "---\nname: shipmates-ship-issue\ndescription: mine\n---\nbody\n",
        );
        let tree = classify_superseded_skills(dir.path(), &[cmd("shipmates-ship-issue")], &[]);
        assert!(tree.ours.is_empty(), "ours: {:?}", tree.ours);
        assert_eq!(tree.foreign, vec!["shipmates-ship-issue"]);
        assert!(
            retire_superseded_home_tree(dir.path(), &[cmd("shipmates-ship-issue")], &[])
                .unwrap()
                .is_none()
        );
        assert!(
            root.join("shipmates-ship-issue/SKILL.md").is_file(),
            "a captain's own file must survive"
        );
    }

    /// A symlinked skills tree is the captain's own arrangement (#462): it is
    /// skipped and reported, never followed — so nothing behind the link is
    /// deleted, however Shipmates-looking the target is.
    #[cfg(unix)]
    #[test]
    fn a_symlinked_home_skills_tree_is_left_alone() {
        let dir = tempdir().unwrap();
        let outside = tempdir().unwrap();
        let target = outside.path().join("skills");
        write(
            &target.join("shipmates-ship-issue/SKILL.md"),
            "---\nname: shipmates-ship-issue\ndescription: \"Shipmates: superseded\"\n---\nbody\n",
        );
        let linked = dir.path().join(SUPERSEDED_HOME_SKILLS);
        fs::create_dir_all(linked.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&target, &linked).unwrap();

        let tree = classify_superseded_skills(dir.path(), &[cmd("shipmates-ship-issue")], &[]);
        assert!(tree.ours.is_empty(), "ours must stay empty: {:?}", tree.ours);
        assert_eq!(tree.linked, vec!["shipmates-ship-issue"]);
        assert!(
            retire_superseded_home_tree(dir.path(), &[cmd("shipmates-ship-issue")], &[])
                .unwrap()
                .is_none()
        );
        assert!(
            target.join("shipmates-ship-issue/SKILL.md").is_file(),
            "the symlink target must survive"
        );
    }

    /// A symlinked `.agents` parent — the shape a dotfiles repo usually has —
    /// must be caught as well. If the containment base were the tree root rather
    /// than the target, a real file behind a linked parent would classify as
    /// ours, `--fix` would then refuse it, and the Problem would persist with a
    /// detail line promising a repair that never comes.
    #[cfg(unix)]
    #[test]
    fn a_symlinked_agents_parent_is_left_alone() {
        let dir = tempdir().unwrap();
        let outside = tempdir().unwrap();
        let target = outside.path().join("agents");
        write(
            &target.join("skills/shipmates-ship-issue/SKILL.md"),
            "---\nname: shipmates-ship-issue\ndescription: \"Shipmates: superseded\"\n---\nbody\n",
        );
        std::os::unix::fs::symlink(&target, dir.path().join(".agents")).unwrap();

        let tree = classify_superseded_skills(dir.path(), &[cmd("shipmates-ship-issue")], &[]);
        assert!(tree.ours.is_empty(), "ours must stay empty: {:?}", tree.ours);
        assert_eq!(tree.linked, vec!["shipmates-ship-issue"]);
        assert!(
            retire_superseded_home_tree(dir.path(), &[cmd("shipmates-ship-issue")], &[])
                .unwrap()
                .is_none()
        );
        assert!(
            target.join("skills/shipmates-ship-issue/SKILL.md").is_file(),
            "nothing behind a linked parent may be deleted"
        );
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
