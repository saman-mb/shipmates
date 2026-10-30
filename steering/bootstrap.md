# Shipmates bootstrap

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
   and next action as applicable. Omit raw command logs and a narrative of every step.
6. **Avoid paid repetition.** Reuse context and results already proven in the current run. Repeat a
   check only when new information or a changed artifact makes it decision-relevant; record what was
   checked rather than replaying a transcript. Cut rework before builders run: verify spec citations
   with `grep`, emit blast radius for shared APIs at plan time, give units machine-checkable owned-paths
   manifests, and route empirical questions to whoever can run code.

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
- Cost is seats × model **plus rework**: Before a spec becomes binding, verify its citations —
  including claims about third-party platform behaviour (CI trigger resolution, harness internals),
  not only `file:line` references in this repo. An unverified platform claim is not a fact. Route
  empirical questions to whoever can run code. Emit the blast radius of any shared API change at plan
  time. Give each unit a machine-checkable owned-paths manifest rather than prose fences.
- Ask every subagent for a compact structured return: decision/status first, criterion findings and
  minimal evidence, then blockers, changed files with one-line rationale, and next action as relevant.
  Return decisions, not transcripts or raw logs.

## Argument intake

- A **mutation-gating knob** is a Parameters row whose `Mutates` column is `yes`: it decides
  whether the command performs a write (closing issues, filing upstream, rewriting an installed
  crew). Only the seven commands whose table declares one carry the column.
- A gating knob takes one of two shapes, and its own row says which. **Ask-first** (an `apply`-style
  row): the safe value is the Default, and the knob is tokenized only by naming its mutating token.
  **Apply-first** (a `report`-style row): the mutating value is the Default and the command declares
  an explicit safe token, which is the opt-out. Only a command whose name *is* the mutation may
  declare an apply-first knob, and its Help says plainly that the safe token is the way out.
- Before any mutating step, classify `$ARGUMENTS`: **empty**, **partial**, or **fully specified**.
  Empty or partial → run intake; fully specified → skip and execute.
- Completeness: Empty = blank/whitespace. Partial = non-empty but ≥1 Required=yes Parameters row
  unresolved, or an ask-first mutation-gating knob left un-tokenized. Fully specified = every
  Required=yes row present/parseable AND every `Mutates: yes` row resolved — an ask-first knob only
  by naming its mutating token, an apply-first knob by its Default when nothing is typed or by its
  safe token when the captain asks for the read-only path. Missing optionals otherwise keep Defaults.
- A command whose table declares no `Mutates: yes` row keeps today's behaviour: supplying its
  Required=yes rows still skips the ask, so the 11 required-row commands (e.g.
  `/shipmates-ship-issue 42`) are unchanged.
- **Always show every Parameters row.** The defaults card lists **each** Name with its current or
  Default value (and a one-line Help hint when useful) — requireds and optionals alike — so the
  captain can see knobs like `board`, `merge_mode`, `dry_run`, or `mode` without memorizing tokens.
  Never hide captain-facing knobs inside a catch-all `guidance` row; free-text focus may be a
  separate optional row after the named knobs.
- **Enums show default and the other choices.** When a row's Values is a finite set (e.g.
  `full / epic-deferred / off`), print the active/default value **and** the remaining options in
  that same line — e.g. `board  full  (also: epic-deferred, off)`. Free-text / open Values stay a
  single default (or `—`); do not invent an enum that Parameters did not declare.
- **Defaults-first, one turn.** Do **not** walk the captain through a long form. Show that full
  defaults summary once, and ask a single question: **OK to proceed, or type what to change?**
  - **OK / yes / empty reply** → lock the proposal and proceed.
  - **Typed changes** → apply only what they named (free text or Token phrases from Parameters),
    restate the updated one-line invocation, then proceed. Do not re-open a full questionnaire.
  - Ask only for a missing **Required=yes** value when Defaults cannot supply it — one question,
    then return to the same OK-or-change confirm. Never invent irreversible choices.
- Prefer the harness native ask/choice surface when present; else a short chat prompt. Never invent
  a TUI outside the session. Canonical prose must NOT name harness-specific tool IDs.
- Non-interactive / no user-turn: **fail closed** if any required is missing (clear missing-arg
  message); apply Defaults for optionals, and an un-tokenized mutation-gating knob keeps its
  declared Default — for an ask-first row that is the value that does not perform the write — and
  the run proceeds. Never hang; never invent irreversible choices.
- After intake, restate one resolved invocation line (`/<command> <tokens…>`), then proceed with the
  existing workflow unchanged.
- Per-command files only declare `## Parameters`; do not copy these intake rules into each command.

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

**The main agent makes the call — nothing is configured in advance.** No file, in the repository or in the
captain's home, supplies a ranking: one static list cannot be right across targets, plans and sign-ins,
and an empty one decides nothing at all. The ranking is the orchestrator's own judgment, exercised **at
spawn, against what the target in front of it offers**:

1. **Ask the target what exists.** Where the target documents a non-interactive listing command — the
   per-target table below names which — run it. The answer is candidates only: it says what exists,
   never which is cheap or which is best.
2. **Where there is no listing command, read what the target documents.** A target whose only model
   surface is a native allow-list (a managed settings key, or a repository-root allow-list file) offers
   what that list permits, alongside the model the session is already on. A target with no such surface
   offers nothing to read.
3. **Then judge.** With those candidates in hand, pick per role: the cheapest capable for `mechanical`,
   the top available for `judgment`, at the effort the unit needs. Name only an identity the target
   itself offered — never one recalled from a price page, a release note or a naming convention.
4. **Record the call.** Every spawn's `MODEL ROUTING:` line carries the identity chosen and whether it
   was observed on the target or inherited. A choice the captain cannot see is a choice they cannot
   correct.
5. **`inherit` when you cannot decide.** No listing command and no allow-list, a listing that exits
   non-zero or returns something unparseable, or a call you are not confident in: run on the
   parent/session model. `inherit` is a deliberate answer, not a failure, and it is always available.

**Never invent a model.** A concrete identifier appears in no shipped file, no default and no example.
The target's own listing is the only source of an identity, and an identity the harness does not accept
is refused or substituted by the harness itself — never quietly clamped into looking honoured.

**Resolution order — stated once, for every target.** The levels, in order:
explicit spawn value → parent/session value → the model's own effort default.
A model chosen without an effort gets **that model's own default effort**, never the parent's effort
carried across a model change: one model's effort scale does not describe another's. A captain may name
an identity in the run's own guidance; that is an explicit spawn value and outranks the orchestrator's
own choice. Where a level of the order does not exist on a target, its row in the table below says so.

**Enforcement.** An identity outside what the target permits is **refused**, not quietly clamped —
choose a different candidate, or stop and report. Not every target's documented mechanism can hold that:
`abort` refuses · `warn` reports the identity but proceeds · `fallback` means the harness substitutes,
which the audit line reports as `substituted` · `none` means no native mechanism exists, so the
orchestrator self-enforces or falls to
`inherit`. The enforcement column in the table below carries each target's value, and the run **states
the discrepancy in the report** instead of pretending enforcement held.

**Audit — one `MODEL ROUTING:` line per spawn.** Every spawn adds one compact line to the run report,
shaped `MODEL ROUTING: <role> tier=<mechanical|judgment> source=<observed|inherit> model=<identity the harness accepts> effort=<requested>→<resolved> <honoured|substituted|inherit>`.
`<requested>` is the neutral scale (`low` / `medium` / `high`, or `none` when no effort was named) and
`<resolved>` is what the harness reports for it — the two differ whenever a target maps the request onto
its own vocabulary, and a clamped level is recorded here rather than dropped. Substitution is reported
**as** substitution, never as honoured: when the harness's own rules replace
the requested identity — an admin block, a plan limit, a hard environment override — the line records
`substituted` and names the condition that fired.

**Drift with no new release.** The installed command is a snapshot; the harness is not. Verify a surface
exists before relying on it — run the listing command once, or check the flag in the target's own help
output — and treat every step above as degradable: a listing command that has vanished or changed shape
falls through to whatever the target documents, and a target with nothing to read falls to `inherit`. A
target with no row in the table below is treated as no listing command → documented surface → `inherit`.

**Per-target surface.** What the target offers to read, the override kind, the enforcement it can
actually hold, and the effort surface with its clamp. No cell is ever blank: a missing feature is a
stated finding.

| Target | Discovery tier | Override kind | Enforcement | Effort surface and clamp |
|--------|----------------|---------------|-------------|--------------------------|
| claude-code | declared | per-spawn | fallback | separate key · unsupported level clamps down |
| opencode | query | static agent file | none | separate key · no clamp documented, provider decides |
| antigravity | query | session-level | abort | run-level · no clamp documented |
| codex | query | per-spawn | none | separate key · gated on model support |
| cursor | query | static agent file | fallback | folded into the model string · model-defined, clamping undocumented |
| github-copilot | declared | per-spawn | abort | separate key · no clamp documented |
| pi | query | per-spawn | none | separate key · per-model map, unsupported clamped away |
| grok-build | query | per-spawn | abort | separate key · model-defined menu, no clamp documented |
| devin | inherit | session-level | none | none · only an interactive cycle |

**Pi.** A zero-cost local endpoint is not necessarily reachable. Only pick local when crew switch routes local seats; otherwise prefer cloud or `inherit`.

**Additive, never a substitute.** Routing refines tiered execution, it does not replace it: the tier is
still the primary cost gate, and the target's surface decides only **which** capable model runs a
mechanical unit — never **whether** a lighter execution path is chosen.

## Reusable acceptance board

Spawn reviewers **in parallel** against the PR head commit — they review exactly what will merge.

**Mandatory seats (never skip)**

- **`product-manager`** (PO): checks every acceptance criterion AND the quality bar (README / {{project-instructions}} / contributing). Returns `ACCEPT` / `ACCEPT-WITH-NITS` / `REJECT` with specifics per criterion. For each docs/content AC that claims specific wording or a registry fact, PO cannot return `ACCEPT` or `ACCEPT-WITH-NITS` without a **machine-checkable pin** (unittest string assert, `--check` golden, or validator content rule). A `manual-only` label does not stand in for the pin: where no pin is possible, the verdict records that criterion as unverified and it fails, unless the issue itself defers it or the captain recorded a deferral. Silent acceptance from reading the page is forbidden.
- **`principal-engineer`** (PE): principal-level diff review — correctness, edge cases, naming, test meaningfulness, scope discipline, security hygiene at review depth (not a `/shipmates-harden` pass). Verifies the PR satisfied the repo's **mandatory ship checklist** for this change class (regenerated generated pages, updated fixture digests, version/changelog when required, site validation, no hand-edited generated paths). Returns `ACCEPT` / `ACCEPT-WITH-NITS` / `REJECT` with `file:line` evidence.

Tiered execution may lean the build path on Simple/Medium, but **must not skip PE+PO** on the **first** board once a PR head exists. Later rounds follow **Retry** below — a PE/PO ACCEPT may be carried when the fixer delta cannot invalidate it.

**Delegation modes (the only two authorized exceptions to the mandatory seats above)**

- **`board=epic-deferred`** — a *deferral*, never a cancel. Set by an orchestrating command that owns a
  mandatory milestone board on the integrated artifact (e.g. `/shipmates-ship-epic`'s Stage 4 integration board on `<EPIC_PR>`).
  The unit's own board is skipped, its CI gate still runs, and the milestone board reviews the integrated
  diff. The deferral is valid only while that milestone board is guaranteed; a delegated run must **not**
  convert it to `board=off`.
- **`board=off`** — an explicit captain opt-out: no board here and none deferred. The run records it loudly
  in the report and the PR body. Agents never choose it on their own.

Every board that is actually convened keeps the mandatory PE+PO seats and follows Retry below.

**Scaled optional seats**

Convene only when the change can plausibly trip the concern. A gated-out seat is **named in the report with its flag or reason** — never silently skipped. **Damp by artifact, not by flag count:** a change confined to one artifact family (one file cluster, one document, one generator) pulls **at most one** specialist beyond PE+PO, and only when that specialist's concern is a genuinely different artifact from the others'. Three flags firing on one prose block is one concern, not three reviews.

**Artifact damping**: when multiple independent `IS_*` flags fire on a change confined to a single artifact family (e.g. documentation-only, build/release metadata, or a single isolated module), pull **at most one** specialist beyond the mandatory PE+PO core. Select the specialist whose concern is distinct from PE/PO on that specific artifact, and gate the others out (naming them in the report). Do not stack redundant specialists on a single artifact.

| Seat | Join when |
|------|-----------|
| `sdet` | Medium+ code changes, or any change where validation is non-trivial. On Simple doc-only runs with a trivial validation plan, PE+PO may suffice — state which validation ran. Skip it when the pre-PR self-check already ran a full independent pass on the same tree **and** CI re-runs that gate set on the pushed head — name the gate that covers it instead of paying a second run. |
| `architect` | `IS_ARCH_SIGNIFICANT` |
| `devops-engineer` | `IS_DELIVERY_SENSITIVE` |
| `technical-writer` | `IS_DOCS_AFFECTING` — doc copy/staleness (PE covers process compliance; both may run) |
| `ux-ui-designer` | `IS_UI_STORY` |
| `art-director` | `IS_VISUAL_STORY` |
| `security-engineer` | `/shipmates-pr-review` only when `IS_SECURITY_SENSITIVE` |
| `performance-engineer` | `/shipmates-pr-review` when the PR claims a perf win or touches a hot path; `/shipmates-refactor` when the stated motivation was performance |
| `site-reliability-engineer` | `/shipmates-pr-review` when runtime behaviour, failure handling, or rollout changes |
| `data-scientist` | `/shipmates-pr-review` when the deliverable is an analysis or model |

The `IS_*` flag vocabulary is shared by `/shipmates-ship-issue` Stage 0 and `/shipmates-pr-review` Stage 0 — a new flag must be added to both classifiers.

**Decision**

- **All spawned reviewers ACCEPT/PASS (nits allowed)** → proceed to deliver / the command's next stage.
- **Any REJECT / FAIL** → remediation loop (where the command defines one), then **Retry** on the new head.

**Retry (after a fixer)** — do not clone the first-convene roster. Re-select from the **fixer delta** (commits since the last board), not a full Stage 0 redo:

1. **Must sit** — every seat that REJECTED / FAILED last round. They review the new head.
2. **Carried by default** — a seat that ACCEPTED is carried forward, and a delta that *implements that seat's own finding* is never grounds to re-seat it: asking a reviewer to re-approve the change they requested is a predictable green. Re-spawn an accepting seat only when the delta changes something it did **not** ask for, or perturbs the artefact its verdict actually rests on.
3. **Gate-covered seats** — a seat whose verdict came from running the gates (tests, digests, CI) is re-covered by re-running them, not by re-seating. Re-spawn it only when the delta changes what the gate measures.
4. **May newly sit** — a seat gated out last round joins if the delta newly trips its flag. Do not invent seats the flags never named.

When a seat is re-spawned, they review the **pushed SHA**. The report lists `re-run` / `carried ACCEPT` / `newly seated` / `still gated` — never a silent skip.

**Harness fallback**

If `principal-engineer` or any role does not resolve to a shipped crew role (skill-only harnesses until crew agents ship), fall back to a general-purpose agent with the role brief inlined and note the fallback — never silently skip a mandatory seat.
If the child-launch / subagent runtime cannot load (spawn dies before any role resolves), **stop** and name the diagnostic — never silently run the board inline, never silently set `board=off`.

**Finding accountability**

Each finding kept in working memory has a stable id for the run. Its final disposition is exactly one of fixed, filed, or declined with a reason. Counts alone are not accountability. The list lives in the PR completion comment, the posted review comment, or the session report when the command opens no PR, plus any issue actually filed — do not tell a later agent to read a local run history. Do not write a per-run findings ledger, JSON dump, or report under `.shipmates/` or anywhere else in the repository, committed or not. Temporary captures stay in the run's existing temporary worktree and leave with that command's existing cleanup. This does not forbid `.shipmates/worktrees/`, install receipts, contributor steering, or an existing config file such as `.shipmates/deslop.toml`.

A blocker is fixed or the run stops and escalates — it is not declined in order to ship. Nits are not auto-filed; they follow the command's existing ladder. Map, do not replace: absorb = fixed, file = filed, pr-note = declined with that note as the reason. Do not change `NITS_MODE` tokens. A read-only review records a recommended disposition in the review comment; it does not fix, file, or write a ledger. Security accepted-risk is declined with a reason, not a new status — do not rename it.

When the change affects documentation or a visual artifact, the accepting reviewer was not given the builder's narrative and names the rendered artifact actually inspected (path or URL). If direct inspection is unavailable, the verdict states that limitation ("needs a human visual pass" is allowed) and does not ACCEPT that surface silently. A machine-checkable pin does not replace that inspection.

## Reusable epic integration board

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

Integration findings stay in working memory until closure notes list each one as fixed, filed, or
declined with a reason — not only counts, and not a second filing of unit nits. Do not write a
per-run findings ledger under `.shipmates/` or elsewhere in the repository; temporary captures stay in
the existing worktree and leave with cleanup. `.shipmates/worktrees/` stays allowed. A docs or visual
acceptance names the rendered artifact inspected, or states that inspection was unavailable and does
not ACCEPT that surface.

## Reusable subagent preamble

## Return discipline

- **Plan and brainstorm first.** Before editing files or executing major actions, formulate a clear, step-by-step plan. If instructions are ambiguous, surface questions rather than guessing.
- **Ingest project context (`{{project-instructions}}`).** Always consult the repo's `{{project-instructions}}` as the primary source of truth for build commands, test runners, code style, and conventions.
- **Leverage Git history.** Utilize `git log` and `git blame` on relevant files to understand historical rationale, linked issues, or past patterns before making changes.
- **Direct CLI discovery.** When invoking unfamiliar local build, test, or deployment tools, run `--help` or inspect tool configurations instead of guessing argument flags.
- **Return discipline.** Return one compact structured result, not a transcript. Lead with `STATUS` or `VERDICT`; include only criterion-level findings (`CRITERION: result — evidence`) and evidence needed to support it; finish with `BLOCKERS`, `CHANGED`, `RATIONALE`, and `NEXT` fields when applicable. Omit raw command logs and narration of routine steps.

## Why merge this (PR body)

**Why merge this**

Any pull request this command opens — and any completion comment it posts on that PR — must open
with or include a short maintainer-facing block under this heading. Write it for the human who owns
the tool: why should they care that this lands? Answer the product-impact bar already required of
every pull request description — **What changes**, **Why it matters**, **Who is affected**. Do not
invent a second template. A file list, a ticket dump, or a bare `Closes #<n>` is not a reason to
merge.
