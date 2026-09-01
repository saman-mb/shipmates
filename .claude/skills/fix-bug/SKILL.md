---
name: fix-bug
description: Fix a bug the honest way — reproduce it as a failing test first, root-cause it, apply the minimal fix, and prove it with the test flipping red→green while the suite stays green. Worktree-isolated, CI-gated, opens a PR.
argument-hint: <issue-number or a description of the bug> [optional repro hints]
allowed-tools: Bash, Read, Write, Edit, Agent, Grep, Glob, WebSearch, WebFetch
disable-model-invocation: true
---
# /fix-bug — reproduce → root-cause → fix → prove
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
- **Quality bar / test commands** = whatever the repo's README / CLAUDE.md / test config states. Read it first.
- Reuse required trailers from the session context (a `Co-Authored-By:` line at minimum); the
  orchestrator owns all git/gh — agents never push.

## Stage 0 — Reproduce as a FAILING test  ⛔ HARD GATE

Spawn the `sdet` (with `site-reliability-engineer` if the bug is a runtime/reliability failure) to
find the **smallest deterministic reproduction** and encode it as a test in the repo's existing test
suite — asserting the *correct* behaviour, so it **fails now** for the real reason (not a typo).
Check CLAUDE.md for test runner invocations; if the test runner flags are unknown, run `--help` to discover them.
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

Spawn reviewers **in parallel** against the PR head commit — they review exactly what will merge.

**Mandatory seats (never skip)**

- **`product-manager`** (PO): checks every acceptance criterion AND the quality bar (README / CLAUDE.md / contributing). Returns `ACCEPT` / `ACCEPT-WITH-NITS` / `REJECT` with specifics per criterion.
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

If `principal-engineer` or any role does not resolve to an `.claude/agents/*.md` file (skill-only harnesses until crew agents ship), fall back to `general-purpose` with the role brief inlined and note the fallback — never silently skip a mandatory seat.

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
- If a role doesn't resolve to an `.claude/agents/*.md`, fall back to `general-purpose` with the brief
  inlined, and note it.

## Runtime input

`$ARGUMENTS` is the complete invocation text: a GitHub issue number or plain-text bug description,
plus optional reproduction hints. If it is a number, pull it with `gh issue view`; if empty, ask
what is broken.