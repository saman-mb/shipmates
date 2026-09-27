//! Turn an install audit into attributable findings, and fingerprint them.
//!
//! This is the issue classification table as code. Only Shipmates-attributable
//! defects become findings: a repairable drift, a partial install that `--fix`
//! repairs, and a correct third-party refusal all arrive as a `Clean` outcome
//! and yield nothing.

use crate::upgrade::audit::AuditOutcome;
use crate::upgrade::types::{Finding, FindingClass};

/// Findings for one install's audit, plus the root-wide refresh status.
///
/// * `refresh_failed` adds one `RefreshFailed` finding; its detail must be
///   non-empty, and a missing/empty detail suppresses the finding rather than
///   filing a hollow one.
/// * Every class carried by a `Problems` outcome becomes a finding.
/// * A `Clean` outcome yields nothing (the non-attributable rows).
pub fn findings_for(
    outcome: &AuditOutcome,
    refresh_failed: bool,
    detail_if_refresh_failed: Option<&str>,
) -> Vec<Finding> {
    let report = outcome.report();
    let mut findings: Vec<Finding> = Vec::new();

    if refresh_failed {
        if let Some(detail) = detail_if_refresh_failed
            && !detail.trim().is_empty()
        {
            findings.push(Finding {
                class: FindingClass::RefreshFailed,
                harness: report.harness.clone(),
                root: report.root.clone(),
                detail: detail.trim().to_string(),
                fingerprint: fingerprint(&report.harness, FindingClass::RefreshFailed),
            });
        }
    }

    if let AuditOutcome::Problems {
        detail, classes, ..
    } = outcome
    {
        for class in classes {
            findings.push(Finding {
                class: *class,
                harness: report.harness.clone(),
                root: report.root.clone(),
                detail: detail.clone(),
                fingerprint: fingerprint(&report.harness, *class),
            });
        }
    }

    findings
}

/// A stable, per-harness/per-class fingerprint:
/// `{harness}|{class}|{version}|{os}/{arch}`.
pub fn fingerprint(harness: &str, class: FindingClass) -> String {
    format!(
        "{}|{}|{}|{}/{}",
        harness,
        class_slug(class),
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH
    )
}

/// The kebab-case slug used in fingerprints and issue titles.
pub(crate) fn class_slug(class: FindingClass) -> &'static str {
    match class {
        FindingClass::RefreshFailed => "refresh-failed",
        FindingClass::UnrepairableDrift => "unrepairable-drift",
        FindingClass::CorruptReceipt => "corrupt-receipt",
        FindingClass::PostUpgradeMismatch => "post-upgrade-mismatch",
        FindingClass::ToolFailure => "tool-failure",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::upgrade::types::{InstallReport, InstallState};

    fn report(harness: &str, root: &str) -> InstallReport {
        InstallReport {
            root: root.to_string(),
            harness: harness.to_string(),
            receipt_version: None,
            layout: None,
            managed: false,
            drift_count: 0,
            tools: Vec::new(),
            state: InstallState::Ok,
        }
    }

    fn clean(harness: &str) -> AuditOutcome {
        AuditOutcome::Clean {
            report: report(harness, "/tmp/example"),
        }
    }

    fn problems(harness: &str, classes: Vec<FindingClass>) -> AuditOutcome {
        AuditOutcome::Problems {
            report: report(harness, "/tmp/example"),
            detail: "post-upgrade audit found problems".to_string(),
            classes,
        }
    }

    // Classification table rows 1, 2 and 7 are the non-attributable outcomes:
    // user-edited drift that `--fix` repairs, a missing/partial install repaired
    // by `--fix`, and a correct third-party refusal. Each arrives as `Clean` and
    // must produce no finding.
    #[test]
    fn repairable_drift_is_not_a_finding() {
        assert!(findings_for(&clean("claude-code"), false, None).is_empty());
    }

    #[test]
    fn repaired_partial_install_is_not_a_finding() {
        assert!(findings_for(&clean("opencode"), false, None).is_empty());
    }

    #[test]
    fn correct_third_party_refusal_is_not_a_finding() {
        assert!(findings_for(&clean("cursor"), false, None).is_empty());
    }

    // Row 3: `doctor --fix` cannot repair — a corrupt receipt and unrepairable
    // drift are both attributable.
    #[test]
    fn corrupt_receipt_is_a_finding() {
        let findings = findings_for(
            &problems("claude-code", vec![FindingClass::CorruptReceipt]),
            false,
            None,
        );
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].class, FindingClass::CorruptReceipt);
        assert_eq!(findings[0].harness, "claude-code");
        assert_eq!(findings[0].root, "/tmp/example");
    }

    #[test]
    fn unrepairable_drift_is_a_finding() {
        let findings = findings_for(
            &problems("opencode", vec![FindingClass::UnrepairableDrift]),
            false,
            None,
        );
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].class, FindingClass::UnrepairableDrift);
    }

    // Row 4: upgrade step / refresh failed.
    #[test]
    fn refresh_failed_is_a_finding_with_detail() {
        let findings = findings_for(&clean("claude-code"), true, Some("refresh failed: exit 1"));
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].class, FindingClass::RefreshFailed);
        assert_eq!(findings[0].detail, "refresh failed: exit 1");
    }

    #[test]
    fn refresh_failed_without_detail_is_suppressed() {
        assert!(findings_for(&clean("claude-code"), true, None).is_empty());
        assert!(findings_for(&clean("claude-code"), true, Some("  ")).is_empty());
    }

    // Row 5: post-upgrade version mismatch, or drift in an untouched install.
    #[test]
    fn post_upgrade_mismatch_is_a_finding() {
        let findings = findings_for(
            &problems("claude-code", vec![FindingClass::PostUpgradeMismatch]),
            false,
            None,
        );
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].class, FindingClass::PostUpgradeMismatch);
    }

    // Row 6: a claimed tool that fails to run.
    #[test]
    fn tool_failure_is_a_finding() {
        let findings = findings_for(
            &problems("claude-code", vec![FindingClass::ToolFailure]),
            false,
            None,
        );
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].class, FindingClass::ToolFailure);
    }

    #[test]
    fn refresh_failure_and_audit_problems_are_both_reported() {
        let findings = findings_for(
            &problems("claude-code", vec![FindingClass::ToolFailure]),
            true,
            Some("refresh failed"),
        );
        assert_eq!(findings.len(), 2);
    }

    #[test]
    fn fingerprint_is_stable_and_independent() {
        let first = fingerprint("claude-code", FindingClass::ToolFailure);
        let second = fingerprint("claude-code", FindingClass::ToolFailure);
        assert_eq!(first, second);
        assert!(first.starts_with("claude-code|tool-failure|"));
        assert!(first.ends_with(&format!(
            "{}/{}",
            std::env::consts::OS,
            std::env::consts::ARCH
        )));

        assert_ne!(
            fingerprint("claude-code", FindingClass::ToolFailure),
            fingerprint("opencode", FindingClass::ToolFailure)
        );
        assert_ne!(
            fingerprint("claude-code", FindingClass::ToolFailure),
            fingerprint("claude-code", FindingClass::CorruptReceipt)
        );
    }
}
