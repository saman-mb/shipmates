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
    let dir = tempdir().unwrap();
    install_ok(dir.path());
    let unmanaged = managed_file(dir.path());
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
    fs::write(&unmanaged, "local agent\n").unwrap();

    let output = install(dir.path());
    assert!(
        output.status.success(),
        "reinstall failed: {}",
        output_text(&output)
    );
    assert_eq!(fs::read_to_string(&unmanaged).unwrap(), "local agent\n");
    let stdout = String::from_utf8_lossy(&output.stdout).to_ascii_lowercase();
    assert!(
        stdout.contains("not managed") || stdout.contains("unmanaged"),
        "reinstall should warn about unmanaged files: {stdout}"
    );
}

#[test]
fn removed_receipt_files_remain_with_warning() {
    let dir = tempdir().unwrap();
    let first = run(
        dir.path(),
        &[
            "install",
            "--harness",
            HARNESS,
            "--with-tools",
            "termgif",
        ],
    );
    assert!(
        first.status.success(),
        "tool install failed: {}",
        output_text(&first)
    );
    let tool = dir.path().join(".claude/skills/termgif/SKILL.md");
    assert!(tool.is_file(), "selected tool should be installed");

    let second = run(
        dir.path(),
        &[
            "install",
            "--harness",
            HARNESS,
            "--with-tools",
            "none",
        ],
    );
    assert!(
        second.status.success(),
        "tool removal install failed: {}",
        output_text(&second)
    );
    assert!(tool.is_file(), "removed tool file must remain untouched");
    let stdout = String::from_utf8_lossy(&second.stdout).to_ascii_lowercase();
    assert!(
        stdout.contains("no longer in payload") || stdout.contains("previous managed"),
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
