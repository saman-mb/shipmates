---
name: ship-fix-bug
description: Shipmates: Fix a bug the honest way — reproduce it as a failing test first, root-cause it, apply the minimal fix, and prove it with the test flipping red→green while the suite stays green. Worktree-isolated, CI-gated, opens a PR.
argument-hint: <issue-number or a description of the bug> [sequential] [board=epic-deferred | board=off] [optional repro hints]
allowed-tools: Bash, Read, Write, Edit, Agent, Grep, Glob, WebSearch, WebFetch
disable-model-invocation: true
---
# /ship-fix-bug — reproduce → root-cause → fix → prove
<!-- shipmates:command-preamble -->

Take a bug from report to a **reviewed, CI-green PR** — but gated on the one signal that actually
proves a bug is fixed: a **regression test that fails before the change and passes after**. No repro,
no fix. Symptom-patching without a root cause is rejected.

The bug description and reproduction hints come from the Runtime input section at the end of this workflow.

---

## Config (override only if the repo needs it)

- `BASE_BRANCH` = the repo's default branch. `WORKTREE_LAYOUT` = `nested` (default) —
  `<repo>/.shipmates/worktrees/`; runtime guidance **`worktree-root=sibling`** selects legacy
  `../<repo>--…` paths. `WORKTREE_DIR` — **nested:** `<repo>/.shipmates/worktrees/bug-<slug>`;
  **sibling:** `../<repo>--bug-<slug>`. Re-runs reuse the same path. `BRANCH` = `fix/<slug>`.
- `EXECUTION` = `fanout` — how sibling bug fixes or independent reproduction/fix targets execute.
  `fanout` (default): when multiple sibling bugs or disjoint files are identified, spawn Builders/SDETs
  concurrently up to `MAX_CONCURRENT_WORKERS`. Guidance `sequential` sets `EXECUTION=sequential` to
  force serial execution.
- `MAX_CONCURRENT_WORKERS` = `5` — concurrency cap in `fanout` mode.
- `BOARD` = `full` (default) — Stage 5 acceptance board. `board=epic-deferred` defers it to an
  orchestrator's milestone board (the deferred board still runs there); `board=off` is an explicit
  captain opt-out with no deferral target. Both are the shared acceptance-board delegation modes.
- `MAX_FIX_ROUNDS` = `3`. `MERGE_MODE` = `manual` (stop at a reviewed PR; `auto` opt-in).
- **Quality bar / test commands** = whatever the repo's README / {{project-instructions}} / test config states. Read it first.
- Reuse required trailers from the session context (a `Co-Authored-By:` line at minimum); the
  orchestrator owns all git/gh — agents never push.

## Stage 0 — Reproduce as a FAILING test  ⛔ HARD GATE

Parse runtime input: the leading issue number or bug description is `<target>`; scan remaining tokens
for guidance:
- **`sequential`** — sets `EXECUTION=sequential` to force serial fix execution instead of default fan-out.
- **`board=epic-deferred`** — sets `BOARD=deferred` to defer Stage 5 to the milestone board named by the
  caller (valid only when that board is guaranteed). **`board=off`** — explicit captain opt-out, `BOARD=off`,
  no deferral target. With either, skip Stage 5 and proceed directly to delivery.

Spawn the `sdet` (with `site-reliability-engineer` if the bug is a runtime/reliability failure) to
find the **smallest deterministic reproduction** and encode it as a test in the repo's existing test
suite — asserting the *correct* behaviour, so it **fails now** for the real reason (not a typo).
Check {{project-instructions}} for test runner invocations; if the test runner flags are unknown, run `--help` to discover them.
Run it and confirm it's red. **If the bug genuinely cannot be reproduced, STOP** and report that with
what was tried — never "fix" an unconfirmed bug. This failing test is the contract for the whole run.

## Stage 1 — Isolate

1. **Resolve `<WORKTREE_DIR>`** from Config. Parse **`worktree-root=sibling`** from runtime guidance
   before resolving.
2. **Gitignore the worktree root** when `WORKTREE_LAYOUT=nested` — from `<repo>`, idempotently ensure
   `.shipmates/worktrees/` is ignored (append only when no line already ignores it; create `.gitignore`
   with `# Shipmates isolated command worktrees (auto-managed)` if missing). Never rewrite unrelated rules.
3. **Sync base ref** — `BASE_REF=origin/<BASE_BRANCH>`. **`git -C <repo> fetch origin`** is required;
   stop with a clear error if fetch fails. Fresh `worktree add … origin/<BASE_BRANCH>` **is**
   pull-latest — no separate `git pull`.
4. **Resume / reuse** — when worktree/branch exist, re-run fetch; rebase onto `BASE_REF` when behind
   (merge when repo docs prefer merge). Sync conflicts count toward `MAX_FIX_ROUNDS`.
5. **Create the worktree** (when branch/worktree do not yet exist):

```bash
mkdir -p "$(dirname "<WORKTREE_DIR>")"
git -C <repo> worktree add <WORKTREE_DIR> -b <BRANCH> origin/<BASE_BRANCH>
```
All work happens in the worktree; the base branch stays clean. Commit the failing test first so the
red→green history is visible.

## Stage 2 — Root-cause  (agent: `senior-engineer`, or `site-reliability-engineer` for runtime/ops bugs)

Diagnose the **actual mechanism** — work backwards from the failure (logs, stack, bisect, diff vs
last-known-good, and `git log` / `git blame` on the affected lines to understand past intent) to the defect itself, not the place it surfaced. The agent returns: the root cause
named, why it produces the symptom, and the minimal change that addresses it. Reject "add a null
check where it crashed" when the real cause is upstream.

## Stage 3 — Fix (minimal & scoped)  (agents: `senior-engineer` × N, parallel when fan-out)

Apply the smallest change that fixes the named root cause. **No unrelated refactors or scope creep** —
this is a bug fix, not a redesign (dispose anything else with nit disposition — absorb when cheap
and in-scope, else PR-note or a capped/batched follow-up). Then check for **sibling bugs**:
grep the codebase for the same defect class elsewhere.

**Execution posture**:
- Under `EXECUTION=fanout` (default), when sibling bugs or independent root causes span file-disjoint areas,
  spawn multiple `senior-engineer` Builders concurrently in a single message up to `MAX_CONCURRENT_WORKERS`,
  each with a machine-checkable **owned-paths manifest** (diffed against `git status` and `git diff --name-only`
  upon completion to prevent cross-unit collision).
- Under `EXECUTION=sequential`, apply fixes one at a time serially.

## Stage 4 — Prove it  ⛔ HARD GATE  (agent: `sdet`)

The regression test must now **pass**, and the **entire suite must still be green** (the fix broke
nothing). Then push and run the **CI gate**: poll `gh pr checks` until done; if red, pull
`gh run view <run-id> --log-failed`, dispatch a `senior-engineer` fixer, re-push, re-poll — bounded by
`MAX_FIX_ROUNDS`. Never advance a red PR. (Long polls: run as a background/until-loop, not chained sleeps.)

## Stage 5 — Review  (agents, on the pushed PR head)

<!-- shipmates:acceptance-board -->

**Deferral check**: with `board=epic-deferred` or `board=off` set (the shared acceptance-board delegation
modes), skip Stage 5 and proceed to Stage 6; `epic-deferred` must name the milestone board that will run
the review, and `board=off` is recorded in the report and PR body.

**Command-specific seats** (in addition to the mandatory PE+PO core):

- `sdet` (first convene): re-runs the suite on the PR head; confirms the regression test is present and green. On a retry it follows the shared Retry rule — re-running the gates covers it, so it re-sits only when the delta changes what the gates measure.
- `senior-engineer` or `site-reliability-engineer` (fresh — not the one who fixed it): confirms the fix
  addresses the root cause, not the symptom, and adds no regression risk.

Any `REJECT`/`FAIL` → loop a `senior-engineer` fixer, re-push, re-run the CI gate, then **Retry**
the board from the fixer delta (shared rule), bounded by `MAX_FIX_ROUNDS`, then escalate.

## Stage 6 — Deliver

Open (or, if `MERGE_MODE=auto`, merge) the PR. Body: the root cause in one paragraph, the fix, the
regression test, `Closes #<issue>`, and the green-CI link. Dispose sibling bugs / deferred cleanups
with the same **nit disposition** ladder as `/ship-issue` Stage 7 (default absorb-first for cheap
in-scope leftovers; cap and batch any filed issues; never open one ticket per trivial leftover).
Report: root cause, the red→green proof, review verdicts, fix rounds, PR link, disposition counts,
and the absolute `<WORKTREE_DIR>` path (for cleanup or resume).

---

### Guardrails
- **The failing test comes first and is non-negotiable.** It's what distinguishes a fix from a guess,
  and it stops the bug ever coming back silently.
- Root cause over symptom — name the mechanism; don't patch where it surfaced.
- Minimal, scoped change; unrelated improvements use nit disposition (absorb when cheap and in-scope,
  else PR-note or a capped/batched follow-up) — do not open one issue per leftover.
- Bounded loops; escalate with the log rather than spinning.
- The reviewer is a **fresh** agent, never the one who wrote the fix.
- If a role doesn't resolve to a shipped crew role, fall back to a general-purpose agent with the brief
  inlined, and note it.

## Parameters

| Name | Required | Token | Values | Default | Help |
|------|----------|-------|--------|---------|------|
| bug | yes | issue number or description | issue number  /  prose | — | Bug to fix; if a number, pull it with `gh issue view`. |
| sequential | no | `sequential` | `sequential` | — | Force serial fix execution instead of fan-out. |
| board | no | `board=…` | `board=epic-deferred`  /  `board=off` | full | Defer Stage 5 to a milestone board, or skip it with no deferral. |
| repro_hints | no | remaining prose | free text | — | Reproduction hints for Stage 0 (steps, env, failing command). |

## Runtime input

Read `## Parameters` first. `$ARGUMENTS` is the captain-supplied or post-intake token string; parse
Tokens/Values from that table (required then optionals). If intake ran, treat the restated
invocation as authoritative for this run.

The leading issue number or bug description is `<target>`; scan remaining tokens for `sequential`,
`board=epic-deferred`, `board=off`, and treat the rest as reproduction hints.
