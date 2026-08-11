//! Fail-closed uninstall driven solely by install receipts.

use crate::digest;
use crate::installer::{
    manifest_db::{InstallReceipt, ReceiptRepository},
    plan,
};
use anyhow::{bail, Context, Result};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct LocatedReceipt {
    pub path: PathBuf,
    pub receipt: InstallReceipt,
}

#[derive(Debug, Clone, Default)]
pub struct UninstallReport {
    pub harness: String,
    pub removed: usize,
    pub warnings: Vec<String>,
    pub receipt_removed: bool,
}

/// Find receipts without guessing a harness. The complete receipt directory is
/// parsed, so one corrupt sibling cannot be hidden by selecting another one.
pub fn discover(target_dir: &Path) -> Result<Vec<LocatedReceipt>> {
    let repository = ReceiptRepository::new(target_dir);
    let mut found = repository
        .load_all()?
        .into_iter()
        .map(|receipt| {
            Ok(LocatedReceipt {
                path: repository.receipt_path(&receipt.harness)?,
                receipt,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    found.sort_by(|a, b| a.receipt.harness.cmp(&b.receipt.harness));
    Ok(found)
}

pub fn select_receipt(target_dir: &Path, harness: Option<&str>) -> Result<Option<LocatedReceipt>> {
    match harness {
        Some(harness) => {
            let repository = ReceiptRepository::new(target_dir);
            let _ = repository.load_all()?;
            let (state, receipt, error) = plan::read_receipt(target_dir, harness);
            match state {
                plan::ReceiptState::Valid => Ok(Some(LocatedReceipt {
                    path: repository.receipt_path(harness)?,
                    receipt: receipt.expect("valid receipt must be present"),
                })),
                plan::ReceiptState::Missing => {
                    bail!("No install receipt found for harness {harness}; refusing to uninstall")
                }
                plan::ReceiptState::Invalid => bail!(
                    "Install receipt for harness {harness} is invalid: {}; refusing to uninstall",
                    error.unwrap_or_else(|| "unknown receipt error".into())
                ),
            }
        }
        None => {
            let found = discover(target_dir)?;
            match found.as_slice() {
                [] if ReceiptRepository::new(target_dir).receipts_dir()?.exists() => {
                    bail!("No valid install receipt found; refusing to uninstall")
                }
                [] => Ok(None),
                [one] => Ok(Some(one.clone())),
                all => {
                    let names = all
                        .iter()
                        .map(|item| item.receipt.harness.as_str())
                        .collect::<Vec<_>>();
                    bail!(
                        "multiple harness installs found ({}); specify --harness",
                        names.join(", ")
                    )
                }
            }
        }
    }
}

/// Remove only files listed by a valid receipt whose raw bytes still match its
/// recorded hash. Other valid receipts claim shared paths; those paths remain.
pub fn uninstall(target_dir: &Path, selected: LocatedReceipt) -> Result<UninstallReport> {
    let mut report = UninstallReport {
        harness: selected.receipt.harness.clone(),
        ..UninstallReport::default()
    };
    let repository = ReceiptRepository::new(target_dir);
    let other_claims = repository
        .load_all()?
        .into_iter()
        .filter(|item| item.harness != selected.receipt.harness)
        .flat_map(|item| item.files.into_iter().map(|file| file.path))
        .collect::<std::collections::BTreeSet<_>>();

    let mut removals = Vec::new();
    let mut blocked = false;

    // Snapshot every byte before changing anything. A later filesystem failure
    // must not turn uninstall into a partial removal with a still-live receipt.
    for file in &selected.receipt.files {
        let rel = PathBuf::from(&file.path);
        let path = crate::installer::manifest_db::resolve_target_relative(target_dir, &rel)?;
        if other_claims.contains(&file.path) {
            report.warnings.push(format!(
                "Warning: shared-managed file preserved: {}",
                rel.display()
            ));
            continue;
        }
        let current = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                report.warnings.push(format!(
                    "Warning: managed file preserved (cannot read {}): {}",
                    rel.display(),
                    error
                ));
                blocked = true;
                continue;
            }
        };
        if digest::hash_bytes(&current) != file.sha256 {
            report.warnings.push(format!(
                "Warning: modified managed file preserved: {}",
                rel.display()
            ));
            blocked = true;
            continue;
        }
        let permissions = fs::metadata(&path)
            .with_context(|| format!("inspecting managed file {}", path.display()))?
            .permissions();
        removals.push(Removal {
            path,
            bytes: current,
            permissions,
        });
    }

    // Modified or unreadable files retain ownership, including hook ownership:
    // do not unregister a harness whose payload cannot be safely removed.
    if blocked {
        return Ok(report);
    }

    // Parse and prepare hook config edits before removing shims. Invalid or
    // unreadable config fails closed, leaving both registration and payload.
    let hook_changes = hook_changes(target_dir, &selected.receipt.harness)?;
    let managed = selected
        .receipt
        .files
        .iter()
        .map(|file| file.path.clone())
        .collect::<std::collections::BTreeSet<_>>();
    for path in plan::unmanaged_files(target_dir, &selected.receipt.roots, &managed)? {
        report.warnings.push(format!(
            "Warning: unmanaged file preserved: {}",
            path.strip_prefix(target_dir).unwrap_or(&path).display()
        ));
    }

    let removed = remove_files_transaction(&removals, |path| fs::remove_file(path))?;
    let mut applied_hooks = Vec::new();
    for change in &hook_changes {
        if let Err(error) = write_hook_change(change, &change.updated) {
            let rollback = rollback_transaction(&removals, &applied_hooks);
            let rollback = combine_rollbacks(rollback, restore_hook_change(change));
            return Err(combine_rollback_error(
                anyhow::anyhow!(
                    "removing Shipmates hook registration {}: {}",
                    change.path.display(),
                    error
                ),
                rollback,
            ));
        }
        applied_hooks.push(change);
    }

    match repository.remove(&selected.receipt.harness) {
        Ok(true) => {
            report.removed = removed;
            report.receipt_removed = true;
            Ok(report)
        }
        Ok(false) => {
            let rollback = rollback_transaction(&removals, &applied_hooks);
            Err(combine_rollback_error(
                anyhow::anyhow!("install receipt disappeared during uninstall"),
                rollback,
            ))
        }
        Err(error) => {
            let rollback = rollback_transaction(&removals, &applied_hooks);
            Err(combine_rollback_error(
                error.context("removing install receipt"),
                rollback,
            ))
        }
    }
}

struct Removal {
    path: PathBuf,
    bytes: Vec<u8>,
    permissions: fs::Permissions,
}

struct HookChange {
    path: PathBuf,
    original: Vec<u8>,
    updated: Vec<u8>,
    permissions: fs::Permissions,
}

fn remove_files_transaction<F>(removals: &[Removal], mut remove: F) -> Result<usize>
where
    F: FnMut(&Path) -> std::io::Result<()>,
{
    let mut removed = Vec::new();
    for removal in removals {
        if let Err(error) = remove(&removal.path) {
            let rollback = restore_removals(&removed);
            return Err(combine_rollback_error(
                anyhow::anyhow!("removing {}: {}", removal.path.display(), error),
                rollback,
            ));
        }
        removed.push(removal);
    }
    Ok(removed.len())
}

fn restore_removals(removals: &[&Removal]) -> Result<()> {
    for removal in removals.iter().rev() {
        crate::installer::atomic_write_bytes(&removal.path, &removal.bytes)
            .with_context(|| format!("restoring removed file {}", removal.path.display()))?;
        fs::set_permissions(&removal.path, removal.permissions.clone())
            .with_context(|| format!("restoring permissions on {}", removal.path.display()))?;
    }
    Ok(())
}

fn write_hook_change(change: &HookChange, bytes: &[u8]) -> Result<()> {
    crate::installer::atomic_write_bytes(&change.path, bytes)?;
    fs::set_permissions(&change.path, change.permissions.clone())?;
    Ok(())
}

fn restore_hook_change(change: &HookChange) -> Result<()> {
    write_hook_change(change, &change.original)
}

fn rollback_transaction(removals: &[Removal], applied_hooks: &[&HookChange]) -> Result<()> {
    let mut errors = Vec::new();
    for change in applied_hooks.iter().rev() {
        if let Err(error) = restore_hook_change(change) {
            errors.push(format!("restoring {}: {}", change.path.display(), error));
        }
    }
    let removed = removals.iter().collect::<Vec<_>>();
    if let Err(error) = restore_removals(&removed) {
        errors.push(error.to_string());
    }
    if errors.is_empty() {
        Ok(())
    } else {
        bail!("rollback failed: {}", errors.join("; "))
    }
}

fn combine_rollbacks(left: Result<()>, right: Result<()>) -> Result<()> {
    match (left, right) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(error), Ok(())) | (Ok(()), Err(error)) => Err(error),
        (Err(left), Err(right)) => bail!("{}; {}", left, right),
    }
}

fn combine_rollback_error(error: anyhow::Error, rollback: Result<()>) -> anyhow::Error {
    match rollback {
        Ok(()) => error,
        Err(rollback) => error.context(rollback.to_string()),
    }
}

fn hook_changes(target_dir: &Path, harness: &str) -> Result<Vec<HookChange>> {
    let Some(config) = hook_config(harness) else {
        return Ok(Vec::new());
    };
    let path =
        crate::installer::manifest_db::resolve_target_relative(target_dir, Path::new(config))?;
    let original = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(error).with_context(|| {
                format!(
                    "reading Shipmates hook config {}; refusing to remove shims",
                    path.display()
                )
            });
        }
    };
    let mut root: Value = serde_json::from_slice(&original).with_context(|| {
        format!(
            "parsing Shipmates hook config {}; refusing to remove shims",
            path.display()
        )
    })?;
    if !root.is_object() {
        bail!(
            "hook config {} must contain a JSON object; refusing to remove shims",
            path.display()
        );
    }
    let changed = root
        .get_mut("hooks")
        .map(|hooks| clean_hook_object(hooks, harness))
        .unwrap_or(false);
    if !changed {
        return Ok(Vec::new());
    }
    let mut updated = serde_json::to_vec_pretty(&root)?;
    updated.push(b'\n');
    let permissions = fs::metadata(&path)
        .with_context(|| {
            format!(
                "inspecting hook config {}; refusing to remove shims",
                path.display()
            )
        })?
        .permissions();
    Ok(vec![HookChange {
        path,
        original,
        updated,
        permissions,
    }])
}

fn hook_config(harness: &str) -> Option<&'static str> {
    match harness {
        "claude-code" => Some(".claude/settings.json"),
        "cursor" => Some(".cursor/hooks.json"),
        "codex" => Some(".codex/hooks.json"),
        "antigravity" => Some(".agents/hooks.json"),
        "github-copilot" => Some(".github/hooks/shipmates-fsm-gate.json"),
        "windsurf" => Some(".windsurf/hooks.json"),
        "opencode" => None,
        _ => None,
    }
}

fn shipmates_hook_command(command: &str, harness: &str) -> bool {
    command.contains("--harness ")
        && command.contains(&format!("--harness {harness}"))
        && command.split_whitespace().any(|part| part == "shipmates")
}

fn shell_hook_command(harness: &str) -> Option<&'static str> {
    match harness {
        "claude-code" => {
            Some("SHIPMATES_NATIVE_HOOK=1 bash \"${CLAUDE_PROJECT_DIR}/.claude/hooks/fsm-gate.sh\"")
        }
        "cursor" => Some("SHIPMATES_NATIVE_HOOK=1 bash .cursor/hooks/fsm-gate.sh"),
        "codex" => Some(
            "SHIPMATES_NATIVE_HOOK=1 bash \"$(git rev-parse --show-toplevel)/.codex/hooks/fsm-gate.sh\"",
        ),
        "antigravity" => Some("SHIPMATES_NATIVE_HOOK=1 bash .agents/hooks/fsm-gate.sh"),
        "github-copilot" => Some(
            "SHIPMATES_NATIVE_HOOK=1 bash \"$(git rev-parse --show-toplevel)/.github/hooks/fsm-gate.sh\"",
        ),
        "windsurf" => Some("SHIPMATES_NATIVE_HOOK=1 bash .windsurf/hooks/fsm-gate.sh"),
        _ => None,
    }
}

fn owned_hook_entry(value: &Value, harness: &str) -> bool {
    ["command", "bash"].iter().any(|key| {
        value
            .get(key)
            .and_then(Value::as_str)
            .is_some_and(|command| {
                Some(command) == shell_hook_command(harness)
                    || shipmates_hook_command(command, harness)
            })
    })
}

fn clean_hook_object(value: &mut Value, harness: &str) -> bool {
    match value {
        Value::Object(object) => {
            let mut changed = false;
            for child in object.values_mut() {
                if let Value::Array(entries) = child {
                    changed |= clean_hook_array(entries, harness);
                } else if let Value::Object(_) = child {
                    changed |= clean_hook_object(child, harness);
                }
            }
            changed
        }
        Value::Array(entries) => clean_hook_array(entries, harness),
        _ => false,
    }
}

fn clean_hook_array(entries: &mut Vec<Value>, harness: &str) -> bool {
    let mut changed = false;
    entries.retain_mut(|entry| {
        if owned_hook_entry(entry, harness) {
            changed = true;
            return false;
        }
        if let Value::Object(object) = entry {
            if let Some(Value::Array(nested)) = object.get_mut("hooks") {
                let nested_changed = clean_hook_array(nested, harness);
                if nested_changed {
                    changed = true;
                    if nested.is_empty() {
                        return false;
                    }
                }
            }
            for child in object.values_mut() {
                if !matches!(child, Value::Array(_)) {
                    changed |= clean_hook_object(child, harness);
                }
            }
        }
        true
    });
    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::installer::manifest_db::{ReceiptFile, LAYOUT_SKILLS};
    use tempfile::tempdir;

    fn receipt(target: &Path, harness: &str, path: &str, content: &[u8]) {
        let receipt = InstallReceipt::new(
            "1",
            harness,
            LAYOUT_SKILLS,
            vec![Path::new(path)
                .components()
                .next()
                .unwrap()
                .as_os_str()
                .to_string_lossy()
                .into_owned()],
            vec![ReceiptFile {
                path: path.into(),
                sha256: digest::hash_bytes(content),
            }],
        )
        .unwrap();
        plan::save_receipt(target, &receipt).unwrap();
    }

    #[test]
    fn modified_file_is_preserved() {
        let dir = tempdir().unwrap();
        let path = dir.path().join(".claude/agents/a.md");
        crate::installer::atomic_write(&path, "installed").unwrap();
        receipt(
            dir.path(),
            "claude-code",
            ".claude/agents/a.md",
            b"original",
        );
        let selected = select_receipt(dir.path(), Some("claude-code"))
            .unwrap()
            .unwrap();
        let report = uninstall(dir.path(), selected).unwrap();
        assert_eq!(report.removed, 0);
        assert!(path.exists());
        assert!(!report.warnings.is_empty());
    }

    #[test]
    fn failed_later_removal_restores_earlier_bytes() {
        let dir = tempdir().unwrap();
        let first = dir.path().join("first");
        let second = dir.path().join("second");
        fs::write(&first, [0, 1, 2, 255]).unwrap();
        fs::write(&second, b"second").unwrap();
        let removals = vec![
            Removal {
                path: first.clone(),
                bytes: fs::read(&first).unwrap(),
                permissions: fs::metadata(&first).unwrap().permissions(),
            },
            Removal {
                path: second.clone(),
                bytes: fs::read(&second).unwrap(),
                permissions: fs::metadata(&second).unwrap().permissions(),
            },
        ];

        let error = remove_files_transaction(&removals, |path| {
            if path == second {
                Err(std::io::Error::other("forced removal failure"))
            } else {
                fs::remove_file(path)
            }
        })
        .unwrap_err();

        assert!(error.to_string().contains("forced removal failure"));
        assert_eq!(fs::read(&first).unwrap(), [0, 1, 2, 255]);
        assert_eq!(fs::read(&second).unwrap(), b"second");
    }

    #[test]
    fn claude_uninstall_removes_only_shipmates_hook_entries() {
        let dir = tempdir().unwrap();
        let shim = dir.path().join(".claude/hooks/fsm-gate.sh");
        crate::installer::atomic_write(&shim, "shim").unwrap();
        let settings = dir.path().join(".claude/settings.json");
        crate::installer::atomic_write(
            &settings,
            r#"{
  "custom": true,
  "hooks": {
    "PreToolUse": [
      {"matcher":"Bash","hooks":[{"type":"command","command":"SHIPMATES_NATIVE_HOOK=1 bash \"${CLAUDE_PROJECT_DIR}/.claude/hooks/fsm-gate.sh\""}]},
      {"matcher":"Bash","hooks":[{"type":"command","command":"user-hook"}]}
    ],
    "SessionStart": [{"hooks":[{"type":"command","command":"shipmates hook context --harness claude-code --event SessionStart"}]}]
  }
}"#,
        )
        .unwrap();
        receipt(
            dir.path(),
            "claude-code",
            ".claude/hooks/fsm-gate.sh",
            b"shim",
        );

        uninstall(
            dir.path(),
            select_receipt(dir.path(), Some("claude-code"))
                .unwrap()
                .unwrap(),
        )
        .unwrap();

        let root: Value = serde_json::from_slice(&fs::read(&settings).unwrap()).unwrap();
        assert_eq!(root["custom"], true);
        assert_eq!(root["hooks"]["PreToolUse"].as_array().unwrap().len(), 1);
        assert_eq!(
            root["hooks"]["PreToolUse"][0]["hooks"][0]["command"],
            "user-hook"
        );
        assert!(root["hooks"]["SessionStart"].as_array().unwrap().is_empty());
        assert!(!shim.exists());
    }

    #[test]
    fn malformed_hook_config_keeps_shim_and_receipt() {
        let dir = tempdir().unwrap();
        let shim = dir.path().join(".claude/hooks/fsm-gate.sh");
        crate::installer::atomic_write(&shim, "shim").unwrap();
        crate::installer::atomic_write(&dir.path().join(".claude/settings.json"), "not json")
            .unwrap();
        receipt(
            dir.path(),
            "claude-code",
            ".claude/hooks/fsm-gate.sh",
            b"shim",
        );

        let error = uninstall(
            dir.path(),
            select_receipt(dir.path(), Some("claude-code"))
                .unwrap()
                .unwrap(),
        )
        .unwrap_err();

        assert!(error.to_string().contains("refusing to remove shims"));
        assert!(shim.exists());
        assert!(ReceiptRepository::new(dir.path())
            .load("claude-code")
            .unwrap()
            .is_some());
    }

    #[test]
    fn json_harness_uninstall_preserves_unrelated_hook_entry() {
        let dir = tempdir().unwrap();
        let shim = dir.path().join(".agents/hooks/fsm-gate.sh");
        crate::installer::atomic_write(&shim, "shim").unwrap();
        let config = dir.path().join(".agents/hooks.json");
        crate::installer::atomic_write(
            &config,
            r#"{"hooks":{"PreToolUse":[{"matcher":"run_command","hooks":[{"type":"command","command":"SHIPMATES_NATIVE_HOOK=1 bash .agents/hooks/fsm-gate.sh"}]},{"matcher":"run_command","hooks":[{"type":"command","command":"keep-me"}]}]},"custom":42}"#,
        )
        .unwrap();
        receipt(
            dir.path(),
            "antigravity",
            ".agents/hooks/fsm-gate.sh",
            b"shim",
        );

        uninstall(
            dir.path(),
            select_receipt(dir.path(), Some("antigravity"))
                .unwrap()
                .unwrap(),
        )
        .unwrap();

        let root: Value = serde_json::from_slice(&fs::read(&config).unwrap()).unwrap();
        assert_eq!(root["custom"], 42);
        assert_eq!(
            root["hooks"]["PreToolUse"][0]["hooks"][0]["command"],
            "keep-me"
        );
        assert!(!shim.exists());
    }

    #[test]
    fn shared_file_is_preserved() {
        let dir = tempdir().unwrap();
        let path = dir.path().join(".agents/skills/x/SKILL.md");
        crate::installer::atomic_write(&path, "installed").unwrap();
        receipt(
            dir.path(),
            "codex",
            ".agents/skills/x/SKILL.md",
            b"installed",
        );
        receipt(
            dir.path(),
            "antigravity",
            ".agents/skills/x/SKILL.md",
            b"installed",
        );
        let selected = select_receipt(dir.path(), Some("codex")).unwrap().unwrap();
        let report = uninstall(dir.path(), selected).unwrap();
        assert_eq!(report.removed, 0);
        assert!(path.exists());
    }
}
