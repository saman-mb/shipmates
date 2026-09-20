//! Regression contracts for command prose that a live run cannot unit-test.
//!
//! These read the canonical `commands/*.md` sources (and the shared preamble
//! they expand from). A missing step or guardrail is a missing sentence here.

fn command(name: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("commands")
        .join(format!("{name}.md"));
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
}

fn cost_preamble() -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/COST.md");
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
}

fn section<'a>(body: &'a str, heading: &str) -> &'a str {
    let start = body
        .find(heading)
        .unwrap_or_else(|| panic!("missing heading {heading}"));
    let rest = &body[start + heading.len()..];
    let end = rest.find("\n## ").unwrap_or(rest.len());
    &rest[..end]
}

/// #516: dangling keep issues must be reconciled into existing open epics
/// *before* Stage 4 invents a new bundle for them.
#[test]
fn consolidate_reconciles_dangling_issues_into_open_epics_before_bundling() {
    let body = command("shipmates-consolidate-issues");

    let stage0 = section(&body, "## Stage 0");
    assert!(
        stage0.contains("open epic") || stage0.contains("open epics"),
        "Stage 0 must inventory the existing open-epic set so later stages can match against it:\n{stage0}"
    );

    let before_stage4 = body.split("## Stage 4").next().expect("Stage 4 heading");
    assert!(
        before_stage4.contains("target_epic")
            || before_stage4.contains("reattach")
            || before_stage4.contains("existing epic"),
        "reconciliation (match dangling issues to an existing epic) must happen before Stage 4 bundling"
    );

    let stage4 = section(&body, "## Stage 4");
    assert!(
        !stage4.contains("Group every `keep` issue into **bundles**"),
        "Stage 4 must not sweep every keep into a new bundle — epic matches are migrate, not keep:\n{stage4}"
    );
    assert!(
        stage4.contains("existing epic") || stage4.contains("reattach") || stage4.contains("migrate"),
        "Stage 4 must say that epic-matched issues never reach new-bundle theming:\n{stage4}"
    );

    let report = section(&body, "## Stage 5");
    assert!(
        report.contains("reattach"),
        "the report must surface how many issues were reattached to existing epics vs bundled fresh:\n{report}"
    );
}

/// #480: verifying a fix must never use a merge to a shared/default branch
/// as the verification step itself.
#[test]
fn ci_verification_never_merges_to_the_default_branch_to_test() {
    for name in ["shipmates-issue", "shipmates-epic"] {
        let body = command(name);
        let guardrails = body
            .split("### Guardrails")
            .nth(1)
            .unwrap_or_else(|| panic!("{name} missing Guardrails"));
        assert!(
            guardrails.to_ascii_lowercase().contains("disposable")
                || guardrails.contains("never itself the verification")
                || guardrails.contains("merge is never itself"),
            "{name} Guardrails must forbid using a merge to a shared/default branch to verify an unverified fix:\n{guardrails}"
        );
    }
}

/// #480: Stage 4.5 must name "no check suite ever appears" as its own
/// condition, distinct from pending or red.
#[test]
fn ship_issue_stage_4_5_names_empty_check_suite() {
    let body = command("shipmates-issue");
    let stage = section(&body, "## Stage 4.5");
    let lower = stage.to_ascii_lowercase();
    assert!(
        lower.contains("no check suite")
            || lower.contains("zero checks")
            || lower.contains("no checks reported")
            || lower.contains("empty check"),
        "Stage 4.5 must name a permanently empty check suite as its own failure mode, not only pending vs red:\n{stage}"
    );
    assert!(
        !lower.contains("poll indefinitely")
            || lower.contains("bounded")
            || lower.contains("root-cause"),
        "empty-suite handling must root-cause rather than poll forever:\n{stage}"
    );
}

/// #480: Stage 0.5 must cheaply confirm a pull_request event actually
/// produces a check suite against the new epic branch.
#[test]
fn ship_epic_stage_0_5_sanity_checks_ci_trigger() {
    let body = command("shipmates-epic");
    let stage = section(&body, "## Stage 0.5");
    let lower = stage.to_ascii_lowercase();
    assert!(
        lower.contains("check suite")
            || lower.contains("pr checks")
            || lower.contains("ci-trigger")
            || lower.contains("ci trigger"),
        "Stage 0.5 must sanity-check that a pull_request event produces checks on <EPIC_BRANCH>:\n{stage}"
    );
}

/// #480: citation-verification covers third-party platform claims, not only
/// in-repo file:line citations.
#[test]
fn cost_discipline_verifies_third_party_platform_claims() {
    let cost = cost_preamble();
    let lower = cost.to_ascii_lowercase();
    assert!(
        lower.contains("third-party") || lower.contains("third party") || lower.contains("platform"),
        "docs/COST.md must extend citation verification to third-party platform behaviour:\n{cost}"
    );
}
