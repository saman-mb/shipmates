---
name: ship-deslop
description: Shipmates: Audit a codebase for health debt — dead code, duplication, redundancy, over-engineering beyond product need, inconsistency, bad patterns, misplaced files and layers, dependency and config rot — grade every finding by risk, then file the work as an epic with sub-issues. Never edits the codebase: it analyses, files tracked work, and stops at a human gate where the captain chooses what ships now and what waits.
argument-hint: <path-or-module> [file|ship] — no args: whole repo, analysis only
allowed-tools: Bash, Read, Agent, Grep, Glob
disable-model-invocation: true
---
# /ship-deslop — audit the debt, file it as an epic, let the captain choose what ships
<!-- shipmates:command-preamble -->

Every repository carries debt no issue tracks: an import nothing imports, two copies of one function
that have quietly drifted apart, a wrapper that only passes its arguments through, a subsystem with
seven internal states whose only caller reads a boolean, a feature flag that has read `true` since the
release after it shipped, a TODO older than the test runner around it. No analyser sees all of it and
no human wants to read the whole tree to find it. "Slop" here is any of the debt below — whatever
wrote it; the command judges the debt, not its author.

`/ship-deslop` inventories that debt, grades every finding by the risk of touching it, and
then **files it as an epic with sub-issues** — durable tracked work, not a report that dies with the
session. The captain reviews that epic, confirms the breakdown is sound, and only then chooses what to
fix now and what to leave for later.

**This command never edits the codebase.** It carries no `Write` and no `Edit`, so read-only is
structural here rather than a promise a mode makes. Fixes happen downstream, in the workflow that owns
shipping, after a human has agreed to them. That division is deliberate. A cleanup change is the most
dangerous kind: the compiler stays green, the tests stay green, and a feature breaks anyway because
the deleted path was the only one a customer actually used. The thing that catches it is a person
reading the proposal — so this command's real job is to make that reading *possible*: evidence on
every finding, work grouped the way the repository is actually owned, and nothing hidden.

This workflow is **discovery-driven** — you do not know what you will find until you look. If the ask
is already "rename X to Y across the codebase" or "swap dependency A for B", stop and run
`/ship-migrate`; if it is "change the shape without changing behaviour", that is `/ship-refactor`. The
scope and how far to go come from the Runtime input section at the end of this workflow.

---

## Config (override only if the repo needs it)

- `MODE` = `report` (default) — **analysis only**: inventory, grade, report. It writes nothing, to the
  tree or to the tracker. `file` — additionally file the epic and its sub-issues, then stop at the
  human gate. `ship` — continue past a confirmed gate into scope election and hand-off. Infer from the
  request; ambiguous → `report`, stating which mode ran.
- **Two write surfaces, named separately.** The working tree is never written, in any mode. The issue
  tracker *is* written under `file` and `ship`. A captain who asked for an audit did not necessarily
  ask for twenty new issues, so the tracker write is its own opt-in.
- `EXECUTION` = `fanout` — how discovery shards. `fanout` (default): independent, file-disjoint units
  run concurrently up to `MAX_CONCURRENT_WORKERS`; guidance `sequential` sets `EXECUTION=sequential`
  to run them one at a time.
- `MAX_CONCURRENT_WORKERS` = `5`.
- `BUDGET` = the run's ceiling on findings surveyed (or wall-clock, when the repo prefers). The run
  stops cleanly at it and reports what was **not** scanned — cleanup is an infinite well, and an
  unbounded sweep is this command's default failure mode.
- `MAX_SUB_ISSUES` = `12` — the ceiling on sub-issues filed in one run. A gate the captain cannot read
  is not a gate: past the ceiling the lowest-priority areas are summarised in the epic body and left in
  the ledger for a later run, and the report says so.
- Thresholds — repo overrides come from `{{project-instructions}}` or `.shipmates/deslop.toml`
  (`.shipmates/deslop-codebase.toml` is still read when the new file is absent, since it shipped under
  the command's previous name):
  `COMPLEXITY_LIMIT` = `15`; `DUP_SIMILARITY` = `80%`; `TODO_HORIZON` = `180d`; `ZOMBIE_HORIZON` = `90d`
  (commented-out blocks older than this with no linked issue).
- **Quality bar / test commands / coverage tooling** = whatever the repo's README /
  `{{project-instructions}}` / CI config states. Read them before Stage 0 grades anything. The
  orchestrator owns all `gh`; agents never file, comment, or push.

### The crew this command spawns — and why it is named at every spawn

This command's workers are **named crew subagents**, invoked by their `{{role-reference}}` — never a
general-purpose agent with a persona pasted inline. Every role below works read-only here: six by their
own definition, the seventh by an explicit brief — which is what lets the read-only promise hold while
the analysis is still parallel and specialist:

| Role | Sits for | Stage |
|---|---|---|
| `architect` | the semantic diff on near-duplicate candidates; structural impact of a boundary-crossing deletion; the placement census and its convention oracle | 1, 2, 4 |
| `sdet` | test-coverage and test-validity evidence — which tests actually pin the code a finding would touch | 1, 2 |
| `product-manager` | the feature-impact summary and the sign-off a `review` or `architectural` sub-issue carries as a criterion | 2, 4 |
| `site-reliability-engineer` | the `failure-path` four questions — hazard provenance, execution evidence, blast radius, upstream elimination | 2 |
| `performance-engineer` | the measurement behind a `performance-sensitive` finding, in the project's own units | 2 |
| `security-engineer` | dependency and supply-chain findings, and anything touching secrets, authz or untrusted input | 1, 2 |
| `technical-writer` | documentation-rot findings — docs describing code that has moved | 1 |

**Read-only is structural for six of those seven; the seventh needs its brief to say so.** `architect`,
`sdet`, `product-manager`, `site-reliability-engineer`, `performance-engineer` and `security-engineer`
carry no write capability in their role definition, so none of them can write whatever the brief says.
**`technical-writer` does carry one** — it is a writing role by trade — so when it sits for a
reported-only audit its brief must state plainly that it reports findings and writes nothing. Never spawn
it here with its default posture: a role that *can* write will, and a captain auditing a repository
read-only would have an unguarded write path they had no reason to look for. The orchestrator carries no
`Write` and no `Edit` of its own, so the command's headline promise holds only while every spawn's
effective permissions do.

**Every spawn names its role explicitly.** A spawn that says only "workers" or "the agents" resolves to
no crew at all: the harness has nothing to look up and falls back to a general-purpose agent, silently
losing the specialism this command depends on. Write the role at the point of spawn, as the siblings do —
`(agent: \`sdet\`)` on the stage, `Spawn a \`security-engineer\`` in the step — and when a role does not
resolve to a shipped crew role, fall back to a general-purpose agent with that role's brief inlined and
**name the fallback in the report**, never silently.

Which roles a run actually needs is decided by what the census finds: a repository with no dependency
manifest pulls no `security-engineer`; a census with no `failure-path` findings pulls no
`site-reliability-engineer`. Name the ones that sit in the report, and name the ones that did not with the
reason — a seat gated out is a stated finding, never a blank.

---

## Stage 0 — Scope, mode, and analyser detection

Parse the runtime input first, then bound the survey. **No arguments means the whole repository** — that
is this command's documented default, not a silent widening, and it is stated back on the first line of
the report alongside the mode. What must never happen is a run that *drifts*: once a path, package or
module is named, the survey stays inside it, and anything outside that was still scanned is reported as
out of scope rather than quietly included.

1. **Scope.** Empty means the **whole repository** — the default when the captain supplies nothing, and the common case. A path, package, or module token narrows the survey to that subtree; an optional `language=` token narrows a multi-language repo to one language's files. When either is given the survey stays inside it. Write the resolved scope down — every later stage prints it back.
2. **Mode.** The word `file` sets `MODE=file`; `ship` sets `MODE=ship`; anything else is `MODE=report`. State the resolved mode on the first line of the report, so nobody reads an audit as a change.
3. **Tracker check** (`file` and `ship` only). Confirm the issue tracker is reachable and its labels readable *before* the survey begins — discovering at filing time that there is nowhere to file wastes the whole run. Confirm too whether the host offers a parent/child sub-issue relationship; Stage 4 degrades explicitly where it does not.
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

Whatever the tools do not cover, the crew read for — each finding class to the role named in the roster
above: pattern inconsistency and architectural redundancy, dead feature flags, machinery out of proportion
to what it delivers, and comments describing code that has moved. Never reimplement a mature analyser —
orchestrate the ones that exist, and say which ones were missing.

---

## Stage 1 — Discovery census  ⛔ the contract  (agents: `sdet`, `security-engineer`, `technical-writer`, `architect` × N, parallel across classes)

**This census is the contract.** Every finding it records ends filed or explicitly excluded, with a
reason. Silent truncation is a failure of the run, not a smaller run.

- Run the Stage 0 analysers and complement them with agent-driven reading. Under `EXECUTION=fanout`,
  split the survey by finding class (or by subtree for a large repo) across named crew subagents up to
  `MAX_CONCURRENT_WORKERS` — spawn each with its role's `{{role-reference}}`, one `sdet` for the
  test-coverage and test-validity classes, one `security-engineer` for dependency and supply-chain
  classes, one `technical-writer` for documentation rot (briefed to report only — see the roster
  above), an `architect` per structural or
  near-duplicate class — each returning rows in the one shape below. **A generic worker is not a
  substitute for a named role here**: the class a worker is assigned is the specialism it must bring.
- Every finding is one row: **stable ID**, `file:line`, **class**, a one-line rationale, the evidence
  that produced it, and the proposed action. A row without evidence is a hunch, not a finding.

The survey covers these classes — the list is the whole of what "cleanup" means here:

- **Dead code** — unused functions, imports, variables, unreachable branches, private symbols with no internal callers, and a state or guard unreachable by construction rather than by control flow.
- **Duplication** — exact clones and near-duplicates that could become one shared helper; read-aware, not a token hash, so two blocks that merely look alike are a *candidate group*, never a conclusion.
- **Redundancy** — unnecessary abstractions, wrappers that only pass arguments through, indirection that adds no name value, intermediate variables that restate their expression, a seam with exactly one implementation, and an extension point nothing registers against.
- **Over-engineering beyond product need** — machinery that distinguishes more internal states, options or
  outcomes than any consumer acts on. The signal is a ratio: count the distinct states, modes or outputs a
  unit produces, then count the distinct behaviours consumers actually branch on. Its shapes are an
  optimisation with no measurement behind it, an in-house rebuild of a capability the platform already
  provides, and data modelled richer than any use. Evidence is both counts and the consumer sites, never an
  impression that the code feels heavy — and the protected list still outranks this class.
- **Inconsistency** — mixed concurrency or callback styles in one codebase, naming drift, error handling that throws at one call site, returns at another, and logs-and-continues at a third.
- **Structural placement** — symbols and files in the wrong module, layer or folder for the repository's own stated architecture: a layer importing inward past its boundary, logic in a presentation or entrypoint file that belongs in the application layer, cross-cutting policy parked inside one feature, a catch-all `utils` / `helpers` / `common` bucket, single-file folders and folders one level deeper than anything needs, and sibling modules that disagree about their own internal layout. **The repository's committed convention is the oracle — never a general taste for how trees should look.** Two guards keep it honest: quote the convention from a committed file (`{{project-instructions}}`, an ADR, a lint rule, a codeowners entry) before recording any placement finding, and where no convention is committed the class degrades to *observed-majority-layout* findings, reported as such — this class never invents a folder taxonomy the project did not choose. Evidence is the convention it violates plus the import or call that proves the misplacement.
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
analysed, which were skipped, and **which classes were not looked for at all**. That last part matters
more than it looks: a footer that reports only *paths scanned* lets a run read as a clean bill of health
for classes it never asked about. When a class is narrowed or unavailable — no committed architecture
convention for structural placement, so it falls back to observed-majority-layout findings; no dependency
manifest, so dependency health has nothing to read — say which, by name, and say what the fallback was.
structural placement, no dependency manifest for dependency health, no coverage tooling for the grades
below — say so by name. What `BUDGET` left unscanned carries into Stage 3 alongside it.

---

## Stage 2 — Risk grade and protected classes  (agents: `sdet`, `product-manager`, `site-reliability-engineer`, `performance-engineer` — each only for the classes below)

Every finding gets exactly one grade, assigned mechanically where it can be and by judgement where it
cannot. The grade decides where the finding goes, so it is assigned here and nowhere else.

| Grade | Means | Typical trigger | Where it goes |
|---|---|---|---|
| `safe` | deleting it cannot change behaviour | unused private symbol, no dynamic reachability | its own sub-issue, ordered first |
| `review` | probably safe, needs a human call | exported symbol with no internal callers, inconsistency fixes | a sub-issue carrying PO sign-off as a criterion |
| `architectural` | changes boundaries or contracts | deletion crossing a module boundary, abstraction with no precedent, public-surface removal | a sub-issue carrying `architect` + PO sign-off |
| `protected` | structurally protected — never proposed for deletion | any member of the protected classes below | the report and ledger only, never a task |

Overlays travel alongside the grade into the report and the sub-issue: `test-infrastructure` (a helper
only tests call), `performance-sensitive` (caching, memoisation, batching, pooling, indexing, rate
limiting, backpressure), `failure-path` (retries, timeouts, fallbacks, degradation, health checks,
quorum), `flaky` (a test that cannot serve as evidence), `integration-gap` (no integration test covers
an affected entry point), `doc-follow-up` (docs need a human's wording), and `needs-review` (reported,
deliberately not proposed).

Grading rules:

- An unused **private** symbol with no dynamic reachability grades `safe`.
- An unused **exported** symbol, or one with no internal callers, grades `review`: nothing inside the
  repo can prove who imports a published package.
- A deletion that crosses a module boundary or changes a contract grades `architectural`.
- **A move is not a delete, so the grading differs.** A relocation that crosses a module boundary already
  grades `architectural`; state plainly that **a pure relocation with no call-site change still grades at
  least `review`**, because a move breaks downstream patches, `git blame` continuity and every path-based
  tool — CI path filters, codeowners, coverage config. Before proposing any move, **grep the moved path
  across CI config, codeowners, coverage config and docs**, so the blast radius is in the finding rather
  than discovered in CI. Relocating a file that external consumers import is a breaking change even when
  nothing inside it changed, so a public or exported path is a protected class *for moves specifically*.
- **No coverage tooling means nothing grades `safe`.** Every finding degrades to `review` at minimum and
  the report says so plainly — a confidence grade is a claim about verification, and without the tooling
  the honest claim is "unverified". Standing up coverage tooling is an explicit, opt-in prerequisite,
  never a silent install.
- **A repo with no tests at all runs `report` only and grades nothing above `review`**, leading the
  report with "this repo has no test suite; cleanup here is a judgement call, not a verified one". Never
  hand back a confident-looking list under those conditions.

**Three rules cap a grade, because a green check cannot see what it is missing:**

- **No over-engineering finding ever grades `safe`.** `safe` means deleting it cannot change behaviour;
  where a unit genuinely produces distinguishable outputs, that is the contested claim itself, not a
  conclusion. It grades `review` at minimum, `architectural` where it crosses a boundary or removes a
  public or persisted surface.
- **No `performance-sensitive` finding grades `safe`.** Machinery the product cannot see maps to zero
  features by construction, so an empty feature lookup is absence of product evidence, not proof of
  safety. The seat defending the machinery owes a number too — "it might be hot" is the same
  absence-of-evidence argument this command rejects for deletions. Judge a delta against the project's
  stated bar in its own units; where none is stated, report the tail as well as the mean with the
  run-to-run variance, and treat a delta inside the noise band as *unmeasured*, not clean. A flat
  percentage means opposite things on a batch path and a per-request one.
- **Working machinery is not dead machinery.** Code that handles a failure looks idle because it is
  succeeding. A `failure-path` finding answers four questions, and any unanswered one caps it at
  `review`: does history name the failure it was built for; has the path ever run, in a test or in
  production; if that failure recurs without it, is the cost recoverable, or is it loss, corruption,
  cascade or money; and if the hazard is gone, what handles it now. Unrecoverable cost is never proposed
  for deletion on evidence alone. **Never-executed is not a licence to delete** — it is a reason to
  instrument or test the path first, which becomes its own sub-issue and blocks the finding that depends
  on it. Theatre has no hazard provenance *and* no execution evidence *and* recoverable blast radius;
  anything with a named hazard, or observed execution, or unrecoverable blast radius is load-bearing.

**Protected classes — never proposed for deletion, and never proposed for relocation, reported with rationale instead:**

- **Database migrations** — an "unused" migration may be the only path that upgrades a deployed instance, and deleting it strands every environment that has not run it yet.
- **Error-path, fallback, and graceful-degradation handlers** — catch blocks, retries, circuit breakers and defaults look dead precisely because they only run when something goes wrong.
- **Version-compatibility shims** — code gated on an older runtime, protocol version, or dependency range the project still supports.
- **Feature-flagged code for unreleased work** — a flag that is `false` today is `true` at launch.
- **Public API with external consumers** — nothing inside the repo can prove who imports it.
- **Framework entry points and lifecycle hooks** — called by the framework, never by your code: route handlers, dependency providers, command registration, test fixtures, plugin hooks.
- **Targets of dynamic dispatch** — anything reached through `eval`, reflection, string-keyed dispatch, serialization, or a configuration file.
- **Code referenced only from docs, issue templates, or CI config** — grep is unreliable there, so treat a hit as `review`.
- **Public or exported paths, for moves specifically** — relocating a file external consumers import breaks them even when its contents are unchanged, so a move is never proposed for one on internal evidence alone.
- **Machinery whose failure is silent rather than loud** — integrity checks, idempotency keys, deduplication, reconciliation. Its absence does not announce itself, so no test going red will tell you it mattered.
- **Anything on a path that cannot be rolled back** — a destructive migration, an external side effect, a payment, an irreversible publish.
- **Load-shedding, rate limiting, and backpressure** — removing them harms a third party, which never appears anywhere in this repository's feature map.

Protected classes are discovered two ways, layered: conventions supply the defaults (a path matching a
migrations directory, a handler registered as an error path) and the repo supplies explicit additions and
overrides. Conventions alone risk a silent miss; an explicit list alone goes stale.

**The principle, stated once and applied throughout: absence of evidence is not evidence of absence.** A
symbol with no static callers is a *hypothesis* of deadness, and the burden of proof is on the deletion. A
protected class is therefore **never** proposed for deletion regardless of evidence — the command reports
it and stops, because that is the only default that cannot strand a deployed environment. The mirror of
that rule: the existence of defensive machinery is evidence someone judged a hazard real, and a linked
incident is evidence it *happened* — grade the strength accordingly, and where the belief was cargo-culted
or the hazard was eliminated upstream, say which and show it.

Reliability caveats set here travel into the sub-issue as acceptance criteria: a flaky test is not evidence
(run the touched subset twice, or read the repo's quarantine list, before trusting it, and report flakes as
findings without fixing them here); assertion-free tests and mock-only paths give false confidence and
downgrade the finding they cover.

**Near-duplicate candidates need a semantic diff before they are reportable.** The single most dangerous
cleanup action is hoisting "duplicated" code into a shared helper when the copies only *look* alike: one
handles a null and the other throws, one trims whitespace and the other does not, one retries and the other
does not. So **spawn an `architect`** to produce a **semantic diff** for every candidate group — inputs, boundary
conditions, error handling, side effects, and ordering — and the finding carries it, with each difference
named as an explicit parameter the eventual helper must take. Never propose silently collapsing a
difference: a genericisation that drops a null check is a behaviour change wearing a cleanup badge, and no
amount of green tests catches it if no test ever exercised the null path. A candidate group whose variants
cannot be reconciled at a call site is not a duplication finding at all.

**Feature-to-code mapping, and what the product owner is actually asked.** Before a `review` or
`architectural` finding is filed, answer "which features depend on this?" — a **call-graph reverse lookup**
from the symbol to its entry points (request handler, command, event consumer, public API), each mapped to
a feature name from the README, the tracker, or the repo's optional feature-map file. A unit that reaches
no observable terminus at all is the strongest finding in the census; a *failed* trace proves only that
this repository contains no consumer, never that the code is dead. The resulting **feature-impact summary**
is mandatory on every `review` and `architectural` sub-issue, grouped by affected feature: a deletion
touching several features is `review`, one touching a revenue-critical feature is `architectural`.

**The mapping may only raise a grade, never lower one.** A finding that arrives graded `review` or
`architectural` stays there even when the reverse lookup finds no feature — the lookup reads the repository,
which is exactly the evidence that was already missing when the finding was graded. "No identifiable
feature" is a *gap in the map*, not a proof of safety.

**Doc and API sync travels with the finding.** A proposed deletion of a symbol still named in the README,
the changelog, generated API docs, or an inline example carries that list into its sub-issue, or is flagged
`doc-follow-up` for the owner's wording. An exported symbol with any external consumer gets a **deprecation
cycle** (marked in one release, deleted in the next), not an outright removal; a finding that would violate
that is `architectural` and carries a version-bump plan.

---

## Stage 3 — The report

Produce both artifacts, in every mode, and write neither into the tree under audit:

- **The report**, grouped by area and risk grade, leading with the finding count per grade. It is an
  artifact you hand back to the captain — printed in the session, posted on the epic, or written outside
  the repository entirely. A repository whose own audit shows up as a diff has been changed by the thing
  that was supposed to be reading it. When you want the run diff to survive between runs, write it
  somewhere durable (`$TMPDIR` is not) and name that location in the report.
- **The JSON ledger** alongside it — the same findings and stable IDs, machine readable, so two runs can
  be diffed, CI can consume them, and a sub-issue can reference a finding ID directly. Emitting it is
  what makes Stage 1's determinism checkable rather than aspirational.
- **The run diff** against the previous report: new, resolved, carried-over.
- **The confidence header** — whether coverage tooling exists, whether the repo has tests at all, and
  whether integration tests cover the affected areas. Any absence is stated before any finding, because
  it changes how every grade below should be read.
- **The coverage-of-the-scan footer** and the unscanned remainder from Stage 1.

Under `MODE=report` the run **stops here**, stating plainly that these findings are not durable work and
what it would take to make them so.

---

## Stage 4 — Decompose into an epic with sub-issues  (agent: `product-manager` for the feature-impact sign-off)

`MODE=file` and `MODE=ship`. This is where the census becomes tracked work, or evaporates.

**File the epic and its sub-issues yourself, and attach the graph with `gh` — spell the mechanics out
here, never delegate them by reference.** `/ship-plan-epics` documents the same sequence; composing
its *planning panel* would re-derive scope from a brief and launder this census's evidence into prose,
so execute the filing steps below directly. The failure to guard against is silent: an epic body with
a tidy `- [ ] #N` checklist and **no parent/child graph** reads as filed, yet every downstream walk
that reads `subIssues` sees an empty parent.

1. **Labels** — `gh label create epic` / `gh label create user-story` when missing.
2. **Epic first** — `gh issue create` the epic (counts by grade and area, recommended build order,
   scan-coverage footer, unscanned remainder, finding-ID manifest, checklist placeholder). Capture
   `<epic>`.
3. **Sub-issues next** — `gh issue create` each sub-issue (finding IDs, evidence, grade and overlays,
   the acceptance criteria below, the per-ID trailer). Capture each `<sub>`. Validate every captured
   number against `^[0-9]+$` before it reaches a command.
4. **Attach every sub-issue to its epic with `gh`** — this step creates the parent/child
   relationship, and nothing else does. A body mention is a mention; a checklist line is display:
   ```bash
   gh issue edit <epic> --add-sub-issue <sub>
   ```
   Several children may be attached in one call as a comma-separated list. Read the parent's current
   children first (`gh issue view <epic> --json subIssues`; child numbers live at
   `subIssues.nodes[].number` — `subIssues` is a connection object, not a list) and skip any already
   attached, so a re-run adds nothing twice.
5. **Backfill the epic checklist** — replace the placeholder with `- [ ] #<sub>` lines for its
   sub-issues.
6. **Verify the graph** — re-fetch `gh issue view <epic> --json subIssues,subIssuesSummary` and
   confirm the child numbers are exactly the sub-issues filed. A missing child means step 4 did not
   land: retry it **once**, re-verify, then report the gap — never report the epic as fully filed on
   a checklist alone.

If the host has no parent/child sub-issue mechanic (the `--add-sub-issue` flag is rejected), keep the
checklist plus a `Part of #<epic>` back-reference on each sub-issue, and say plainly in the report
that the graph is missing — never fake it.

**How findings group into sub-issues.** One sub-issue = **one reviewable diff, one owner, one rollback
scope**, partitioned primarily **by subsystem or ownership boundary**. Tie-breakers in order: never mix
risk grades in one issue (a `safe` finding must not wait on an `architectural` one, and sign-off is per
grade); split anything above the repo's reviewable-diff norm; merge a single-finding issue into its area
sibling. **Finding class is a label, never a partition** — partition by class and the captain gets
"unused imports, 200 files, every subsystem, no owner", which is neither a reviewable unit nor a
rollback scope, and the gate below becomes a wall of noise instead of a decision.

**Three tiers, no overlap:**

- **A sub-issue** carries: the finding IDs; the `file:line` and symbol set; the evidence and which
  analyser produced it; the grade and its overlays; the acceptance criteria below; the feature-impact
  summary and whose sign-off it needs; and its **negative scope** — the adjacent findings that went
  elsewhere, so a builder does not widen. An over-engineering finding additionally carries the
  state-and-consumer counts, the simpler shape in one sentence, and the hazard answers from Stage 2.
- **The epic body** carries counts by grade and area, the recommended build order, the scan-coverage
  footer, the unscanned remainder, and the machine-readable finding-ID manifest. Summary only — evidence
  belongs on the sub-issue.
- **The ledger only, never tracked work:** `protected` findings, declined findings, and anything below
  the reporting threshold. A protected finding is still *reported* — it is intelligence the captain
  wants — but it never becomes a task.

**Acceptance criteria written into each sub-issue.** This command does not enforce these; it states them
where the downstream workflow's mandatory product-owner seat will check them, so write them
checkbox-shaped:

- Green baseline before any edit; a red suite stops the work rather than absorbing the blame for it.
- Protected classes are never deleted, whatever the grade, the evidence, or the brief.
- For a **deletion**: surviving-code coverage does not fall, and every deleted symbol is checked for test
  orphaning first — a helper unused in production but exercised by tests is `test-infrastructure`.
- For a **simplification** — a change that replaces code rather than removing it — **coverage is measured
  on behaviour, not on lines.** The baseline is the set of observable behaviours the unit's entry points
  exhibit, each named and pinned by a passing test *before* the change; the invariant is that every
  behaviour in that set is still asserted afterwards, by a test that fails when the behaviour breaks. Line
  and percentage movement is reported, never gated — a fall is expected, because coverage of machinery the
  product never needed was never worth what it cost to keep. Where the existing tests prove only
  machinery, writing the missing behaviour test against the *unchanged* code is the first commit, not an
  afterthought.
- Assertion-free tests, mock-only paths and flaky tests are not evidence, and their presence downgrades
  the finding rather than passing it.
- A `performance-sensitive` change carries its numbers, measured in **both** directions — removing
  indirection is sometimes faster, and the retention claim needs a number as much as the removal does.
- Docs, changelog, generated API docs and examples naming a removed symbol are updated or flagged; an
  exported symbol with an external consumer gets its deprecation cycle, not a removal.

**Idempotency.** Finding IDs live in the epic body's manifest and in a per-ID trailer on each sub-issue —
**never in titles**, because the whole point of the gate is that the captain edits titles. On a re-run: a
finding that still reproduces refreshes its `file:line` only; a new finding joins the **existing** epic,
keyed by scope rather than by run; a human-closed finding that still reproduces is not re-filed but marked
declined. A finding that no longer reproduces gets a comment proposing closure and is **never
auto-closed** — and that comment must distinguish *fixed* from *not scanned this run*, because a narrowed
scope silently closing a captain's issues is the worst thing this command could do.

**Degradation, one line each.** No tracker or no tracker CLI: emit the report and ledger, and say plainly
that the findings are not durable work. A tracker with no parent/child mechanic: inherit the composed
command's checklist-plus-back-reference fallback and report the gap rather than faking the graph. No
downstream shipping workflow installed: the epic still stands — name the *shape* of the next step, and
name workflows only as examples.

---

## Stage 5 — The human review gate  ⛔

**Stop here and hand the epic to the captain.** This is the point of the command. Print the epic link,
the sub-issue list with grade and area, the counts by grade, and the unscanned remainder.

Ask two questions, and keep them separate — conflating them is what makes a review exhausting:

1. **Is the breakdown right?** Are these findings real, correctly described, and grouped the way this
   repository is actually owned? Titles, grouping and scope are the captain's to edit, and any edit they
   make is authoritative over anything this run proposed — on this run and every re-run.
2. **What do you want fixed?** Only once the first question is answered.

A **non-interactive run stops here permanently**: it files the epic and never elects scope on the
captain's behalf. Silence is not confirmation.

---

## Stage 6 — Scope election and hand-off

`MODE=ship`, and only after the captain has confirmed the epic at Stage 5.

Offer the choice plainly: **the whole epic**, **a single sub-issue**, or **a named bundle of them**.
Order the offer by what the captain can act on — `safe`, single-owner work first; `architectural` and
cross-boundary work last — and name any blocking dependency, since an instrument-this-path issue must
land before the simplification that depends on it.

Then hand off, and **stop**. Deliver the exact invocation for the chosen scope: the epic workflow for the
whole epic, the single-issue workflow for one, and for a bundle, one invocation per issue in the order the
dependencies allow. Everything downstream — the worktree, the builders, the verification gate, the review
board, the pull request and its CI — belongs to that workflow, which already owns every one of them.
Rebuilding any of it here would be a second implementation of shipping, and a second place for it to be
wrong.

**Report:** the mode that ran, the scan-coverage footer and the unscanned remainder, the grade counts, the
epic and sub-issue links, what was filed versus left in the ledger, the captain's election, and the
hand-off. Anything discovered that is not cleanup is routed with the evidence already gathered, so the
captain does not re-discover it: a "cleanup" that turns out to be a bug is `/ship-fix-bug`; a missing guard
is `/ship-harden`; a genericisation needing a new abstraction with no precedent is `/ship-refactor` or
`/ship-spike`; a mechanical rename across many call sites is `/ship-migrate`.

---

### Guardrails

- **This command cannot edit the codebase, and that is structural.** `allowed-tools` carries no `Write` and no `Edit`, and no working-tree write goes through `Bash` either. The read-only promise is enforced by what the command *can do*, not by a mode agreeing to honour it. Six of the seven crew roles it spawns are read-only by their own definition; the seventh (`technical-writer`) is a writing role by trade and must be briefed to report only — see the roster above. A spawn's effective permissions are part of this promise, not a detail beside it.
- **Every spawn names its crew role.** A spawn that says only "workers" or "the agents" has nothing for the harness to resolve and lands on a general-purpose agent, silently discarding the specialism the finding class needs. Name the role at the point of spawn; where a role genuinely does not resolve to a shipped crew role, inline that role's brief and **say so in the report**. Never let a fallback pass unremarked — the run's whole value is that a specialist read the thing.
- **Two write surfaces, named separately.** The working tree is never written, in any mode. The tracker is written under `file` and `ship` only, and that is its own opt-in.
- **Nothing is proposed for deletion without a caller audit** — reflection, dynamic import, serialization, config keys, and framework entry points are all checked, even for `safe` findings.
- **Verdicts are evidence, not impressions.** Every finding row names its evidence; every grade names the rule that produced it.
- **Nothing is dropped silently.** Declined findings, protected findings, integration gaps, doc follow-ups, unscanned paths, and flaky-test caveats are all listed loudly in the report and the epic.
- **The orchestrator owns all `gh`.** Agents read and analyse; they never file, comment, or push.
- **The captain's edits win.** Anything changed at the gate outranks anything this run proposed, now and on every re-run.
- **Stay in scope.** An audit that quietly proposes a bug fix, an API rename, or an abstraction with no precedent has left the class — route it instead.

## Parameters

| Name | Required | Token | Values | Default | Help |
|------|----------|-------|--------|---------|------|
| scope | no | free text | path / module / package | whole repo | Narrow the survey to one subtree; empty audits everything. |
| language | no | `language=<name>` | a language name | all detected | Narrow a multi-language repo to one language's files. |
| mode | no | `file`  /  `ship` | report  /  file  /  ship | report | `file` files the epic and stops at the human gate; `ship` continues past a confirmed gate into election and hand-off. |
| sequential | no | `sequential` | sequential  /  (absent→fanout) | fanout | Run discovery shards one at a time instead of fan-out. |
| budget | no | `budget=<n>` | a finding count or wall-clock | repo default | Stop cleanly at the cap and report what was not scanned. |
| max_sub_issues | no | `max-sub-issues=<n>` | a count | 12 | Ceiling on sub-issues filed in one run; the remainder is summarised and left in the ledger. |

## Runtime input

Read `## Parameters` first. `$ARGUMENTS` is the captain-supplied or post-intake token string; parse
Tokens/Values from that table (required then optionals). If intake ran, treat the restated invocation as
authoritative for this run.

The scope tokens come first: a path, module, or package to audit, optionally followed by a `language=`
filter for a multi-language repo. The word `file` files the epic and its sub-issues and stops at the human
gate; `ship` continues past a confirmed gate into scope election and hand-off. Without either, the run is
`MODE=report` and writes nothing anywhere. Empty means the **whole repository in `MODE=report`**. Guidance
tokens (`sequential`, `budget=…`, `max-sub-issues=…`) are stripped from the scope description when present;
remaining free text is part of what to audit. When in doubt, report only — turning an audit into twenty
issues is a decision the captain makes, not the default.
