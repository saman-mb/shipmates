"""Human-facing guide copy for Shipmates command pages (site layer only).

Not imported by install payloads. Consumed by tools/gen_command_pages.py.
Each page: one sentence what it is, 4–6 process steps with who sits and when, when to pick it.
"""
from dataclasses import dataclass


@dataclass(frozen=True, slots=True)
class CrewWhen:
    """An optional seat: the role, and the plain-English criterion that pulls it."""

    role: str
    when: str


@dataclass(frozen=True, slots=True)
class ProcessStep:
    label: str  # 1–4 words
    line: str  # what happens, and how the crew is chosen
    always: tuple = ()  # role slugs that sit every time
    also: tuple = ()  # tuple[CrewWhen, ...] — sit only when the criterion holds
    solo: str = ""  # when nobody is spawned: the run does this itself


@dataclass(frozen=True, slots=True)
class CommandPageCopy:
    guide_blurb: str  # one sentence, ≤120 chars
    when_to_use: tuple  # 2–3 bullets vs siblings
    process: tuple  # tuple[ProcessStep, ...] — 4–6 steps
    process_lead: str = ""  # optional: how this command picks the crew


def _also(role: str, when: str) -> CrewWhen:
    return CrewWhen(role, when)


# Shared acceptance board — /ship-issue Stage 5, and any command that reuses it.
BOARD_ALSO = (
    _also("architect", "new subsystem, schema, or a change that crosses many modules"),
    _also("ux-ui-designer", "on-screen UI — screens, flows, components"),
    _also("art-director", "rendered art (games, illustration), not app chrome"),
    _also("sdet", "the plan asks for a specialist test review"),
    _also("devops-engineer", "pipeline, images, or how the project ships"),
    _also("technical-writer", "documented behaviour or a public API/CLI"),
)

# /pr-review adds seats /ship-issue does not: it cannot run /shipmates-harden on a branch it does not own.
PR_BOARD_ALSO = BOARD_ALSO + (
    _also("security-engineer", "auth, secrets, crypto, or untrusted input"),
    _also("performance-engineer", "a claimed perf win, or a known hot path"),
    _also("site-reliability-engineer", "runtime behaviour, failure handling, or rollout"),
    _also("data-scientist", "the deliverable is an analysis or a model"),
)

# /plan-epics panel: always PM, then up to two of these from the brief (or named by you).
PLAN_PANEL_ALSO = (
    _also("architect", "new subsystems, schema, or platform boundaries"),
    _also("ux-ui-designer", "screens, flows, design system, accessibility"),
    _also("art-director", "rendered art in an art-producing domain"),
    _also("data-scientist", "data, ML, metrics, or experimentation"),
    _also("performance-engineer", "latency, throughput, or resource limits"),
    _also("devops-engineer", "build, CI, packaging, or toolchain pins"),
    _also("security-engineer", "auth, secrets, crypto, or untrusted input"),
    _also("site-reliability-engineer", "reliability, incidents, rollback, or SLOs"),
    _also("technical-writer", "documented behaviour or public API/CLI"),
)


COMMAND_PAGE_COPY: dict[str, CommandPageCopy] = {
    "ship-issue": CommandPageCopy(
        guide_blurb="One GitHub issue in, a reviewed CI-green pull request out.",
        process_lead=(
            "The architect reads the issue and sets flags. Those flags decide who else sits — "
            "a UI story pulls a designer, a schema change pulls an architect. "
            "Product-manager and principal-engineer always review."
        ),
        when_to_use=(
            "A tracked ticket (or a small related bundle) is ready to build.",
            "Whole epic? Use /ship-epic. A defect? Use /shipmates-fix-bug.",
        ),
        process=(
            ProcessStep(
                "Plan",
                "The architect reads the issue, writes the plan, and sets the flags that pick every later specialist.",
                always=("architect",),
            ),
            ProcessStep(
                "Build",
                "Engineers implement in an isolated worktree. Any specialist flagged in Plan writes a spec first; the build follows it.",
                always=("senior-engineer",),
                also=(
                    _also("ux-ui-designer", "the story changes on-screen UI"),
                    _also("art-director", "the story changes rendered art"),
                    _also("architect", "the story changes structure or schema"),
                ),
            ),
            ProcessStep(
                "PR + CI",
                "Open a pull request. The SDET runs the test plan; if checks go red, an engineer fixes until they are green.",
                always=("sdet", "senior-engineer"),
            ),
            ProcessStep(
                "Review",
                "Two reviewers sit on the first pushed head. After a fixer, failers sit again; a prior ACCEPT carries unless the delta can invalidate it.",
                always=("product-manager", "principal-engineer"),
                also=BOARD_ALSO,
            ),
            ProcessStep(
                "Hand off",
                "You get a PR to merge — or it merges if you asked.",
                solo="No extra spawn — the run closes out.",
            ),
        ),
    ),
    "ship-epic": CommandPageCopy(
        guide_blurb="Ship every story on an epic onto one branch, then one PR to main.",
        process_lead=(
            "One architect pass groups the epic. Each unit is then a full /ship-issue run, "
            "so that unit's flags pick its specialists. You merge one epic PR at the end."
        ),
        when_to_use=(
            "An epic issue already has stories on GitHub (sub-issues and/or a checklist).",
            "One ticket? /ship-issue. No backlog yet? /plan-epics first.",
        ),
        process=(
            ProcessStep(
                "Map",
                "The architect reads the epic's stories (sub-issue graph, union the checklist), classifies every story, and groups them into shipping units.",
                always=("architect",),
            ),
            ProcessStep(
                "Integrate",
                "Cut a shared epic branch and open the epic PR. Unit PRs target that branch, not main.",
                solo="No specialist — the run cuts the branch and the epic PR.",
            ),
            ProcessStep(
                "Loop",
                "Each unit is a /ship-issue: plan, build, CI, review. That unit's flags pick its extra seats.",
                always=("architect", "senior-engineer", "sdet", "product-manager", "principal-engineer"),
                also=tuple(s for s in BOARD_ALSO if s.role not in ("architect", "sdet")),
            ),
            ProcessStep(
                "One PR",
                "When every story is done, you review and merge the single epic PR.",
                solo="No extra spawn — you merge the epic PR, not the unit PRs.",
            ),
        ),
    ),
    "shipmates-fix-bug": CommandPageCopy(
        guide_blurb="Prove the bug with a failing test, fix the cause, prove it gone.",
        process_lead=(
            "SDET owns the failing test. An engineer (or SRE, if it is a runtime bug) finds the "
            "cause and fixes it. Review always includes SDET, product-manager, and principal-engineer."
        ),
        when_to_use=(
            "Something is broken and you want red→green proof, not a guess.",
            "New behaviour? /ship-issue. Same behaviour, new shape? /shipmates-refactor.",
        ),
        process=(
            ProcessStep(
                "Reproduce",
                "Write a test that fails on the bug. No fix until that test is red.",
                always=("sdet",),
                also=(_also("site-reliability-engineer", "it is a runtime or ops failure"),),
            ),
            ProcessStep(
                "Cause",
                "Find the mechanism that produces the symptom — not just the line that crashed.",
                always=("senior-engineer",),
                also=(_also("site-reliability-engineer", "it is a runtime or ops failure"),),
            ),
            ProcessStep(
                "Fix",
                "Change the smallest thing that addresses the cause. A fresh engineer, not the one who just diagnosed it, will review later.",
                always=("senior-engineer",),
            ),
            ProcessStep(
                "Prove",
                "The new test passes, CI is green, and reviewers sign off on the pushed head.",
                always=("sdet", "product-manager", "principal-engineer"),
                also=(
                    _also("senior-engineer", "a fresh engineer confirms the fix addresses the cause"),
                    _also("site-reliability-engineer", "it was a runtime bug — a fresh SRE reviews instead"),
                ),
            ),
        ),
    ),
    "report-bug": CommandPageCopy(
        guide_blurb="Turn a live Shipmates failure into a structured upstream issue.",
        when_to_use=(
            "Shipmates itself misbehaved and maintainers need a triage-ready report.",
            "Fixing your own repo? /shipmates-fix-bug. Cleaning a backlog? /consolidate-issues.",
        ),
        process=(
            ProcessStep(
                "Capture",
                "Record what broke, which harness, which command, and the expected vs observed result.",
                solo="No specialist — the run harvests context from the session.",
            ),
            ProcessStep(
                "Dedupe",
                "Search existing upstream issues so a duplicate is a comment, not a new ticket.",
                solo="Still the run — no spawn until the draft.",
            ),
            ProcessStep(
                "Draft",
                "Shape a title and body a maintainer can act on.",
                always=("product-manager",),
                also=(_also("technical-writer", "the report cites command-spec behaviour"),),
            ),
            ProcessStep(
                "File",
                "Preview first; create the GitHub issue only when you say apply.",
                solo="No extra spawn — you confirm, then it files.",
            ),
        ),
    ),
    "plan-epics": CommandPageCopy(
        guide_blurb="Turn a brief into GitHub epics and linked, labelled stories.",
        process_lead=(
            "Product-manager always authors the backlog. Up to two more specialists join when the "
            "brief implicates their domain — or when you name them. A bookkeeping brief stays PM-only."
        ),
        when_to_use=(
            "You know the work, but GitHub has no epics or stories yet.",
            "Ready to build? /ship-epic. Messy existing backlog? /consolidate-issues.",
        ),
        process=(
            ProcessStep(
                "Read",
                "Take the brief, the repo's labels, and what is already open, then pick the panel.",
                solo="No specialist yet — intake and panel selection are the run.",
            ),
            ProcessStep(
                "Slice",
                "Split the work into epics with clear edges. The panel is product-manager plus at most two of the roles below.",
                always=("product-manager",),
                also=PLAN_PANEL_ALSO,
            ),
            ProcessStep(
                "Write",
                "One product-manager per epic authors the stories. The same panel then reviews slices in their domain.",
                always=("product-manager",),
                also=PLAN_PANEL_ALSO,
            ),
            ProcessStep(
                "Create",
                "Open the issues on GitHub, attach each story as a sub-issue of its epic, and verify they connect.",
                solo="No extra spawn — the run creates what the panel wrote.",
            ),
        ),
    ),
    "consolidate-issues": CommandPageCopy(
        guide_blurb="Triage the backlog against git history; keep what still matters.",
        when_to_use=(
            "Open issues are stale, duplicated, or already shipped.",
            "Starting from a brief? /plan-epics. Shipping survivors? /ship-issue.",
        ),
        process=(
            ProcessStep(
                "Inventory",
                "List the open issues in scope.",
                solo="No specialist — the run lists the backlog.",
            ),
            ProcessStep(
                "Evidence",
                "Match them against commits and merged PRs so a close has a reason.",
                solo="Still evidence-first, no spawn.",
            ),
            ProcessStep(
                "Verdicts",
                "Close, keep, or rewrite each one. The product-manager owns every verdict.",
                always=("product-manager",),
            ),
            ProcessStep(
                "Bundle",
                "Group what remains into themes you can actually ship.",
                always=("product-manager",),
            ),
        ),
    ),
    "shipmates-harden": CommandPageCopy(
        guide_blurb="Threat-model a surface, rank findings, fix blockers — or just report.",
        process_lead=(
            "Security-engineer always threat-models. An engineer remediates only if you asked for a PR; "
            "the default is a report. Security then re-reviews the same surface."
        ),
        when_to_use=(
            "Auth, secrets, or another sensitive surface needs a security pass.",
            "/ship-issue may recommend this; it does not replace it.",
        ),
        process=(
            ProcessStep(
                "Scope",
                "Name the entry points, trust boundaries, and what is out of play.",
                solo="No specialist yet — you name the surface.",
            ),
            ProcessStep(
                "Find",
                "Walk threats and rank them by severity. Nothing Critical or High may hang without a written call.",
                always=("security-engineer",),
            ),
            ProcessStep(
                "Decide",
                "Default is report-only. A PR run has an engineer fix blockers, or record accepted risk in writing.",
                also=(_also("senior-engineer", "you asked for a PR, not a report"),),
            ),
            ProcessStep(
                "Close",
                "Re-review the remediated surface until nothing Critical or High is hanging.",
                always=("security-engineer",),
            ),
        ),
    ),
    "shipmates-spike": CommandPageCopy(
        guide_blurb="Prototype the options, pick one, write the decision down.",
        process_lead=(
            "One engineer prototypes each approach in parallel. The architect always judges. "
            "Extra judges sit only when the decision hinges on their axis — security, performance, or data."
        ),
        when_to_use=(
            "A technical choice is still open and you need evidence, not opinions.",
            "The path is chosen? /ship-issue. Mechanical rewrite? /shipmates-migrate.",
        ),
        process=(
            ProcessStep(
                "Frame",
                "State the question, the constraints, and the criteria each option will be scored on.",
                solo="No specialist — you set the criteria the judges will use.",
            ),
            ProcessStep(
                "Prototype",
                "One engineer per approach, in parallel throwaway worktrees. Nothing lands on the base branch.",
                always=("senior-engineer",),
            ),
            ProcessStep(
                "Judge",
                "Score the prototypes on the criteria you set. Extra judges join only when their axis is load-bearing.",
                always=("architect",),
                also=(
                    _also("security-engineer", "the decision hinges on a security axis"),
                    _also("performance-engineer", "the decision hinges on a performance axis"),
                    _also("data-scientist", "the decision hinges on a data or modelling axis"),
                ),
            ),
            ProcessStep(
                "Record",
                "Ship an ADR with the decision, the rejected options, and why.",
                always=("architect", "technical-writer"),
            ),
        ),
    ),
    "shipmates-migrate": CommandPageCopy(
        guide_blurb="Find every call site, rewrite them, leave none of the old pattern.",
        process_lead=(
            "Engineers rewrite in parallel batches. SDET proves the old pattern is gone. "
            "The same review board as /ship-issue then sits — extra seats from the same flags."
        ),
        when_to_use=(
            "An API, library, or idiom must change everywhere it appears.",
            "Behaviour stays, shape changes? /shipmates-refactor. Choice still open? /shipmates-spike.",
        ),
        process=(
            ProcessStep(
                "Census",
                "Find every occurrence. Miss one and the migration is not done.",
                solo="No specialist — the run must see every call site.",
            ),
            ProcessStep(
                "Rewrite",
                "Several engineers, disjoint files, in parallel. Non-mechanical sites are handled one by one, not blind-replaced.",
                always=("senior-engineer",),
            ),
            ProcessStep(
                "Sweep",
                "Re-search the repo. The old pattern must be gone, and the suite green.",
                always=("sdet",),
            ),
            ProcessStep(
                "Land",
                "Two reviewers sit first; a retry re-selects from the fixer delta. The same flags as /ship-issue pull everyone else.",
                always=("product-manager", "principal-engineer"),
                also=BOARD_ALSO,
            ),
        ),
    ),
    "shipmates-document": CommandPageCopy(
        guide_blurb="Write docs from the code, then make a new reader complete them.",
        when_to_use=(
            "User-facing docs drifted from what the repo actually does.",
            "Agent-facing map of the repo? /shipmates-onboard.",
        ),
        process=(
            ProcessStep(
                "Audience",
                "Who is this for, and what kind of doc is it — tutorial, how-to, reference, or explanation?",
                solo="No specialist — you pick audience and type.",
            ),
            ProcessStep(
                "Draft",
                "The writer works from the real signatures, flags, and outputs — not from memory.",
                always=("technical-writer",),
            ),
            ProcessStep(
                "Test",
                "A fresh agent that did not draft the doc follows the steps against the repo and must reach the stated result.",
                solo="A fresh agent — not the writer, and not a named specialist.",
            ),
            ProcessStep(
                "Fix",
                "The writer repairs whatever the reader could not complete. Loop until they can finish without help.",
                always=("technical-writer",),
            ),
        ),
    ),
    "shipmates-release": CommandPageCopy(
        guide_blurb="Changelog, version bump, green CI, then tag — publish if you say so.",
        process_lead=(
            "The writer assembles notes from what actually merged. SRE checks rollback and migration "
            "safety before anything is tagged. Publish is opt-in."
        ),
        when_to_use=(
            "Merged work is ready as a named version with notes.",
            "A single feature PR that should bump version? That lives in /ship-issue.",
        ),
        process=(
            ProcessStep(
                "Since last tag",
                "Collect every merged PR and commit since the last release.",
                solo="No specialist — the run reads git history.",
            ),
            ProcessStep(
                "Notes + bump",
                "Write the changelog from that history and move the version files together.",
                always=("technical-writer",),
            ),
            ProcessStep(
                "Gate",
                "CI must be green on the exact commit you will tag. SRE checks rollback and migration safety.",
                always=("site-reliability-engineer",),
            ),
            ProcessStep(
                "Ship",
                "Tag it. Publish a GitHub release only when you opt in.",
                solo="No extra spawn — tag and optional publish.",
            ),
        ),
    ),
    "shipmates-polish": CommandPageCopy(
        guide_blurb="Show the artifact, take critique, fix, repeat until a specialist signs off.",
        process_lead=(
            "One reviewer sits, chosen by the artifact: designer for UI, art-director for pictures "
            "judged as art, product-manager for other output. An engineer applies each round of fixes."
        ),
        when_to_use=(
            "A screen, chart, or render needs to look right — behaviour already exists.",
            "Prose docs? /shipmates-document. New feature? /ship-issue.",
        ),
        process=(
            ProcessStep(
                "See it",
                "Put the real output in front of the reviewer — not the source that made it.",
                always=("senior-engineer",),
            ),
            ProcessStep(
                "Baseline",
                "Capture round zero so later rounds have something to diff against.",
                solo="No spawn — produce the first version.",
            ),
            ProcessStep(
                "Loop",
                "Critique, fix, recapture. One reviewer by domain; the engineer applies that reviewer's list. Bounded rounds.",
                always=("senior-engineer",),
                also=(
                    _also("ux-ui-designer", "the artifact is on-screen UI"),
                    _also("art-director", "the artifact is rendered art"),
                    _also("product-manager", "the artifact is other output — copy, a chart, a data view"),
                ),
            ),
            ProcessStep(
                "Sign-off",
                "Stop when that specialist accepts — not when the round cap is merely tiring.",
                solo="No extra spawn — report their verdict.",
            ),
        ),
    ),
    "pr-review": CommandPageCopy(
        guide_blurb="Review someone else's PR. Report a verdict. Do not change the code.",
        process_lead=(
            "The diff is classified with the same flags as /ship-issue. Product-manager and "
            "principal-engineer always sit; the flags pull the rest. This command reports — it never edits the branch."
        ),
        when_to_use=(
            "A pull request needs an adversarial pass and you do not own the branch.",
            "You are delivering the work? /ship-issue, not this.",
        ),
        process=(
            ProcessStep(
                "Classify",
                "Read the diff and set the flags. Those flags are how the board is assembled — a role whose flag is off is not spawned.",
                solo="No spawn yet — classification is the run.",
            ),
            ProcessStep(
                "Read CI",
                "Note red checks as findings. This command does not repair them: you do not own the branch.",
                solo="Still the run. It reports CI; it never repairs.",
            ),
            ProcessStep(
                "Board",
                "Specialists review the pushed head in parallel. Because this is someone else's PR, security sits here when the flag is on — /shipmates-harden is not available.",
                always=("product-manager", "principal-engineer"),
                also=PR_BOARD_ALSO,
            ),
            ProcessStep(
                "Verdict",
                "One ranked accept-or-block, with reasons attributed to the role that raised them.",
                solo="You synthesise — no extra spawn.",
            ),
        ),
    ),
    "shipmates-onboard": CommandPageCopy(
        guide_blurb="Read an unfamiliar repo and write the agent-facing map the crew needs.",
        process_lead=(
            "Architect and SDET always recon. DevOps joins when there is a pipeline or image to inspect. "
            "A writer drafts the file; a fresh agent — not the writer — must be able to answer from it alone."
        ),
        when_to_use=(
            "Agents (or you) lack a trustworthy picture of how this repo works.",
            "User-facing docs? /shipmates-document. Then start shipping with /ship-issue.",
        ),
        process=(
            ProcessStep(
                "Survey",
                "How the repo is laid out, tested, and run. Commands that end up in the file must have been run, not guessed.",
                always=("architect", "sdet"),
                also=(_also("devops-engineer", "the repo has a pipeline, image, or infrastructure definition"),),
            ),
            ProcessStep(
                "Draft",
                "Write the agent-facing contract from what recon actually found: commands, boundaries, quality bar.",
                always=("technical-writer",),
            ),
            ProcessStep(
                "Prove",
                "A fresh agent gets the file and nothing else, and must answer the questions the crew actually asks.",
                solo="A fresh agent — not the writer, and not a named specialist.",
            ),
            ProcessStep(
                "Land",
                "Leave a file the next session can trust, usually as a PR so you can see the diff.",
                solo="No extra spawn — the guide is the deliverable.",
            ),
        ),
    ),
    "shipmates-refactor": CommandPageCopy(
        guide_blurb="Pin today's behaviour, change the shape, prove callers still see the same thing.",
        process_lead=(
            "SDET pins current behaviour first. An engineer reshapes; architect sits when the shape is "
            "structural. Review always includes architect and a fresh SDET, plus PE+PO."
        ),
        when_to_use=(
            "The code is wrong-shaped; the product behaviour is not.",
            "The behaviour is wrong? /shipmates-fix-bug. Find-and-replace an API? /shipmates-migrate.",
        ),
        process=(
            ProcessStep(
                "Pin",
                "Tests that describe current behaviour — including behaviour you think is wrong — while it is still true.",
                always=("sdet",),
            ),
            ProcessStep(
                "Reshape",
                "Change structure without changing what callers observe. Architect sits first when the target shape is a real structural change.",
                always=("senior-engineer",),
                also=(_also("architect", "the change crosses module boundaries or moves a public surface"),),
            ),
            ProcessStep(
                "Prove",
                "The pinned tests pass unmodified. No existing test is deleted, skipped, or loosened.",
                always=("sdet",),
            ),
            ProcessStep(
                "Land",
                "Architect always asks whether the structure actually improved. A fresh SDET audits the test diff. PE+PO accept.",
                always=("architect", "sdet", "product-manager", "principal-engineer"),
                also=(_also("performance-engineer", "the stated motivation was performance"),),
            ),
        ),
    ),
}
