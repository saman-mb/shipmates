//! `shipmates doctor` — diagnose an install's health and, with `--fix`, repair it.
//!
//! Read-only by default: `diagnose` inspects the on-disk tree against the payload
//! the running binary would install and reports what is healthy, stale, missing or
//! superseded. `fix` repairs only paths claimed by a valid receipt; without one,
//! it may restore missing files but never overwrites existing content. It then
//! re-diagnoses and hands back the fresh report.

use crate::adapters::{self, Adapter};
use crate::catalog::{CanonicalCommand, CanonicalRole, CanonicalTool};
use crate::digest;
use crate::installer::{manifest_db, migrate, plan};
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
) -> Result<Report> {
    let adapter = adapters::select(harness)?;
    let steering = crate::catalog::steering_for_target(target_dir, Path::new("."))
        .map_err(|e| anyhow::anyhow!(e))?;
    let built = adapters::build_payload(
        adapter.as_ref(),
        roles,
        cmds,
        steering.as_deref(),
    )?;
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
    let mut expected = strip_container(built, adapter.container());
    crate::steering::adjust_expected_map(target_dir, harness, &mut expected);
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
        let mut detail = format!(
            "{} core file(s) missing: {}",
            missing.len(),
            missing.join(", ")
        );
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
                    || Path::new(k)
                        .file_stem()
                        .and_then(|s| s.to_str())
                        == Some(t.name.as_str())
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
        if any_on_disk
            && receipt.is_some()
            && !files.iter().any(|(k, _)| claimed(k))
        {
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
    let (severity, detail) = if !tool_missing.is_empty() || !tool_unreadable.is_empty() || !tool_orphaned.is_empty() {
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
            detail.push_str(&format!(
                "; orphaned: {}",
                tool_orphaned.join(", ")
            ));
        }
        (Severity::Problem, detail)
    } else if installed.is_empty() && tool_drift.is_empty() {
        (
            Severity::Ok,
            "no optional tools installed — tools are opt-in".to_string(),
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

/// Repair an install: migrate superseded commands, then restore any missing or
/// drifted crew/skill files, backing up everything it touches. Re-diagnoses and
/// returns the fresh report.
///
/// With `no_migrate`, the legacy-command sweep is skipped — parity with
/// `install --no-migrate`: missing/drifted files are still restored, but a
/// superseded `commands/<name>.md` is left in place.
pub fn fix(
    target_dir: &Path,
    harness: &str,
    roles: &[CanonicalRole],
    cmds: &[CanonicalCommand],
    tools: &[CanonicalTool],
    no_migrate: bool,
) -> Result<Report> {
    let adapter = adapters::select(harness)?;
    let steering = crate::catalog::steering_for_target(target_dir, Path::new("."))
        .map_err(|e| anyhow::anyhow!(e))?;
    let built = adapters::build_payload(
        adapter.as_ref(),
        roles,
        cmds,
        steering.as_deref(),
    )?;
    let mut expected = strip_container(&built, adapter.container());
    crate::steering::adjust_expected_map(target_dir, harness, &mut expected);
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
    let tool_prefix = format!("{}/", adapter.container());
    let tool_expected: BTreeMap<String, String> = adapter
        .build_tools(tools)
        .into_iter()
        .filter_map(|(k, v)| k.strip_prefix(&tool_prefix).map(|r| (r.to_string(), v)))
        .collect();
    for rel in tool_expected.keys() {
        manifest_db::resolve_target_relative(target_dir, Path::new(rel))?;
    }
    let mut repair_expected = expected.clone();
    // Only pull optional-tool files into the repair set when the receipt
    // actually claims them. A no-tools install has no tool files to restore,
    // and listing every uninstalled tool as "skipped" is alarming noise (#267).
    // When some tools are installed, only their files are included so that
    // uninstalled tools do not appear in the skipped report either.
    if let Some(receipt) = receipt.as_ref() {
        for (k, v) in tool_expected {
            if receipt.file(&k).is_some() {
                repair_expected.insert(k, v);
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
                    if receipt_state != plan::ReceiptState::Valid || !owned {
                        skipped.push(rel.clone());
                        continue;
                    }
                    // Preserve arbitrary bytes before replacing drift, then verify
                    // the backup byte-for-byte. This is required for --fix too:
                    // payload files are text, user files need not be.
                    let backup_path = backup_root.join(rel);
                    let backup_relative = backup_path.strip_prefix(target_dir).map_err(|error| {
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
                    if receipt_state == plan::ReceiptState::Valid && !owned {
                        skipped.push(rel.clone());
                        continue;
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
        return Err(combine_rollback_error(
            combine_rollback_error(error, repair_rollback),
            migration_rollback,
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
                let path =
                    manifest_db::resolve_target_relative(target_dir, Path::new(&file.path))?;
                file.sha256 = digest::compute_sha256(&path)?;
            }
        }
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
        return Err(combine_rollback_error(
            combine_rollback_error(error, repair_rollback),
            migration_rollback,
        ));
    }

    // 3. Re-diagnose and hand back the fresh report — reusing the single built
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
        let tool_files = strip_container(
            &adapter.build_tools(&tools),
            adapter.container(),
        );
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
        let tools_check = report.checks.iter().find(|check| check.name == "Tools").unwrap();
        assert_eq!(tools_check.severity, Severity::Warn);
        assert!(tools_check.detail.contains("cannot repair without receipt ownership"));
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
        let mut receipt_rel = files
            .keys()
            .find(|k| k.ends_with(".sha256"))
            .cloned();
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
        let install =
            crate::installer::plan::InstallPlan::from_payload(
                adapter.as_ref(),
                "opencode",
                built,
                tool_built,
            )
            .unwrap();
        let receipt = install.receipt_for(install.files.keys().cloned()).unwrap();
        crate::installer::plan::save_receipt(target, &receipt).unwrap();

        let report = diagnose(target, "opencode", &roles, &cmds, &tools).unwrap();
        let tools_check = report
            .checks
            .iter()
            .find(|c| c.name == "Tools")
            .unwrap();
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
        let install =
            crate::installer::plan::InstallPlan::from_payload(
                adapter.as_ref(),
                "claude-code",
                adapter.build(&roles, &cmds).unwrap(),
                tool_built,
            )
            .unwrap();
        let receipt = install.receipt_for(badge_keys).unwrap();
        crate::installer::plan::save_receipt(target, &receipt).unwrap();

        let report = diagnose(target, "claude-code", &roles, &cmds, &tools).unwrap();
        let tools_check = report
            .checks
            .iter()
            .find(|c| c.name == "Tools")
            .unwrap();
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
