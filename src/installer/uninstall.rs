//! Fail-closed uninstall driven solely by install receipts.

use crate::digest;
use crate::installer::{
    manifest_db::{self, InstallReceipt, ReceiptRepository},
    plan,
};
use anyhow::{Context, Result, bail};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct LocatedReceipt {
    pub(crate) path: PathBuf,
    pub receipt: InstallReceipt,
}

#[derive(Debug, Clone, Default)]
pub struct UninstallReport {
    pub harness: String,
    pub removed: usize,
    pub warnings: Vec<String>,
    pub receipt_removed: bool,
    /// Outcome of the pi resource-broker teardown, when this uninstall was for pi.
    pub broker: Option<crate::pi_broker::Teardown>,
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
                    // Pi's machine-wide broker outlives the receipt. Returning
                    // None lets main run broker teardown for an explicit
                    // `uninstall --harness pi` even after the receipt is gone.
                    // Every other harness still fails closed.
                    if harness == "pi" {
                        Ok(None)
                    } else {
                        bail!(
                            "No install receipt found for harness {harness}; refusing to uninstall"
                        )
                    }
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

/// Remove only files listed by a valid receipt whose paths and expected bytes
/// still belong to the current harness payload. Other valid receipts claim
/// shared paths; those paths remain. Receipt entries from an older payload are
/// preserved with a warning rather than treated as deletion authority.
pub(crate) fn uninstall(target_dir: &Path, selected: LocatedReceipt) -> Result<UninstallReport> {
    let known_payload = current_payload(&selected.receipt.harness)?;
    uninstall_with_payload(target_dir, selected, &known_payload)
}

pub fn uninstall_with_payload(
    target_dir: &Path,
    selected: LocatedReceipt,
    known_payload: &BTreeMap<String, String>,
) -> Result<UninstallReport> {
    let mut report = UninstallReport {
        harness: selected.receipt.harness.clone(),
        ..UninstallReport::default()
    };

    // The catalog payload is built in workspace shape. Global installs write
    // (and receipts record) relocated paths, so a receipt entry must be looked
    // up against the relocated form of the known payload (#597).
    let known_payload: BTreeMap<String, String> = if manifest_db::is_global_target(target_dir) {
        known_payload
            .iter()
            .map(|(path, content)| {
                let relocated = manifest_db::global_relocate(&selected.receipt.harness, path)
                    .unwrap_or_else(|| path.clone());
                (relocated, content.clone())
            })
            .collect()
    } else {
        known_payload.clone()
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
        let Some(expected) = known_payload.get(&file.path) else {
            report.warnings.push(format!(
                "Warning: old or unknown payload entry preserved: {}",
                rel.display()
            ));
            blocked = true;
            continue;
        };
        if digest::hash_bytes(expected.as_bytes()) != file.sha256 {
            report.warnings.push(format!(
                "Warning: old payload entry preserved (receipt hash does not match current payload): {}",
                rel.display()
            ));
            blocked = true;
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

    if blocked {
        report.warnings.push(format!(
            "Warning: uninstall blocked for {}; managed files were preserved and the receipt was kept",
            report.harness
        ));
        return Ok(report);
    }

    let managed = selected
        .receipt
        .files
        .iter()
        .map(|file| file.path.clone())
        .collect::<std::collections::BTreeSet<_>>();
    for path in plan::unmanaged_files(target_dir, &managed) {
        report.warnings.push(format!(
            "Warning: unmanaged file preserved: {}",
            path.strip_prefix(target_dir).unwrap_or(&path).display()
        ));
    }

    let removed = remove_files_transaction(&removals, |path| fs::remove_file(path))?;

    match repository.remove(&selected.receipt.harness) {
        Ok(true) => {
            report.removed = removed;
            report.receipt_removed = true;
            if removed == 0 {
                report.warnings.push(format!(
                    "Warning: install receipt removed for {} but no managed files were present",
                    report.harness
                ));
            }
        }
        Ok(false) => {
            let rollback = rollback_transaction(&removals);
            return Err(combine_rollback_error(
                anyhow::anyhow!("install receipt disappeared during uninstall"),
                rollback,
            ));
        }
        Err(error) => {
            let rollback = rollback_transaction(&removals);
            return Err(combine_rollback_error(
                error.context("removing install receipt"),
                rollback,
            ));
        }
    }

    // Clean up empty directories left behind by file removal and receipt removal.
    // Walk up from each removed path's parent, removing directories only when
    // they are empty. Best-effort: errors are warnings, not failures.
    let mut all_removed_paths: Vec<PathBuf> = removals.iter().map(|r| r.path.clone()).collect();
    if let Ok(receipt_path) = repository.receipt_path(&selected.receipt.harness) {
        all_removed_paths.push(receipt_path);
    }
    for path in all_removed_paths {
        let mut dir = path.parent().unwrap_or(target_dir).to_path_buf();
        while dir != *target_dir {
            match fs::read_dir(&dir) {
                Ok(mut entries) => {
                    if entries.next().is_none() {
                        // Directory is empty — remove it.
                        if let Err(error) = fs::remove_dir(&dir) {
                            report.warnings.push(format!(
                                "Warning: cannot remove empty dir {}: {}",
                                dir.strip_prefix(target_dir).unwrap_or(&dir).display(),
                                error
                            ));
                        }
                        dir = dir
                            .parent()
                            .map(|p| p.to_path_buf())
                            .unwrap_or_else(|| target_dir.to_path_buf());
                    } else {
                        break;
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    // A sibling path already removed this directory — keep walking up.
                    dir = dir
                        .parent()
                        .map(|p| p.to_path_buf())
                        .unwrap_or_else(|| target_dir.to_path_buf());
                }
                Err(error) => {
                    report.warnings.push(format!(
                        "Warning: cannot read dir {}: {}",
                        dir.strip_prefix(target_dir).unwrap_or(&dir).display(),
                        error
                    ));
                    break;
                }
            }
        }
    }

    // Warn about `.shipmates-backup/` if it still exists — it is owned by a prior
    // `doctor --fix` and uninstall does not remove user files, but the user
    // should know it survives.
    let backup_dir = target_dir.join(crate::installer::migrate::BACKUP_DIR);
    if backup_dir.is_dir() {
        report.warnings.push(format!(
            "Warning: backup directory preserved: {}",
            backup_dir
                .strip_prefix(target_dir)
                .unwrap_or(&backup_dir)
                .display()
        ));
    }

    // Pi's resource broker is machine-wide and outside the install receipt. Stop
    // it only after file removal, and only when no pi extension remains at the
    // agent dir (a project-scope uninstall must not kill a global install's broker).
    if report.harness == "pi" && report.receipt_removed {
        let teardown = crate::pi_broker::teardown_after_uninstall();
        if let Some(line) = teardown.warning_line() {
            report.warnings.push(line);
        }
        report.broker = Some(teardown);
    }

    Ok(report)
}

/// Build complete current payload knowledge for receipt validation. All tools
/// are included deliberately: uninstall has no tool-selection flag, and an
/// old receipt entry must be recognized only when current Shipmates still
/// knows its exact path and bytes.
fn current_payload(harness: &str) -> Result<BTreeMap<String, String>> {
    let source = crate::catalog::CatalogSource::Embedded;
    let roles = source.load_roles()?;
    let commands = source.load_commands()?;
    let tools = source.load_tools()?;
    payload_for(harness, &roles, &commands, &tools, &source.load_steering()?)
}

/// Build the payload uninstall recognizes. `steering` comes from the same
/// catalog source the caller installed from, so a checkout install is still
/// removable by the same command that wrote it.
pub fn payload_for(
    harness: &str,
    roles: &[crate::catalog::CanonicalRole],
    commands: &[crate::catalog::CanonicalCommand],
    tools: &[crate::catalog::CanonicalTool],
    steering: &str,
) -> Result<BTreeMap<String, String>> {
    let adapter = crate::adapters::select(harness)?;
    let plan = crate::installer::plan::InstallPlan::from_payload(
        adapter.as_ref(),
        harness,
        crate::adapters::build_payload(adapter.as_ref(), roles, commands, Some(steering))?,
        adapter.build_tools(tools),
    )?;
    Ok(plan
        .files
        .into_iter()
        .map(|(path, content)| (path.to_string_lossy().into_owned(), content))
        .collect())
}

struct Removal {
    path: PathBuf,
    bytes: Vec<u8>,
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

fn rollback_transaction(removals: &[Removal]) -> Result<()> {
    let removed = removals.iter().collect::<Vec<_>>();
    restore_removals(&removed)
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
    use crate::installer::manifest_db::{LAYOUT_SKILLS, ReceiptFile};
    use tempfile::tempdir;

    fn receipt(target: &Path, harness: &str, path: &str, content: &[u8]) {
        let receipt = InstallReceipt::new(
            "1",
            harness,
            LAYOUT_SKILLS,
            vec![
                Path::new(path)
                    .components()
                    .next()
                    .unwrap()
                    .as_os_str()
                    .to_string_lossy()
                    .into_owned(),
            ],
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

    #[test]
    fn old_receipt_entry_is_preserved_when_not_in_current_payload() {
        let dir = tempdir().unwrap();
        let path = dir.path().join(".claude/agents/old.md");
        crate::installer::atomic_write(&path, "old payload").unwrap();
        receipt(
            dir.path(),
            "claude-code",
            ".claude/agents/old.md",
            b"old payload",
        );
        let selected = select_receipt(dir.path(), Some("claude-code"))
            .unwrap()
            .unwrap();
        let report = uninstall_with_payload(
            dir.path(),
            selected,
            &BTreeMap::from([(
                String::from(".claude/agents/current.md"),
                String::from("current"),
            )]),
        )
        .unwrap();

        assert_eq!(report.removed, 0);
        assert!(!report.receipt_removed);
        assert!(path.exists());
        assert!(
            report
                .warnings
                .iter()
                .any(|warning| warning.contains("old or unknown payload entry"))
        );
    }

    #[test]
    fn current_payload_entry_is_removed_only_when_hash_matches() {
        let dir = tempdir().unwrap();
        let path = dir.path().join(".claude/agents/current.md");
        crate::installer::atomic_write(&path, "current").unwrap();
        receipt(
            dir.path(),
            "claude-code",
            ".claude/agents/current.md",
            b"current",
        );
        let selected = select_receipt(dir.path(), Some("claude-code"))
            .unwrap()
            .unwrap();
        let report = uninstall_with_payload(
            dir.path(),
            selected,
            &BTreeMap::from([(
                String::from(".claude/agents/current.md"),
                String::from("current"),
            )]),
        )
        .unwrap();

        assert_eq!(report.removed, 1);
        assert!(!path.exists());
        assert!(report.receipt_removed);
    }

    #[test]
    fn global_pi_uninstall_removes_relocated_tree_and_preserves_foreign_files() {
        let dir = tempdir().unwrap();
        crate::installer::retired::test_env::with_isolated_home(dir.path(), || {
            let target = dir.path();
            let agent = target.join(".pi/agent/agents/architect.md");
            let foreign = target.join(".pi/agent/settings.json");
            crate::installer::atomic_write(&agent, "crew").unwrap();
            crate::installer::atomic_write(&foreign, "foreign").unwrap();
            receipt(target, "pi", ".pi/agent/agents/architect.md", b"crew");

            let selected = select_receipt(target, Some("pi")).unwrap().unwrap();
            let report = uninstall_with_payload(
                target,
                selected,
                &BTreeMap::from([(
                    String::from(".pi/agents/architect.md"),
                    String::from("crew"),
                )]),
            )
            .unwrap();

            assert_eq!(report.removed, 1);
            assert!(report.receipt_removed);
            assert!(!agent.exists());
            assert!(foreign.exists());
            assert!(
                report.warnings.iter().all(|w| !w.contains("old or unknown")),
                "relocated entries must match the known payload, not be treated as old"
            );
        });
    }

    #[test]
    fn global_devin_uninstall_removes_relocated_tree() {
        let dir = tempdir().unwrap();
        crate::installer::retired::test_env::with_isolated_home(dir.path(), || {
            let target = dir.path();
            let agent = target.join(".config/devin/agents/architect.md");
            crate::installer::atomic_write(&agent, "crew").unwrap();
            receipt(target, "devin", ".config/devin/agents/architect.md", b"crew");

            let selected = select_receipt(target, Some("devin")).unwrap().unwrap();
            let report = uninstall_with_payload(
                target,
                selected,
                &BTreeMap::from([(
                    String::from(".devin/agents/architect.md"),
                    String::from("crew"),
                )]),
            )
            .unwrap();

            assert_eq!(report.removed, 1);
            assert!(report.receipt_removed);
            assert!(!agent.exists());
        });
    }

    #[test]
    fn global_antigravity_uninstall_removes_relocated_tree() {
        let dir = tempdir().unwrap();
        crate::installer::retired::test_env::with_isolated_home(dir.path(), || {
            let target = dir.path();
            let agent = target.join(".gemini/config/agents/sdet/agent.md");
            crate::installer::atomic_write(&agent, "crew").unwrap();
            receipt(
                target,
                "antigravity",
                ".gemini/config/agents/sdet/agent.md",
                b"crew",
            );

            let selected = select_receipt(target, Some("antigravity"))
                .unwrap()
                .unwrap();
            let report = uninstall_with_payload(
                target,
                selected,
                &BTreeMap::from([(
                    String::from(".agents/agents/sdet/agent.md"),
                    String::from("crew"),
                )]),
            )
            .unwrap();

            assert_eq!(report.removed, 1);
            assert!(report.receipt_removed);
            assert!(!agent.exists());
        });
    }

    #[test]
    fn non_global_uninstall_uses_workspace_paths_without_relocation() {
        let dir = tempdir().unwrap();
        let agent = dir.path().join(".pi/agents/architect.md");
        crate::installer::atomic_write(&agent, "crew").unwrap();
        receipt(dir.path(), "pi", ".pi/agents/architect.md", b"crew");

        let selected = select_receipt(dir.path(), Some("pi")).unwrap().unwrap();
        let report = uninstall_with_payload(
            dir.path(),
            selected,
            &BTreeMap::from([(
                String::from(".pi/agents/architect.md"),
                String::from("crew"),
            )]),
        )
        .unwrap();

        assert_eq!(report.removed, 1);
        assert!(report.receipt_removed);
        assert!(!agent.exists());
    }

    #[test]
    fn zero_files_removed_warns_when_receipt_is_removed() {
        let dir = tempdir().unwrap();
        receipt(dir.path(), "claude-code", ".claude/agents/a.md", b"current");

        let selected = select_receipt(dir.path(), Some("claude-code"))
            .unwrap()
            .unwrap();
        let report = uninstall_with_payload(
            dir.path(),
            selected,
            &BTreeMap::from([(
                String::from(".claude/agents/a.md"),
                String::from("current"),
            )]),
        )
        .unwrap();

        assert_eq!(report.removed, 0);
        assert!(report.receipt_removed);
        assert!(report
            .warnings
            .iter()
            .any(|w| w.contains("no managed files were present")));
    }

    #[test]
    fn blocked_uninstall_warns_and_keeps_receipt() {
        let dir = tempdir().unwrap();
        let path = dir.path().join(".claude/agents/a.md");
        crate::installer::atomic_write(&path, "modified").unwrap();
        receipt(dir.path(), "claude-code", ".claude/agents/a.md", b"original");

        let selected = select_receipt(dir.path(), Some("claude-code"))
            .unwrap()
            .unwrap();
        let report = uninstall_with_payload(
            dir.path(),
            selected,
            &BTreeMap::from([(
                String::from(".claude/agents/a.md"),
                String::from("original"),
            )]),
        )
        .unwrap();

        assert_eq!(report.removed, 0);
        assert!(!report.receipt_removed);
        assert!(path.exists());
        assert!(report.warnings.iter().any(|w| w.contains("uninstall blocked")));
    }
}
