---
name: ship-epic
description: Loop /ship-issue over an epic's stories in dependency order — one epic plan amortizes overhead, cohesive stories batch into single runs, gate stories pause for sign-off.
argument-hint: <epic-issue-number> [resume | dry-run | epic close auto | epic merge auto | batch off | unit merge manual]
allowed-tools: Bash, Read, Write, Edit, Agent, Grep, Glob, WebSearch, WebFetch
disable-model-invocation: true
---
# /ship-epic — sequential epic delivery
<!-- shipmates:command-preamble -->

Deliver a whole **epic** by driving the `/ship-issue` pipeline over its unchecked story checklist —
in **shipping units** (one story or a small cohesive bundle), dependency order preserved — until
every non-gate story lands on a shared **epic integration branch**, then hand the captain **one**
CI-green **epic PR** against the repo default branch for final review.

**Epic cost discipline.** A naïve loop pays `/ship-issue`'s full fixed overhead **once per story**
(Planner, worktree, CI poll, acceptance board). On a five-story epic that is roughly five times the
cost of one run for little extra diff. This command **amortizes** that overhead instead:

1. **One epic plan for all stories** — a single `architect` pass (Stage 1.5) classifies every pending
   story and groups them into `<units>` before any build. No re-planning the epic shape per story.
2. **Batch cohesive units** — when Stage 1.5 groups two–four small, same-area stories with
   non-overlapping file ownership, invoke `/ship-issue` **once** with every story number in the unit.
   Multi-issue input is already bundle consent in `/ship-issue`; one board reads one combined diff.
3. **Pre-classification passthrough** — pass the epic plan's complexity and domain flags into each
   delegation as guidance so `/ship-issue`'s tiered execution (Simple / Medium / High) fires without
   re-deriving from scratch. The story-level Planner may **amend** the plan if the issue body
   contradicts it; it must not ignore it.
4. **Epic context capsule** — after each successful unit, append a compact `<epic-capsule>`:
   validation commands that worked, touched paths, conventions established. Pass only the capsule
   plus the current unit's issue text to the next delegation — not a full epic re-brief every time.
5. **Mandatory board inheritance** — each delegated `/ship-issue` run convenes the **mandatory PE+PO
   acceptance board** on the pushed PR head. Tiered execution may lean the build path but never skips
   that core; scaled optional seats follow the shared board contract.
6. **Epic integration branch** — at kickoff, cut `<EPIC_BRANCH>` from the default branch and open
   **epic PR `<EPIC_PR>`** (base = default branch, head = epic branch). Each unit PR targets
   `<EPIC_BRANCH>`, not the default branch; green units **auto-merge into the epic branch**. The captain
   reviews **one** epic PR at the end — not N story PRs on the default branch.
7. **Value-gated batching** — never batch to save tokens alone. Gate stories, `complex` stories,
   `IS_ARCH_SIGNIFICANT` / `IS_SECURITY_SENSITIVE` stories, and unrelated areas always ship as
   **singleton units**. CI and acceptance still run **per unit**; nothing ships without green checks.

Hard limits that **pause the epic loop** (end the turn; post `/ship-epic <epic> resume`):

| Limit | When it fires |
|-------|----------------|
| **Gate story** | Unit contains a `gate`-labelled (or sign-off) story still awaiting human sign-off |
| **External blocker, no shippable slice** | Every AC in the unit requires an owner action with no prep work the crew can land now |
| **`MAX_FIX_ROUNDS` exhausted** | Stage 4.5 or Stage 6 on this unit could not get CI green / acceptance pass |
| **Manual unit merge** | Unit used `MERGE_MODE=manual` (`IS_SECURITY_SENSITIVE`, `UNIT_MERGE_MODE=manual`, or other forced manual path) — green PR awaits captain merge into `<EPIC_BRANCH>` before the next unit |
| **Shell safety abort** | Untrusted input, cycle in dependency graph, invalid epic token |

Everything else — including **red CI on the unit PR**, **red CI already on `<EPIC_BRANCH>`**, and
**partly blocked stories** — stays in the **fix / defer / next-unit** loop. Do **not** pause the epic.

**Confirmed-green CI** is a per-unit requirement inside `/ship-issue` Stage 4.5; remediate there, not
by stopping `/ship-epic` early. Pause is **not** a substitute for the Fixer loop.

The epic issue number and optional guidance come from the Runtime input section at the end of this
workflow.

---

## Config (defaults — override only if the repo clearly needs it)

- `EPIC_LABEL` = `epic` — the epic must carry this label (or be clearly an epic by checklist shape).
- `GATE_LABEL` = `gate` — stories with this label pause the loop for human sign-off before shipping.
- `EPIC_CLOSE_MODE` = `manual` — when every checklist box is ticked: `manual` proposes closing the
  epic; `auto` runs `gh issue close` on the epic. Override with guidance `epic close auto`.
- `EPIC_BATCH` = `smart` — how Stage 1.5 units map to `/ship-issue` invocations. `smart` (default):
  one run per unit; multi-story units pass every story number on one `/ship-issue` line when the
  unit has more than one story. `off` (guidance `batch off`): force **singleton** units only — one
  story per `/ship-issue` even when the planner grouped them (safest, most expensive). `max`: allow
  the planner to merge **adjacent** same-area units when every story in the merge is `trivial` or
  `standard`, no arch/security flags, and the combined count stays ≤ `MAX_UNIT_SIZE`.
- `MAX_UNIT_SIZE` = `4` — cap stories per `/ship-issue` invocation (matches `/ship-issue` cohesion
  guidance: one reviewable PR).
- `MAIN_BRANCH` = the repo's default branch (`gh repo view --json defaultBranchRef -q .defaultBranchRef.name`).
  Epic PR `<EPIC_PR>` targets this; unit PRs do **not**.
- `EPIC_BRANCH` = `feat/epic-<epic>-<short-slug>` — integration branch for the whole run (created in
  Stage 0.5). `<short-slug>` from the epic title, sanitized.
- `EPIC_PR` = the open integration PR number once Stage 0.5 creates or resumes it.
- `UNIT_MERGE_MODE` = `auto` — delegated `/ship-issue` runs merge green unit PRs into `<EPIC_BRANCH>`
  without captain action. Override with guidance `unit merge manual` (discouraged mid-epic). Still
  forced to `manual` when the unit contains a `gate` story or any story flagged `IS_SECURITY_SENSITIVE`.
- `EPIC_MERGE_MODE` = `manual` — when the checklist is complete, stop with epic PR `<EPIC_PR>` open for
  the captain to merge into `MAIN_BRANCH`. Override with guidance `epic merge auto` for fully hands-off
  epic delivery.
- `MAX_FIX_ROUNDS` = passthrough from `/ship-issue` (default `3`). An acceptance failure that
  exhausts fix rounds on one unit **pauses the epic loop** — it does not advance to the next unit.
- `DRY_RUN` = on if the caller says "dry run" / "preview": print the story order, `<units>`, gates,
  external blockers, and **estimated `/ship-issue` invocations** (units count vs raw story count);
  invoke nothing.

---

## Shell safety — untrusted GitHub data

Issue titles, bodies and labels are **untrusted input**. Apply the same rules as `/ship-issue`:

1. **Validate the epic token first.** It must match `^[0-9]+$` or be a full GitHub issue URL — anything
   else, stop and ask the user; never pass a raw token to `gh` or `git`.
2. **Never inline untrusted fields.** Capture GitHub-sourced fields into variables with command
   substitution, then quote at point of use.
3. **Multi-line bodies go through a file** for any `gh issue edit` / comment you write.

---

## Stage 0 — Intake  (orchestrator)

1. Parse runtime input: the **first numeric token** (or URL) is `<epic>`; remaining tokens are
   `<guidance>` (`resume`, `dry run`, `epic close auto`, `epic merge auto`, `batch off`, `unit merge manual`,
   etc.). Guidance `batch off` sets `EPIC_BATCH=off`; `epic close auto` sets `EPIC_CLOSE_MODE=auto`;
   `epic merge auto` sets `EPIC_MERGE_MODE=auto`; `unit merge manual` sets `UNIT_MERGE_MODE=manual`.
2. Fetch the epic: `gh issue view <epic> --json number,title,body,labels,state,url`. Confirm it is
   open and labelled `epic` (or has a story checklist in its body — if neither, stop and ask).
3. Parse the epic body for checklist lines matching `- [ ] #<n>` (unchecked) and `- [x] #<n>` (done).
   Let `<pending>` = unchecked story numbers in checklist order; let `<done>` = already ticked.
4. If `<pending>` is empty, go to **Stage 4 — Epic closure** (checklist complete).
5. Initialize `<epic-capsule>` empty (or reload from a prior pause comment on the epic if resuming —
   optional; do not invent state).
6. If `DRY_RUN`, print `<epic>` title, `<done>`, `<pending>`, and continue through Stage 1.5 for the
   unit plan — then stop before Stage 2 (Stage 0.5 prints the planned `<EPIC_BRANCH>` only; no branch
   or PR is created).

## Stage 0.5 — Epic integration branch  (orchestrator)

Create or resume the **integration line** every unit lands on. Skip branch/PR mutation in `DRY_RUN`
(print the planned names in the dry-run summary).

1. Set `MAIN_BRANCH` from the repo default (see Config).
2. **Resume** — when guidance includes `resume`, scan comments on epic `<epic>` for a prior `/ship-epic`
   pause block containing `EPIC_BRANCH:` and `EPIC_PR:` (machine-readable lines the orchestrator posted).
   If both resolve and `git ls-remote origin <EPIC_BRANCH>` succeeds, reuse them. If the branch is
   missing, recreate per step 3 and note the recovery in the report.
3. **Create** (when not resuming and branch does not already exist):
   ```bash
   git -C <repo> fetch origin
   git -C <repo> push origin origin/<MAIN_BRANCH>:refs/heads/<EPIC_BRANCH>
   ```
   (Or branch locally and push — same result: `<EPIC_BRANCH>` tracks current `MAIN_BRANCH`.)
4. **Open epic PR** — if `<EPIC_PR>` is unset, open one PR with base `MAIN_BRANCH`, head `<EPIC_BRANCH>`.
   Title/body via `--body-file` (see **Shell safety**): epic checklist, pending stories, note that this
   is the **integration PR — merge only when the epic is complete**. Record `<EPIC_PR>`.
5. **Persist state** — post a comment on epic `<epic>` with `EPIC_BRANCH:`, `EPIC_PR:`, `MAIN_BRANCH:`
   so `/ship-epic <epic> resume` can reload. Update the same block after each pause.

## Stage 1 — Story graph  (orchestrator)

For each story in `<pending>`:

1. Fetch `gh issue view <n> --json number,title,body,labels,state,url`.
2. Skip closed stories — treat as already done; note them for checklist backfill in Stage 3.
3. Detect **gate stories**: `gate` label present, or the body contains an explicit sign-off section
   (e.g. a heading like "Sign-off" / "Gate" with criteria the story says must pass before merge).
   Record `<gates>`. Gate stories are always **singleton units** — never batched.
4. Parse **dependencies** from the story body: `Blocked by #<n>`, `blocked_by #<n>`, or checklist
   cross-refs. Build a directed graph over `<pending>` plus any **external** blockers outside the epic.
5. **Topological sort** `<pending>` respecting dependencies. If a cycle is detected, stop and report
   the cycle — do not ship out of order.
6. Flag **external blockers**: any dependency on an open issue outside the epic (or outside `<done>`).
   Record `<blocked-externally>` with story → blocker mapping. For each, note **full block** (zero
   shippable AC without the blocker closing) vs **partial block** (story body names prep, config,
   or other work that can ship before the owner action).

Print (always, not only dry-run): ordered story list, gate stories, external blockers (full vs partial).

## Stage 1.5 — Epic shipping plan  (agent: `architect`, once)

Spawn **one** `architect` with: the epic title/body, every pending story's title + body + labels
(truncate bodies to acceptance-criteria sections when huge), Stage 1 dependency order, repo
`README` / {{project-instructions}}, and `<done>`. Ask for **structured data only**:

- Per story: `complexity` (`trivial`, `standard`, or `complex`), domain flags (same vocabulary as
  `/ship-issue` Stage 0), `blocker_class` (`none`, `full`, or `partial` when externally blocked),
  and one-line rationale.
- `<units>`: an **ordered** list of shipping units covering every non-gate pending story exactly
  once. Each unit: `stories` (issue numbers), `batch_rationale` (why together or alone),
  `expected_files` (non-overlapping ownership across stories in the unit).
- **Batching rules the planner must enforce:** gate stories → singleton; `complex` or
  `IS_ARCH_SIGNIFICANT` or `IS_SECURITY_SENSITIVE` → singleton; different `area:*` labels → separate
  units unless each story is `trivial` and file-disjoint; max `MAX_UNIT_SIZE` stories per multi-story
  unit; dependencies satisfied within unit ordering.

Apply `EPIC_BATCH`:
- `off` → split every multi-story unit into singletons (keep order).
- `max` → merge adjacent units only when the architect's rules still hold and combined size ≤
  `MAX_UNIT_SIZE`.

Record `<units>` and `<story-classification>`. In `DRY_RUN`, print `<units>` with story titles,
unit sizes, and **token rationale**: "`N` stories → `U` `/ship-issue` invocations".

## Stage 2 — Loop  (orchestrator)

Walk `<units>` in order — one `/ship-issue` delegation per unit (not one per story unless the unit
is a singleton).

For each `<unit>`:

1. **Stories already ticked or closed** — skip the whole unit.
2. **Gate unit** — if any story in the unit is in `<gates>` and still open: **pause** with
   **awaiting sign-off**. Do **not** invoke `/ship-issue`. **Stop** — this is the only deliberate
   human gate in the loop.
3. **External / mixed blocker** — for each story in the unit blocked by an open external issue:

   a. **Shippable slice** — if the story body (or Stage 1.5 `blocker_class: partial`) names work that
      does **not** require the blocker to close, treat that slice as the **unit scope**. Invoke
      `/ship-issue` on that slice; in the PR body note the residual owner action and link the blocker.
      **Do not pause the epic.**

   b. **Residual only** — file or update a follow-up issue for the blocked flip / owner action.
      Tick or comment on the story that prep shipped; leave the flip open.

   c. **Pause only when** every remaining AC for the unit truly requires the external blocker and
      there is **zero** shippable remainder (`blocker_class: full`). Then **pause** with blocker
      details and **Stop**.

   Do **not** pause the whole epic because one story is *partly* blocked.
4. **Build delegation guidance** (compact prose, not a transcript). Include:
   - `epic-run` — story numbers in this unit belong to epic `<epic>`; do **not** scan the wider
     backlog or propose bundle widening beyond this unit's story list.
   - `epic-base=<EPIC_BRANCH>` — unit PR base and worktree cut from `origin/<EPIC_BRANCH>`, **not**
     `MAIN_BRANCH`.
   - `epic-plan` — paste the unit's pre-classification (complexity + flags per story); the
     story-level Planner should treat this as the starting plan and amend only on contradiction.
   - `epic-capsule` — paste `<epic-capsule>` when non-empty (validation commands, paths, conventions
     from prior units in this run).
   - `MERGE_MODE=<UNIT_MERGE_MODE>` — merge the green unit PR into `<EPIC_BRANCH>` via `/ship-issue`
     Stage 8 when `UNIT_MERGE_MODE=auto` (default). When `manual`, or when any story in the unit is
     flagged `IS_SECURITY_SENSITIVE`, pass `MERGE_MODE=manual` and **pause after the unit** per the
     **Manual unit merge** hard limit — do not advance to the next unit until the captain merges into
     `<EPIC_BRANCH>` and the orchestrator resumes. **Never ask the captain to merge a green epic unit when
     `UNIT_MERGE_MODE=auto`.**
   - Tier hint — when **every** story in the unit is `trivial` and no specialist flags are set,
     include `complexity tier: simple` so `/ship-issue` takes the Simple path. When all are
     `trivial` or `standard` with no arch/security/delivery flags, include `complexity tier: medium`.
     Otherwise omit (full High path).
5. **Delegate** — invoke `/ship-issue` with **all story numbers in the unit** as the leading numeric
   tokens, then the guidance from step 4. Example shape: `/ship-issue 101 102 epic-run …` for a
   two-story unit. Singleton: `/ship-issue 103 epic-run …`. Do **not** set `BUNDLE=off` — explicit
   multi-story tokens **are** the bundle; singletons behave as today.
6. **Outcome:**
   - **Success (auto-merged)** — unit used `MERGE_MODE=auto` and landed on `<EPIC_BRANCH>` → Stage 3 for
     **each** story in the unit (tick every checklist line the unit closed), extend `<epic-capsule>` with
     validation commands used, key paths touched, and any convention the board enforced — keep the
     capsule **short** (bullet list, not a narrative). Continue to the next unit.
   - **Success (manual merge required)** — unit finished with `MERGE_MODE=manual` and a green PR open →
     **pause the epic loop** (hard limit **Manual unit merge**): post state (`EPIC_BRANCH`, `EPIC_PR`,
     unit PR link, `/ship-epic <epic> resume`). **Stop** — captain merges the unit PR into
     `<EPIC_BRANCH>`, then resume.
   - **`/ship-issue` escalated after `MAX_FIX_ROUNDS`** → **pause the epic loop** with state report
     (epic, `EPIC_BRANCH`, `EPIC_PR`, completed units, current unit, PR link, failure summary,
     `/ship-epic <epic> resume`). **Stop.**
   - **Red CI / fix in progress** → **not** an epic pause. The delegation must finish Stage 4.5
     inside `/ship-issue` before returning. If it returned early, **re-delegate** with explicit
     guidance `finish-ci-gate` — do not end the epic turn.
   - **Gate mid-run** → pause and stop (same as step 2).

**Pre-existing CI on the merge base** — when a failing check **already fails on `<EPIC_BRANCH>`**
(same job red on the epic integration line) and the unit PR touches unrelated files:

- **Not an epic pause.** Remediation belongs in **this unit's** `/ship-issue` run (Stage 4.5).
- Fix what makes **this PR head** green: format only touched files if that suffices; otherwise fix
  the minimal set the log requires; gate or skip tests that cannot run under the repo's build flags
  when that matches project convention.
- Document extra commits in the PR (note the job was already red on `<EPIC_BRANCH>`). Escalate to
  the captain **only** after `MAX_FIX_ROUNDS`, or when the fix is a one-way product decision — not
  because the integration branch is dirty.

When every unit has succeeded, go to Stage 4.

## Stage 3 — Tick epic checklist  (orchestrator)

After each successful unit, for **each story** in that unit:

1. Re-fetch the epic body.
2. Replace `- [ ] #<story>` with `- [x] #<story>`.
3. Write via `--body-file`; verify the tick landed.

Backfill ticks for stories closed before this run but still unchecked in the epic body.

`/ship-issue` ticks the epic when `MERGE_MODE=auto`; this stage ensures ticks when a unit used
`MERGE_MODE=manual` (gate / security-sensitive units).

## Stage 4 — Epic closure  (orchestrator)

When no `- [ ] #n` lines remain:

1. **Epic PR gate** — poll `gh pr checks <EPIC_PR>` on the integration PR head until green or
   `MAX_FIX_ROUNDS`-bounded fix loop exhausted (fix on `<EPIC_BRANCH>`, not on `MAIN_BRANCH`).
2. **`EPIC_MERGE_MODE=manual`** (default): post completion summary with **epic PR `<EPIC_PR>`** link,
   green CI link, unit PR links, checklist state. **Stop** — the captain merges the epic PR into
   `MAIN_BRANCH` once satisfied. Do **not** ask the captain to merge individual unit PRs.
3. **`EPIC_MERGE_MODE=auto`** (guidance `epic merge auto`): squash-merge epic PR into `MAIN_BRANCH`
   with `match-head-commit`, then run epic close per `EPIC_CLOSE_MODE`.
4. **`EPIC_CLOSE_MODE`** — `manual`: propose closing epic `<epic>`; `auto`: `gh issue close <epic>`.

## Final report

One concise summary: epic link, **epic PR `<EPIC_PR>`** link (integration vs `MAIN_BRANCH`), units
shipped (`U` invocations for `N` stories), unit PR links, gate pauses, external blockers, capsule
highlights, epic close state, and resume command if paused. For each pause, state **which hard-limit
row fired** — "waiting for captain" without a limit name is a spec violation. Include **economy line**:
"`N` stories in `U` `/ship-issue` runs" so the captain sees what batching saved.

---

### Guardrails

- **Compose, don't duplicate** — each unit is a `/ship-issue` run; never reimplement its stages inline.
- **One epic planner** — Stage 1.5 runs once per `/ship-epic` invocation; do not spawn a second
  epic-wide planner inside the loop.
- **Batch only on merit** — cohesion, file ownership, and flags gate batching; never merge unrelated
  stories because the epic is long.
- **No backlog widening** — delegated runs stay inside the unit's story list; no epic-scoped `next`
  picking siblings from outside the checklist.
- **Resumable** — re-running skips `- [x]` lines; paused loops resume with `/ship-epic <epic> resume`.
- **Gate-aware** — never auto-ship a gate-labelled story.
- **Failure-aware** — never advance after `MAX_FIX_ROUNDS` exhaustion on a unit.
- **No silent stops** — before ending a `/ship-epic` turn, name which hard-limit row fired. If none
  fired, you are **not allowed** to stop; continue the loop or re-delegate the current unit.
- **No captain merge per unit** — green accepted units merge into `<EPIC_BRANCH>` via delegated
  `MERGE_MODE=auto`. Never hand the captain a unit PR link and wait. The only default human merge gate
  is the **epic PR** into `MAIN_BRANCH` at Stage 4.
- **CI every unit** — economy comes from fewer Planner/board **invocations**, not from skipping
  validation or acceptance on shipped code.
- **Orchestrator owns `gh`** — epic edits and loop control only; builders/reviewers live in
  `/ship-issue`.
- **Domain-neutral** — read {{project-instructions}} at intake; pass the quality bar into Stage 1.5.

## Runtime input

`$ARGUMENTS` contains the epic issue number and optional guidance. The first token is `<epic>`; the
rest is guidance (`resume`, `dry run`, `epic close auto`, `epic merge auto`, `batch off`, `unit merge manual`, etc.).
