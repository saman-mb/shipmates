# Prompt cost discipline

Prompt cost is a design constraint. Spend context on decisions that change the outcome; keep
repeated instructions and low-signal output out of the main context.

## Six principles

1. **Amortize fixed overhead.** Bundle cohesive, low-risk work when one plan, worktree, validation
   pass, and review can serve it. Never bundle unrelated work merely to reduce token count.
2. **Value-gate seats.** Add a specialist only when the change can plausibly benefit from that
   decision. A gated-out reviewer is an explicit result, not an omission.
3. **Route by difficulty.** Use the cheapest capable model and effort for mechanical work; reserve
   stronger reasoning for planning, architecture, security, and acceptance decisions. Choose this at
   spawn time, never in canonical content. The **Model routing** section below states the mechanism:
   the discovery ladder, the resolution order, and the audit line every spawn reports.
4. **Keep prompts cache-friendly.** Put stable instructions and role context first. Put invocation
   arguments, issue bodies, diffs, and other volatile material in one runtime-input section at the
   bottom. Keep a shared stable prefix before role-specific instructions across subagent spawns.
5. **Return decisions, not transcripts.** Require compact structured output: status or verdict first,
   criterion-level findings and only supporting evidence next, then blockers, changed files, rationale,
   and next action. Do not return command logs or a narrative of every step.
6. **Avoid paid repetition.** Reuse context and results already proven in the current run. Repeat a
   check only when new information or a changed artifact makes it decision-relevant; record what was
   checked rather than replaying a transcript.

## Reusable command preamble

The marker below is expanded into every rendered command. Keep this block short and stable: command
authors reference it instead of copying cost rules into each workflow. **Model routing** is the one
deliberately large member: it is policy every command needs — each drives the crew differently — so it
globalises here rather than being opted into per command, and its size is tracked in #450.

<!-- command-preamble:start -->
## Cost discipline

- Stable workflow instructions come before runtime input. Read and parse the complete runtime-input
  section at the end before acting; do not weave volatile issue text, arguments, diffs, or generated
  output through this prefix.
- **Complexity-Based Tiered Execution**: Before starting the workflow, evaluate the task complexity based on the input and repository context to select one of three execution paths:
  - **Simple**: Minor/straightforward changes (e.g. documentation, typos, single config line, small edits affecting <= 2 files and <= 15 lines of code, no specialist flags). The main agent (you) executes, validates, and delivers the PR directly — but **must still convene the mandatory PE+PO acceptance board** on the pushed head (see shared board below). Cost savings come from skipping Planner/Builder spawns and optional specialists, not from skipping review.
  - **Medium**: Moderate changes (<= 5 files, no major module boundaries, no architectural/security/delivery flags). Spawn a Planner and a single SDET and a Builder — one Builder per independent file-disjoint slice when the plan has them (see the command's execution-mode config), a single Builder otherwise; skip Stage 1.5 design specs when no flags apply. **Must convene PE+PO** (and SDET on the board when validation is non-trivial) — not main-agent review.
  - **High**: Complex or high-risk changes (e.g. major refactors, architectural boundaries, security/delivery changes). Follow the full multi-agent process loop described in the command, including Stage 1.5 when flagged and scaled optional board seats.
- Spend subagent seats only where their decision can change the outcome. Route model and effort at
  spawn by work difficulty; never hardcode a model in canonical content.
- Ask every subagent for a compact structured return: decision/status first, criterion findings and
  minimal evidence, then blockers, changed files with one-line rationale, and next action as relevant.
  Return decisions, not transcripts or raw logs.

<!-- shipmates:model-routing -->

<!-- command-preamble:end -->

## Reusable acceptance board

The marker below is expanded into every command that convenes an acceptance review on a pushed PR
head. Command authors reference it instead of copying board rules into each workflow.

<!-- acceptance-board:start -->
Spawn reviewers **in parallel** against the PR head commit — they review exactly what will merge.

**Mandatory seats (never skip)**

- **`product-manager`** (PO): checks every acceptance criterion AND the quality bar (README / {{project-instructions}} / contributing). Returns `ACCEPT` / `ACCEPT-WITH-NITS` / `REJECT` with specifics per criterion.
- **`principal-engineer`** (PE): principal-level diff review — correctness, edge cases, naming, test meaningfulness, scope discipline, security hygiene at review depth (not a `/ship-harden` pass). Verifies the PR satisfied the repo's **mandatory ship checklist** for this change class (regenerated generated pages, updated fixture digests, version/changelog when required, site validation, no hand-edited generated paths). Returns `ACCEPT` / `ACCEPT-WITH-NITS` / `REJECT` with `file:line` evidence.

Tiered execution may lean the build path on Simple/Medium, but **must not skip PE+PO** on the **first** board once a PR head exists. Later rounds follow **Retry** below — a PE/PO ACCEPT may be carried when the fixer delta cannot invalidate it.

**Delegation modes (the only two authorized exceptions to the mandatory seats above)**

- **`board=epic-deferred`** — a *deferral*, never a cancel. Set by an orchestrating command that owns a
  mandatory milestone board on the integrated artifact (e.g. `/ship-epic`'s Stage 4 integration board on `<EPIC_PR>`).
  The unit's own board is skipped, its CI gate still runs, and the milestone board reviews the integrated
  diff. The deferral is valid only while that milestone board is guaranteed; a delegated run must **not**
  convert it to `board=off`.
- **`board=off`** — an explicit captain opt-out: no board here and none deferred. The run records it loudly
  in the report and the PR body. Agents never choose it on their own.

Every board that is actually convened keeps the mandatory PE+PO seats and follows Retry below.

**Scaled optional seats**

Convene only when the change can plausibly trip the concern. A gated-out seat is **named in the report with its flag or reason** — never silently skipped.

| Seat | Join when |
|------|-----------|
| `sdet` | Medium+ code changes, or any change where validation is non-trivial. On Simple doc-only runs with a trivial validation plan, PE+PO may suffice — state which validation ran. |
| `architect` | `IS_ARCH_SIGNIFICANT` |
| `devops-engineer` | `IS_DELIVERY_SENSITIVE` |
| `technical-writer` | `IS_DOCS_AFFECTING` — doc copy/staleness (PE covers process compliance; both may run) |
| `ux-ui-designer` | `IS_UI_STORY` |
| `art-director` | `IS_VISUAL_STORY` |
| `security-engineer` | `/ship-pr-review` only when `IS_SECURITY_SENSITIVE` |
| `performance-engineer` | `/ship-pr-review` when the PR claims a perf win or touches a hot path; `/ship-refactor` when the stated motivation was performance |
| `site-reliability-engineer` | `/ship-pr-review` when runtime behaviour, failure handling, or rollout changes |
| `data-scientist` | `/ship-pr-review` when the deliverable is an analysis or model |

The `IS_*` flag vocabulary is shared by `/ship-issue` Stage 0 and `/ship-pr-review` Stage 0 — a new flag must be added to both classifiers.

**Decision**

- **All spawned reviewers ACCEPT/PASS (nits allowed)** → proceed to deliver / the command's next stage.
- **Any REJECT / FAIL** → remediation loop (where the command defines one), then **Retry** on the new head.

**Retry (after a fixer)** — do not clone the first-convene roster. Re-select from the **fixer delta** (commits since the last board), not a full Stage 0 redo:

1. **Must sit** — every seat that REJECTED / FAILED last round. They review the new head.
2. **Reassess, default off** — every seat that ACCEPTED (including PE/PO). Cheap look at the delta with the same `IS_*` flags, scoped to what just changed. Re-spawn only when that delta can invalidate their ACCEPT. Otherwise **carry the ACCEPT forward**.
3. **May newly sit** — a seat gated out last round joins if the delta newly trips its flag. Do not invent seats the flags never named.

When a seat is re-spawned, they review the **pushed SHA**. The report lists `re-run` / `carried ACCEPT` / `newly seated` / `still gated` — never a silent skip.

**Harness fallback**

If `principal-engineer` or any role does not resolve to an `{{agents-glob}}` file (skill-only harnesses until crew agents ship), fall back to `{{general-purpose}}` with the role brief inlined and note the fallback — never silently skip a mandatory seat.
<!-- acceptance-board:end -->

## Reusable epic integration board

The marker below is expanded into `/ship-epic` Stage 4 (epic closure) when every checklist story has landed.
It reviews the **combined** epic PR head — not a re-litigation of each unit PR.

<!-- epic-integration-board:start -->
Spawn reviewers **in parallel** against epic PR `<EPIC_PR>` head — the integration artifact that
would merge into `MAIN_BRANCH`. Pass each reviewer: the epic issue title/body, `<epic-log>`,
Stage 1.5 plan (when present), `<epic-capsule>`, and the full integration diff.

**This board is the deferral target.** Unit runs delegated with `board=epic-deferred` gate on green CI
and merge into `<EPIC_BRANCH>` without their own board; the mandatory PE+PO core (plus scaled seats)
convenes here, once, on the integrated diff. Never skip it on full closure — a deferral without this
board running is not a deferral, it is an unauthorised skip.

**Integration questions (mandatory lens — answer explicitly)**

1. **Coherence** — APIs, data model, copy, UX flows, error handling consistent across units?
2. **Continuity** — integrated head reads as one deliberate change, not independent PRs stacked?
3. **Epic value** — combined deliverable satisfies the epic goal, not only individual story boxes?
4. **Architecture** — module boundaries, dependency direction, public surfaces sane at epic scale?
5. **Engineering quality** — regressions, checklist compliance, cross-unit scope discipline on the
   integration head (not a line-by-line re-review of every unit unless integration introduced new risk)?
6. **Product / brand vision** — outcome and quality bar; does this epic ship something worth merging?
7. **Design continuity** — cross-unit visual/UX consistency when any unit touched UI or visual art?

**Mandatory seats (never skip on full closure)**

- **`product-manager`** (PO): epic-level `ACCEPT` / `ACCEPT-WITH-NITS` / `REJECT` against epic goal
  and quality bar — criteria above, especially value and continuity.
- **`principal-engineer`** (PE): integration diff review + mandatory ship checklist at epic scope.
- **`architect`**: join when Stage 1.5 flagged arch significance for any unit, **or** the epic has
  **≥2 code units** (units that merged non-doc-only changes), **or** integration review finds
  boundary/coupling concerns.

**Scaled optional seats**

Convene when the integrated epic plausibly trips the concern; name gated-out seats in the report.

| Seat | Join when |
|------|-----------|
| `ux-ui-designer` | Any unit was `IS_UI_STORY` or integration touches UI |
| `art-director` | Any unit was `IS_VISUAL_STORY` or brand/visual continuity matters |
| `sdet` | Re-run validation on epic head; confirm green CI matches integration reality |
| `technical-writer` | Epic materially changed docs across units — combined doc set coherent? |
| `security-engineer` | Any unit was `IS_SECURITY_SENSITIVE` or integration expands attack surface |
| `devops-engineer` | Any unit was `IS_DELIVERY_SENSITIVE` |
| Others | Same scaled rules as the unit acceptance board where relevant |

**Decision**

- **All spawned reviewers ACCEPT/PASS (nits allowed)** → proceed to Stage 4 finalize and captain handoff.
- **Any REJECT / FAIL** → fix on `<EPIC_BRANCH>`, push, re-poll epic PR CI, then **Retry** this board
  from the fixer delta (same rule as the unit acceptance board) — bounded by `MAX_FIX_ROUNDS`;
  exhaustion pauses the epic with integration blockers.

**Harness fallback**

Same as the unit acceptance board — never silently skip a mandatory seat.
<!-- epic-integration-board:end -->

## Reusable subagent preamble

Adapters expand this marker before each role's instructions, giving every subagent a stable common
prefix while preserving harness-neutral role content.

<!-- subagent-preamble:start -->
## Return discipline

- **Plan and brainstorm first.** Before editing files or executing major actions, formulate a clear, step-by-step plan. If instructions are ambiguous, surface questions rather than guessing.
- **Ingest project context (`{{project-instructions}}`).** Always consult the repo's `{{project-instructions}}` as the primary source of truth for build commands, test runners, code style, and conventions.
- **Leverage Git history.** Utilize `git log` and `git blame` on relevant files to understand historical rationale, linked issues, or past patterns before making changes.
- **Direct CLI discovery.** When invoking unfamiliar local build, test, or deployment tools, run `--help` or inspect tool configurations instead of guessing argument flags.
- **Return discipline.** Return one compact structured result, not a transcript. Lead with `STATUS` or `VERDICT`; include only criterion-level findings (`CRITERION: result — evidence`) and evidence needed to support it; finish with `BLOCKERS`, `CHANGED`, `RATIONALE`, and `NEXT` fields when applicable. Omit raw command logs and narration of routine steps.

<!-- subagent-preamble:end -->

<!-- model-routing:start -->
## Model routing — what the orchestrator may pick from

**Tier first, then effort — two separate decisions.** A spawn names one of two neutral tiers:
`mechanical` (the cheapest capable model) or `judgment` (the top model available). Complexity scales the
tier — `complex` moves up, `trivial` moves down, `standard` holds the role's baseline — and the effort
level is chosen at the same time, but as its own decision. Neither tier nor effort is ever baked into
canonical content: both resolve at spawn time, on the harness in front of you.

**Which tier.** A role sets the **baseline** tier and the work unit's complexity scales it:

- **`mechanical`** — building, test and validation runs, straightforward fixes: the cheapest capable
  model, low effort.
- **`judgment`** — planning, architecture, security review, and the acceptance call: the top model
  available, higher effort.
- A `complex` unit moves the tier up and a `trivial` unit moves it down; `standard` holds the baseline,
  so a hard task on a mechanical role is not left cheap and a trivial task on a judgment role is not
  overpaid. When a unit's tier is genuinely unclear, inherit the session model rather than guess one.

**The discovery ladder — three tiers, walked in order.** A tier narrows what you may pick from; it ends
the walk early only if it produced a **ranked** pool:

1. **Query** — where the target documents a non-interactive enumeration command, run it. This yields
   **candidates only**: it reports which models exist, never which one is cheap or which one is best, so
   it never terminates the ladder on its own.
2. **Declared** — a membership **filter** where the target documents a native allow-list of its own (a
   managed/policy settings allow-list, or a repository-root allow-list file with a single fallback
   directive), **intersected with the user's declared pool, which supplies the rank**. A filter alone
   never picks a tier. The declared pool is a **ranking over user-chosen patterns**, not a list of
   names: the project file `<repo>/.shipmates/model-pool.json` wins over the user file
   `~/.shipmates/model-pool.json`, and its shape is `schema_version` (`1`), `tiers.mechanical[]`,
   `tiers.judgment[]`, and optional `effort.mechanical` / `effort.judgment` on the neutral
   `low` / `medium` / `high` scale. Entries are patterns the target's own model surface accepts.
   **Treat every entry as one literal argument** — pass it to the harness surface as a single value,
   never spliced into a shell string or a command line. Shipmates never writes a value into either
   file. A pool file that is present but unusable — an unrecognised `schema_version`, malformed keys,
   unreadable — is a reported one-line warning in the run report, never a silent absence.
3. **`inherit`** — the terminal fallback: run on the parent/session model. This is a deliberate
   answer, not a failure, and it is always available.

**A pool is required even when the pool is enumerable.** A query answers *what exists*; it never
answers *what is cheap* or *what is top*. No harness documents a relative-capability ladder, so with no
declared tiering the orchestrator falls back to `inherit` — and **never infers a capability order**
from a listing, a price page, or a naming convention.

**Resolution order — stated once, for every target.** The levels, in order:
explicit spawn value → declared default → parent/session value → the model's own effort default.
A model chosen without an effort gets **that model's own default effort**, never the parent's effort
carried across a model change: one model's effort scale does not describe another's. Resolve the pool
once per run and reuse it for every spawn in that run; re-resolve only when a surface fails. Where a
level of the order does not exist on a target, its row in the per-target table below says so.

**Never guess.** An unknown, empty, or unreadable pool produces `inherit`, recorded as
`inherit (no pool)`. A concrete model identifier is never a fallback, never a default, and never an
example. An enumeration command that exits non-zero, or whose output cannot be parsed, leaves the pool
unknown: continue down the ladder.

**Enforcement.** An identity outside the resolved pool is **refused**, not quietly clamped — resolve a
different candidate, or stop and report. Not every target's documented mechanism can hold that:
`abort` refuses · `fallback` means the harness substitutes, which the audit line reports as
`substituted` · `none` means no native mechanism exists, so the orchestrator self-enforces or falls to
`inherit`. The enforcement column in the table below carries each target's value, and the run **states
the discrepancy in the report** instead of pretending enforcement held.

**Audit — one `MODEL ROUTING:` line per spawn.** Every spawn adds one compact line to the run report,
shaped `MODEL ROUTING: <role> tier=<mechanical|judgment> pool=<project|user|harness-native|inherit> model=<identity the harness accepts> effort=<requested>→<resolved> <honoured|substituted|inherit>`.
Substitution is reported **as** substitution, never as honoured: when the harness's own rules replace
the requested identity — an admin block, a plan limit, a hard environment override — the line records
`substituted` and names the condition that fired.

**Drift with no new release.** The installed command is a snapshot; the harness is not. Verify a surface
exists before relying on it — run the enumeration command once, or check the flag in the target's own
help output — and treat every step above as degradable: a vanished enumeration command falls through to
the declared pool, a missing or unreadable pool falls through to `inherit`, and a target with no row in
the table below is treated as no-enumeration → declared → `inherit`.

**Per-target surface.** Discovery tier, the override mechanism the harness documents, the enforcement
it can actually hold, and the effort surface with its clamp. No cell is ever blank: a missing feature
is a stated finding.

| Target | Discovery tier | Override kind | Enforcement | Effort surface and clamp |
|--------|----------------|---------------|-------------|--------------------------|
| claude-code | declared | per-spawn, over a documented session/frontmatter/env chain | fallback · the interactive switch rejects, other surfaces substitute | separate key · a 5-step depth scale plus a non-model orchestration pseudo-level; an unsupported level clamps down |
| opencode | query | static agent file, plus a global config value and a session flag | none · provider-level exclusion only, silent | separate key · provider-defined vocabulary, no fixed enum, plus a run-level variant preset |
| antigravity | query | session-level; no per-agent model key | abort · an unknown run value exits non-zero | run-level · a 3-value flag, separate from the reasoning tier folded into the model slug |
| codex | query | per-spawn, with an agent-default layer and a static agent file beneath it | none · no documented allow-list | separate key · a 6-step scale, gated on model support |
| cursor | query | static agent file per subagent, plus a session-wide flag | fallback · admin, plan or legacy gates substitute a compatible model | folded into the model string · a bracketed effort parameter; accepted values are model-defined |
| github-copilot | declared | per-spawn, plus a static agent file and a settings override map | abort · an invalid allow-list is rejected before the run | separate key · three first-party vocabularies that do not match: a 5-name flag, a 4-name settings key, a free-string agent field |
| pi | query | session-level; the target ships no built-in subagents by design | none · scoping only, no abort | separate key · a 7-step scale with a per-model tristate support map; an unsupported level is clamped away |
| windsurf | inherit | session-level on the surface we target | none · admin-side filtering, no user file | none · only an interactive shortcut-bound cycle, not expressible non-interactively |

**Additive, never a substitute.** Routing refines tiered execution, it does not replace it: the tier is
still the primary cost gate, and pool discovery decides only **which** cheap model runs a mechanical
unit — never **whether** a lighter execution path is chosen.
<!-- model-routing:end -->

## Authoring checklist

- Keep the **Model routing** block the single statement of pool discovery: reference it by its marker
  instead of restating the ladder, the resolution order, or the per-target table inside a command.

- Put one shared preamble marker near the start of every command and keep its runtime input section at
  the end of the stable workflow.
- Give reviewers a status/verdict and one finding per acceptance criterion.
- Give builders changed paths and one-line rationale per path or group; list verification commands and
  results separately.
- Keep issue text, user guidance, diffs, and other untrusted or volatile data quoted and below stable
  instructions. Never introduce positional argument placeholders; `$ARGUMENTS` is the only command
  input token.

See [CONTRIBUTING.md](../CONTRIBUTING.md) for source and validation conventions.
