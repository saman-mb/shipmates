---
name: fix-bug
description: Fix a bug the honest way — reproduce it as a failing test first, root-cause it, apply the minimal fix, and prove it with the test flipping red→green while the suite stays green. Worktree-isolated, CI-gated, opens a PR.
argument-hint: <issue-number or a description of the bug> [optional repro hints]
allowed-tools: Bash, Read, Write, Edit, Agent, Grep, Glob, WebSearch, WebFetch
disable-model-invocation: true
---
# /fix-bug — reproduce → root-cause → fix → prove
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
- `MAX_FIX_ROUNDS` = `3`. `MERGE_MODE` = `manual` (stop at a reviewed PR; `auto` opt-in).
- **Quality bar / test commands** = whatever the repo's README / {{project-instructions}} / test config states. Read it first.
- Reuse required trailers from the session context (a `Co-Authored-By:` line at minimum); the
  orchestrator owns all git/gh — agents never push.

## Stage 0 — Reproduce as a FAILING test  ⛔ HARD GATE

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
3. **Create the worktree:**

```bash
mkdir -p "$(dirname "<WORKTREE_DIR>")"
git -C <repo> fetch origin
git -C <repo> worktree add <WORKTREE_DIR> -b <BRANCH> origin/<BASE_BRANCH>
```
All work happens in the worktree; the base branch stays clean. Commit the failing test first so the
red→green history is visible.

## Stage 2 — Root-cause  (agent: `senior-engineer`, or `site-reliability-engineer` for runtime/ops bugs)

Diagnose the **actual mechanism** — work backwards from the failure (logs, stack, bisect, diff vs
last-known-good, and `git log` / `git blame` on the affected lines to understand past intent) to the defect itself, not the place it surfaced. The agent returns: the root cause
named, why it produces the symptom, and the minimal change that addresses it. Reject "add a null
check where it crashed" when the real cause is upstream.

## Stage 3 — Fix (minimal & scoped)  (agent: `senior-engineer`)

Apply the smallest change that fixes the named root cause. **No unrelated refactors or scope creep** —
this is a bug fix, not a redesign (file anything else as a follow-up). Then check for **sibling bugs**:
grep the codebase for the same defect class elsewhere and fix those in the same pass if trivial, or
file them.

## Stage 4 — Prove it  ⛔ HARD GATE  (agent: `sdet`)

The regression test must now **pass**, and the **entire suite must still be green** (the fix broke
nothing). Then push and run the **CI gate**: poll `gh pr checks` until done; if red, pull
`gh run view <run-id> --log-failed`, dispatch a `senior-engineer` fixer, re-push, re-poll — bounded by
`MAX_FIX_ROUNDS`. Never advance a red PR. (Long polls: run as a background/until-loop, not chained sleeps.)

## Stage 5 — Review  (agents, on the pushed PR head)

<!-- shipmates:acceptance-board -->

**Command-specific seats** (in addition to the mandatory PE+PO core):

- `sdet` (always): re-runs the suite on the PR head; confirms the regression test is present and green.
- `senior-engineer` or `site-reliability-engineer` (fresh — not the one who fixed it): confirms the fix
  addresses the root cause, not the symptom, and adds no regression risk.

Any `REJECT`/`FAIL` → loop a `senior-engineer` fixer, re-push, re-run the CI gate and this stage,
bounded by `MAX_FIX_ROUNDS`, then escalate.

## Stage 6 — Deliver

Open (or, if `MERGE_MODE=auto`, merge) the PR. Body: the root cause in one paragraph, the fix, the
regression test, `Closes #<issue>`, and the green-CI link. File sibling bugs / deferred cleanups as
follow-up issues. Report: root cause, the red→green proof, review verdicts, fix rounds, PR link, and
the absolute `<WORKTREE_DIR>` path (for cleanup or resume).

---

### Guardrails
- **The failing test comes first and is non-negotiable.** It's what distinguishes a fix from a guess,
  and it stops the bug ever coming back silently.
- Root cause over symptom — name the mechanism; don't patch where it surfaced.
- Minimal, scoped change; unrelated improvements become follow-ups, not part of this PR.
- Bounded loops; escalate with the log rather than spinning. Never advance a red PR.
- The reviewer is a **fresh** agent, never the one who wrote the fix.
- If a role doesn't resolve to an `{{agents-glob}}`, fall back to `{{general-purpose}}` with the brief
  inlined, and note it.

## Runtime input

`$ARGUMENTS` is the complete invocation text: a GitHub issue number or plain-text bug description,
plus optional reproduction hints. If it is a number, pull it with `gh issue view`; if empty, ask
what is broken.
