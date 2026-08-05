//! `shipmates doctor` — diagnose an install's health and, with `--fix`, repair it.
//!
//! Read-only by default: `diagnose` inspects the on-disk tree against the payload
//! the running binary would install and reports what is healthy, stale, missing or
//! superseded. `fix` migrates superseded commands (backed up first) and rewrites
//! any missing or drifted crew/skill files (backing up whatever it overwrites),
//! then re-diagnoses and hands back the fresh report.

use crate::adapters::{self, Adapter};
use crate::catalog::{CanonicalCommand, CanonicalRole, CanonicalTool};
use crate::digest;
use crate::installer::{atomic_write, migrate};
use anyhow::Result;
use std::collections::{BTreeMap, BTreeSet};
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

/// The files a healthy install must contain, keyed by their on-disk path relative
/// to the target directory (the `<container>/` prefix stripped, exactly as the
/// installer writes them).
fn expected_files(
    adapter: &dyn Adapter,
    roles: &[CanonicalRole],
    cmds: &[CanonicalCommand],
) -> Result<BTreeMap<String, String>> {
    let prefix = format!("{}/", adapter.container());
    Ok(adapter
        .build(roles, cmds)?
        .into_iter()
        .filter_map(|(k, v)| k.strip_prefix(&prefix).map(|rel| (rel.to_string(), v)))
        .collect())
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
    let expected = expected_files(adapter.as_ref(), roles, cmds)?;
    let built = adapter.build(roles, cmds)?; // container-prefixed, for migrate::plan
    let version = env!("CARGO_PKG_VERSION");
    let mut checks = Vec::new();

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
    let migration_items = migrate::plan(target_dir, &built, adapter.container());
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
    for (rel, want) in &expected {
        if let Ok(on_disk) = std::fs::read_to_string(target_dir.join(rel)) {
            if digest::hash(&on_disk) != digest::hash(want) {
                drifted.push(rel.clone());
            }
        }
    }
    if drifted.is_empty() {
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

    // 5. Tool status — which opt-in tool skills are installed, and consistent.
    let prefix = format!("{}/", adapter.container());
    let tool_expected: BTreeMap<String, String> = adapter
        .build_tools(tools)
        .into_iter()
        .filter_map(|(k, v)| k.strip_prefix(&prefix).map(|r| (r.to_string(), v)))
        .collect();
    let mut installed: Vec<String> = Vec::new();
    let mut tool_drift: Vec<String> = Vec::new();
    for t in tools {
        let files: Vec<(&String, &String)> = tool_expected
            .iter()
            .filter(|(k, _)| k.split('/').any(|s| s == t.name))
            .collect();
        if files.is_empty() || !files.iter().all(|(k, _)| target_dir.join(k).exists()) {
            continue;
        }
        installed.push(t.name.clone());
        for (k, want) in &files {
            if let Ok(on_disk) = std::fs::read_to_string(target_dir.join(k)) {
                if digest::hash(&on_disk) != digest::hash(want) {
                    tool_drift.push(t.name.clone());
                    break;
                }
            }
        }
    }
    let (severity, detail) = if installed.is_empty() {
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
        fixable: !tool_drift.is_empty(),
    });

    // 6. Orphan/unmanaged detection — deferred.
    // #190: true orphan detection needs the receipt manifest to know which files
    // shipmates wrote; until it lands we cannot tell a user's own file from a
    // stale one, so this is an informational note rather than a check that acts.
    checks.push(Check {
        name: "Unmanaged files".into(),
        severity: Severity::Ok,
        detail: "orphan detection is deferred until the receipt manifest lands (#190)".into(),
        fixable: false,
    });

    Ok(Report { checks })
}

/// Repair an install: migrate superseded commands, then restore any missing or
/// drifted crew/skill files, backing up everything it touches. Re-diagnoses and
/// returns the fresh report.
pub fn fix(
    target_dir: &Path,
    harness: &str,
    roles: &[CanonicalRole],
    cmds: &[CanonicalCommand],
    tools: &[CanonicalTool],
) -> Result<Report> {
    let adapter = adapters::select(harness)?;
    let expected = expected_files(adapter.as_ref(), roles, cmds)?;
    let built = adapter.build(roles, cmds)?;
    let backup_root = migrate::new_backup_root(target_dir);

    // 1. Migrate any superseded command files (backed up before removal).
    let items = migrate::plan(target_dir, &built, adapter.container());
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

    // 2. Write any missing or drifted crew/skill files, backing up what we overwrite.
    let mut restored = 0usize;
    for (rel, want) in &expected {
        let path = target_dir.join(rel);
        let needs = match std::fs::read_to_string(&path) {
            Ok(on_disk) => digest::hash(&on_disk) != digest::hash(want),
            Err(_) => true, // missing
        };
        if !needs {
            continue;
        }
        // Back up an existing (drifted) file before overwriting it.
        if let Ok(on_disk) = std::fs::read_to_string(&path) {
            let _ = atomic_write(&backup_root.join(rel), &on_disk);
        }
        atomic_write(&path, want)?;
        restored += 1;
    }
    if restored > 0 {
        println!(
            "Restored {} crew/skill file(s) to shipmates v{} (backup: {})",
            restored,
            env!("CARGO_PKG_VERSION"),
            backup_root.display()
        );
    }

    // 3. Re-diagnose and hand back the fresh report.
    diagnose(target_dir, harness, roles, cmds, tools)
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
            loop_max: 0,
            stages: vec![],
            narrative: "n".into(),
            invocation: "".into(),
            board: "".into(),
            source: PathBuf::from(""),
        }
    }

    fn install_healthy(target: &Path, roles: &[CanonicalRole], cmds: &[CanonicalCommand]) {
        let adapter = adapters::select("claude-code").unwrap();
        for (rel, content) in expected_files(adapter.as_ref(), roles, cmds).unwrap() {
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
    fn test_fix_restores_missing_and_migrates_legacy() {
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

        let after = fix(target, "claude-code", &roles, &cmds, &[]).unwrap();
        assert!(
            !after.has_problems(),
            "fix should leave a healthy install: {:?}",
            after
        );
        assert!(target.join(".claude/agents/devops-engineer.md").exists());
        assert!(!target.join(".claude/commands/ship-issue.md").exists());
    }
}
