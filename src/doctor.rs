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
use crate::installer::{atomic_write, manifest_db, migrate, plan};
use anyhow::{bail, Result};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;

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
    let built = adapter.build(roles, cmds)?;
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
    let expected = strip_container(built, adapter.container());
    let version = env!("CARGO_PKG_VERSION");
    let mut checks = Vec::new();

    match plan::read_receipt(target_dir, harness) {
        (plan::ReceiptState::Valid, _, _) => checks.push(Check {
            name: "Ownership".into(),
            severity: Severity::Ok,
            detail: "install receipt is valid".into(),
            fixable: true,
        }),
        (plan::ReceiptState::Missing, _, _) => checks.push(Check {
            name: "Ownership".into(),
            severity: Severity::Warn,
            detail: "install receipt missing; ownership is unknown, existing files will be left untouched".into(),
            fixable: false,
        }),
        (plan::ReceiptState::Invalid, _, error) => checks.push(Check {
            name: "Ownership".into(),
            severity: Severity::Problem,
            detail: format!(
                "install receipt is invalid; refusing ownership-based repair: {}",
                error.unwrap_or_else(|| "unknown receipt error".into())
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
    let migration_items = migrate::plan(target_dir, built, adapter.container());
    let (owned, unmanaged): (Vec<&migrate::MigrationItem>, Vec<&migrate::MigrationItem>) =
        migration_items.iter().partition(|i| {
            let name = i
                .legacy_path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or_default();
            migrate::is_shipmates_owned(&target_dir.join(&i.legacy_path), name)
        });
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
    let mut drifted: Vec<String> = Vec::new();
    let mut unreadable: Vec<String> = Vec::new();
    for (rel, want) in &expected {
        match std::fs::read_to_string(target_dir.join(rel)) {
            Ok(on_disk) => {
                if digest::hash(&on_disk) != digest::hash(want) {
                    drifted.push(rel.clone());
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => unreadable.push(rel.clone()),
        }
    }
    if unreadable.is_empty() && drifted.is_empty() {
        checks.push(Check {
            name: "Content".into(),
            severity: Severity::Ok,
            detail: format!("every installed file matches shipmates v{}", version),
            fixable: false,
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

    // 5. Tool status — which opt-in tool skills are installed, and consistent.
    let prefix = format!("{}/", adapter.container());
    let tool_expected: BTreeMap<String, String> = adapter
        .build_tools(tools)
        .into_iter()
        .filter_map(|(k, v)| k.strip_prefix(&prefix).map(|r| (r.to_string(), v)))
        .collect();
    let mut installed: Vec<String> = Vec::new();
    let mut tool_drift: Vec<String> = Vec::new();
    let mut tool_unreadable: Vec<String> = Vec::new();
    for t in tools {
        let files: Vec<(&String, &String)> = tool_expected
            .iter()
            .filter(|(k, _)| k.split('/').any(|s| s == t.name))
            .collect();
        if files.is_empty() {
            continue;
        }
        let complete = files.iter().all(|(k, _)| target_dir.join(k).exists());
        if complete {
            installed.push(t.name.clone());
        }
        for (k, want) in &files {
            match std::fs::read_to_string(target_dir.join(k)) {
                Ok(on_disk) => {
                    if complete && digest::hash(&on_disk) != digest::hash(want) {
                        tool_drift.push(t.name.clone());
                        break;
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => {
                    tool_unreadable.push(t.name.clone());
                    break;
                }
            }
        }
    }
    let (severity, detail) = if !tool_unreadable.is_empty() {
        tool_unreadable.sort();
        tool_drift.sort();
        let mut details = format!(
            "installed: {}; unreadable: {}",
            installed.join(", "),
            tool_unreadable.join(", ")
        );
        if !tool_drift.is_empty() {
            details.push_str(&format!("; drifted: {}", tool_drift.join(", ")));
        }
        (Severity::Problem, details)
    } else if installed.is_empty() {
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
                "installed: {}; drifted: {}",
                installed.join(", "),
                tool_drift.join(", ")
            ),
        )
    };
    checks.push(Check {
        name: "Tools".into(),
        severity,
        detail,
        fixable: !tool_drift.is_empty() && tool_unreadable.is_empty(),
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
    let built = adapter.build(roles, cmds)?;
    let expected = strip_container(&built, adapter.container());
    let repository = manifest_db::ReceiptRepository::new(target_dir);
    repository.load_all()?;
    let (receipt_state, mut receipt, receipt_error) = plan::read_receipt(target_dir, harness);
    if receipt_state == plan::ReceiptState::Invalid {
        bail!(
            "install receipt for harness {} is invalid; refusing doctor --fix: {}",
            harness,
            receipt_error.unwrap_or_else(|| "unknown receipt error".into())
        );
    }
    for rel in expected.keys() {
        manifest_db::resolve_target_relative(target_dir, Path::new(rel))?;
    }
    let backup_root = migrate::new_backup_root(target_dir);

    // 1. Migrate any superseded command files (backed up before removal), unless
    // the caller opted out with `--no-migrate`.
    if !no_migrate {
        let mut items = if receipt_state == plan::ReceiptState::Valid {
            migrate::plan(target_dir, &built, adapter.container())
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
            if !report.migrated.is_empty() {
                println!(
                    "Migrated {} superseded command(s) → skills (backup: {})",
                    report.migrated.len(),
                    backup_root.display()
                );
            }
        }
    }

    // 2. Write any missing or drifted crew/skill files, backing up what we overwrite.
    let mut restored = 0usize;
    let mut backed_up = 0usize;
    let mut skipped: Vec<String> = Vec::new();
    let mut repaired: BTreeSet<String> = BTreeSet::new();
    for (rel, want) in &expected {
        let path = manifest_db::resolve_target_relative(target_dir, Path::new(rel))?;
        let owned = receipt
            .as_ref()
            .and_then(|current| current.file(rel))
            .is_some();
        match std::fs::read_to_string(&path) {
            Ok(on_disk) => {
                if digest::hash(&on_disk) == digest::hash(want) {
                    continue; // already current — nothing to restore
                }
                if receipt_state != plan::ReceiptState::Valid || !owned {
                    skipped.push(rel.clone());
                    continue;
                }
                // Drifted: back up the user's file and VERIFY the copy exists
                // before overwriting, mirroring `migrate::apply`. If the backup
                // can't be written we skip this file rather than destroy the
                // customization with no recoverable copy.
                let backup_path = backup_root.join(rel);
                if atomic_write(&backup_path, &on_disk).is_err() || !backup_path.exists() {
                    skipped.push(rel.clone());
                    continue;
                }
                backed_up += 1;
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                if receipt_state == plan::ReceiptState::Valid && !owned {
                    skipped.push(rel.clone());
                    continue;
                }
            }
            Err(_) => {
                // Present but unreadable (chmod 000, non-UTF-8, …). `read_to_string`
                // conflates this with "missing", but the two are not the same: we
                // cannot hash the file to tell a drifted copy from a
                // correct-but-unreadable one, and our text-based backup path cannot
                // faithfully preserve arbitrary bytes. Overwriting here would
                // destroy an existing file with no recoverable copy, so — the same
                // "never destroy without a verified backup" rule `migrate::apply`
                // follows — we leave it exactly as-is and report it skipped.
                skipped.push(rel.clone());
                continue;
            }
        }
        atomic_write(&path, want)?;
        restored += 1;
        repaired.insert(rel.clone());
    }
    if restored > 0 {
        // A backup dir is only created for drifted overwrites; restoring only
        // missing files writes no backup, so don't advertise one that isn't there.
        if backed_up > 0 {
            println!(
                "Restored {} crew/skill file(s) to shipmates v{} (backup: {})",
                restored,
                env!("CARGO_PKG_VERSION"),
                backup_root.display()
            );
        } else {
            println!(
                "Restored {} crew/skill file(s) to shipmates v{}",
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

    if let Some(current) = receipt.as_mut() {
        if restored > 0 {
            for file in &mut current.files {
                if repaired.contains(&file.path) {
                    let path =
                        manifest_db::resolve_target_relative(target_dir, Path::new(&file.path))?;
                    file.sha256 = digest::compute_sha256(&path)?;
                }
            }
            current.version = env!("CARGO_PKG_VERSION").into();
            current.validate()?;
            repository.save(current)?;
        }
    }

    // 3. Re-diagnose and hand back the fresh report — reusing the single built
    // payload rather than rebuilding it.
    diagnose_built(target_dir, harness, adapter.as_ref(), &built, tools)
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
}
