//! CLI coverage for `check --target` digest set equality.
//!
//! `check_digests` is per-target and must fail when a built path is absent
//! from `tests/payload-digests/<t>.sha256`, not only when a digest entry is
//! missing from the payload or a hash mismatches. Drive the binary against a
//! temp `--root` so the committed digest fixtures stay untouched.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use tempfile::TempDir;

const TARGET: &str = "cursor";

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_shipmates")
}

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn copy_tree(src: &Path, dst: &Path) {
    fs::create_dir_all(dst).unwrap();
    for entry in fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let to = dst.join(entry.file_name());
        let ty = entry.file_type().unwrap();
        if ty.is_dir() {
            copy_tree(&entry.path(), &to);
        } else if ty.is_file() {
            fs::copy(entry.path(), &to).unwrap();
        }
    }
}

fn output_text(output: &Output) -> String {
    format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn run_check(root: &Path) -> Output {
    Command::new(bin())
        .current_dir(manifest_dir())
        .args(["check", "--target", TARGET, "--root"])
        .arg(root)
        .output()
        .expect("failed to execute shipmates check")
}

fn digest_path(root: &Path) -> PathBuf {
    root.join("tests/payload-digests")
        .join(format!("{TARGET}.sha256"))
}

fn digest_body(root: &Path) -> String {
    fs::read_to_string(digest_path(root)).unwrap()
}

fn write_digest(root: &Path, body: &str) {
    fs::write(digest_path(root), body).unwrap();
}

/// Catalog + a matching digest under a temp root (never the committed fixtures).
fn prepared_root() -> TempDir {
    let tmp = tempfile::tempdir().unwrap();
    let src = manifest_dir();
    copy_tree(&src.join("crew"), &tmp.path().join("crew"));
    copy_tree(&src.join("commands"), &tmp.path().join("commands"));
    copy_tree(&src.join("steering"), &tmp.path().join("steering"));

    let output = Command::new(bin())
        .current_dir(&src)
        .args(["build", "--target", TARGET, "--update", "--root"])
        .arg(tmp.path())
        .output()
        .expect("failed to execute shipmates build --update");
    assert!(
        output.status.success(),
        "build --update failed: {}",
        output_text(&output)
    );
    tmp
}

fn content_lines(body: &str) -> Vec<&str> {
    body.lines()
        .skip(2)
        .filter(|line| !line.trim().is_empty())
        .collect()
}

#[test]
fn check_fails_when_digest_omits_a_built_path() {
    let root = prepared_root();
    let body = digest_body(root.path());
    let lines = content_lines(&body);
    let dropped = *lines.last().expect("digest has payload entries");
    let dropped_path = dropped
        .split_whitespace()
        .next()
        .expect("digest entry has a path");

    let mut shortened = body
        .lines()
        .take(2)
        .chain(lines.iter().copied().take(lines.len() - 1))
        .collect::<Vec<_>>()
        .join("\n");
    shortened.push('\n');
    write_digest(root.path(), &shortened);

    let output = run_check(root.path());
    let text = output_text(&output);
    assert!(
        !output.status.success(),
        "check should fail when a built path is missing from the digest: {text}"
    );
    assert!(
        text.contains(dropped_path),
        "failure must name the missing payload path {dropped_path}: {text}"
    );
}

#[test]
fn check_fails_when_digest_entry_is_not_in_payload() {
    let root = prepared_root();
    let extra_path = "not-in-payload/SKILL.md";
    let mut body = digest_body(root.path());
    body.push_str(extra_path);
    body.push(' ');
    body.push_str(&"0".repeat(64));
    body.push('\n');
    write_digest(root.path(), &body);

    let output = run_check(root.path());
    let text = output_text(&output);
    assert!(
        !output.status.success(),
        "check should fail for a digest entry absent from the payload: {text}"
    );
    assert!(
        text.contains("Payload is missing a digest entry"),
        "expected digest-entry-not-in-payload failure: {text}"
    );
    assert!(
        text.contains(extra_path),
        "failure must name the extra digest path {extra_path}: {text}"
    );
}

#[test]
fn check_fails_on_digest_hash_mismatch() {
    let root = prepared_root();
    let body = digest_body(root.path());
    let mut lines: Vec<String> = body.lines().map(str::to_string).collect();
    let entry = lines
        .iter_mut()
        .find(|line| line.split_whitespace().count() == 2 && !line.starts_with("target="))
        .expect("digest has a hashed payload entry");
    let (path, hash) = entry.split_once(' ').expect("path hash");
    let path = path.to_string();
    let flipped = if hash.starts_with('0') {
        format!("f{}", &hash[1..])
    } else {
        format!("0{}", &hash[1..])
    };
    *entry = format!("{path} {flipped}");
    let mut mutated = lines.join("\n");
    mutated.push('\n');
    write_digest(root.path(), &mutated);

    let output = run_check(root.path());
    let text = output_text(&output);
    assert!(
        !output.status.success(),
        "check should fail on hash mismatch: {text}"
    );
    assert!(
        text.contains("Digest mismatch"),
        "expected hash mismatch failure: {text}"
    );
    assert!(
        text.contains(&path),
        "failure must name the mismatched path {path}: {text}"
    );
}
