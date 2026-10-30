//! Regression contracts for command prose that a live run cannot unit-test.
//!
//! These read the canonical `commands/*.md` sources (and the shared preamble
//! they expand from). A missing guardrail is a missing sentence here.

use shipmates::adapters;
use shipmates::catalog;

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

/// Command prose is hard-wrapped, so a sentence-level guardrail is asserted
/// against the reflowed text: a deliberate rewrap must never look like a
/// deleted sentence.
fn prose(body: &str) -> String {
    body.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// #565: a dedupe must *consolidate* — the duplicate's substance reaches the
/// named survivor before the duplicate is closed, so the open count falls
/// because the work moved rather than because detail was dropped. The merge is
/// a numbered sub-step of Stage 5, never a `## Stage` heading: the demo-GIF reel
/// is generated from the parsed stage headings (`tools/gen_command_demos.py`).
#[test]
fn consolidate_merges_duplicate_substance_into_the_survivor_before_closing() {
    let body = command("shipmates-consolidate-issues");

    let stage_list = body
        .lines()
        .find(|line| line.starts_with("# /shipmates-consolidate-issues"))
        .expect("H1 stage list");
    let merge = stage_list.find("merge").expect("stage list must name merge");
    assert!(
        stage_list[..merge].contains("migrate") && stage_list[merge..].contains("bundle"),
        "the H1 stage list must place merge between migrate and bundle:\n{stage_list}"
    );

    let stage2 = prose(&section(&body, "## Stage 2"));
    assert!(
        stage2.contains("dedupe map") && stage2.contains("payload to carry over"),
        "Stage 2's dedupe map must carry the content to carry over, not just a pairing:\n{stage2}"
    );
    for carried in [
        "body sections",
        "acceptance criteria",
        "repro steps",
        "comment thread",
    ] {
        assert!(
            stage2.contains(carried),
            "the Stage 2 dedupe-map payload must name {carried} as content to carry over:\n{stage2}"
        );
    }

    let stage5 = section(&body, "## Stage 5");
    let migrated = stage5.find("**Migrate**").expect("Stage 5 migrate step");
    let merged = stage5
        .find("**Merge**")
        .expect("Stage 5 must carry a merge sub-step");
    let closed = stage5.find("**Close**").expect("Stage 5 close step");
    assert!(
        migrated < merged && merged < closed,
        "the merge sub-step must sit between migrate and close in Stage 5's apply order:\n{stage5}"
    );

    let staged = prose(stage5);
    assert!(
        staged.contains("only **after** its merge has landed"),
        "a duplicate must not be closed until its merge has landed:\n{stage5}"
    );
    assert!(
        staged.contains("reported, not closed") && staged.contains("Never close-and-drop"),
        "an unmerged pair is reported, not closed — no close-and-drop path may exist:\n{stage5}"
    );
    assert!(
        staged.contains("closed as a duplicate of `#X` (substance merged into `#X`)"),
        "the report must count merged duplicates as their own bucket:\n{stage5}"
    );
    assert!(
        staged.contains("neither closed nor merged anything"),
        "a run that only tagged must say so outright:\n{stage5}"
    );

    let guardrails = prose(&section(&body, "### Guardrails"));
    assert!(
        guardrails.contains("Never lose intent")
            && guardrails.contains("distinct from both `migrate` and `bundle`")
            && guardrails.contains("never closed-and-dropped"),
        "Never-lose-intent must bind the dedupe/merge path, naming merge distinct from migrate and bundle:\n{guardrails}"
    );
}

/// #564: a zero-required Parameters table used to skip the intake ask even when
/// the captain never tokenized the knob that decides whether the run mutates —
/// `skip and execute` won on *any* non-empty argument, so no defaults card and
/// no OK-or-change question could ever fire. Each of the seven zero-required
/// commands must mark its mutation-gating knob(s) `Mutates: yes`, and the
/// shared preamble must redefine "fully specified" so those knobs gate the
/// skip, not merely the Required column. An ask-first knob defaults to its safe
/// value; an apply-first knob defaults to the mutation and declares an explicit
/// safe token, so the table's Default cell is what each entry pins.
#[test]
fn mutating_knobs_are_token_gated_in_zero_required_parameter_tables() {
    let cost = prose(&cost_preamble());
    assert!(
        cost.contains(
            "every Required=yes row present/parseable AND every `Mutates: yes` row resolved"
        ),
        "docs/COST.md must redefine fully-specified to require tokenized mutation-gating knobs:\n{cost}"
    );

    // command -> (mutation-gating token name, its safe Default)
    let gated: &[(&str, &[(&str, &str)])] = &[
        ("shipmates-consolidate-issues", &[("apply", "report")]),
        ("shipmates-deslop", &[("mode", "report")]),
        ("shipmates-onboard", &[("mode", "`pr`"), ("merge_mode", "manual")]),
        ("shipmates-pr-review", &[("mode", "report")]),
        ("shipmates-release", &[("publish_mode", "manual")]),
        ("shipmates-report-bug", &[("apply", "omit (`MODE=report`)")]),
        ("shipmates-upgrade", &[("report", "apply (`MODE=apply`)")]),
    ];

    for (name, knobs) in gated {
        let body = command(name);
        for (token, safe_default) in *knobs {
            let row = body
                .lines()
                .find(|line| line.trim_start().starts_with(&format!("| {token} |")))
                .unwrap_or_else(|| panic!("{name} Parameters table must declare a `{token}` row"));
            let cells: Vec<&str> = row
                .trim()
                .trim_matches('|')
                .split('|')
                .map(str::trim)
                .collect();
            assert_eq!(
                cells.get(5).copied(),
                Some("yes"),
                "{name} must mark its `{token}` knob `Mutates: yes`:\n{row}"
            );
            // Compare the Default cell itself, never the whole row: the safe
            // token also appears in the Values cell of several tables, so a
            // `contains` check would pass even if the Default were flipped to
            // the mutating value — the exact regression this pin exists for.
            assert_eq!(
                cells.get(4).copied(),
                Some(*safe_default),
                "{name} `{token}` must keep its safe Default `{safe_default}`:\n{row}"
            );
        }
    }
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

/// #420: the standalone story command must never force-push. Only
/// `/shipmates-ship-epic`'s rebase protocol is allowed a lease-scoped force, and
/// only to its own story branch — so a force flag appearing here at all is a
/// contract break, not a wording choice.
#[test]
fn ship_issue_never_force_pushes() {
    let body = command("shipmates-ship-issue");
    assert!(
        !body.contains("--force"),
        "shipmates-ship-issue must not carry a force-push flag; a story PR is never force-pushed"
    );
    assert!(
        !body.contains("force-with-lease"),
        "shipmates-ship-issue must not carry even a lease-scoped force; that belongs to ship-epic"
    );
    for line in body.lines().filter(|line| line.contains("push")) {
        assert!(
            !line.contains(" -f"),
            "a push in shipmates-ship-issue must not carry the short force flag: {line}"
        );
    }
}

/// #420: the epic's in-flight rebase force-push is lease-scoped to the
/// worker's own story branch and explicitly not to the shared integration line.
#[test]
fn ship_epic_force_push_stays_lease_scoped_to_its_own_story_branch() {
    let body = command("shipmates-ship-epic");
    let body = prose(&body);
    assert!(
        body.contains("git push --force-with-lease origin <BRANCH>"),
        "ship-epic's rebase protocol must spell the lease-scoped force push: `git push --force-with-lease origin <BRANCH>`"
    );
    assert!(
        body.contains("NOT to the shared integration branch"),
        "the lease-scoped force must say it never targets the shared integration branch"
    );
}

/// Is `after` the remainder of a genuine lease-scoped force flag — exactly
/// `-with-lease` / `-if-includes`, optionally followed by `=ref` or punctuation?
/// A near-miss such as `-with-lease-x` is not a real Git option, so it is
/// treated as a bare force (fail-closed).
fn lease_suffix_is_well_formed(after: &str) -> bool {
    for form in ["-with-lease", "-if-includes"] {
        if let Some(rest) = after.strip_prefix(form) {
            return rest.is_empty()
                || rest.chars().next().is_some_and(|c| {
                    c.is_whitespace()
                        || matches!(
                            c,
                            '=' | ':' | ',' | ';' | '`' | '"' | '\'' | ')' | ']'
                        )
                });
        }
    }
    false
}

/// Does this line instruct a push that is not a lease-scoped force?
///
/// Two revisions of this guard were bypassed, and both forms are pinned by
/// tests below: a `--force-with-lease` mention elsewhere on the line used to
/// excuse a bare `--force`, and the short flag used to be matched as the
/// literal substring `"push -f"`, which missed `push  -f`, a tab, `push -f;`,
/// and similar shell separators. Detection is per long-flag occurrence and, for
/// the short flag, token-based.
fn bare_force_push(line: &str) -> bool {
    let lowered = line.to_ascii_lowercase();
    if !lowered.contains("push") {
        return false;
    }
    let mut rest = lowered.as_str();
    while let Some(position) = rest.find("--force") {
        let after = &rest[position + "--force".len()..];
        if !lease_suffix_is_well_formed(after) {
            return true;
        }
        rest = after;
    }
    lowered
        .split(|c: char| {
            c.is_whitespace() || matches!(c, ';' | '&' | '|' | '(' | ')' | '`' | '"' | '\'')
        })
        .any(|token| token == "-f")
}

/// #420: the no-force-push guardrail is repo-wide, not a property of the two
/// commands that happen to own the safety contract. `git push --force` — bare,
/// or with the short flag — must not appear in *any* command; ship-epic's
/// lease-scoped rebase rule is the single deliberate exception, and it is the
/// only command allowed to name the lease form at all. This is the check that
/// makes the epic's "a guardrail removal from any command fails CI" claim true
/// for the strongest guardrail, rather than only for the two commands its named
/// contracts live in.
#[test]
fn no_command_instructs_a_bare_force_push() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("commands");
    let mut violations = Vec::new();
    let mut seen = 0;
    for entry in std::fs::read_dir(&dir).expect("read commands/") {
        let path = entry.expect("dir entry").path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("md") {
            continue;
        }
        seen += 1;
        let is_ship_epic = path.file_stem().and_then(|stem| stem.to_str()) == Some("shipmates-ship-epic");
        let body = std::fs::read_to_string(&path).expect("read command");
        for (index, line) in body.lines().enumerate() {
            let lowered = line.to_ascii_lowercase();
            let context = format!("{}:{}: {}", path.display(), index + 1, line.trim());
            if bare_force_push(line) {
                violations.push(format!("bare force push: {context}"));
            }
            if !is_ship_epic && lowered.contains("force-with-lease") {
                violations.push(format!(
                    "only shipmates-ship-epic may name the lease-scoped force: {context}"
                ));
            }
        }
    }
    assert!(
        seen >= 19,
        "expected the command tree to hold every canonical command, saw {seen} files"
    );
    assert!(
        violations.is_empty(),
        "no command may instruct a bare force push, and only ship-epic may carry the lease-scoped form:\n{}",
        violations.join("\n")
    );
}

/// The first revision of the repo-wide guard let a `--force-with-lease` mention
/// elsewhere on a line excuse a bare force on that same line. This pins the fix:
/// detection is per `--force` occurrence, not per line.
#[test]
fn a_lease_mention_elsewhere_on_a_force_line_does_not_excuse_it() {
    assert!(bare_force_push(
        "git push --force origin main # --force-with-lease is the ship-epic exception"
    ));
    assert!(bare_force_push("run `git push --force origin main`"));
    assert!(bare_force_push(
        "git push --force-with-lease origin a && git push --force origin b"
    ));
    assert!(!bare_force_push("git push --force-with-lease origin <BRANCH>"));
    assert!(!bare_force_push("git push --force-if-includes origin <BRANCH>"));
    assert!(bare_force_push("git push --force-with-lease-x origin main"));
    assert!(!bare_force_push("the worker force-pushes with lease to its own branch"));
    assert!(!bare_force_push("no push on this line"));
}

/// The second revision matched the short flag as the literal substring
/// `"push -f"`, so double spaces, a tab, and a trailing shell separator escaped
/// it. Detection is token-based now: a push line carrying the exact token `-f`
/// is a violation whatever the whitespace or separator.
#[test]
fn a_short_force_flag_is_caught_whatever_the_spacing_or_separator() {
    assert!(bare_force_push("git push -f origin main"));
    assert!(bare_force_push("git push  -f origin main"));
    assert!(bare_force_push("git push\t-f origin main"));
    assert!(bare_force_push("git push -f; git status"));
    assert!(bare_force_push("git push origin main -f"));
    assert!(!bare_force_push("git push --force-with-lease=origin/main origin main"));
    assert!(!bare_force_push("git push --atomic origin main"));
    assert!(!bare_force_push("no push on this line"));
}

/// #420: an acceptance → fix → re-acceptance loop with no cap can burn a run
/// indefinitely, so the cap is declared and the exhausted path escalates
/// instead of pushing on.
#[test]
fn ship_issue_bounds_its_fix_loops_and_escalates() {
    let body = command("shipmates-ship-issue");
    assert!(
        body.contains("`MAX_FIX_ROUNDS` = `3`"),
        "ship-issue must declare `MAX_FIX_ROUNDS` as a bounded default in Config"
    );
    assert!(
        body.matches("MAX_FIX_ROUNDS").count() >= 4,
        "the remediation stages must each bound themselves by MAX_FIX_ROUNDS, not only the declaration"
    );
    assert!(
        prose(&body)
            .matches("stop and escalate to the user")
            .count()
            >= 2,
        "both bounded loops (the CI gate and the acceptance loop) must stop and escalate to the user when rounds run out"
    );
}

/// #420: GitHub only auto-closes the issues it is told individually, so the
/// per-issue `Closes #<n>` repetition has to stay explicit in both the snippet
/// and the sentence that explains it.
#[test]
fn ship_issue_repeats_the_closes_keyword_per_issue() {
    let body = command("shipmates-ship-issue");
    assert!(
        body.contains("ISSUES_CLOSES=$(echo \"<issues>\""),
        "the PR body snippet must build the close keywords from the validated <issues> list"
    );
    assert!(
        body.contains("sed 's/ / · Closes #/g; s/^/Closes #/'"),
        "ISSUES_CLOSES must repeat `Closes #` for every issue via the sed substitution"
    );
    assert!(
        prose(&body).contains("silently leaves all but the first open"),
        "the command must say why the repetition matters: a comma-separated list closes only the first"
    );
}

/// #420: a merge to the base branch is the captain's decision by default, and
/// the epic PR never auto-merges at all.
#[test]
fn manual_merge_is_the_default_and_epic_prs_never_auto_merge() {
    let issue = command("shipmates-ship-issue");
    assert!(
        issue.contains("`MERGE_MODE` = `manual`"),
        "ship-issue must default `MERGE_MODE` to `manual` — a green PR waits for the captain"
    );

    let epic = command("shipmates-ship-epic");
    assert!(
        epic.contains("`EPIC_MERGE_MODE` = `manual`"),
        "ship-epic must fix `EPIC_MERGE_MODE` to `manual`"
    );
    assert!(
        prose(&epic).contains("**no** auto-merge path for epic PRs"),
        "ship-epic must state that there is no auto-merge path for epic PRs"
    );
}

/// #420: the four contracts above are canonical-source facts, but what a
/// captain installs is the *rendered* command. A dialect rule that rewrote a
/// guardrail away would ship an unguarded workflow, so render each target
/// in-process and re-assert the contracts against its output.
#[test]
fn every_target_renders_the_ship_issue_contracts_intact() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let roles = catalog::load_roles(&root.join("crew")).expect("load crew");
    let commands = catalog::load_commands(&root.join("commands")).expect("load commands");

    for target in adapters::targets() {
        let adapter = adapters::select(target).expect("select target");
        let payload = adapters::build_payload(adapter.as_ref(), &roles, &commands, None)
            .unwrap_or_else(|error| panic!("build {target} payload: {error}"));
        let rendered: Vec<&String> = payload
            .iter()
            .filter(|(path, _)| path.contains("shipmates-ship-issue"))
            .map(|(_, body)| body)
            .collect();
        assert!(
            !rendered.is_empty(),
            "{target} payload carries no shipmates-ship-issue command — the guard cannot be checked"
        );

        for body in rendered {
            let body = prose(body);
            assert!(
                !body.contains("--force"),
                "{target} renders a force-push into shipmates-ship-issue"
            );
            assert!(
                body.contains("stop and escalate to the user"),
                "{target} renders ship-issue without its escalation sentence"
            );
            assert!(
                body.contains("silently leaves all but the first open"),
                "{target} renders ship-issue without the repeated-`Closes` rationale"
            );
            assert!(
                body.contains("`MERGE_MODE` = `manual`"),
                "{target} renders ship-issue without its manual-merge default"
            );
        }
    }
}

/// #584: /shipmates-auto-trunk commits directly to trunk, so its guardrails are
/// load-bearing: safe defaults, bounded loops, test-first, and a protected-branch
/// degrade path.
#[test]
fn auto_trunk_defaults_safe_and_degrades_on_protected_trunk() {
    let body = command("shipmates-auto-trunk");

    assert!(
        body.contains("`MODE` = `plan`"),
        "auto-trunk must default to plan mode so it does not silently mutate trunk"
    );
    assert!(
        body.contains("`PUSH_MODE` = `no-push`"),
        "auto-trunk must default to no-push so remote writes are opt-in"
    );
    assert!(
        body.contains("protected branch") && body.contains("branch+PR"),
        "auto-trunk must detect a protected default branch and degrade to branch+PR or stop loudly"
    );
    assert!(
        body.contains("`MAX_RED_ROUNDS` = `3`") && body.contains("`STOP_AFTER` = `10`"),
        "auto-trunk must declare bounded loops"
    );
    assert!(
        prose(&body).contains("failing test first") || prose(&body).contains("failing test"),
        "auto-trunk must require a failing test before each source change when tests exist"
    );
    assert!(
        !body.contains("--force"),
        "auto-trunk must never force-push"
    );
}
