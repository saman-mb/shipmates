---
name: ship-epic
description: Shipmates: Loop /ship-issue over an epic's stories in dependency order — one epic plan amortizes overhead, cohesive stories batch into single runs, gate stories pause for sign-off.
argument-hint: <epic-issue-number> [resume | dry-run | epic close auto | batch off | unit merge manual | retry-story <n>]
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
8. **Captain-facing digest** — maintain one living **Epic progress** comment on epic `<epic>` (what
   shipped, board verdicts, unit PR links) and keep epic PR `<EPIC_PR>` notes current with **what the
   epic delivers** and a **quick review guide** — so the captain reviews one integration artifact without
   hunting scattered unit threads.
9. **Land-state skip gates** — treat checklist ticks, closed stories, `<epic-log>` / progress-comment
   merged units, and merged PRs into `<EPIC_BRANCH>` as **done** before re-delegating. Reconcile drift
   (unchecked box but already landed) by backfilling ticks — never pay for duplicate unit PRs.
10. **Captain always reviews the epic PR** — a human **always** merges epic PR `<EPIC_PR>` into
    `MAIN_BRANCH`. This command never auto-merges the epic PR. Standalone `/ship-issue` may still use
    `MERGE_MODE=auto` for smaller, single-issue PRs.

Hard limits that **pause the epic loop** (end the turn; post `/ship-epic <epic> resume`):

| Limit | When it fires |
|-------|----------------|
| **Gate story** | Unit contains a `gate`-labelled (or sign-off) story still awaiting human sign-off |
| **`MAX_FIX_ROUNDS` exhausted** | Stage 4.5 or Stage 6 on this unit could not get CI green / acceptance pass |
| **Manual unit merge** | Unit used `MERGE_MODE=manual` (`UNIT_MERGE_MODE=manual`, a `gate` story, or other explicit forced-manual path) — green PR awaits captain merge into `<EPIC_BRANCH>` before the next unit |
| **Shell safety abort** | Untrusted input, cycle in dependency graph, invalid epic token |

Everything else — including **red CI on the unit PR**, **red CI already on `<EPIC_BRANCH>`**,
**partly blocked stories**, and **owner-only remainders** (DNS, registrar, deploy-console attach,
or any AC that only the captain can flip with zero in-repo prep left) — stays in the **fix / defer /
next-unit / crew-complete** loop. **Do not pause the epic** for owner-only work. Naming an external
dependency is not a license to end the turn.

**Not a pause — harness backgrounding.** A harness hint that child agents were backgrounded, or that
you should **end the turn** so it can deliver their completion later (Cursor's Task tool says this
when builders run in the background), is **not** a hard-limit row. The current `/ship-issue` unit is
still in Stage 2. Stay in that unit until every builder returns (or fails), or **immediately
re-delegate the same unit**. Ending the turn here is a silent stop.

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
- `EPIC_PR` = the open integration PR number once Stage 0.5 creates or resumes it. **Required** — a
  terminal report with `EPIC_PR: n/a` or `EPIC_BRANCH: n/a` is a spec violation.
- `UNIT_MERGE_MODE` = `auto` — delegated `/ship-issue` runs merge green unit PRs into `<EPIC_BRANCH>`
  without captain action. Override with guidance `unit merge manual` (discouraged mid-epic). Forced
  to `manual` when the unit contains a `gate` story. `IS_SECURITY_SENSITIVE` does **not** force
  `manual` here: those units still auto-merge into `<EPIC_BRANCH>` under `auto`, still carry the
  `/shipmates-harden` recommendation in the unit report, and still wait for the captain at the
  **epic PR** (`EPIC_MERGE_MODE=manual`). Security-sensitive stories stay **singleton units** (Stage 1.5).
- `EPIC_MERGE_MODE` = `manual` — **fixed**. When the checklist is complete (or crew-complete with owner
  residuals), stop with epic PR `<EPIC_PR>` open for the captain to merge into `MAIN_BRANCH`. There is
  **no** auto-merge path for epic PRs. Guidance `epic merge auto` is **hard-rejected** at Stage 0 — stop
  and tell the captain that epic PRs always require human review; use standalone `/ship-issue` with
  `MERGE_MODE=auto` for hands-off single-issue delivery instead.
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
   `<guidance>` (`resume`, `dry run`, `epic close auto`, `batch off`, `unit merge manual`,
   `retry-story <n>`, etc.). Guidance `batch off` sets `EPIC_BATCH=off`; `epic close auto` sets
   `EPIC_CLOSE_MODE=auto`; `unit merge manual` sets `UNIT_MERGE_MODE=manual`. If guidance includes
   `epic merge auto`, **stop** — epic PRs always require captain review (see Config).
2. Fetch the epic: `gh issue view <epic> --json number,title,body,labels,state,url`. Confirm it is
   open and labelled `epic` (or has a story checklist in its body — if neither, stop and ask).
3. Parse the epic body for checklist lines matching `- [ ] #<n>` (unchecked) and `- [x] #<n>` (done).
   Let `<pending>` = unchecked story numbers in checklist order; let `<done>` = already ticked.
   Let `<all-stories>` = every checklist story number (done + pending).
4. **Load progress state** — scan epic `<epic>` comments for `<!-- shipmates-epic-progress -->`
   (**always**, not only when guidance includes `resume`). When found, parse machine-readable lines
   `EPIC_BRANCH:`, `EPIC_PR:`, `MAIN_BRANCH:`, and optional `SHIPPED_STORIES:` (space-separated issue
   numbers). Reload `<epic-log>` from the **Shipped units** section. Initialize `<epic-capsule>` from
   the same comment when present — otherwise empty. Guidance `resume` is a captain hint; idempotent
   re-entry must work on a bare `/ship-epic <epic>` whenever this comment exists.
5. **Reconcile landed stories** — build `<landed>` = stories already in `<done>`, plus every number in
   `SHIPPED_STORIES:` and every `#<n>` referenced in `<epic-log>` unit bullets marked merged. Also scan
   merged PRs into `<EPIC_BRANCH>` when `EPIC_BRANCH` is known (`gh pr list --base <EPIC_BRANCH> --state
   merged --json number,title,body`) and add any story whose `Closes #<n>` appears in the PR body.
   **Mis-targeted units** — also scan merged PRs with base `MAIN_BRANCH` whose body contains
   `Closes #<n>` for any `<n>` in `<all-stories>` (or `Part of #<epic>`). Record each in
   `<mis-merged-to-main>` with PR URL and merge SHA — these landed on the default branch instead of
   `<EPIC_BRANCH>` and trigger reconstruction in Stage 0.5. Add their story numbers to `<landed>`.
   Remove `<landed>` from `<pending>`. For each story still `- [ ]` on the epic body but in `<landed>`,
   **backfill now** (Stage 3 tick) and note **checklist recovery** in the report — do not wait for a
   successful unit. Stories in `<landed>` are **never** re-delegated unless guidance includes
   `retry-story <n>` for that number.
6. If `DRY_RUN`, print `<epic>` title, `<done>`, `<landed>`, `<pending>`, `<mis-merged-to-main>`, and
   continue through Stage 1.5 for the unit plan — then stop before Stage 2 (Stage 0.5 prints the
   planned `<EPIC_BRANCH>` only; no branch or PR is created).

## Stage 0.5 — Epic integration branch  (orchestrator)

Create or resume the **integration line** every unit lands on. **Always run** after Stage 0 (even when
`<pending>` is empty or every story is `<landed>`) — the captain must always get epic PR `<EPIC_PR>`.
Skip branch/PR mutation in `DRY_RUN` (print the planned names in the dry-run summary).

1. Set `MAIN_BRANCH` from the repo default (see Config).
2. **Reuse integration state** — when Stage 0 loaded `EPIC_BRANCH:` and `EPIC_PR:` from the progress
   comment, reuse them when `git ls-remote origin <EPIC_BRANCH>` succeeds. If the branch is missing,
   recreate per step 4 (including the identical-tip kickoff) and note recovery in the report. Do **not**
   open a second epic PR when `<EPIC_PR>`
   is already set — refresh its body in Stage 3.5 instead.
3. **Reconstruct mis-targeted work** — when `<mis-merged-to-main>` is non-empty and `<EPIC_BRANCH>` is
   unset or equals a fresh branch with no unit commits:

   a. Create or reset `<EPIC_BRANCH>` from `MAIN_BRANCH` at the merge-base before the first mis-merge.

   b. Cherry-pick (or merge) each mis-merged unit's commits onto `<EPIC_BRANCH>` in checklist order.
      Prefer the recorded merge SHAs from `<mis-merged-to-main>`.

   c. Push `<EPIC_BRANCH>` and note **integration recovery** in the report — the epic PR will show
      the combined in-repo diff the captain should have reviewed.

   When `<landed>` is non-empty but `<mis-merged-to-main>` is empty and `<EPIC_BRANCH>` already contains
   those commits, skip reconstruction.
4. **Create** (when `<EPIC_BRANCH>` is unset and branch does not already exist on the remote):
   ```bash
   git -C <repo> fetch origin
   git -C <repo> push origin origin/<MAIN_BRANCH>:refs/heads/<EPIC_BRANCH>
   ```
   Hosts refuse a pull request when head and base tips are identical (no commits between the refs).
   After the push — and after any recreate of a missing branch whose tip still matches
   `origin/<MAIN_BRANCH>` — check those two tips. If they match, check out `<EPIC_BRANCH>`, create
   **one** empty commit with the fixed message `chore: epic kickoff`, and push it **before** step 5.
   That is the normal kickoff path, not an error-recovery footnote. Skip when the tips already
   differ (reconstruction or landed work). Never stack a second kickoff commit. Skip the whole
   create **and** the kickoff in `DRY_RUN` (print that a kickoff commit would be made).
5. **Open epic PR** — if `<EPIC_PR>` is unset, open one PR with base `MAIN_BRANCH`, head `<EPIC_BRANCH>`.
   **Never skip** because `<pending>` is empty or work is already on `MAIN_BRANCH` — reconstruction
   (step 3) ensures the epic PR head reflects landed in-repo slices. Title/body via `--body-file` (see
   **Shell safety**). Structure the body for fast captain review:

   - **What this epic delivers** — one plain-language paragraph: epic goal plus which story numbers land
     here (from the epic issue title/body; no jargon).
   - **Quick review guide** — bullets: review once at epic completion (each unit already passed mandatory
     PE+PO on its PR head); start with the story checklist; scan the combined diff for cross-story
     interactions; trust green CI on this head; **the captain merges this PR** — it is never auto-merged.
   - **Stories** — copy unchecked checklist lines from the epic body; refresh after each unit in Stage 3.5.
   - **Shipped so far** — placeholder until the first unit lands; then copy from `<epic-log>`. When
     `<mis-merged-to-main>` was reconstructed, note which unit PRs were recovered from `MAIN_BRANCH`.
   - **Integration** — epic issue link, integration branch name, and explicit note: **human review
     required** — merge only when satisfied with the epic scope.

   Record `<EPIC_PR>`.
6. **Persist state** — post or **edit** the single `<!-- shipmates-epic-progress -->` comment on epic
   `<epic>` (see **Stage 3.5**). Include machine-readable lines `EPIC_BRANCH:`, `EPIC_PR:`, `MAIN_BRANCH:`,
   and `SHIPPED_STORIES:` (space-separated numbers from `<landed>`) so any re-run reloads idempotently.
   Update after every unit and every pause — one living comment, not a new thread per unit.
7. **Empty pending shortcut** — when `<pending>` is empty after Stage 0 reconciliation, skip Stages 1–2
   and go directly to **Stage 4** (full closure or crew-complete). `<EPIC_PR>` must already be set from
   steps above.

## Stage 1 — Story graph  (orchestrator)

Skip when Stage 0.5 step 7 sent the run to Stage 4.

For each story still in `<pending>` (after Stage 0 reconciliation):

1. Fetch `gh issue view <n> --json number,title,body,labels,state,url`.
2. Skip closed stories — treat as already done; add to `<landed>` and backfill checklist ticks in Stage 3.
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

Skip when Stage 0.5 step 7 sent the run to Stage 4.

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

Skip when Stage 0.5 step 7 sent the run to Stage 4.

Walk `<units>` in order — one `/ship-issue` delegation per unit (not one per story unless the unit
is a singleton).

For each `<unit>`:

1. **Stories already landed** — skip the whole unit when every story in the unit is in `<landed>`,
   ticked, or closed. When the unit is **mixed** (some landed, some not), drop landed stories from the
   unit scope and delegate only the remainder — never re-open a merged unit PR for an already-landed
   story. If guidance includes `retry-story <n>`, remove `<n>` from `<landed>` for this run only before
   evaluating skip rules.
2. **Pre-delegate guard** — immediately before step 6, assert **no** story in the unit scope appears
   in `<landed>` or `<epic-log>` as merged (unless `retry-story <n>`). Violation means reconcile failed —
   stop and report; do not open a duplicate unit PR. Assert delegated runs will use
   `epic-base=<EPIC_BRANCH>` — unit PR base **must not** be `MAIN_BRANCH`.
3. **Gate unit** — if any story in the unit is in `<gates>` and still open: **pause** with
   **awaiting sign-off**. Do **not** invoke `/ship-issue`. **Stop** — this is the only deliberate
   human gate in the loop.
4. **External / mixed blocker** — for each story in the unit blocked by an open external issue:

   a. **Shippable slice** — if the story body (or Stage 1.5 `blocker_class: partial`) names work that
      does **not** require the blocker to close, treat that slice as the **unit scope**. Invoke
      `/ship-issue` on that slice; in the PR body note the residual owner action and link the blocker.
      **Do not pause the epic.**

   b. **Owner-only remainder** — when every remaining AC for the unit truly requires the external
      blocker and there is **zero** shippable remainder (`blocker_class: full`): record the owner
      action in `<epic-log>` (residual, not a pause), file or update a follow-up if useful, tick or
      comment on the story that prep shipped, leave the flip open, and **continue to the next unit**
      without pausing. **Do not** post `/ship-epic <epic> resume` for owner DNS / registrar / deploy
      console work.

   Do **not** pause the whole epic because one story is *partly* blocked or *fully* owner-only.
5. **Build delegation guidance** (compact prose, not a transcript). Include:
   - `epic-run` — story numbers in this unit belong to epic `<epic>`; do **not** scan the wider
     backlog or propose bundle widening beyond this unit's story list.
   - `epic-id=<epic>` — parent epic issue number; `/ship-issue` must return an **Epic unit record**
     block in its final report (see that command) so this orchestrator can append to `<epic-log>`.
   - `epic-base=<EPIC_BRANCH>` — unit PR base and worktree cut from `origin/<EPIC_BRANCH>`, **not**
     `MAIN_BRANCH`. **Mandatory** — a unit PR targeting `MAIN_BRANCH` is a spec violation. When
     resuming after another unit merged, include **`sync-base`** so `/ship-issue` Stage 1 re-fetches
     and rebases onto the current integration tip before build work.
   - `epic-plan` — paste the unit's pre-classification (complexity + flags per story); the
     story-level Planner should treat this as the starting plan and amend only on contradiction.
   - `epic-capsule` — paste `<epic-capsule>` when non-empty (validation commands, paths, conventions
     from prior units in this run).
   - `MERGE_MODE=<UNIT_MERGE_MODE>` — merge the green unit PR into `<EPIC_BRANCH>` via `/ship-issue`
     Stage 8 when `UNIT_MERGE_MODE=auto` (default). When `manual` (guidance `unit merge manual`, a
     `gate` story, or other explicit forced-manual path), pass `MERGE_MODE=manual` and **pause after
     the unit** per the **Manual unit merge** hard limit — do not advance to the next unit until the
     captain merges into `<EPIC_BRANCH>` and the orchestrator resumes. `IS_SECURITY_SENSITIVE` does
     **not** force `manual` on an epic unit. **Never ask the captain to merge a green epic unit when
     `UNIT_MERGE_MODE=auto`.**
   - Tier hint — when **every** story in the unit is `trivial` and no specialist flags are set,
     include `complexity tier: simple` so `/ship-issue` takes the Simple path. When all are
     `trivial` or `standard` with no arch/security/delivery flags, include `complexity tier: medium`.
     Otherwise omit (full High path).
6. **Delegate** — invoke `/ship-issue` with **all story numbers in the unit scope** (after step 1
   trimming) as the leading numeric tokens, then the guidance from step 5. Example shape:
   `/ship-issue 101 102 epic-run …` for a two-story unit. Singleton: `/ship-issue 103 epic-run …`.
   Do **not** set `BUNDLE=off` — explicit multi-story tokens **are** the bundle; singletons behave as today.
7. **Outcome:**
   - **Success (auto-merged)** — unit used `MERGE_MODE=auto` and landed on `<EPIC_BRANCH>` → Stage 3 for
     **each** story in the unit (tick every checklist line the unit closed), **Stage 3.5** (append unit
     delivery + review summary to `<epic-log>` and refresh epic PR notes), extend `<epic-capsule>` with
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
   - **Gate mid-run** → pause and stop (same as step 3).

**Pre-existing CI on the merge base** — when a failing check **already fails on `<EPIC_BRANCH>`**
(same job red on the epic integration line) and the unit PR touches unrelated files:

- **Not an epic pause.** Remediation belongs in **this unit's** `/ship-issue` run (Stage 4.5).
- Fix what makes **this PR head** green: format only touched files if that suffices; otherwise fix
  the minimal set the log requires; gate or skip tests that cannot run under the repo's build flags
  when that matches project convention.
- Document extra commits in the PR (note the job was already red on `<EPIC_BRANCH>`). Escalate to
  the captain **only** after `MAX_FIX_ROUNDS`, or when the fix is a one-way product decision — not
  because the integration branch is dirty.

When every unit has succeeded **or** every remaining unit was handled as **owner-only remainder**
(step 4b) with nothing left to delegate, go to Stage 4.

## Stage 3 — Tick epic checklist  (orchestrator)

After each successful unit, for **each story** in that unit:

1. Re-fetch the epic body.
2. Replace `- [ ] #<story>` with `- [x] #<story>`.
3. Write via `--body-file`; verify the tick landed.

Backfill ticks for stories closed before this run but still unchecked in the epic body.

`/ship-issue` ticks the epic when `MERGE_MODE=auto`; this stage ensures ticks when a unit used
`MERGE_MODE=manual` (gate stories and other explicit manual units).

## Stage 3.5 — Epic progress log & PR notes  (orchestrator)

After each auto-merged unit (and after a manual unit once the captain has merged it and the orchestrator
resumes), ingest the delegated `/ship-issue` **Epic unit record** and update captain-facing artifacts.

1. **Parse the unit record** from the delegation's final report (`EPIC_UNIT_RECORD:` block — stories,
   PR link, merge SHA, one-line **delivered** summary per story, **reviews** one-liner, green CI link,
   fix-round count).
2. **Append to `<epic-log>`** — one bullet per unit, newest last. Shape: unit index, story numbers, PR
   URL, merge SHA, one-line delivered summary, reviews one-liner, green CI link. Keep `<epic-log>`
   scannable — no transcripts, no raw board dumps.
3. **Edit the epic progress comment** on epic `<epic>` — single comment anchored
   `<!-- shipmates-epic-progress -->`. Include: machine-readable `EPIC_BRANCH` / `EPIC_PR` / `MAIN_BRANCH`
   lines; **`SHIPPED_STORIES:`** (all numbers in `<landed>` after this unit); a **Shipped units** section
   (paste `<epic-log>` bullets); **Pending stories** (remaining checklist lines); **Latest reviews** (one
   line from the most recent unit record); and an updated timestamp. One living comment — edit in place,
   do not open a new thread per unit.

4. **Refresh epic PR `<EPIC_PR>` body** via `--body-file` — keep **What this epic delivers** and **Quick
   review guide** intact; update **Stories** checklist ticks, **Shipped so far** (copy `<epic-log>`), and
   add a **Review status** line: "`U` of `N` stories landed; all units passed PE+PO board before merge
   into `<EPIC_BRANCH>`." Goal: the captain opens epic PR or epic issue and knows what shipped and what
   was already reviewed without opening every unit PR.

On pause, include `<epic-log>` and a link to the progress comment in the pause report.

## Stage 4 — Epic closure  (orchestrator)

**Full closure** — when no `- [ ] #n` lines remain on the epic checklist:

1. **Epic PR gate** — poll `gh pr checks <EPIC_PR>` on the integration PR head until green or
   `MAX_FIX_ROUNDS`-bounded fix loop exhausted (fix on `<EPIC_BRANCH>`, not on `MAIN_BRANCH`).
2. **Epic integration board** — **never skip on full closure** (not crew-complete with owner
   residuals). After step 1 is green, convene specialists on epic PR `<EPIC_PR>` head:

<!-- shipmates:epic-integration-board -->

   Fix any REJECT on `<EPIC_BRANCH>`, push, re-poll step 1, **Retry** the board from the fixer
   delta (shared rule) — bounded by `MAX_FIX_ROUNDS`. Exhaustion **pauses the epic** with
   integration blockers (distinct from unit pause).
   Record integration verdicts separately from per-unit `<epic-log>` reviews.
3. **Finalize epic PR notes** — edit `<EPIC_PR>` body: confirm **Quick review guide** still accurate;
   set **Shipped so far** to the full `<epic-log>`; add **Ready to merge** checklist (all stories ticked,
   CI green, **integration board** summary plus per-unit PE+PO in progress log). Post the same summary on
   epic `<epic>` progress comment under `### Ready for captain`.
4. Post completion summary with **epic PR `<EPIC_PR>`** link, green CI link, **integration board
   verdicts**, pointer to epic progress comment, checklist state. **Stop** — the captain merges the
   epic PR into `MAIN_BRANCH` once satisfied. Do **not** auto-merge the epic PR. Do **not** ask the
   captain to merge individual unit PRs.
5. **`EPIC_CLOSE_MODE`** — `manual`: propose closing epic `<epic>`; `auto`: `gh issue close <epic>`.

**Crew-complete (owner residuals remain)** — when `- [ ] #n` lines still exist but **every** pending
story is owner-only with zero in-repo remainder (the crew has nothing left to delegate):

1. **Epic PR must exist** — if `<EPIC_PR>` is unset, Stage 0.5 was skipped or failed; stop and report.
   Crew-complete without a reviewable epic PR is invalid.
2. Refresh epic progress comment: **Shipped units** (`<epic-log>`), **Owner residuals** (checklist per
   pending story — what prep landed, what the captain must do), **Pending stories** (unchecked lines).
3. Refresh epic PR `<EPIC_PR>` notes with the same split — crew work complete; owner checklist explicit;
   **human review required** before merge to `MAIN_BRANCH`.
4. Post a **completion-style final report** on the epic issue (not a pause block — **no**
   `/ship-epic <epic> resume`). State that the crew loop is **finished**; the captain reviews and merges
   epic PR `<EPIC_PR>` when ready — not auto-merged.
5. **Stop** — this is a normal terminal turn, not a hard-limit pause. Do **not** ask the captain to
   poke the command again for owner DNS / registrar work.

## Final report

One concise summary: epic link, **epic progress comment** link on the epic issue, **epic PR `<EPIC_PR>`**
link (integration vs `MAIN_BRANCH`), units shipped (`U` invocations for `N` stories), `<epic-log>`
highlights, **integration board verdicts** (when full closure ran), gate pauses, **owner residuals** (distinct from pauses), integration recovery notes when
`<mis-merged-to-main>` was reconstructed, capsule highlights, epic close or crew-complete state, and
resume command **only if** a hard-limit pause occurred. For each pause, state **which hard-limit row
fired** — "waiting for captain" without a limit name is a spec violation. Owner-only remainder must
**not** appear as a pause reason. Include **economy line**: "`N` stories in `U` `/ship-issue` runs" so
the captain sees what batching saved. **Never** report `EPIC_PR: n/a` or `EPIC_BRANCH: n/a`.

---

### Guardrails

- **Compose, don't duplicate** — each unit is a `/ship-issue` run; never reimplement its stages inline.
- **One epic planner** — Stage 1.5 runs once per `/ship-epic` invocation; do not spawn a second
  epic-wide planner inside the loop.
- **Batch only on merit** — cohesion, file ownership, and flags gate batching; never merge unrelated
  stories because the epic is long.
- **No backlog widening** — delegated runs stay inside the unit's story list; no epic-scoped `next`
  picking siblings from outside the checklist.
- **Resumable** — any re-run reloads the progress comment when present (not only with `resume`);
  `<landed>` + checklist backfill prevent redoing merged work. Paused loops continue with
  `/ship-epic <epic> resume`.
- **No duplicate unit PRs** — never delegate a story in `<landed>` / `<epic-log>` unless
  `retry-story <n>` is explicit.
- **Gate-aware** — never auto-ship a gate-labelled story.
- **Failure-aware** — never advance after `MAX_FIX_ROUNDS` exhaustion on a unit.
- **No silent stops** — before ending a `/ship-epic` turn on a **pause**, name which hard-limit row
  fired. If none fired, you are **not allowed** to stop on a pause/resume handshake. Owner-only
  remainder uses **crew-complete** (Stage 4) — a normal terminal report, not a pause. Harness
  backgrounding / "end your turn for notifications" is **not** a pause — see **Not a pause —
  harness backgrounding** above; keep or re-delegate the in-flight `/ship-issue` unit.
- **Owner-only remainder is not a pause** — DNS, registrar, deploy-console attach, or any AC only the
  captain can satisfy with zero in-repo slice left: record residual, continue or crew-complete; never
  post `/ship-epic <epic> resume` for it.
- **Epic PR always open** — every terminal turn (full closure, crew-complete, or pause) must name a
  real `EPIC_PR` and `EPIC_BRANCH`. Skipping the epic PR because in-repo work is done or already on
  `MAIN_BRANCH` is forbidden — reconstruct the integration line instead.
- **Never auto-merge epic PRs** — `EPIC_MERGE_MODE` is fixed at `manual`. Reject `epic merge auto`
  guidance. Standalone `/ship-issue` `MERGE_MODE=auto` is unchanged.
- **Unit PRs never target `MAIN_BRANCH`** — delegated runs must pass `epic-base=<EPIC_BRANCH>`. Mis-targeted
  unit PRs are recovered via Stage 0.5 reconstruction, not accepted as the terminal state.
- **No captain merge per unit** — green accepted units merge into `<EPIC_BRANCH>` via delegated
  `MERGE_MODE=auto`, **including** `IS_SECURITY_SENSITIVE` units. Never hand the captain a unit PR
  link and wait unless a hard-limit row actually fired (`gate`, `unit merge manual`). The only human
  merge gate for shipped code is the **epic PR** into `MAIN_BRANCH` at Stage 4; security still
  surfaces as the `/shipmates-harden` recommendation plus the integration board, not a per-unit pause.
- **CI every unit** — economy comes from fewer Planner/board **invocations**, not from skipping
  validation or acceptance on shipped code. **Integration board** at epic closure is never skipped on
  full closure — it is the holistic review of the combined epic PR.
- **Orchestrator owns `gh`** — epic edits and loop control only; builders/reviewers live in
  `/ship-issue`.
- **Captain digest** — `<epic-log>` and the `<!-- shipmates-epic-progress -->` comment are the source of
  truth for human review; epic PR notes stay in sync. Never leave the captain to reconstruct unit work
  from scattered PR threads alone.

## Runtime input

`$ARGUMENTS` contains the epic issue number and optional guidance. The first token is `<epic>`; the
rest is guidance (`resume`, `dry run`, `epic close auto`, `batch off`, `unit merge manual`, `retry-story <n>`, etc.).
