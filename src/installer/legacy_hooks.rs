//! Remove hook files and registrations left by releases that shipped FSM hooks.
//!
//! This migration intentionally does not use a receipt: those releases did not
//! write one. Ownership is proved by both the exact legacy path and the known
//! Shipmates hook/registration content. Anything else is left untouched.

use crate::installer::{atomic_write, atomic_write_bytes, migrate};
use anyhow::Result;
use serde_json::Value;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

const HOOK_MARKER: &str = "Shipmates FSM tool-gate";
const STATE_GATE_MARKER: &str = "shipmates state gate";

const CLAUDE_GATE: &str =
    "SHIPMATES_NATIVE_HOOK=1 bash \"${CLAUDE_PROJECT_DIR}/.claude/hooks/fsm-gate.sh\"";
const CURSOR_GATE: &str = "SHIPMATES_NATIVE_HOOK=1 bash .cursor/hooks/fsm-gate.sh";
const CODEX_GATE: &str =
    "SHIPMATES_NATIVE_HOOK=1 bash \"$(git rev-parse --show-toplevel)/.codex/hooks/fsm-gate.sh\"";
const ANTIGRAVITY_GATE: &str = "SHIPMATES_NATIVE_HOOK=1 bash .agents/hooks/fsm-gate.sh";
const COPILOT_GATE: &str = "SHIPMATES_NATIVE_HOOK=1 bash .github/hooks/fsm-gate.sh";
const WINDSURF_GATE: &str = "SHIPMATES_NATIVE_HOOK=1 bash .windsurf/hooks/fsm-gate.sh";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CleanupReport {
    pub removed_files: Vec<PathBuf>,
    pub changed_configs: Vec<PathBuf>,
    pub skipped: Vec<PathBuf>,
}

impl CleanupReport {
    pub fn changed(&self) -> bool {
        !self.removed_files.is_empty() || !self.changed_configs.is_empty()
    }
}

fn hook_path(harness: &str) -> Option<&'static str> {
    match harness {
        "claude-code" => Some(".claude/hooks/fsm-gate.sh"),
        "opencode" => Some(".opencode/plugins/fsm-gate.ts"),
        "windsurf" => Some(".windsurf/hooks/fsm-gate.sh"),
        "antigravity" => Some(".agents/hooks/fsm-gate.sh"),
        "codex" => Some(".codex/hooks/fsm-gate.sh"),
        "github-copilot" => Some(".github/hooks/fsm-gate.sh"),
        "cursor" => Some(".cursor/hooks/fsm-gate.sh"),
        _ => None,
    }
}

fn config_paths(harness: &str) -> &'static [&'static str] {
    match harness {
        "claude-code" => &[".claude/settings.json"],
        "cursor" => &[".cursor/hooks.json"],
        "codex" => &[".codex/hooks.json", ".codex/config.toml"],
        "antigravity" => &[".agents/hooks.json"],
        "github-copilot" => &[".github/hooks/shipmates-fsm-gate.json"],
        "windsurf" => &[".windsurf/hooks.json"],
        _ => &[],
    }
}

fn registration_commands(harness: &str) -> BTreeSet<&'static str> {
    let mut commands = BTreeSet::new();
    match harness {
        "claude-code" => commands.extend([
            CLAUDE_GATE,
            "shipmates hook context --harness claude-code --event SessionStart",
            "shipmates hook subagent-start --harness claude-code",
            "shipmates hook record --harness claude-code --event PreCompact",
            "shipmates hook post-tool-use-advance --harness claude-code",
            "shipmates hook record --harness claude-code --event SubagentStop",
            "shipmates hook stop --harness claude-code",
        ]),
        "codex" => commands.extend([
            CODEX_GATE,
            "shipmates hook context --harness codex --event SessionStart",
            "shipmates hook subagent-start --harness codex",
            "shipmates hook record --harness codex --event PreCompact",
            "shipmates hook post-tool-use-advance --harness codex",
            "shipmates hook record --harness codex --event SubagentStop",
            "shipmates hook stop --harness codex",
        ]),
        "cursor" => {
            commands.insert(CURSOR_GATE);
        }
        "antigravity" => {
            commands.insert(ANTIGRAVITY_GATE);
        }
        "github-copilot" => {
            commands.insert(COPILOT_GATE);
        }
        "windsurf" => {
            commands.insert(WINDSURF_GATE);
        }
        "opencode" => {}
        _ => {}
    }
    commands
}

fn owned_hook_body(body: &str) -> bool {
    body.contains(HOOK_MARKER) && body.contains(STATE_GATE_MARKER)
}

/// Refuse paths containing symlinks before any cleanup read, write, or delete.
/// Missing trailing components are allowed because backup writes create them.
fn path_is_safe(base: &Path, path: &Path) -> Result<bool> {
    let Ok(relative) = path.strip_prefix(base) else {
        return Ok(false);
    };
    match fs::symlink_metadata(base) {
        Ok(metadata) if metadata.file_type().is_symlink() => return Ok(false),
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error.into()),
    }
    let mut current = base.to_path_buf();
    for component in relative.components() {
        current.push(component.as_os_str());
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => return Ok(false),
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(true),
            Err(error) => return Err(error.into()),
        }
    }
    Ok(true)
}

fn existing_regular_file(base: &Path, path: &Path) -> Result<bool> {
    if !path_is_safe(base, path)? {
        return Ok(false);
    }
    match fs::symlink_metadata(path) {
        Ok(metadata) => Ok(metadata.file_type().is_file()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

fn backup_verified(target_dir: &Path, rel: &Path, contents: &[u8], root: &Path) -> Result<bool> {
    let backup = root.join(rel);
    let root_safe = path_is_safe(target_dir, root)?;
    let backup_safe = path_is_safe(target_dir, &backup)?;
    if !root_safe || !backup_safe {
        return Ok(false);
    }
    if atomic_write_bytes(&backup, contents).is_err()
        || !path_is_safe(target_dir, &backup)?
        || !existing_regular_file(target_dir, &backup)?
    {
        return Ok(false);
    }
    Ok(fs::read(&backup).ok().as_deref() == Some(contents))
}

fn remove_hook_file(
    target_dir: &Path,
    rel: &str,
    root: &Path,
    report: &mut CleanupReport,
) -> Result<()> {
    let path = target_dir.join(rel);
    if !existing_regular_file(target_dir, &path)? {
        if !path_is_safe(target_dir, &path)? {
            report.skipped.push(PathBuf::from(rel));
        }
        return Ok(());
    }
    let contents = match fs::read(&path) {
        Ok(contents) => contents,
        Err(_) => {
            report.skipped.push(PathBuf::from(rel));
            return Ok(());
        }
    };
    match String::from_utf8(contents.clone()) {
        Ok(body) if owned_hook_body(&body) => {}
        Ok(_) | Err(_) => {
            report.skipped.push(PathBuf::from(rel));
            return Ok(());
        }
    }
    if !backup_verified(target_dir, Path::new(rel), &contents, root)? {
        report.skipped.push(PathBuf::from(rel));
        return Ok(());
    }
    if !path_is_safe(target_dir, &path)? {
        report.skipped.push(PathBuf::from(rel));
        return Ok(());
    }
    fs::remove_file(&path)?;
    report.removed_files.push(PathBuf::from(rel));
    Ok(())
}

fn command_at_entry(entry: &Value) -> Option<&str> {
    entry
        .get("command")
        .and_then(Value::as_str)
        .or_else(|| entry.get("bash").and_then(Value::as_str))
}

fn contains_command(value: &Value, commands: &BTreeSet<&str>) -> bool {
    if command_at_entry(value).is_some_and(|command| commands.contains(command)) {
        return true;
    }
    value
        .get("hooks")
        .and_then(Value::as_array)
        .is_some_and(|entries| {
            entries
                .iter()
                .any(|entry| contains_command(entry, commands))
        })
}

fn simple_hook_group(value: &Value) -> bool {
    value.as_object().is_some_and(|object| {
        object
            .keys()
            .all(|key| matches!(key.as_str(), "hooks" | "matcher" | "timeout" | "type"))
    })
}

fn clean_entries(entries: &mut Vec<Value>, commands: &BTreeSet<&str>) -> usize {
    let mut removed = 0;
    let mut kept = Vec::with_capacity(entries.len());
    for mut entry in entries.drain(..) {
        if command_at_entry(&entry).is_some_and(|command| commands.contains(command)) {
            removed += 1;
            continue;
        }
        if let Some(nested) = entry.get_mut("hooks").and_then(Value::as_array_mut) {
            removed += clean_entries(nested, commands);
            if nested.is_empty() && simple_hook_group(&entry) {
                continue;
            }
        }
        kept.push(entry);
    }
    *entries = kept;
    removed
}

fn clean_json(root: &mut Value, commands: &BTreeSet<&str>) -> usize {
    let Some(hooks) = root.get_mut("hooks").and_then(Value::as_object_mut) else {
        return 0;
    };
    let mut removed = 0;
    let events: Vec<String> = hooks.keys().cloned().collect();
    for event in events {
        let Some(entries) = hooks.get_mut(&event).and_then(Value::as_array_mut) else {
            continue;
        };
        removed += clean_entries(entries, commands);
        if entries.is_empty() {
            hooks.remove(&event);
        }
    }
    if hooks.is_empty() {
        root.as_object_mut().unwrap().remove("hooks");
    }
    removed
}

fn only_version_metadata(root: &Value) -> bool {
    root.as_object()
        .is_some_and(|object| object.keys().all(|key| key == "version" || key == "schema"))
}

fn cleanup_json_config(
    target_dir: &Path,
    rel: &str,
    commands: &BTreeSet<&str>,
    root: &Path,
    report: &mut CleanupReport,
) -> Result<bool> {
    let path = target_dir.join(rel);
    if !existing_regular_file(target_dir, &path)? {
        if !path_is_safe(target_dir, &path)? {
            report.skipped.push(PathBuf::from(rel));
        }
        return Ok(false);
    }
    let old_bytes = match fs::read(&path) {
        Ok(old) => old,
        Err(_) => {
            report.skipped.push(PathBuf::from(rel));
            return Ok(false);
        }
    };
    let old = match String::from_utf8(old_bytes.clone()) {
        Ok(old) => old,
        Err(_) => {
            report.skipped.push(PathBuf::from(rel));
            return Ok(false);
        }
    };
    let mut value: Value = match serde_json::from_str(&old) {
        Ok(value) => value,
        Err(_) => {
            report.skipped.push(PathBuf::from(rel));
            return Ok(false);
        }
    };
    let removed = clean_json(&mut value, commands);
    if removed == 0 {
        return Ok(false);
    }
    if !backup_verified(target_dir, Path::new(rel), &old_bytes, root)? {
        report.skipped.push(PathBuf::from(rel));
        return Ok(false);
    }
    let remove_config = value.as_object().is_some_and(|object| object.is_empty())
        || (rel == ".github/hooks/shipmates-fsm-gate.json" && only_version_metadata(&value));
    if !path_is_safe(target_dir, &path)? {
        report.skipped.push(PathBuf::from(rel));
        return Ok(false);
    }
    if remove_config {
        fs::remove_file(&path)?;
    } else {
        let mut next = serde_json::to_string_pretty(&value)?;
        next.push('\n');
        atomic_write(&path, &next)?;
    }
    report.changed_configs.push(PathBuf::from(rel));
    Ok(true)
}

fn json_has_hooks(target_dir: &Path, path: &Path) -> bool {
    if !path_is_safe(target_dir, path).unwrap_or(false) {
        return false;
    }
    fs::read(path)
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .and_then(|value| value.get("hooks").cloned())
        .and_then(|hooks| hooks.as_object().cloned())
        .is_some_and(|hooks| {
            hooks.values().any(|entries| {
                entries
                    .as_array()
                    .map(|values| !values.is_empty())
                    .unwrap_or(true)
            })
        })
}

fn cleanup_codex_feature(target_dir: &Path, root: &Path, report: &mut CleanupReport) -> Result<()> {
    let path = target_dir.join(".codex/config.toml");
    if !existing_regular_file(target_dir, &path)? {
        if !path_is_safe(target_dir, &path)? {
            report.skipped.push(PathBuf::from(".codex/config.toml"));
        }
        return Ok(());
    }
    let old_bytes = match fs::read(&path) {
        Ok(old) => old,
        Err(_) => {
            report.skipped.push(PathBuf::from(".codex/config.toml"));
            return Ok(());
        }
    };
    let old = match String::from_utf8(old_bytes.clone()) {
        Ok(old) => old,
        Err(_) => {
            report.skipped.push(PathBuf::from(".codex/config.toml"));
            return Ok(());
        }
    };
    let mut in_features = false;
    let mut removed = false;
    let lines: Vec<String> = old
        .lines()
        .map(|line| {
            let trimmed = line.trim();
            if trimmed.starts_with('[') {
                in_features = trimmed == "[features]";
            }
            if in_features && trimmed == "hooks = true" {
                removed = true;
                String::new()
            } else {
                line.to_string()
            }
        })
        .collect();
    if !removed {
        return Ok(());
    }
    if !backup_verified(
        target_dir,
        Path::new(".codex/config.toml"),
        &old_bytes,
        root,
    )? {
        report.skipped.push(PathBuf::from(".codex/config.toml"));
        return Ok(());
    }
    if !path_is_safe(target_dir, &path)? {
        report.skipped.push(PathBuf::from(".codex/config.toml"));
        return Ok(());
    }
    let mut next = lines.join("\n");
    next.push('\n');
    atomic_write(&path, &next)?;
    report
        .changed_configs
        .push(PathBuf::from(".codex/config.toml"));
    Ok(())
}

/// Remove stale FSM hook files and only the exact Shipmates registrations for
/// one harness. Safe to run repeatedly and on installs with no receipt.
pub fn cleanup(target_dir: &Path, harness: &str) -> Result<CleanupReport> {
    let mut report = CleanupReport::default();
    let root = migrate::new_backup_root(target_dir);
    if let Some(rel) = hook_path(harness) {
        remove_hook_file(target_dir, rel, &root, &mut report)?;
    }

    let commands = registration_commands(harness);
    let mut codex_registration_removed = false;
    for rel in config_paths(harness) {
        if *rel == ".codex/config.toml" {
            continue;
        }
        codex_registration_removed |=
            cleanup_json_config(target_dir, rel, &commands, &root, &mut report)?;
    }
    if harness == "codex"
        && codex_registration_removed
        && !json_has_hooks(target_dir, &target_dir.join(".codex/hooks.json"))
    {
        cleanup_codex_feature(target_dir, &root, &mut report)?;
    }
    Ok(report)
}

/// Read-only ownership probe used by `doctor`.
pub fn has_legacy(target_dir: &Path, harness: &str) -> bool {
    if let Some(rel) = hook_path(harness) {
        let path = target_dir.join(rel);
        if path_is_safe(target_dir, &path).unwrap_or(false)
            && existing_regular_file(target_dir, &path).unwrap_or(false)
            && fs::read(&path)
                .ok()
                .and_then(|body| String::from_utf8(body).ok())
                .is_some_and(|body| owned_hook_body(&body))
        {
            return true;
        }
    }
    let commands = registration_commands(harness);
    config_paths(harness).iter().any(|rel| {
        let path = target_dir.join(rel);
        path_is_safe(target_dir, &path).unwrap_or(false)
            && existing_regular_file(target_dir, &path).unwrap_or(false)
            && fs::read(&path)
                .ok()
                .and_then(|bytes| String::from_utf8(bytes).ok())
                .and_then(|text| serde_json::from_str::<Value>(&text).ok())
                .is_some_and(|value| contains_command(&value, &commands))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::installer::atomic_write;
    use tempfile::tempdir;

    const OLD_SHELL: &str =
        "#!/usr/bin/env bash\n# Shipmates FSM tool-gate\nshipmates state gate\n";
    const OLD_PLUGIN: &str = "// Shipmates FSM tool-gate\n// shipmates state gate\n";

    fn old_registration(harness: &str) -> (&'static str, &'static str, &'static str) {
        match harness {
            "claude-code" => (
                ".claude/hooks/fsm-gate.sh",
                ".claude/settings.json",
                CLAUDE_GATE,
            ),
            "opencode" => (".opencode/plugins/fsm-gate.ts", "", ""),
            "windsurf" => (
                ".windsurf/hooks/fsm-gate.sh",
                ".windsurf/hooks.json",
                WINDSURF_GATE,
            ),
            "antigravity" => (
                ".agents/hooks/fsm-gate.sh",
                ".agents/hooks.json",
                ANTIGRAVITY_GATE,
            ),
            "codex" => (".codex/hooks/fsm-gate.sh", ".codex/hooks.json", CODEX_GATE),
            "github-copilot" => (
                ".github/hooks/fsm-gate.sh",
                ".github/hooks/shipmates-fsm-gate.json",
                COPILOT_GATE,
            ),
            "cursor" => (
                ".cursor/hooks/fsm-gate.sh",
                ".cursor/hooks.json",
                CURSOR_GATE,
            ),
            _ => panic!("unknown harness"),
        }
    }

    #[test]
    fn removes_receipt_free_legacy_install_for_all_harnesses() {
        for harness in [
            "claude-code",
            "opencode",
            "windsurf",
            "antigravity",
            "codex",
            "github-copilot",
            "cursor",
        ] {
            let dir = tempdir().unwrap();
            let (hook, config, command) = old_registration(harness);
            atomic_write(
                &dir.path().join(hook),
                if harness == "opencode" {
                    OLD_PLUGIN
                } else {
                    OLD_SHELL
                },
            )
            .unwrap();
            if !config.is_empty() {
                let body = if harness == "github-copilot" {
                    serde_json::json!({
                        "version": 1,
                        "hooks": {"preToolUse": [{"type": "command", "bash": command, "timeoutSec": 30}]}
                    })
                    .to_string()
                } else if harness == "cursor" {
                    serde_json::json!({
                        "hooks": {"beforeShellExecution": [{"command": command}]}
                    })
                    .to_string()
                } else if harness == "windsurf" {
                    serde_json::json!({
                        "hooks": {"pre_run_command": [{"command": command}]}
                    })
                    .to_string()
                } else {
                    serde_json::json!({
                        "hooks": {"PreToolUse": [{"matcher": "Bash", "hooks": [{"type": "command", "command": command}]}]}
                    })
                    .to_string()
                };
                atomic_write(&dir.path().join(config), &body).unwrap();
            }
            if harness == "codex" {
                atomic_write(
                    &dir.path().join(".codex/config.toml"),
                    "[features]\nhooks = true\n",
                )
                .unwrap();
            }
            assert!(
                has_legacy(dir.path(), harness),
                "{harness} should be detected"
            );
            let report = cleanup(dir.path(), harness).unwrap();
            assert!(report.changed(), "{harness} should be cleaned");
            assert!(!dir.path().join(hook).exists(), "{harness} hook remains");
            if harness == "codex" {
                assert!(
                    !fs::read_to_string(dir.path().join(".codex/config.toml"))
                        .unwrap()
                        .contains("hooks = true")
                );
            }
            assert!(
                !has_legacy(dir.path(), harness),
                "{harness} remains detected"
            );
        }
    }

    #[test]
    fn preserves_unrelated_hooks_and_same_named_user_file() {
        let dir = tempdir().unwrap();
        atomic_write(
            &dir.path().join(".claude/hooks/fsm-gate.sh"),
            "#!/bin/sh\nuser hook\n",
        )
        .unwrap();
        atomic_write(
            &dir.path().join(".claude/settings.json"),
            &serde_json::json!({
                "hooks": {"PreToolUse": [{
                    "matcher": "Bash",
                    "hooks": [
                        {"type": "command", "command": CLAUDE_GATE},
                        {"type": "command", "command": "user-hook"}
                    ]
                }]}
            })
            .to_string(),
        )
        .unwrap();

        let report = cleanup(dir.path(), "claude-code").unwrap();
        assert!(!report.changed_configs.is_empty());
        assert_eq!(
            fs::read_to_string(dir.path().join(".claude/hooks/fsm-gate.sh")).unwrap(),
            "#!/bin/sh\nuser hook\n"
        );
        let config = fs::read_to_string(dir.path().join(".claude/settings.json")).unwrap();
        assert!(!config.contains(CLAUDE_GATE));
        assert!(config.contains("user-hook"));
    }

    #[test]
    fn verifies_backup_byte_for_byte() {
        let dir = tempdir().unwrap();
        let bytes = [0xff, 0x00, 0x80, 0x41];
        let root = dir.path().join("backup/run");
        assert!(backup_verified(dir.path(), Path::new("hook"), &bytes, &root).unwrap());
        assert_eq!(fs::read(root.join("hook")).unwrap(), bytes);
    }

    #[cfg(unix)]
    #[test]
    fn skips_symlinked_target_hook_without_touching_link_target() {
        use std::os::unix::fs::symlink;

        let dir = tempdir().unwrap();
        let outside = tempdir().unwrap();
        let outside_file = outside.path().join("hook.sh");
        atomic_write(&outside_file, OLD_SHELL).unwrap();
        let target = dir.path().join(".claude/hooks/fsm-gate.sh");
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        symlink(&outside_file, &target).unwrap();

        let report = cleanup(dir.path(), "claude-code").unwrap();
        assert!(
            report
                .skipped
                .contains(&PathBuf::from(".claude/hooks/fsm-gate.sh"))
        );
        assert!(target.is_symlink());
        assert_eq!(fs::read_to_string(outside_file).unwrap(), OLD_SHELL);
    }

    #[cfg(unix)]
    #[test]
    fn skips_symlinked_backup_component_before_writing() {
        use std::os::unix::fs::symlink;

        let dir = tempdir().unwrap();
        let outside = tempdir().unwrap();
        atomic_write(&dir.path().join(".claude/hooks/fsm-gate.sh"), OLD_SHELL).unwrap();
        symlink(outside.path(), dir.path().join(migrate::BACKUP_DIR)).unwrap();

        let report = cleanup(dir.path(), "claude-code").unwrap();
        assert!(
            report
                .skipped
                .contains(&PathBuf::from(".claude/hooks/fsm-gate.sh"))
        );
        assert!(dir.path().join(".claude/hooks/fsm-gate.sh").exists());
        assert!(fs::read_dir(outside.path()).unwrap().next().is_none());
    }

    #[test]
    fn cleanup_is_idempotent() {
        let dir = tempdir().unwrap();
        atomic_write(
            &dir.path().join(".opencode/plugins/fsm-gate.ts"),
            OLD_PLUGIN,
        )
        .unwrap();
        let first = cleanup(dir.path(), "opencode").unwrap();
        let second = cleanup(dir.path(), "opencode").unwrap();
        assert!(first.changed());
        assert!(!second.changed());
    }
}
