//! End-to-end coverage for install receipts and doctor safety checks.
//!
//! These tests use the compiled CLI so receipt persistence, upgrade behavior,
//! uninstall selection, and doctor exit status stay covered at the public
//! boundary rather than through implementation-detail assertions.

use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use tempfile::tempdir;

const HARNESS: &str = "claude-code";

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_shipmates")
}

fn run(target: &Path, args: &[&str]) -> Output {
    Command::new(bin())
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .args(args)
        .arg("--dir")
        .arg(target)
        .output()
        .expect("failed to execute shipmates")
}

fn install(target: &Path) -> Output {
    run(target, &["install", "--harness", HARNESS])
}

fn install_ok(target: &Path) {
    let output = install(target);
    assert!(
        output.status.success(),
        "install failed: {}",
        output_text(&output)
    );
}

fn output_text(output: &Output) -> String {
    format!(
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn managed_file(target: &Path) -> PathBuf {
    target.join(".claude/agents/architect.md")
}

fn receipt_path(target: &Path) -> PathBuf {
    // One target-relative receipt per harness. This also permits shared roots
    // such as `.agents` to have independent ownership records.
    target.join(".shipmates/receipts/claude-code.json")
}

fn read_receipt(target: &Path) -> Value {
    let path = receipt_path(target);
    let bytes = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("receipt missing at {}: {error}", path.display()));
    serde_json::from_str(&bytes)
        .unwrap_or_else(|error| panic!("receipt at {} is not JSON: {error}", path.display()))
}

fn backup_files(target: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    walk(target, &mut |path| {
        let is_backup_name = path
            .file_name()
            .is_some_and(|name| name.to_string_lossy().contains(".bak"));
        let is_backup_dir = path
            .components()
            .any(|component| component.as_os_str() == ".shipmates-backup");
        if is_backup_name || is_backup_dir {
            files.push(path.to_path_buf());
        }
    });
    files
}

fn walk(dir: &Path, visit: &mut impl FnMut(&Path)) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(&path, visit);
        } else {
            visit(&path);
        }
    }
}

fn receipt_files(receipt: &Value) -> &[Value] {
    receipt["files"]
        .as_array()
        .expect("receipt files must be an array")
}

fn resolve_receipt_file(target: &Path, relative: &str) -> PathBuf {
    let path = Path::new(relative);
    assert!(
        !path.is_absolute(),
        "receipt path must be relative: {relative}"
    );
    assert!(
        !path
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir)),
        "receipt path must not escape install root: {relative}"
    );

    // Receipt entries are target-relative. Keep harness-relative fallback for
    // payloads from older receipt fixtures; both forms verify ownership and
    // bytes on disk.
    let harness_relative = target.join(".claude").join(path);
    if harness_relative.exists() {
        harness_relative
    } else {
        target.join(path)
    }
}

#[test]
fn fresh_install_writes_receipt_with_harness_layout_and_hashes() {
    let dir = tempdir().unwrap();
    install_ok(dir.path());
    assert!(
        !dir.path().join(".claude/.shipmates/manifest.json").exists(),
        "install must not create a receipt below harness root"
    );

    let receipt = read_receipt(dir.path());
    assert_eq!(receipt["version"].as_str(), Some(env!("CARGO_PKG_VERSION")));
    assert_eq!(receipt["harness"].as_str(), Some(HARNESS));
    assert_eq!(receipt["layout"].as_str(), Some("skills"));

    let files = receipt_files(&receipt);
    assert!(!files.is_empty(), "fresh install receipt must own files");
    for entry in files {
        let relative = entry["path"].as_str().expect("receipt file path missing");
        let expected_hash = entry["sha256"]
            .as_str()
            .expect("receipt file sha256 missing");
        assert_eq!(expected_hash.len(), 64, "sha256 must be hex: {relative}");
        let path = resolve_receipt_file(dir.path(), relative);
        assert!(
            path.is_file(),
            "receipt-listed file missing: {}",
            path.display()
        );
        assert_eq!(
            shipmates::digest::compute_sha256(&path).unwrap(),
            expected_hash,
            "receipt hash mismatch: {}",
            path.display()
        );
    }
}

#[test]
fn third_party_collision_refuses_install_and_leaves_bytes_untouched() {
    // A file shipmates does not own at a payload path stops the install before
    // anything is written, and names the flag that would replace it (#386).
    let dir = tempdir().unwrap();
    let collision = managed_file(dir.path());
    fs::create_dir_all(collision.parent().unwrap()).unwrap();
    fs::write(&collision, b"user content\n").unwrap();

    let output = install(dir.path());

    assert!(
        !output.status.success(),
        "third-party collision must fail closed: {}",
        output_text(&output)
    );
    assert_eq!(fs::read(&collision).unwrap(), b"user content\n");
    assert!(
        !receipt_path(dir.path()).exists(),
        "a refused install must publish no receipt"
    );
    let text = output_text(&output);
    assert!(
        text.contains("shipmates install --force"),
        "refusal must name the flag that replaces it: {text}"
    );
    assert!(
        text.contains(".claude/agents/architect.md"),
        "refusal must name the colliding path: {text}"
    );
}

#[test]
fn install_adopts_an_unowned_shipmates_file_at_a_payload_path() {
    // The #386 case: a flagship skill on disk that no receipt claims. Install
    // backs it up, writes the current payload, and claims it — rather than
    // leaving it stale forever behind an "unmanaged" warning.
    let dir = tempdir().unwrap();
    let skill = dir.path().join(".claude/skills/report-bug/SKILL.md");
    fs::create_dir_all(skill.parent().unwrap()).unwrap();
    fs::write(&skill, "---\nname: report-bug\n---\nstale 0.1.13 body\n").unwrap();

    install_ok(dir.path());

    let installed = fs::read_to_string(&skill).unwrap();
    assert!(
        !installed.contains("stale 0.1.13 body"),
        "an adopted file must be rewritten from the payload"
    );
    let backups = backup_files(dir.path());
    assert!(
        backups.iter().any(|path| fs::read_to_string(path)
            .unwrap()
            .contains("stale 0.1.13 body")),
        "adoption must back up the bytes it replaces: {backups:?}"
    );
    let receipt = read_receipt(dir.path());
    assert!(
        receipt_files(&receipt)
            .iter()
            .any(|file| file["path"] == ".claude/skills/report-bug/SKILL.md"),
        "an adopted path must be claimed by the receipt"
    );
}

#[test]
fn invalid_receipt_aborts_install_without_writes() {
    let dir = tempdir().unwrap();
    let collision = managed_file(dir.path());
    fs::create_dir_all(collision.parent().unwrap()).unwrap();
    fs::write(&collision, b"keep\n").unwrap();
    fs::create_dir_all(receipt_path(dir.path()).parent().unwrap()).unwrap();
    fs::write(receipt_path(dir.path()), b"{\"schema_version\":99}\n").unwrap();

    let output = install(dir.path());

    assert!(!output.status.success(), "invalid receipt must fail closed");
    assert_eq!(fs::read(&collision).unwrap(), b"keep\n");
    assert_eq!(
        fs::read(receipt_path(dir.path())).unwrap(),
        b"{\"schema_version\":99}\n"
    );
    assert!(!dir.path().join(".claude/skills").exists());
}

#[cfg(unix)]
#[test]
fn symlinked_harness_root_cannot_write_outside_target() {
    use std::os::unix::fs::symlink;

    let target = tempdir().unwrap();
    let outside = tempdir().unwrap();
    symlink(outside.path(), target.path().join(".claude")).unwrap();

    let output = install(target.path());

    assert!(
        !output.status.success(),
        "symlinked harness root must fail closed"
    );
    assert!(fs::read_dir(outside.path()).unwrap().next().is_none());
    assert!(!receipt_path(target.path()).exists());
}

#[test]
fn invalid_sibling_receipt_aborts_other_harness_install() {
    let dir = tempdir().unwrap();
    let first = run(dir.path(), &["install", "--harness", "codex"]);
    assert!(
        first.status.success(),
        "first install failed: {}",
        output_text(&first)
    );
    fs::write(
        dir.path().join(".shipmates/receipts/codex.json"),
        b"corrupt\n",
    )
    .unwrap();

    let second = run(dir.path(), &["install", "--harness", "antigravity"]);

    assert!(!second.status.success(), "corrupt sibling must fail closed");
    assert!(
        !dir.path()
            .join(".shipmates/receipts/antigravity.json")
            .exists()
    );
}

#[test]
fn install_preflight_failure_keeps_prior_tree_and_receipt_absent() {
    let dir = tempdir().unwrap();
    let agents = dir.path().join(".claude/agents");
    fs::create_dir_all(agents.parent().unwrap()).unwrap();
    fs::write(&agents, b"user file\n").unwrap();

    let output = install(dir.path());

    assert!(!output.status.success());
    assert_eq!(fs::read(&agents).unwrap(), b"user file\n");
    assert!(!receipt_path(dir.path()).exists());
}

#[test]
fn unchanged_reinstall_creates_no_backups() {
    let dir = tempdir().unwrap();
    install_ok(dir.path());

    let output = install(dir.path());
    assert!(
        output.status.success(),
        "reinstall failed: {}",
        output_text(&output)
    );
    assert!(
        backup_files(dir.path()).is_empty(),
        "unchanged reinstall must not create backups"
    );
}

#[test]
fn changed_managed_file_gets_only_changed_file_backup() {
    let dir = tempdir().unwrap();
    install_ok(dir.path());
    let managed = managed_file(dir.path());
    fs::write(&managed, b"local edit\n").unwrap();

    let output = install(dir.path());
    assert!(
        output.status.success(),
        "reinstall failed: {}",
        output_text(&output)
    );

    let backups = backup_files(dir.path());
    assert_eq!(
        backups.len(),
        1,
        "only changed managed file should be backed up"
    );
    assert_eq!(fs::read(&backups[0]).unwrap(), b"local edit\n");
    assert_ne!(fs::read(&managed).unwrap(), b"local edit\n");
}

#[test]
fn upgrade_prints_version_and_file_summary() {
    let dir = tempdir().unwrap();
    install_ok(dir.path());
    let manifest = receipt_path(dir.path());
    let mut receipt = read_receipt(dir.path());
    receipt["version"] = Value::String("0.0.0".into());
    fs::write(&manifest, serde_json::to_vec_pretty(&receipt).unwrap()).unwrap();

    let output = install(dir.path());
    assert!(
        output.status.success(),
        "upgrade failed: {}",
        output_text(&output)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Upgrading"),
        "missing upgrade notice: {stdout}"
    );
    assert!(
        stdout.contains("0.0.0"),
        "missing previous version: {stdout}"
    );
    assert!(
        stdout.contains(env!("CARGO_PKG_VERSION")),
        "missing current version: {stdout}"
    );
    for word in ["changed", "new", "removed"] {
        assert!(
            stdout.contains(word),
            "missing {word} count in summary: {stdout}"
        );
    }
}

#[test]
fn unmanaged_file_survives_reinstall_with_warning() {
    // A file of the user's own inside a payload subtree — not at a payload path
    // — is still reported, and still survives.
    let dir = tempdir().unwrap();
    install_ok(dir.path());
    let unmanaged = dir.path().join(".claude/skills/my-notes/SKILL.md");
    fs::create_dir_all(unmanaged.parent().unwrap()).unwrap();
    fs::write(&unmanaged, "local skill\n").unwrap();

    let output = install(dir.path());
    assert!(
        output.status.success(),
        "reinstall failed: {}",
        output_text(&output)
    );
    assert_eq!(fs::read_to_string(&unmanaged).unwrap(), "local skill\n");
    let stdout = String::from_utf8_lossy(&output.stdout).to_ascii_lowercase();
    assert!(
        stdout.contains("unmanaged file left untouched")
            && stdout.contains(".claude/skills/my-notes/skill.md"),
        "reinstall should warn about the unmanaged file: {stdout}"
    );
}

#[cfg(unix)]
#[test]
fn install_ignores_package_manager_symlinks_inside_a_harness_root() {
    // A lived-in opencode tree keeps its own runtime beside the payload. The
    // unmanaged scan must neither resolve those symlinks nor walk node_modules,
    // or the whole upgrade dies on a `.bin` shim (#384).
    use std::os::unix::fs::symlink;

    let dir = tempdir().unwrap();
    let first = run(dir.path(), &["install", "--harness", "opencode"]);
    assert!(
        first.status.success(),
        "opencode install failed: {}",
        output_text(&first)
    );

    let bin = dir.path().join(".opencode/node_modules/.bin");
    fs::create_dir_all(&bin).unwrap();
    let package = dir.path().join(".opencode/node_modules/pkg/cli.js");
    fs::create_dir_all(package.parent().unwrap()).unwrap();
    fs::write(&package, "#!/usr/bin/env node\n").unwrap();
    symlink("../pkg/cli.js", bin.join("node-gyp-build")).unwrap();
    symlink("../pkg/does-not-exist.js", bin.join("dangling")).unwrap();

    let second = run(dir.path(), &["install", "--harness", "opencode"]);

    assert!(
        second.status.success(),
        "install must survive package manager symlinks: {}",
        output_text(&second)
    );
    let stdout = String::from_utf8_lossy(&second.stdout);
    assert!(
        !stdout.contains("node_modules"),
        "the scan must not walk the user's runtime: {stdout}"
    );
    assert!(bin.join("node-gyp-build").symlink_metadata().is_ok());
}

#[test]
fn install_all_continues_past_a_failed_harness_and_exits_non_zero() {
    // `--harness all` is not transactional: one target failing must not abandon
    // the rest, and the run must still report failure (#384).
    #[cfg(unix)]
    use std::os::unix::fs::symlink;

    let dir = tempdir().unwrap();
    let outside = tempdir().unwrap();
    #[cfg(unix)]
    symlink(outside.path(), dir.path().join(".claude")).unwrap();
    #[cfg(not(unix))]
    let _ = &outside;

    let output = run(dir.path(), &["install", "--harness", "all"]);

    assert!(
        !output.status.success(),
        "a failed harness must make the run non-zero: {}",
        output_text(&output)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Harness summary:"),
        "a multi-harness run must summarize every target: {stdout}"
    );
    assert!(
        stdout.contains("claude-code: failed"),
        "the failed harness must be named: {stdout}"
    );
    assert!(
        stdout.contains(&format!(
            "opencode: installed v{}",
            env!("CARGO_PKG_VERSION")
        )),
        "healthy harnesses must report their version: {stdout}"
    );
    assert!(
        dir.path()
            .join(".opencode/commands/ship-issue.md")
            .is_file(),
        "later harnesses must still install"
    );
    assert!(
        dir.path()
            .join(".shipmates/receipts/opencode.json")
            .is_file(),
        "later harnesses must still publish a receipt"
    );
    assert!(
        !dir.path()
            .join(".shipmates/receipts/claude-code.json")
            .exists()
    );
}

#[test]
fn removed_receipt_files_are_removed_with_backup() {
    let dir = tempdir().unwrap();
    let first = run(
        dir.path(),
        &["install", "--harness", HARNESS, "--with-tools", "termgif"],
    );
    assert!(
        first.status.success(),
        "tool install failed: {}",
        output_text(&first)
    );
    let tool = dir.path().join(".claude/skills/shipmates-termgif/SKILL.md");
    assert!(tool.is_file(), "selected tool should be installed");

    let second = run(
        dir.path(),
        &["install", "--harness", HARNESS, "--with-tools", "none"],
    );
    assert!(
        second.status.success(),
        "tool removal install failed: {}",
        output_text(&second)
    );
    assert!(
        !tool.is_file(),
        "removed tool file should be deleted, not left orphaned"
    );
    let stdout = String::from_utf8_lossy(&second.stdout).to_ascii_lowercase();
    assert!(
        stdout.contains("removed dropped file"),
        "removed receipt file should be reported: {stdout}"
    );
}

#[test]
fn uninstall_removes_receipt_owned_files_but_preserves_unmanaged_files() {
    let dir = tempdir().unwrap();
    install_ok(dir.path());
    let unmanaged = dir.path().join(".claude/agents/my-local-agent.md");
    fs::write(&unmanaged, "local agent\n").unwrap();

    let output = run(dir.path(), &["uninstall"]);
    assert!(
        output.status.success(),
        "uninstall failed: {}",
        output_text(&output)
    );
    assert!(!managed_file(dir.path()).exists());
    assert!(unmanaged.exists(), "unmanaged file must survive uninstall");
    assert!(
        !receipt_path(dir.path()).exists(),
        "receipt must be removed"
    );
    let stdout = String::from_utf8_lossy(&output.stdout).to_ascii_lowercase();
    assert!(
        stdout.contains("not managed") || stdout.contains("unmanaged"),
        "uninstall should warn about preserved files: {stdout}"
    );
}

#[test]
fn uninstall_removes_empty_directories() {
    let dir = tempdir().unwrap();
    install_ok(dir.path());

    // Verify some expected directories exist after install.
    assert!(
        dir.path().join(".claude").is_dir(),
        ".claude/ should exist after install"
    );
    assert!(
        dir.path().join(".claude/agents").is_dir(),
        ".claude/agents/ should exist after install"
    );
    assert!(
        dir.path().join(".shipmates").is_dir(),
        ".shipmates/ should exist after install"
    );

    let output = run(dir.path(), &["uninstall"]);
    assert!(
        output.status.success(),
        "uninstall failed: {}",
        output_text(&output)
    );

    // Receipt and managed file must be gone.
    assert!(
        !managed_file(dir.path()).exists(),
        "managed file must be removed"
    );
    assert!(
        !receipt_path(dir.path()).exists(),
        "receipt must be removed"
    );

    // Empty directories left behind by file removal should be cleaned up.
    assert!(
        !dir.path().join(".claude/agents").is_dir(),
        "empty .claude/agents/ should be removed"
    );
    assert!(
        !dir.path().join(".shipmates/receipts").is_dir(),
        "empty .shipmates/receipts/ should be removed"
    );

    let stdout = String::from_utf8_lossy(&output.stdout).to_ascii_lowercase();
    assert!(
        !stdout.contains("empty dir"),
        "should not warn about removing empty dirs: {stdout}"
    );
}

#[test]
fn uninstall_warns_about_shipmates_backup() {
    let dir = tempdir().unwrap();
    install_ok(dir.path());

    // Simulate a prior `doctor --fix` creating a backup directory.
    let backup = dir.path().join(".shipmates-backup");
    fs::create_dir_all(&backup).unwrap();
    fs::write(backup.join("some-backup.md"), "backed up\n").unwrap();

    let output = run(dir.path(), &["uninstall"]);
    assert!(
        output.status.success(),
        "uninstall failed: {}",
        output_text(&output)
    );

    // Backup should survive (it contains user files).
    assert!(
        backup.is_dir(),
        ".shipmates-backup/ should survive uninstall"
    );

    let stdout = String::from_utf8_lossy(&output.stdout).to_ascii_lowercase();
    assert!(
        stdout.contains("backup directory preserved"),
        "uninstall should warn about backup dir: {stdout}"
    );
}

#[test]
fn uninstall_preserves_modified_managed_file() {
    let dir = tempdir().unwrap();
    install_ok(dir.path());
    let managed = managed_file(dir.path());
    fs::write(&managed, b"user modification\n").unwrap();

    let output = run(dir.path(), &["uninstall"]);
    assert!(
        output.status.success(),
        "uninstall failed: {}",
        output_text(&output)
    );
    assert_eq!(fs::read(&managed).unwrap(), b"user modification\n");
}

#[test]
fn uninstall_refuses_missing_or_corrupt_receipt_without_deleting_files() {
    let missing = tempdir().unwrap();
    install_ok(missing.path());
    fs::remove_file(receipt_path(missing.path())).unwrap();
    let output = run(missing.path(), &["uninstall"]);
    assert!(!output.status.success(), "missing receipt must fail closed");
    assert!(managed_file(missing.path()).exists());

    let corrupt = tempdir().unwrap();
    install_ok(corrupt.path());
    fs::write(receipt_path(corrupt.path()), b"not json\n").unwrap();
    let output = run(corrupt.path(), &["uninstall"]);
    assert!(!output.status.success(), "corrupt receipt must fail closed");
    assert!(managed_file(corrupt.path()).exists());
}

#[test]
fn shared_path_uninstall_does_not_remove_files_owned_by_another_harness() {
    // Codex and Antigravity both use `.agents/skills`. Exercise ownership at
    // the CLI boundary: uninstalling one harness must not remove files still
    // claimed by the other receipt.
    let dir = tempdir().unwrap();
    let codex = run(dir.path(), &["install", "--harness", "codex"]);
    assert!(
        codex.status.success(),
        "codex install failed: {}",
        output_text(&codex)
    );
    let antigravity = run(dir.path(), &["install", "--harness", "antigravity"]);
    assert!(
        antigravity.status.success(),
        "antigravity install failed: {}",
        output_text(&antigravity)
    );
    let shared_skill = dir.path().join(".agents/skills/ship-issue/SKILL.md");
    assert!(shared_skill.is_file());

    let output = run(dir.path(), &["uninstall", "--harness", "codex"]);
    assert!(
        output.status.success(),
        "uninstall failed: {}",
        output_text(&output)
    );
    assert!(
        shared_skill.exists(),
        "uninstalling codex must preserve shared skill still owned by antigravity"
    );
    assert!(!dir.path().join(".shipmates/receipts/codex.json").exists());
    assert!(
        dir.path()
            .join(".shipmates/receipts/antigravity.json")
            .exists()
    );
}

#[test]
fn doctor_fix_leaves_third_party_drift_untouched_and_names_force() {
    let dir = tempdir().unwrap();
    install_ok(dir.path());
    let managed = managed_file(dir.path());
    let mut receipt = read_receipt(dir.path());
    receipt["files"]
        .as_array_mut()
        .unwrap()
        .retain(|entry| entry["path"] != ".claude/agents/architect.md");
    fs::write(
        receipt_path(dir.path()),
        serde_json::to_vec_pretty(&receipt).unwrap(),
    )
    .unwrap();
    fs::write(&managed, b"unmanaged edit\n").unwrap();

    let output = run(dir.path(), &["doctor", "--fix"]);

    assert_eq!(
        output.status.code(),
        Some(2),
        "a collision doctor cannot repair stays a reported problem: {}",
        output_text(&output)
    );
    assert_eq!(fs::read(&managed).unwrap(), b"unmanaged edit\n");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("shipmates install --force"),
        "doctor must name the required next step (#386): {stdout}"
    );
}

#[test]
fn doctor_fix_adopts_an_unowned_shipmates_file_and_claims_it() {
    let dir = tempdir().unwrap();
    install_ok(dir.path());
    let skill = dir.path().join(".claude/skills/report-bug/SKILL.md");
    let payload = fs::read_to_string(&skill).unwrap();
    let mut receipt = read_receipt(dir.path());
    receipt["files"]
        .as_array_mut()
        .unwrap()
        .retain(|entry| entry["path"] != ".claude/skills/report-bug/SKILL.md");
    fs::write(
        receipt_path(dir.path()),
        serde_json::to_vec_pretty(&receipt).unwrap(),
    )
    .unwrap();
    fs::write(&skill, "---\nname: report-bug\n---\nstale body\n").unwrap();

    let output = run(dir.path(), &["doctor", "--fix"]);

    assert!(
        output.status.success(),
        "doctor --fix must repair an adoptable collision: {}",
        output_text(&output)
    );
    assert_eq!(fs::read_to_string(&skill).unwrap(), payload);
    let refreshed = read_receipt(dir.path());
    assert!(
        receipt_files(&refreshed)
            .iter()
            .any(|file| file["path"] == ".claude/skills/report-bug/SKILL.md"),
        "the adopted path must be claimed"
    );
}

#[test]
fn doctor_fix_leaves_third_party_skills_in_the_payload_directory_alone() {
    // A user's own skill beside ours is not a payload path and is never touched.
    let dir = tempdir().unwrap();
    install_ok(dir.path());
    let theirs = dir.path().join(".claude/skills/3-amigos/SKILL.md");
    fs::create_dir_all(theirs.parent().unwrap()).unwrap();
    fs::write(&theirs, "---\nname: 3-amigos\n---\ntheirs\n").unwrap();

    let output = run(dir.path(), &["doctor", "--fix"]);

    assert!(
        output.status.success(),
        "an unrelated skill must not make doctor unhealthy: {}",
        output_text(&output)
    );
    assert_eq!(
        fs::read_to_string(&theirs).unwrap(),
        "---\nname: 3-amigos\n---\ntheirs\n"
    );
    let receipt = read_receipt(dir.path());
    assert!(
        !receipt_files(&receipt)
            .iter()
            .any(|file| file["path"] == ".claude/skills/3-amigos/SKILL.md"),
        "an unrelated skill must never be claimed"
    );
}

#[test]
fn doctor_fix_without_receipt_reports_unknown_ownership_and_preserves_drift() {
    let dir = tempdir().unwrap();
    install_ok(dir.path());
    let managed = managed_file(dir.path());
    fs::remove_file(receipt_path(dir.path())).unwrap();
    fs::write(&managed, b"unknown owner\n").unwrap();

    let output = run(dir.path(), &["doctor", "--fix"]);

    assert!(
        output.status.success(),
        "doctor fix failed: {}",
        output_text(&output)
    );
    assert_eq!(fs::read(&managed).unwrap(), b"unknown owner\n");
    assert!(String::from_utf8_lossy(&output.stdout).contains("ownership is unknown"));
}

#[test]
fn doctor_fix_invalid_receipt_fails_without_writing() {
    let dir = tempdir().unwrap();
    install_ok(dir.path());
    let managed = managed_file(dir.path());
    fs::write(&managed, b"keep this drift\n").unwrap();
    fs::write(receipt_path(dir.path()), b"not json\n").unwrap();

    let output = run(dir.path(), &["doctor", "--fix"]);

    assert!(!output.status.success());
    assert_eq!(fs::read(&managed).unwrap(), b"keep this drift\n");
    assert_eq!(fs::read(receipt_path(dir.path())).unwrap(), b"not json\n");
}

#[test]
fn doctor_fix_refreshes_receipt_hash_after_owned_repair() {
    let dir = tempdir().unwrap();
    install_ok(dir.path());
    let managed = managed_file(dir.path());
    let mut receipt = read_receipt(dir.path());
    for file in receipt["files"].as_array_mut().unwrap() {
        if file["path"] == ".claude/agents/architect.md" {
            file["sha256"] = Value::String("a".repeat(64));
        }
    }
    fs::write(
        receipt_path(dir.path()),
        serde_json::to_vec_pretty(&receipt).unwrap(),
    )
    .unwrap();
    fs::write(&managed, b"drift\n").unwrap();

    let output = run(dir.path(), &["doctor", "--fix"]);

    assert!(
        output.status.success(),
        "doctor fix failed: {}",
        output_text(&output)
    );
    let refreshed = read_receipt(dir.path());
    let entry = receipt_files(&refreshed)
        .iter()
        .find(|file| file["path"] == ".claude/agents/architect.md")
        .unwrap();
    let expected_hash = shipmates::digest::compute_sha256(&managed).unwrap();
    assert_eq!(entry["sha256"].as_str(), Some(expected_hash.as_str()));
}

#[test]
fn doctor_reports_unreadable_managed_file_as_nonzero() {
    let dir = tempdir().unwrap();
    install_ok(dir.path());
    let managed = managed_file(dir.path());
    fs::write(&managed, [0xff, 0xfe, 0x00, 0x9c]).unwrap();

    let output = run(dir.path(), &["doctor"]);
    assert_eq!(
        output.status.code(),
        Some(2),
        "doctor output: {}",
        output_text(&output)
    );
}

#[test]
fn doctor_fix_leaves_unreadable_file_untouched_and_fails() {
    let dir = tempdir().unwrap();
    install_ok(dir.path());
    let managed = managed_file(dir.path());
    let unreadable = [0xff, 0xfe, 0x00, 0x9c];
    fs::write(&managed, unreadable).unwrap();

    let output = run(dir.path(), &["doctor", "--fix"]);
    assert_eq!(
        output.status.code(),
        Some(2),
        "doctor --fix must remain failed when repair is unsafe: {}",
        output_text(&output)
    );
    assert_eq!(fs::read(&managed).unwrap(), unreadable);
}

#[test]
fn doctor_no_migrate_requires_fix_and_fix_leaves_legacy_file() {
    let invalid = tempdir().unwrap();
    let output = run(invalid.path(), &["doctor", "--no-migrate"]);
    assert!(
        !output.status.success(),
        "--no-migrate without --fix must not be a no-op"
    );

    let dir = tempdir().unwrap();
    install_ok(dir.path());
    let missing = dir.path().join(".claude/agents/architect.md");
    fs::remove_file(&missing).unwrap();
    let legacy = dir.path().join(".claude/commands/ship-issue.md");
    fs::create_dir_all(legacy.parent().unwrap()).unwrap();
    fs::write(&legacy, "---\nname: ship-issue\n---\nlegacy\n").unwrap();

    let output = run(dir.path(), &["doctor", "--fix", "--no-migrate"]);
    assert_eq!(
        output.status.code(),
        Some(2),
        "legacy layout should remain a reported problem: {}",
        output_text(&output)
    );
    assert!(
        missing.exists(),
        "--no-migrate must still restore missing files"
    );
    assert!(legacy.exists(), "--no-migrate must preserve legacy command");
}

#[test]
fn doctor_fix_skipped_does_not_list_never_installed_tools() {
    // A healthy no-tools install must not print a scary "Skipped N file(s)"
    // line listing every optional tool that was never installed (#267).
    let dir = tempdir().unwrap();
    install_ok(dir.path());

    let output = run(dir.path(), &["doctor", "--fix"]);
    assert!(
        output.status.success(),
        "doctor --fix failed: {}",
        output_text(&output)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("Skipped"),
        "no-tools install must not report skipped tool files: {}",
        stdout
    );
}

#[test]
fn doctor_fix_still_repairs_drifted_installed_tool() {
    // When tools ARE installed, doctor --fix must still repair drifted files
    // among them. The #267 fix only suppresses the skip report for tools that
    // were never claimed by the receipt.
    let dir = tempdir().unwrap();
    let output = run(
        dir.path(),
        &["install", "--harness", HARNESS, "--with-tools", "termgif"],
    );
    assert!(
        output.status.success(),
        "tool install failed: {}",
        output_text(&output)
    );
    let tool_skill = dir.path().join(".claude/skills/shipmates-termgif/SKILL.md");
    assert!(
        tool_skill.is_file(),
        "shipmates-termgif SKILL.md must exist"
    );

    // Drift the tool file.
    fs::write(&tool_skill, "drifted content\n").unwrap();

    let output = run(dir.path(), &["doctor", "--fix"]);
    assert!(
        output.status.success(),
        "doctor --fix failed: {}",
        output_text(&output)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    // The drifted file should have been restored, not skipped.
    let restored = fs::read_to_string(&tool_skill).unwrap();
    assert!(
        restored != "drifted content\n",
        "drifted tool file must be repaired, not left alone"
    );
    assert!(
        !stdout.contains("Skipped"),
        "installed tool repair must not produce a skipped line: {}",
        stdout
    );
}
