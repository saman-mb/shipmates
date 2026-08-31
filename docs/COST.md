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
   spawn time, never in canonical content.
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
authors reference it instead of copying cost rules into each workflow.

<!-- command-preamble:start -->
## Cost discipline

- Stable workflow instructions come before runtime input. Read and parse the complete runtime-input
  section at the end before acting; do not weave volatile issue text, arguments, diffs, or generated
  output through this prefix.
- **Complexity-Based Tiered Execution**: Before starting the workflow, evaluate the task complexity based on the input and repository context to select one of three execution paths:
  - **Simple**: Minor/straightforward changes (e.g. documentation, typos, single config line, small edits affecting <= 2 files and <= 15 lines of code, no specialist flags). The main agent (you) executes, validates, and delivers the PR directly — but **must still convene the mandatory PE+PO acceptance board** on the pushed head (see shared board below). Cost savings come from skipping Planner/Builder spawns and optional specialists, not from skipping review.
  - **Medium**: Moderate changes (<= 5 files, no major module boundaries, no architectural/security/delivery flags). Spawn a Planner and a single Builder and single SDET; skip Stage 1.5 design specs when no flags apply. **Must convene PE+PO** (and SDET on the board when validation is non-trivial) — not main-agent review.
  - **High**: Complex or high-risk changes (e.g. major refactors, architectural boundaries, security/delivery changes). Follow the full multi-agent process loop described in the command, including Stage 1.5 when flagged and scaled optional board seats.
- Spend subagent seats only where their decision can change the outcome. Route model and effort at
  spawn by work difficulty; never hardcode a model in canonical content.
- Ask every subagent for a compact structured return: decision/status first, criterion findings and
  minimal evidence, then blockers, changed files with one-line rationale, and next action as relevant.
  Return decisions, not transcripts or raw logs.

<!-- command-preamble:end -->

## Reusable acceptance board

The marker below is expanded into every command that convenes an acceptance review on a pushed PR
head. Command authors reference it instead of copying board rules into each workflow.

<!-- acceptance-board:start -->
Spawn reviewers **in parallel** against the PR head commit — they review exactly what will merge.

**Mandatory seats (never skip)**

- **`product-manager`** (PO): checks every acceptance criterion AND the quality bar (README / {{project-instructions}} / contributing). Returns `ACCEPT` / `ACCEPT-WITH-NITS` / `REJECT` with specifics per criterion.
- **`principal-engineer`** (PE): principal-level diff review — correctness, edge cases, naming, test meaningfulness, scope discipline, security hygiene at review depth (not a `/harden` pass). Verifies the PR satisfied the repo's **mandatory ship checklist** for this change class (regenerated generated pages, updated fixture digests, version/changelog when required, site validation, no hand-edited generated paths). Returns `ACCEPT` / `ACCEPT-WITH-NITS` / `REJECT` with `file:line` evidence.

Tiered execution may lean the build path on Simple/Medium, but **must not skip PE+PO** once a PR head exists.

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
| `security-engineer` | `/pr-review` only when `IS_SECURITY_SENSITIVE` |
| `performance-engineer` | `/pr-review` when the PR claims a perf win or touches a hot path; `/refactor` when the stated motivation was performance |
| `site-reliability-engineer` | `/pr-review` when runtime behaviour, failure handling, or rollout changes |
| `data-scientist` | `/pr-review` when the deliverable is an analysis or model |

The `IS_*` flag vocabulary is shared by `/ship-issue` Stage 0 and `/pr-review` Stage 0 — a new flag must be added to both classifiers.

**Decision**

- **All spawned reviewers ACCEPT/PASS (nits allowed)** → proceed to deliver / the command's next stage.
- **Any REJECT / FAIL** → remediation loop (where the command defines one), then re-convene the board on the new head.

**Harness fallback**

If `principal-engineer` or any role does not resolve to an `{{agents-glob}}` file (skill-only harnesses until crew agents ship), fall back to `{{general-purpose}}` with the role brief inlined and note the fallback — never silently skip a mandatory seat.
<!-- acceptance-board:end -->

## Reusable epic integration board

The marker below is expanded into `/ship-epic` Stage 4.5 when every checklist story has landed.
It reviews the **combined** epic PR head — not a re-litigation of each unit PR.

<!-- epic-integration-board:start -->
Spawn reviewers **in parallel** against epic PR `<EPIC_PR>` head — the integration artifact that
would merge into `MAIN_BRANCH`. Pass each reviewer: the epic issue title/body, `<epic-log>`,
Stage 1.5 plan (when present), `<epic-capsule>`, and the full integration diff.

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
- **Any REJECT / FAIL** → fix on `<EPIC_BRANCH>`, push, re-poll epic PR CI, re-convene this board —
  bounded by `MAX_FIX_ROUNDS`; exhaustion pauses the epic with integration blockers.

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

## Authoring checklist

- Put one shared preamble marker near the start of every command and keep its runtime input section at
  the end of the stable workflow.
- Give reviewers a status/verdict and one finding per acceptance criterion.
- Give builders changed paths and one-line rationale per path or group; list verification commands and
  results separately.
- Keep issue text, user guidance, diffs, and other untrusted or volatile data quoted and below stable
  instructions. Never introduce positional argument placeholders; `$ARGUMENTS` is the only command
  input token.

See [CONTRIBUTING.md](../CONTRIBUTING.md) for source and validation conventions.
