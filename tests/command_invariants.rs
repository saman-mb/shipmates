//! Regression contracts for command prose that a live run cannot unit-test.
//!
//! These read the canonical `commands/*.md` sources. A missing step in the
//! workflow is a missing sentence here.

fn command(name: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("commands")
        .join(format!("{name}.md"));
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
