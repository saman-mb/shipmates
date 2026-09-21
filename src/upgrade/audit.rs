//! Read-only audit of one harness install, reused by `status`, `upgrade --check`
//! and the post-upgrade audit in `upgrade`.
//!
//! The audit composes the receipt and `doctor::diagnose` rather than duplicating
//! either: the receipt decides ownership and version, `doctor` decides content
//! drift, and the two extra checks below catch the cases a self-consistent
//! receipt alone cannot — a payload file that is missing/empty, and (at
//! `AuditDepth::Full`) a claimed tool that fails its `--help` smoke.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::adapters;
use crate::catalog::{CanonicalCommand, CanonicalRole, CanonicalTool, CatalogSource};
use crate::doctor::{self, Severity};
use crate::installer::manifest_db::{self, InstallReceipt};
use crate::installer::plan;
use crate::upgrade::types::{FindingClass, InstallReport, InstallState};

/// How deep an audit goes. `Status` is the read-only surface used by `status`
/// and `upgrade --check`; `Full` additionally smoke-tests every claimed tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuditDepth {
    Status,
    Full,
}

/// Result of auditing one install. `Clean` carries no filing classes (a
/// repairable drift, a partial install and a correct third-party refusal all
/// land here); `Problems` carries the classes that become findings.
#[derive(Debug)]
pub enum AuditOutcome {
    Clean {
        report: InstallReport,
    },
    Problems {
        report: InstallReport,
        detail: String,
        classes: Vec<FindingClass>,
    },
}

impl AuditOutcome {
    /// The install report, whatever the outcome.
    pub fn report(&self) -> &InstallReport {
        match self {
            AuditOutcome::Clean { report } | AuditOutcome::Problems { report, .. } => report,
        }
    }

    /// Consume the outcome and hand back its install report.
    pub fn into_report(self) -> InstallReport {
        match self {
            AuditOutcome::Clean { report } | AuditOutcome::Problems { report, .. } => report,
        }
    }
}

/// Read-only audit of ONE install. Reuses `doctor::diagnose`; never writes.
pub fn audit_install(
    root: &Path,
    harness: &str,
    roles: &[CanonicalRole],
    cmds: &[CanonicalCommand],
    tools: &[CanonicalTool],
    source: &CatalogSource,
    depth: AuditDepth,
) -> anyhow::Result<AuditOutcome> {
    let current = env!("CARGO_PKG_VERSION");
    let (receipt_state, receipt, receipt_error) = plan::read_receipt(root, harness);

    match receipt_state {
        plan::ReceiptState::Invalid => {
            let report = InstallReport {
                root: root.to_string_lossy().into_owned(),
                harness: harness.to_string(),
                receipt_version: None,
                layout: None,
                managed: false,
                drift_count: 0,
                tools: Vec::new(),
                state: InstallState::CorruptReceipt,
            };
            let detail =
                receipt_error.unwrap_or_else(|| "install receipt is unreadable".to_string());
            Ok(AuditOutcome::Problems {
                report,
                detail,
                classes: vec![FindingClass::CorruptReceipt],
            })
        }
        plan::ReceiptState::Missing => {
            let report = InstallReport {
                root: root.to_string_lossy().into_owned(),
                harness: harness.to_string(),
                receipt_version: None,
                layout: None,
                managed: false,
                drift_count: 0,
                tools: Vec::new(),
                state: InstallState::MissingReceipt,
            };
            Ok(AuditOutcome::Clean { report })
        }
        plan::ReceiptState::Valid => {
            let receipt = receipt.expect("valid receipt must be present");
            let mut report = InstallReport {
                root: root.to_string_lossy().into_owned(),
                harness: harness.to_string(),
                receipt_version: Some(receipt.version.clone()),
                layout: Some(receipt.layout.clone()),
                managed: true,
                drift_count: 0,
                tools: claimed_tools(&receipt, tools),
                state: InstallState::Ok,
            };

            // A stale receipt means the refresh did not apply; the on-disk tree
            // is by definition the old version, so further diagnosis would only
            // re-report that same fact as drift. Report the mismatch once.
            if receipt.version != current {
                report.state = InstallState::Drift;
                let detail = format!(
                    "install is at {} but this binary is {current}",
                    receipt.version
                );
                return Ok(AuditOutcome::Problems {
                    report,
                    detail,
                    classes: vec![FindingClass::PostUpgradeMismatch],
                });
            }

            let mut classes: Vec<FindingClass> = Vec::new();
            let mut parts: Vec<String> = Vec::new();
            let mut drift_count = 0usize;

            // 3. doctor's content/payload comparison. A doctor error means the
            // receipt cannot be trusted to describe the tree, which is a corrupt
            // receipt, not a repairable drift.
            match doctor::diagnose(root, harness, roles, cmds, tools, source, "") {
                Ok(doc_report) => {
                    let problems = doc_report
                        .checks
                        .iter()
                        .filter(|check| check.severity == Severity::Problem)
                        .count();
                    drift_count += problems;
                    if doc_report.has_problems() {
                        classes.push(FindingClass::UnrepairableDrift);
                        parts.push(format!("{problems} problem(s) doctor cannot repair"));
                    }
                }
                Err(error) => {
                    classes.push(FindingClass::CorruptReceipt);
                    parts.push(format!("doctor could not diagnose: {error}"));
                }
            }

            // 4. Payload completeness by capability — every payload path the
            // running binary would write must exist and be non-empty.
            let missing = missing_payload_paths(root, harness, roles, cmds, source)?;
            if !missing.is_empty() {
                drift_count += missing.len();
                parts.push(format!("{} missing payload path(s)", missing.len()));
                if !classes.contains(&FindingClass::UnrepairableDrift) {
                    classes.push(FindingClass::UnrepairableDrift);
                }
            }

            // 5. Tool smoke, Full depth only.
            if depth == AuditDepth::Full {
                let mut smoke_failures: Vec<String> = Vec::new();
                for tool in tools {
                    let Some(entry) = tool_entry(root, &receipt, tool) else {
                        continue;
                    };
                    if !smoke_tool(&entry) {
                        smoke_failures.push(tool.name.clone());
                    }
                }
                if !smoke_failures.is_empty() {
                    classes.push(FindingClass::ToolFailure);
                    parts.push(format!(
                        "claimed tool(s) failed their --help smoke: {}",
                        smoke_failures.join(", ")
                    ));
                }
            }

            report.drift_count = drift_count;
            if classes.is_empty() {
                Ok(AuditOutcome::Clean { report })
            } else {
                report.state = InstallState::Drift;
                let detail = if parts.is_empty() {
                    "post-upgrade audit found problems".to_string()
                } else {
                    parts.join("; ")
                };
                Ok(AuditOutcome::Problems {
                    report,
                    detail,
                    classes,
                })
            }
        }
    }
}

/// Tool names the receipt claims, from its owned files. Sorted for stable output.
fn claimed_tools(receipt: &InstallReceipt, tools: &[CanonicalTool]) -> Vec<String> {
    let mut claimed: Vec<String> = Vec::new();
    for tool in tools {
        if receipt
            .files
            .iter()
            .any(|file| tool.owns_path(Path::new(&file.path)))
        {
            claimed.push(tool.name.clone());
        }
    }
    claimed.sort();
    claimed
}

/// The installed entry script for a tool, if the receipt claims one.
///
/// Prefers a file whose stem is the tool name (the native single-file form), then
/// a runnable-looking asset, then any claimed file — mirroring the two shapes an
/// adapter can emit.
fn tool_entry(root: &Path, receipt: &InstallReceipt, tool: &CanonicalTool) -> Option<PathBuf> {
    let mut claimed: Vec<&str> = receipt
        .files
        .iter()
        .map(|file| file.path.as_str())
        .filter(|path| tool.owns_path(Path::new(path)))
        .collect();
    claimed.sort();

    if let Some(path) = claimed.iter().find(|path| {
        Path::new(path).file_stem().and_then(|s| s.to_str()) == Some(tool.name.as_str())
    }) {
        return Some(root.join(path));
    }
    claimed
        .iter()
        .find(|path| {
            matches!(
                Path::new(path).extension().and_then(|e| e.to_str()),
                Some("py" | "ts" | "js")
            )
        })
        .or_else(|| claimed.first())
        .map(|path| root.join(path))
}

/// Run one installed entry script with `--help`, capped at 10s. A spawn failure
/// (missing interpreter, unreadable script) counts as a failed smoke.
fn smoke_tool(entry: &Path) -> bool {
    run_smoke(entry, Duration::from_secs(10))
}

fn run_smoke(entry: &Path, timeout: Duration) -> bool {
    let mut command = match entry.extension().and_then(|e| e.to_str()) {
        Some("py") => {
            let mut cmd = Command::new("python3");
            cmd.arg(entry);
            cmd
        }
        Some("ts") | Some("js") => {
            let mut cmd = Command::new("node");
            cmd.arg(entry);
            cmd
        }
        _ => Command::new(entry),
    };
    command.arg("--help");

    let mut child = match command.stdout(Stdio::null()).stderr(Stdio::null()).spawn() {
        Ok(child) => child,
        Err(_) => return false,
    };
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.success(),
            Ok(None) => {}
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return false;
            }
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return false;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Payload paths (relative to the install root, container prefix stripped) that
/// are missing or empty on disk. Uses the same global relocation the installer
/// applies, so a global antigravity/pi install is checked at its real paths.
fn missing_payload_paths(
    root: &Path,
    harness: &str,
    roles: &[CanonicalRole],
    cmds: &[CanonicalCommand],
    source: &CatalogSource,
) -> anyhow::Result<Vec<String>> {
    let adapter = adapters::select(harness)?;
    let steering = source.steering_for_target(root)?;
    let built = adapters::build_payload(adapter.as_ref(), roles, cmds, steering.as_deref())?;
    let relocated = manifest_db::relocate_payload(harness, adapter.container(), &built);
    let prefix = format!("{}/", adapter.container());

    let mut missing: Vec<String> = Vec::new();
    for key in relocated.keys() {
        let rel = key
            .strip_prefix(&prefix)
            .ok_or_else(|| anyhow::anyhow!("payload path outside install container: {key}"))?;
        let path = root.join(rel);
        match std::fs::metadata(&path) {
            Ok(meta) if meta.len() > 0 => {}
            _ => missing.push(rel.to_string()),
        }
    }
    missing.sort();
    Ok(missing)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::installer::manifest_db::{InstallReceipt, ReceiptFile, ReceiptRepository};

    fn dummy_sha() -> String {
        "a".repeat(64)
    }

    fn save_receipt(root: &Path, version: &str, files: Vec<ReceiptFile>) {
        let receipt = InstallReceipt::new(
            version,
            "claude-code",
            "skills",
            vec![".claude".to_string()],
            files,
        )
        .unwrap();
        ReceiptRepository::new(root).save(&receipt).unwrap();
    }

    fn scrub_tool() -> CanonicalTool {
        CanonicalTool {
            name: "shipmates-scrub".to_string(),
            description: String::new(),
            body: String::new(),
            assets: vec![(
                "scrub.py".to_string(),
                "#!/usr/bin/env python3\n".to_string(),
            )],
            requires: Vec::new(),
            source: PathBuf::new(),
        }
    }

    #[test]
    fn missing_receipt_is_clean_missing_state() {
        let dir = tempfile::tempdir().unwrap();
        let outcome = audit_install(
            dir.path(),
            "claude-code",
            &[],
            &[],
            &[],
            &CatalogSource::Embedded,
            AuditDepth::Status,
        )
        .unwrap();
        match outcome {
            AuditOutcome::Clean { report } => {
                assert_eq!(report.state, InstallState::MissingReceipt);
                assert!(!report.managed);
                assert_eq!(report.receipt_version, None);
            }
            other => panic!("expected Clean, got {other:?}"),
        }
    }

    #[test]
    fn corrupt_receipt_is_reported_not_panicked() {
        let dir = tempfile::tempdir().unwrap();
        let receipts_dir = ReceiptRepository::new(dir.path()).receipts_dir().unwrap();
        std::fs::create_dir_all(&receipts_dir).unwrap();
        std::fs::write(receipts_dir.join("claude-code.json"), "{ not json").unwrap();

        let outcome = audit_install(
            dir.path(),
            "claude-code",
            &[],
            &[],
            &[],
            &CatalogSource::Embedded,
            AuditDepth::Status,
        )
        .unwrap();
        match outcome {
            AuditOutcome::Problems {
                report, classes, ..
            } => {
                assert_eq!(classes, vec![FindingClass::CorruptReceipt]);
                assert_eq!(report.state, InstallState::CorruptReceipt);
                assert!(!report.managed);
            }
            other => panic!("expected Problems, got {other:?}"),
        }
    }

    #[test]
    fn version_mismatch_is_post_upgrade_mismatch() {
        let dir = tempfile::tempdir().unwrap();
        save_receipt(dir.path(), "0.11.0", vec![]);

        let outcome = audit_install(
            dir.path(),
            "claude-code",
            &[],
            &[],
            &[],
            &CatalogSource::Embedded,
            AuditDepth::Full,
        )
        .unwrap();
        match outcome {
            AuditOutcome::Problems {
                report, classes, ..
            } => {
                assert_eq!(classes, vec![FindingClass::PostUpgradeMismatch]);
                assert_eq!(report.state, InstallState::Drift);
                assert_eq!(report.receipt_version.as_deref(), Some("0.11.0"));
            }
            other => panic!("expected Problems, got {other:?}"),
        }
    }

    #[test]
    fn status_depth_never_runs_tool_smoke() {
        let dir = tempfile::tempdir().unwrap();
        let scrub = scrub_tool();
        let file = ReceiptFile {
            path: ".claude/skills/shipmates-scrub/scrub.py".to_string(),
            sha256: dummy_sha(),
        };
        save_receipt(dir.path(), env!("CARGO_PKG_VERSION"), vec![file]);
        let tools = std::slice::from_ref(&scrub);

        let status = audit_install(
            dir.path(),
            "claude-code",
            &[],
            &[],
            tools,
            &CatalogSource::Embedded,
            AuditDepth::Status,
        )
        .unwrap();
        match status {
            AuditOutcome::Problems { classes, .. } => {
                assert!(!classes.contains(&FindingClass::ToolFailure));
            }
            AuditOutcome::Clean { .. } => {}
        }

        // At Full depth the same install does smoke its claimed tool, which is
        // missing on disk and therefore fails — proving the smoke is wired in.
        let full = audit_install(
            dir.path(),
            "claude-code",
            &[],
            &[],
            tools,
            &CatalogSource::Embedded,
            AuditDepth::Full,
        )
        .unwrap();
        match full {
            AuditOutcome::Problems { classes, .. } => {
                assert!(classes.contains(&FindingClass::ToolFailure));
            }
            AuditOutcome::Clean { .. } => panic!("expected ToolFailure at Full depth"),
        }
    }
}
