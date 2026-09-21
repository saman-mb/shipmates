//! Regression contracts for command prose that a live run cannot unit-test.
//!
//! These read the canonical `commands/*.md` sources (and the shared preamble
//! they expand from). A missing guardrail is a missing sentence here.

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

    let before_stage4 = body
        .split("## Stage 4")
        .next()
        .expect("Stage 4 heading");
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
    for name in ["shipmates-ship-issue", "shipmates-ship-epic"] {
        let body = command(name);
        let guardrails = body
            .split("### Guardrails")
            .nth(1)
            .unwrap_or_else(|| panic!("{name} missing Guardrails"));
        let lower = guardrails.to_ascii_lowercase();
        assert!(
            lower.contains("disposable") && lower.contains("never itself the verification"),
            "{name} Guardrails must forbid using a merge to a shared/default branch to verify an unverified fix:\n{guardrails}"
        );
    }
}

/// #480: Stage 4.5 must name "no check suite ever appears" as its own
/// condition, distinct from pending or red, and the snippet must encode it.
#[test]
fn ship_issue_stage_4_5_names_empty_check_suite() {
    let body = command("shipmates-ship-issue");
    let stage = section(&body, "## Stage 4.5");
    let lower = stage.to_ascii_lowercase();
    assert!(
        lower.contains("no check suite") && lower.contains("root-cause"),
        "Stage 4.5 must name a permanently empty check suite and require root-cause:\n{stage}"
    );
    assert!(
        stage.contains("empty-check-suite") && stage.contains("-ge 8"),
        "Stage 4.5 snippet must emit empty-check-suite only after a bounded empty wait:\n{stage}"
    );
    assert!(
        !stage.contains("if [ -z \"$st\" ]; then echo empty-check-suite; break"),
        "empty must not break on the first poll; keep looping until the bound:\n{stage}"
    );
    assert!(
        stage.contains("no checks reported"),
        "empty-suite path must classify gh's tabless 'no checks reported' stderr as empty, not as -n success:\n{stage}"
    );
    assert!(
        stage.contains("pending") && stage.contains("continue"),
        "pending must keep waiting and not share the empty-suite cap:\n{stage}"
    );
}

/// #480: Stage 0.5 must probe a pull_request whose **base** is the epic
/// branch — `<EPIC_PR>` (base = main) cannot catch the slash-glob bug.
#[test]
fn ship_epic_stage_0_5_sanity_checks_ci_trigger() {
    let body = command("shipmates-ship-epic");
    let stage = section(&body, "## Stage 0.5");
    let lower = stage.to_ascii_lowercase();
    assert!(
        lower.contains("disposable")
            && lower.contains("base is `<epic_branch>`")
            && lower.contains("check suite"),
        "Stage 0.5 must sanity-check a disposable PR whose base is <EPIC_BRANCH>, not gh pr checks on <EPIC_PR>:\n{stage}"
    );
}

/// #480: citation-verification covers third-party platform claims, not only
/// in-repo file:line citations.
#[test]
fn cost_discipline_verifies_third_party_platform_claims() {
    let cost = cost_preamble();
    let lower = cost.to_ascii_lowercase();
    assert!(
        lower.contains("third-party") && lower.contains("platform"),
        "docs/COST.md must extend citation verification to third-party platform behaviour:\n{cost}"
    );
}
