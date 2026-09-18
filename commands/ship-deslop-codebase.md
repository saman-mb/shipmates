---
name: ship-deslop-codebase
description: Shipmates: Audit a codebase for health debt — dead code, duplication, redundancy, inconsistency, bad patterns, dependency and config rot — grade every finding by risk, then optionally fix the safe findings in a worktree and open a CI-green PR. Read-only by default — it reports; fixes happen on a branch, opt-in, behind coverage, feature-impact and product-owner gates.
argument-hint: <path-or-module> [apply] — no args: whole repo, report-only
allowed-tools: Bash, Read, Write, Edit, Agent, Grep, Glob
disable-model-invocation: true
---
# /ship-deslop-codebase — discover → grade → report, or fix the safe part behind gates
<!-- shipmates:command-preamble -->

Every repository carries debt no issue tracks: an import nothing imports, two copies of one function
that have quietly drifted apart, a wrapper that only passes its arguments through, a feature flag that
has read `true` since the release after it shipped, a TODO older than the test runner around it. No
analyser sees all of it and no human wants to read the whole tree to find it. `/ship-deslop-codebase`
inventories that debt, grades every finding by the risk of touching it, and either **reports** it or
**fixes the safe part** on a branch. "Slop" here is any of it — code that is dead, duplicated,
redundant or inconsistent — whatever wrote it; the command judges the debt, not its author.

**Reporting is the default. Deleting is a decision.** Under `MODE=report` the working tree is left
exactly as found. Under `MODE=apply` the safe findings are fixed in a worktree and offered as a
CI-green PR — behind a green test baseline, a coverage contract, a feature-impact statement and a
product-owner sign-off. A cleanup PR that deletes code is the most dangerous kind of change: the
compiler stays green, the tests stay green, and a feature breaks anyway because the deleted path was
the only one a customer actually used. Every gate below exists to catch that case.

This workflow is **discovery-driven** — you do not know what you will find until you look. If the ask
is already "rename X to Y across the codebase" or "swap dependency A for B", stop and run
`/ship-migrate`; if it is "change the shape without changing behaviour", that is `/ship-refactor`. The
scope and the go-ahead to change anything come from the Runtime input section at the end of this
workflow.

---

## Config (override only if the repo needs it)

- `MODE` = `report` (default) — **read-only**: inventory, grade, report; it writes nothing, not even a
  deletion that looks obviously safe. `apply` — fix the `safe` findings in a worktree on a branch and
  open a CI-gated PR. Infer from the request; ambiguous → `report`, stating which mode ran.
- Under `MODE=apply` only: `BASE_BRANCH` = the branch the PR targets (the repo's default branch) — the
  worktree is cut from `origin/<BASE_BRANCH>`, the clean mergeable baseline a codebase-wide change
  wants. `WORKTREE_LAYOUT` = `nested` (default) — `<repo>/.shipmates/worktrees/`; **`worktree-root=sibling`**
  selects legacy `../<repo>--…` paths. `WORKTREE_DIR` — **nested:**
  `<repo>/.shipmates/worktrees/ship-deslop-codebase-<slug>`; **sibling:** `../<repo>--deslop-codebase-<slug>`. Re-runs
  reuse the same path. `BRANCH` = `chore/ship-deslop-codebase-<slug>`.
- `EXECUTION` = `fanout` — how discovery shards and per-theme fix batches run. `fanout` (default):
  independent, file-disjoint units run concurrently up to `MAX_CONCURRENT_WORKERS`; guidance
  `sequential` sets `EXECUTION=sequential` to run them one at a time.
- `MAX_CONCURRENT_WORKERS` = `5`. `MAX_FIX_ROUNDS` = `3` — bounds both the verify/fix loop and the
  CI-fix loop. `MERGE_MODE` = `manual` (stop at a reviewed PR; `auto` opt-in).
- `BOARD` = `full` (default) — the Stage 7 board. `board=epic-deferred` defers it to an orchestrator's
  milestone board (the deferred board still runs there); `board=off` is an explicit captain opt-out
  with no deferral target. Both are the shared acceptance-board delegation modes.
- `BUDGET` = the run's ceiling on findings applied (or wall-clock, when the repo prefers). The run stops
  cleanly at it and reports what was **not** scanned — cleanup is an infinite well, and an unbounded
  sweep is this command's default failure mode.
- Thresholds — repo overrides come from `{{project-instructions}}` or `.shipmates/deslop-codebase.toml`:
  `COMPLEXITY_LIMIT` = `15`; `DUP_SIMILARITY` = `80%`; `TODO_HORIZON` = `180d`; `ZOMBIE_HORIZON` = `90d`
  (commented-out blocks older than this with no linked issue).
- **Quality bar / test commands / coverage tooling** = whatever the repo's README /
  `{{project-instructions}}` / CI config states. Read them before Stage 0 grades anything. The
  orchestrator owns all git/gh; agents never push.

---

## Stage 0 — Scope, mode, and analyser detection

Parse the runtime input first, then bound the survey. Nothing is scanned until the scope is named — a
report that silently widened to the whole repo is a report nobody can act on.

1. **Scope.** A path, package, or module token narrows the survey to that subtree; an optional `language=` token narrows a multi-language repo to one language's files. Empty means the whole repository. Write the resolved scope down — Stages 1 and 3 print it back.
2. **Mode.** The word `apply` sets `MODE=apply`; anything else is `MODE=report`. State the resolved mode on the first line of the report, so nobody reads an audit as a change.
3. **Baseline check** (`apply` only). Run `git -C <repo> status --porcelain`; a dirty caller tree means **stop and say so** — commit or stash first, because the census locates findings against the branch that will be cut and a worktree cut from `origin/<BASE_BRANCH>` holds committed work only. Then `git -C <repo> fetch origin`; stop if the fetch fails.
4. **Detect the analysers the repo already has.** Read the manifests, CI config, task runner, and the repo's own instructions, and inventory which analyser classes are present. **Never install an analyser silently**, and never stand up coverage tooling as a side effect of a scan — Stage 2's honesty rule depends on knowing whether the tooling exists. Run `--help` on unfamiliar tools.
5. **Coverage-of-scan plan.** Decide up front which paths, languages, and file types are in scope and which are skipped by policy — vendored trees, generated code, binaries, lockfiles, large data files. "Found nothing in one subtree" and "found nothing anywhere" are different findings; the report must not conflate them.

What to look for, and what each analyser class feeds:

| Capability class | Feeds | Found in |
|---|---|---|
| Unused-symbol / dead-code analysis | dead symbols, imports, unreachable branches | the repo's compiler/linter settings |
| Lint + style rules | bad patterns by class, naming drift | the repo's lint config |
| Duplication detection | exact clones and near-duplicates over `DUP_SIMILARITY` | a clone detector, or agent reading |
| Complexity + type/strictness checking | hotspots over `COMPLEXITY_LIMIT`, overly broad types, unchecked null paths | a metrics tool or the repo's type checker, at its strictest |
| Dependency audit + import-graph analysis | unused declarations, lockfile version skew, circular module dependencies | the package manager's audit surface, a graph tool, or agent reading |
| Comment, marker, and docs scanning | zombie blocks, stale TODOs, docs describing moved code | grep-shaped scans plus `git blame` for age |

Whatever the tools do not cover, the agents read for: pattern inconsistency, architectural redundancy,
dead feature flags, and comments describing code that has moved. Never reimplement a mature analyser —
orchestrate the ones that exist, and say which ones were missing.

---

## Stage 1 — Discovery census  ⛔ the contract

**This census is the contract.** Every finding it records ends applied or explicitly excluded, with a
reason. Silent truncation is a failure of the run, not a smaller run.

- Run the Stage 0 analysers and complement them with agent-driven reading. Under `EXECUTION=fanout`,
  split the survey by finding class (or by subtree for a large repo) across workers up to
  `MAX_CONCURRENT_WORKERS`, each returning rows in the one shape below.
- Every finding is one row: **stable ID**, `file:line`, **class**, a one-line rationale, the evidence
  that produced it, and the proposed action. A row without evidence is a hunch, not a finding.

The survey covers these classes — the list is the whole of what "cleanup" means here:

- **Dead code** — unused functions, imports, variables, unreachable branches, private symbols with no internal callers, and a state or guard unreachable by construction rather than by control flow.
- **Duplication** — exact clones and near-duplicates that could become one shared helper; read-aware, not a token hash, so two blocks that merely look alike are a *candidate group* proven out in Stage 5.
- **Redundancy** — unnecessary abstractions, wrappers that only pass arguments through, indirection that adds no name value, intermediate variables that restate their expression, a seam with exactly one implementation, and an extension point nothing registers against.
- **Over-engineering beyond product need** — machinery that distinguishes more internal states, options or outcomes than any consumer acts on. The signal is the ratio, not the size: count the distinct states, modes or outputs a unit produces, then count the distinct behaviours those states actually cause at a consumer; a unit producing many where consumers branch on one or none is a candidate. Its shapes are an optimisation with no measurement behind it, an in-house rebuild of a capability the platform already provides, and data modelled richer than any use — the cases a consumer could never observe being removed belong to the three classes above, which already contain them. Evidence is the count on both sides and the consumer sites that produced it, never an impression that the code feels heavy. This class is subordinate to the protected list, never a way around it.
- **Inconsistency** — mixed concurrency or callback styles in one codebase, naming drift, error handling that throws at one call site, returns at another, and logs-and-continues at a third.
- **Bad patterns, by class** — resource leaks (acquisition with no matching release on every path), blocking calls inside an asynchronous context, N+1-style repeated access in a loop, repeated work a single pass would do.
- **Complexity hotspots** — functions and classes over `COMPLEXITY_LIMIT`, deep nesting.
- **Dependency health** — declared-but-unused dependencies, several versions of one transitive dependency in the lockfile, import cycles that interface extraction would break.
- **Repository rot** — commented-out blocks older than `ZOMBIE_HORIZON` with no linked issue; a stale TODO/FIXME inventory with age and whether the surrounding code was since rewritten; flags and settings with only one reachable value, whether or not anything reads them; duplicated CI work; docs describing moved code.
- **Type and contract health** — overly broad types, missing strictness (implicit returns, unchecked null paths, absent exhaustiveness), public surface with no caller in the repo or its consumers.
- **Constants and literals** — repeated magic numbers and strings that should be named once, and regex or format strings recompiled at every use.

Two properties make the census usable as a tracker:

- **Stable IDs.** Derive each ID from content — kind + path + symbol — never from row order, a counter,
  or a timestamp. Two runs against an identical commit must produce an identical finding set.
- **The run diff.** Because IDs are stable, the report diffs this run against the previous report:
  **new**, **resolved**, and **carried-over** findings. Without that diff the report is a snapshot, not
  a progress tracker.

Close the stage by printing the coverage-of-the-scan footer — which paths, languages, and file types were
analysed, which were skipped — and recording what `BUDGET` left unscanned. Both carry into Stage 3.

---

## Stage 2 — Risk grade and protected classes

Every finding gets exactly one grade, assigned mechanically where it can be and by judgement where it
cannot:

| Grade | Means | Typical trigger | Default action |
|---|---|---|---|
| `safe` | deleting it cannot change behaviour | unused private symbol, no dynamic reachability, satisfiable under the Stage 4 contract | fix in Stages 4–6 |
| `review` | probably safe, needs a human call | exported symbol with no internal callers, inconsistency fixes | queue for Stage 5, PO sign-off required |
| `architectural` | changes boundaries or contracts | deletion crossing a module boundary, abstraction with no precedent, public-surface removal | queue for Stage 5, `architect` + PO sign-off required |
| `protected` | structurally protected — never auto-deleted | any member of the protected classes below | report with rationale only |

Overlays travel alongside the grade into the report and the PR body: `test-infrastructure` (a helper only
tests call), `performance-sensitive` (a deleted wrapper that held caching, memoisation, batching or
pooling), `flaky` (a test that cannot serve as evidence), `integration-gap` (no integration test covers
an affected entry point), `doc-follow-up` (docs need a human's wording), and `needs-review` (reported,
deliberately not applied).

Grading rules:

- An unused **private** symbol with no dynamic reachability grades `safe` — but only when the Stage 4
  test-confidence contract can be met for it.
- An unused **exported** symbol, or one with no internal callers, grades `review`: nothing inside the
  repo can prove who imports a published package.
- A deletion that crosses a module boundary or changes a contract grades `architectural`.
- **No coverage tooling means nothing grades `safe`.** Every finding degrades to `review` at minimum and
  the report says so plainly — a confidence grade is a claim about verification, and without the tooling
  the honest claim is "unverified". Standing up coverage tooling is an explicit, opt-in prerequisite,
  never a silent install.
- **A repo with no tests at all runs `report` only and grades nothing above `review`**, leading the
  report with "this repo has no test suite; cleanup here is a judgement call, not a verified one". Never
  hand back a confident-looking list under those conditions.

**Protected classes — never auto-deleted, reported with rationale instead:**

- **Database migrations** — an "unused" migration may be the only path that upgrades a deployed instance, and deleting it strands every environment that has not run it yet.
- **Error-path, fallback, and graceful-degradation handlers** — catch blocks, retries, circuit breakers and defaults look dead precisely because they only run when something goes wrong.
- **Version-compatibility shims** — code gated on an older runtime, protocol version, or dependency range the project still supports.
- **Feature-flagged code for unreleased work** — a flag that is `false` today is `true` at launch.
- **Public API with external consumers** — nothing inside the repo can prove who imports it.
- **Framework entry points and lifecycle hooks** — called by the framework, never by your code: route handlers, dependency providers, command registration, test fixtures, plugin hooks.
- **Targets of dynamic dispatch** — anything reached through `eval`, reflection, string-keyed dispatch, serialization, or a configuration file.
- **Code referenced only from docs, issue templates, or CI config** — grep is unreliable there, so treat a hit as `review`.

Protected classes are discovered two ways, layered: conventions supply the defaults (a path matching a
migrations directory, a handler registered as an error path) and the repo supplies explicit additions and
overrides. Conventions alone risk a silent miss; an explicit list alone goes stale.

**The principle, stated once and applied throughout: absence of evidence is not evidence of absence.** A
symbol with no static callers is a *hypothesis* of deadness, and the burden of proof is on the deletion. A
protected class is therefore **never** auto-deleted regardless of evidence — the command reports it and
stops, because that is the only default that cannot strand a deployed environment.

Reliability caveats set here are enforced in Stage 6: a flaky test is not evidence (run the touched subset
twice, or read the repo's quarantine list, before trusting it, and report flakes as findings without
fixing them here); assertion-free tests and mock-only paths give false confidence and downgrade the
finding they cover.

---

## Stage 3 — Report (default) or plan and isolate (apply)

`MODE=report` — orchestrator only, writes nothing to the repository, not even a deletion that looks
obviously safe:

- **The report**, grouped by theme, leading with the finding count per risk grade. It is an artifact you
  hand back to the captain — printed in the session, posted on the run's issue or PR, or written to a
  scratch path that is not part of the repository's tracked content. Never a write into the tree under audit:
  `MODE=report` runs no worktree, so there is no isolated checkout to write into, and a gitignored path
  under the repository is still the repository. When you want the run diff to survive between runs, write it
  where it persists (`$TMPDIR` is not durable) and name that location in the report.
- **The JSON ledger** carried alongside the markdown report — the same findings and stable IDs, machine
  readable, so two runs can be diffed, CI can consume them, and a later issue can reference a finding ID
  directly. It travels with the report and obeys the same rule: it is produced, not committed. Emitting it
  is what makes Stage 1's determinism checkable rather than aspirational.
- **The run diff** against the previous report: new, resolved, carried-over.
- **The confidence header** — whether coverage tooling exists, whether the repo has tests at all, and
  whether integration tests cover the affected areas. Any absence is stated before any finding, because
  it changes how every grade below should be read.
- **The coverage-of-the-scan footer** and the unscanned remainder from Stage 1.

Then **STOP**. Report mode changes nothing in the tree it audited — no file, no branch, no index entry.

`MODE=apply` — plan, then isolate:

1. **Select.** `safe` findings go to Stage 4; `review` and `architectural` findings queue for Stage 5;
   `protected` findings never leave the report.
2. **Chunk.** Group by theme, then by risk grade (safe first, building confidence), then by feature
   boundary so each chunk has one owner and one rollback scope. A large census ships as several reviewable
   PRs, not one unreviewable one.
3. **Isolate.** Resolve `<WORKTREE_DIR>`, gitignore `.shipmates/worktrees/` idempotently when
   `WORKTREE_LAYOUT=nested` (append only when missing; never rewrite unrelated rules), then:

```bash
mkdir -p "$(dirname "<WORKTREE_DIR>")"
git -C <repo> worktree add <WORKTREE_DIR> -b <BRANCH> origin/<BASE_BRANCH>
```

Every edit from here on lands inside `<WORKTREE_DIR>`; the base branch stays clean and the caller's
checkout is left as they left it.

---

## Stage 4 — Apply the safe fixes  (agent: `senior-engineer`)

Nothing is deleted until this contract is satisfied. It is the command's central policy.

**Test-confidence contract — the gate on every deletion:**

1. **Green baseline or no cleanup.** Run the full suite on the clean checkout first. A red baseline **stops the run before any edit** — you cannot tell "my cleanup broke it" from "it was already broken". Report the pre-existing failures and route them (Stage 5's hand-off rules).
2. **Baseline coverage snapshot.** Before the first deletion, record per-file and per-function coverage from the repo's coverage tool. That snapshot is the contract the rest of the run is measured against.
3. **No deletion that lowers surviving-code coverage.** A dead-code deletion may legitimately lower the repo-wide percentage (fewer lines remain), but it must not lower the coverage of the code that remains. If deleting one function removes the only test that exercised another, that finding is `review` or `architectural`, not `safe`.
4. **Test-orphan detection.** For every deleted symbol, check whether tests called it. A helper unused in production but exercised by tests is `test-infrastructure` — never auto-deleted. When two production functions were deleted together and a test reached the second only through the first, verify the second was never the thing under test.
5. **Coverage-gap reporting.** If coverage of surviving code drops below the baseline, the Stage 6 gate fails and the PR body lists baseline %, post-cleanup %, and every file whose coverage fell.
6. **Test validity, not just coverage.** Flag tests that assert nothing ("did not throw", trivial identity) and paths whose only real exercise came from a mocked dependency — `mock-heavy` survivors may pass while the integration breaks.
7. **Bounded mutation testing, where the repo has it** — a bounded subset on the changed modules. A test that passes with a constant flipped or a branch inverted is not a safety net.
8. **Flaky is not evidence.** A test that flips cannot verify a deletion; the deletion it covers drops to `review` at best. A green gate resting on a coin flip is worse than a red one.

**Protected classes are restated here on purpose**, because this is the stage that deletes: no finding in
the protected list below may be applied by you, whatever its grade, whatever the evidence, whatever the
brief. If a protected finding reached your manifest, stop and report it rather than acting on it.

- Database migrations; error-path, fallback and graceful-degradation handlers; version-compatibility
  shims; feature-flagged code for unreleased work; public API with external consumers; framework entry
  points and lifecycle hooks; targets of dynamic dispatch (`eval`, reflection, string-keyed dispatch,
  serialization, config); and code referenced only from docs, issue templates or CI config.

Then apply:

- Under `EXECUTION=fanout` (default), spawn one `senior-engineer` per theme **in a single message** —
  concurrent, up to `MAX_CONCURRENT_WORKERS` — each with a machine-checkable **owned-paths manifest**
  (diffed against `git status` and `git diff --name-only` on return, so two builders cannot collide).
  Under `EXECUTION=sequential`, walk themes one at a time.
- **Override the agent's default minimal-diff instinct explicitly in the brief.** This is the inverse of
  `/ship-issue`: the deletion or consolidation *is* the assignment, so the builder removes what the finding
  names rather than trimming around it. What stays bounded is the theme — no finding outside the manifest,
  no opportunistic tidying.
- Remove dead code, inline pass-through wrappers, drop unused imports, consolidate repeated literals —
  whatever the theme names — preserving observable behaviour and matching the surrounding style.
- After each theme, re-run the suite and the coverage snapshot. A theme that lowers surviving-code
  coverage, or that a flaky test is the only thing vouching for, is **reverted and its finding downgraded
  to `review`** — never softened to make green.
- **Commit per theme.** Each commit covers one theme, so one objection costs one `git revert` and not the
  rest of the PR. Every message names the finding ID, the risk grade, and a
  one-line rationale, e.g. `chore(dead-code): drop unused <symbol> (safe; no callers, no dynamic
  reachability) — finding <ID>`. Record every applied finding, and every theme the builders refused, with
  the reason. Under the default fan-out the themes share one branch, so "green alone" is claimed only where
  the run verified it: when a theme's revert is clean against the branch, say so; when the themes interlock,
  record that too and treat the revert as a whole-branch operation.

---

## Stage 5 — Review-grade fixes and generic extraction  (agents: `architect` design + `senior-engineer` build)

`MODE=apply` only. This is the highest-leverage half of the command — a smaller codebase, not a longer
to-do list — and the half best able to hide a behaviour change, so it is gated hardest.

**Review-grade fixes, one at a time.** Apply each `review` finding on its own with a justification comment
in its commit. Anything the builder is unsure about **stays out of the PR** and is listed in the
intentionally-skipped log with its reason. A skipped finding is a valid outcome; a silent one is not.

**`architectural` findings — the procedure, stated once.** An `architectural` finding is applied only when
all three hold: the `architect` has specified the change to the boundary or contract it crosses, the
`product-manager` has signed off its feature impact, and the change carries its migration or deprecation
plan where one applies (a public-surface removal gets the deprecation cycle below, not a deletion). Where
those cannot all be met inside this run, the finding is **reported and left in place** — that is the
expected outcome for most `architectural` findings, and it is why Stage 6 treats a surviving
`architectural` finding as correct rather than as a miss.

**Near-duplicate genericisation — semantic diff before any extraction.** The single most dangerous cleanup
action is hoisting "duplicated" code into a shared helper when the copies only *look* alike: one handles a
null and the other throws, one trims whitespace and the other does not, one retries and the other does not.
So the `architect` produces a **semantic diff** for every candidate group — inputs, boundary conditions,
error handling, side effects, and ordering — and the extracted helper must reproduce **each variant's
behaviour at each call site**, or those differences become **explicit parameters**. Never silently collapse
a difference: a genericisation that drops a null check is a behaviour change wearing a cleanup badge, and
no amount of green tests catches it if no test ever exercised the null path. A candidate group whose
variants cannot be reconciled at a call site is not a duplication finding at all. The `senior-engineer`
then refactors the call sites into the agreed shape, under the same owned-paths manifest and per-theme
commit discipline as Stage 4.

**Performance guard.** Any deleted wrapper that contained caching, memoisation, batching or pooling is
`performance-sensitive` — that behaviour may have been masking a hot path. Benchmark the affected entry
points before and after where the repo has fixtures; a regression above **5%** fails the gate unless the
`performance-engineer` signs off, and the numbers go in the PR body.

**Doc and API sync.** A deleted symbol still named in the README, the changelog, generated API docs, or an
inline example is either updated (when the change is mechanical) or flagged `doc-follow-up` for the owner's
wording — the PR never ships docs describing code that no longer exists. An exported symbol with any
external consumer gets a **deprecation cycle** (marked in one release, deleted in the next), not an outright
removal; a deletion that would violate that is `architectural` and needs a version-bump plan.

**Feature-to-code mapping and the PO veto.** Before a `review` or `architectural` finding ships, answer
"which features depend on this?" — a **call-graph reverse lookup** from the symbol to its entry points
(request handler, command, event consumer, public API), each mapped to a feature name from the README, the
tracker, or the repo's optional feature-map file. The resulting **feature-impact summary** is mandatory in
the PR body, grouped by affected feature: a deletion touching several features is `review`, one touching a
revenue-critical feature is `architectural`.

**The mapping may only raise a grade, never lower one.** A finding that arrives graded `review` or
`architectural` stays there even when the reverse lookup finds no feature — the lookup reads the repository,
which is exactly the evidence that was already missing when the finding was graded. "No identifiable
feature" is a *gap in the map*, not a proof of safety: `safe` is assigned in Stage 2 and nowhere else, so
there is no grade here for the mapping to lower. The `product-manager` must explicitly
sign off — "the listed features are acceptable to risk" — for every `review` and `architectural` finding,
or those findings stay out of the PR. No sign-off is a removal, not a warning.

**Stop and route rather than improvise.** Discovery finds things that are not cleanup; each ends the finding
here and is handed off with the evidence already gathered, so the captain does not re-discover it: a
"cleanup" that turns out to be a bug is `/ship-fix-bug`; a deletion that needs a behaviour change to make
tests pass is not cleanup at all, so stop; a genericisation needing a new abstraction with no precedent is
`/ship-refactor` or `/ship-spike`; a mechanical rename across many call sites is `/ship-migrate`.

---

## Stage 6 — Verification  ⛔ HARD GATE  (agent: `sdet`)

A fresh `sdet` verifies the worktree, not the builders' summaries. Every item below is a gate, not a report
line.

- **Suite green** on the worktree, run in full — not the changed subset only.
- **The coverage contract holds.** Compare against the Stage 4 baseline snapshot: no surviving file's
  coverage fell. Report baseline %, post-cleanup %, and any file that dropped. A drop fails the gate.
- **Flaky reality check.** Any surviving test that covers a deletion and flips on a second run means the
  deletion is unverified — downgrade it to `review` and pull it from the PR.
- **Analyser re-run.** Re-run Stage 0's analysers over the branch. What remains should be only the
  `architectural` findings deferred and the `review` findings intentionally skipped; a finding graded `safe`
  that is still present means the apply stage missed it — fail and loop.
- **Integration tests for the affected entry points.** Where the repo has none for a touched feature, flag
  `integration-gap` in the PR body: the PO decides whether to accept the risk, and the gate does not pass
  silently over the hole.
- **Smoke / end-to-end rerun** wherever the repo defines one. These are slow, so they may run in parallel
  with the Stage 7 board — but a failure is a `REJECT`, not a footnote.
- **Bounded loop.** A failing gate reverts the offending theme (per-theme commits make that one `git
  revert`), downgrades the finding, and re-runs — bounded by `MAX_FIX_ROUNDS`, after which the run stops and
  escalates with the failing evidence. Never advance a red gate.
- **Empirical questions go to whoever can run code.** If an item is uncertain, measure it; do not resolve it
  by argument.

---

## Stage 7 — Review board and deliver

<!-- shipmates:acceptance-board -->

**Deferral check.** With `board=epic-deferred` or `board=off` set (the shared acceptance-board delegation
modes), skip the board and go to delivery; `epic-deferred` must name the milestone board that will run the
review, and `board=off` is recorded loudly in the report and the PR body.

**Command-specific seats** (in addition to the mandatory `principal-engineer` + `product-manager` core, and
the mandatory PO sign-off on the feature-impact summary from Stage 5):

- `architect` — the structural impact of what was deleted and extracted: did each deletion respect a
  boundary, and was the genericisation worth its diff, or churn that moved the problem?
- `sdet` — verification: suite green, coverage contract held, integration gaps named, no deleted code that
  was secretly tested.
- `performance-engineer` — **only** when a `performance-sensitive` finding was applied; binds the change to
  the benchmark numbers or rejects it.
- **Harness fallback guardrail:** if a role does not resolve to a shipped crew role, fall back to a
  general-purpose agent with the role's brief inlined and note the fallback in the report — never silently
  drop a seat.

Any `REJECT`/`FAIL` → fix, re-push, re-run the CI gate, then **Retry** the board from the fixer delta
(shared rule), bounded by `MAX_FIX_ROUNDS`, then escalate.

**Dry-run before the push.** Produce the complete prospective diff and the finding ledger **first**, before
any branch is pushed — that artifact is what lets the captain narrow the scope, drop a theme, or defer an
`architectural` item without rewriting git history.

**Deliver.** Commit per theme (Stages 4–5), push `<BRANCH>`, open the PR (or, under `MERGE_MODE=auto`, merge
it once green). The PR body carries: counts by theme and risk grade; what was deleted; what was genericised
and the semantic differences that became parameters; the feature-impact summary; the
**intentionally-skipped log** — every finding not applied, with its grade and its reason; every
`integration-gap` and `doc-follow-up`; every `architectural` finding, marked applied under the Stage 5
procedure or reported and left in place; baseline vs
post coverage; and the green-CI link. Then **the CI gate**: poll `gh pr checks` until nothing is pending. A
red check means pulling the failing log, fixing, re-pushing, re-polling — bounded by `MAX_FIX_ROUNDS`, then
escalate with the log. Never advance a red PR.

**Post-merge watch** (opt-in `watch`). Re-run the smoke suite on the merged commit on the default branch,
and compare error-rate and log-severity metrics for the affected entry points where the project has them. If
the watch fires, the PR is **reverted as one unit** — which is exactly why per-theme commits matter — and
the finding is re-graded.

**Report:** the mode that ran, the scan-coverage footer and the unscanned remainder, the grade counts,
findings applied vs intentionally skipped, the verification results, and the PR link. File bugs found and
not fixed as follow-up issues.

---

### Guardrails

- **`report` is read-only, and that is the whole point.** No write into the tree under audit of any kind — no `Write`, no `Edit`, no `git` mutation, and no working-tree write via `Bash` either; the report and its ledger are produced and handed back, never committed. `allowed-tools` still carries `Write`, `Edit` and `Bash` because `apply` needs them — the mode is the gate.
- **`apply` is opt-in, and the worktree is the isolation.** Edits land on a branch in a worktree; the caller's checkout is left as they left it. Staging is per-path (never `git add -A`), since the tree may hold unrelated work.
- **Nothing is deleted without a caller audit** — reflection, dynamic import, serialization, config keys, and framework entry points are all checked, even for `safe` findings.
- **Verdicts are evidence, not impressions.** Every finding row names its evidence; every grade names the rule that produced it.
- **Nothing is dropped silently.** Intentional skips, integration gaps, doc follow-ups, unscanned paths, and flaky-test caveats are all listed loudly in the report and the PR body.
- **The orchestrator owns all git/gh.** Builders edit files and commit; they never push or open PRs.
- **Bounded loops; a fresh reviewer.** Fix rounds are capped; the `sdet` gate and the board run against the pushed head, not a builder's claim.
- **Stay in scope.** A cleanup PR that quietly fixes a bug, renames an API, or adds an abstraction with no precedent has left the class — route it (Stage 5) instead.

**Robustness policy — the ten rules this run enforces**, stated once, so every stage above can be read
against it:

1. **Green baseline or no cleanup.** A red suite stops the run before any edit.
2. **No deletion on unverified coverage.** Surviving-code coverage must not drop; no coverage tooling means
   nothing grades `safe`.
3. **No genericisation without a semantic diff.** Behaviour differences between near-duplicates are
   parameterised or preserved, never collapsed.
4. **Structurally protected classes are never auto-deleted** — migrations, error paths, compatibility shims,
   unreleased flags, public API, framework hooks, dynamic-dispatch targets, docs-referenced code.
5. **Test validity, not just coverage.** Assertion-free tests, mock-only paths and flaky tests do not count
   as evidence, and their presence downgrades a finding's grade.
6. **Feature impact is stated before the PO signs.** Every `review`/`architectural` finding names the
   features it can affect.
7. **The PO seat is mandatory** for `review` and `architectural` findings, and the feature-impact mapping
   may only raise a grade — never lower one; no sign-off means the finding stays out.
8. **Every commit covers one theme and is independently revertible**, so one objection costs one `git revert`
   rather than the rest of the PR; "green alone" is claimed only where the run verified it.
9. **Nothing is dropped silently.** Intentional skips, integration gaps, doc follow-ups, unscanned paths and
   flaky-test caveats are all listed loudly in the PR body.
10. **The command stops at its hand-off boundaries** — bugs, behaviour changes, new abstractions and
    mechanical sweeps route to the command that owns them.

## Parameters

| Name | Required | Token | Values | Default | Help |
|------|----------|-------|--------|---------|------|
| scope | no | free text | path / module / package | whole repo | Narrow the survey to one subtree; empty audits everything. |
| language | no | `language=<name>` | a language name | all detected | Narrow a multi-language repo to one language's files. |
| apply | no | `apply` | apply | report | Fix the `safe` findings in a worktree and open a CI-green PR; otherwise read-only. |
| sequential | no | `sequential` | sequential  /  (absent→fanout) | fanout | Run discovery shards and fix themes one at a time instead of fan-out. |
| board | no | `board=epic-deferred`  /  `board=off` | full  /  epic-deferred  /  off | full | Defer the acceptance board to a milestone, or captain opt-out with no deferral target. |
| worktree_root | no | `worktree-root=sibling` | nested  /  sibling | nested | Under `apply`, use legacy sibling worktree paths. |
| merge_mode | no | `MERGE_MODE=auto` / `auto` | manual  /  auto | manual | Under `apply`, merge the PR when CI is green. |
| watch | no | `watch` | on  /  off | off | Under `apply`, re-run the smoke suite on the merged commit and revert as one unit if it fires. |
| budget | no | `budget=<n>` | a finding count or wall-clock | repo default | Stop cleanly at the cap and report what was not scanned. |

## Runtime input

Read `## Parameters` first. `$ARGUMENTS` is the captain-supplied or post-intake token string; parse
Tokens/Values from that table (required then optionals). If intake ran, treat the restated invocation as
authoritative for this run.

The scope tokens come first: a path, module, or package to audit, optionally followed by a `language=`
filter for a multi-language repo. The word `apply` switches `MODE` to `apply` and opts into the
worktree-and-PR path — without it the run is `MODE=report` and changes nothing on disk. Empty means the
**whole repository in `MODE=report`**. Guidance tokens (`sequential`, `board=epic-deferred`, `board=off`,
`worktree-root=sibling`, `watch`, `budget=…`) are stripped from the scope description when present; remaining
free text is part of what to audit. When in doubt, report only — deleting someone's code is a decision the
captain makes, not the default.
