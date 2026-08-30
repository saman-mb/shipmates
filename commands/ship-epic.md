---
name: ship-epic
description: Loop /ship-issue over an epic's unchecked stories in dependency order — gate stories pause for sign-off, failures pause with state, epic closes when the checklist is done.
argument-hint: <epic-issue-number> [resume | dry-run | epic close auto]
allowed-tools: Bash, Read, Write, Edit, Agent, Grep, Glob, WebSearch, WebFetch
disable-model-invocation: true
---
# /ship-epic — sequential epic delivery
<!-- shipmates:command-preamble -->

Deliver a whole **epic** by looping the existing `/ship-issue` pipeline over its unchecked story
checklist items — one story at a time, in dependency order — until every non-gate story ships or the
loop pauses for human action. Each story gets its own worktree, PR, CI gate, and acceptance board;
this command is the **orchestrator** only: it parses the epic, orders stories, delegates to
`/ship-issue`, ticks checklist boxes, and reports pause/resume state.

The epic issue number and optional guidance come from the Runtime input section at the end of this
workflow.

---

## Config (defaults — override only if the repo clearly needs it)

- `EPIC_LABEL` = `epic` — the epic must carry this label (or be clearly an epic by checklist shape).
- `GATE_LABEL` = `gate` — stories with this label pause the loop for human sign-off before shipping.
- `EPIC_CLOSE_MODE` = `manual` — when every checklist box is ticked: `manual` proposes closing the
  epic; `auto` runs `gh issue close` on the epic. Override with guidance `epic close auto`.
- `MERGE_MODE` = passthrough from `/ship-issue` (default `manual` there). Set `MERGE_MODE=auto` for
  fully hands-off delivery between stories.
- `MAX_FIX_ROUNDS` = passthrough from `/ship-issue` (default `3`). An acceptance failure that
  exhausts fix rounds on one story **pauses the epic loop** — it does not advance to the next story.
- `BUNDLE` = forced `off` for every delegated `/ship-issue` run — epic scope must not widen.
- `DRY_RUN` = on if the caller says "dry run" / "preview": print the story order, gates, and external
  blockers; invoke nothing.

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
   `<guidance>` (`resume`, `dry run`, `epic close auto`, etc.).
2. Fetch the epic: `gh issue view <epic> --json number,title,body,labels,state,url`. Confirm it is
   open and labelled `epic` (or has a story checklist in its body — if neither, stop and ask).
3. Parse the epic body for checklist lines matching `- [ ] #<n>` (unchecked) and `- [x] #<n>` (done).
   Let `<pending>` = unchecked story numbers in checklist order; let `<done>` = already ticked.
4. If `<pending>` is empty, go to **Stage 4 — Epic closure** (checklist complete).
5. If `DRY_RUN`, print `<epic>` title, `<done>`, `<pending>`, and continue to Stage 1 without
   invoking `/ship-issue`.

## Stage 1 — Story graph  (orchestrator)

For each story in `<pending>`:

1. Fetch `gh issue view <n> --json number,title,body,labels,state,url`.
2. Skip closed stories — treat as already done; note them for checklist backfill in Stage 3.
3. Detect **gate stories**: `gate` label present, or the body contains an explicit sign-off section
   (e.g. a heading like "Sign-off" / "Gate" with criteria the story says must pass before merge).
   Record `<gates>`.
4. Parse **dependencies** from the story body: `Blocked by #<n>`, `blocked_by #<n>`, or checklist
   cross-refs. Build a directed graph over `<pending>` plus any **external** blockers outside the epic.
5. **Topological sort** `<pending>` respecting dependencies. If a cycle is detected, stop and report
   the cycle — do not ship out of order.
6. Flag **external blockers**: any dependency on an open issue outside the epic (or outside `<done>`).
   Record `<blocked-externally>` with story → blocker mapping.

Print (always, not only dry-run): ordered story list, gate stories, external blockers.

## Stage 2 — Loop  (orchestrator)

Walk the topologically sorted list **sequentially** — one story at a time. Parallel independent
stories are a future opt-in; the default is strict sequence so each `/ship-issue` run owns its
worktree cleanly.

For each `<story>` in order:

1. **Already ticked or closed** — skip (resumable re-runs land here).
2. **Gate story** — if `<story>` is in `<gates>` and still open: **pause** with an **awaiting
   sign-off** report naming the epic, the gate story, and what closes/resumes the loop (human closes
   the gate story or removes the gate label after sign-off). Do **not** invoke `/ship-issue`. Stop.
3. **External blocker** — if `<story>` is in `<blocked-externally>` and the blocker is still open:
   **pause** with a report naming the story, the external blocker issue, and that the loop resumes
   when the blocker closes. Do **not** ship out of order. Stop.
4. **Unmet in-epic dependency** — if a `Blocked by` parent in the epic is still unchecked: skip
   forward only after the parent completes in this same run; if the sort was wrong, stop and report.
5. **Delegate** — invoke `/ship-issue <story>` with `BUNDLE=off` and the configured `MERGE_MODE` /
   `MAX_FIX_ROUNDS`. The orchestrator runs the full ship-issue workflow for that story number only.
6. **Outcome:**
   - **Success** (reviewed, CI-green PR per ship-issue; merged too when `MERGE_MODE=auto`) → Stage 3
     for this story, then continue to the next story.
   - **Paused / escalated** (ship-issue stopped after `MAX_FIX_ROUNDS`, ambiguous scope, or user
     escalation) → **pause the epic loop** with a **state report**: epic number, completed stories,
     current story, PR link if any, failure summary, and explicit **resume** instruction
     (`/ship-epic <epic> resume`). Do **not** advance to the next story. Stop.
   - **Gate encountered mid-run** — same as step 2 if the story was misclassified; pause and stop.

When every non-gate story in `<pending>` has succeeded, go to Stage 4.

## Stage 3 — Tick epic checklist  (orchestrator)

After each successful story shipment:

1. Re-fetch the epic body.
2. Replace the line `- [ ] #<story>` with `- [x] #<story>` (match the story number exactly).
3. Write the updated body via a temp file: `gh issue edit <epic> --body-file <file>`.
4. Verify the tick landed (re-fetch; grep for `- [x] #<story>`).

Also backfill ticks for stories that were already closed before this run but still showed unchecked
in the epic body.

`/ship-issue` may tick the epic when `MERGE_MODE=auto`; this stage ensures the tick happens on the
**manual** path too so the epic loop stays consistent.

## Stage 4 — Epic closure  (orchestrator)

When no `- [ ] #n` lines remain (all stories ticked or closed):

- **`EPIC_CLOSE_MODE=manual`** (default): report that the epic checklist is complete and **propose
  closing** the epic — print `gh issue close <epic>` as the suggested next step (or ask the user).
- **`EPIC_CLOSE_MODE=auto`**: `gh issue close <epic>` with a short comment summarizing shipped
  stories and PR links collected during the loop.

## Final report

One concise summary: epic link, stories shipped (with PR links), gate pauses encountered, external
blockers hit, whether the epic was closed or proposed for closure, and — if paused — the **resume**
command and outstanding blocker.

---

### Guardrails

- **Compose, don't duplicate** — each story is a full `/ship-issue` run; never reimplement its stages
  inline.
- **Sequential default** — one active ship-issue at a time per epic loop.
- **Resumable** — re-running `/ship-epic <epic>` skips `- [x]` lines and closed stories; a paused
  loop resumes with `/ship-epic <epic> resume` (same as re-invoking without advancing past the pause).
- **Gate-aware** — never auto-ship a gate-labelled story; always pause with **awaiting sign-off**.
- **Failure-aware** — never silently continue after `MAX_FIX_ROUNDS` exhaustion on one story.
- **No bundling** — delegated runs always use `BUNDLE=off`.
- **Orchestrator owns `gh`** — subagents spawned inside delegated `/ship-issue` runs follow that
  command's rules; this orchestrator owns epic edits and loop control only.
- **Domain-neutral** — same crew and quality bar as `/ship-issue`; read the project's
  {{project-instructions}} at intake.

## Runtime input

`$ARGUMENTS` contains the epic issue number and optional guidance. The first token is `<epic>`; the
rest is guidance (`resume`, `dry run`, `epic close auto`, etc.).
