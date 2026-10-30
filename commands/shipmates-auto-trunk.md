---
name: shipmates-auto-trunk
description: "Shipmates: Run a test-driven, trunk-based development loop — parse a goal into a task list, write a failing test first, implement the smallest change, run the repo's checks, and commit to trunk. Repeat until the task list is empty and checks are green. Commits and pushes are opt-in."
argument-hint: <goal> [mode=plan|run] [push=no-push|push] [stop-after=N] [verify-command=<cmd>]
allowed-tools: Bash, Read, Write, Edit, Agent, Grep, Glob
disable-model-invocation: true
---
# /shipmates-auto-trunk — TDD loop that drives trunk green, one commit at a time

<!-- shipmates:bootstrap -->
Take a single goal and run it to done on the default branch: break it into an
ordered task list, write or modify a failing test before every source change,
implement the smallest change that makes the test pass, run the repo's own
verification command, and commit each green result to trunk. Keep going until
the task list is empty and the suite stays green.

This is the workflow for a repo where you want every change to land as a
small, proven commit on the main line rather than as a reviewed PR. It is also
the most autonomous thing Shipmates ships: by default it **plans only**
(`mode=plan`) and **never pushes** (`push=no-push`). Both commits and pushes are
opt-in, and the run stops for the captain whenever a hard bound is hit.

**Scope vs `/shipmates-ship-issue`.** `/shipmates-ship-issue` takes a tracked
issue to a reviewed PR. `/shipmates-auto-trunk` takes a free-text goal straight to
trunk commits. Use it when the work is small, the tests are trustworthy, and
trunk-based delivery is the intended model. When in doubt, use
`/shipmates-ship-issue` — a PR review is a stronger gate than a local green run.

The goal and optional knobs come from the Runtime input section at the end of
this workflow.

---

## Config

- `MODE` = `plan` (default) — build the task list, preview the intended commits,
  and stop without writing to the working tree or the tracker. `run` — perform
  the TDD loop and commit each green change to `COMMIT_TARGET`.
- `PUSH_MODE` = `no-push` (default). `push` pushes commits to the remote after
  each green commit, but only when `MODE=run`.
- `COMMIT_TARGET` = the repo's default branch
  (`gh repo view --json defaultBranchRef -q .defaultBranchRef.name`).
- `VERIFY_COMMAND` = the command that proves a change — discovered from the
  repo's README / `{{project-instructions}}` / CI config. It should run tests,
  lint, type-check, and anything else the repo treats as a green gate.
- `MAX_RED_ROUNDS` = `3` — how many fix-or-rollback attempts a single task gets
  before the run stops and escalates.
- `STOP_AFTER` = `10` — hard brake on the number of tasks processed in one run.
  The run stops cleanly when the limit is reached and reports what remains.
- `QUALITY_BAR` / test commands = whatever the repo's README /
  `{{project-instructions}}` / CI config states. Read it in Stage 0.

## Stage 0 — Intake & repo context

Parse the runtime input first:
- **`mode=run`** — set `MODE=run`. Anything else keeps `plan`.
- **`push=push`** — set `PUSH_MODE=push`. Anything else keeps `no-push`.
- **`stop-after=<N>`** — override `STOP_AFTER`; `<N>` must be a positive integer.
- **`verify-command=<cmd>`** — override `VERIFY_COMMAND`. Captain-supplied shell;
  echoed back in the report so its blast radius is explicit.
- Everything else is the `<goal>`.

Then read the repo context:
1. Default branch and whether it is protected. If the default branch cannot
   accept direct pushes, record the protection status; Stage 2 will degrade to
   a branch+PR workflow or stop loudly rather than fail mid-loop.
2. Whether the repo has tests. A repo with no test runner is a **no-test repo**;
   the run states this explicitly and still verifies with `VERIFY_COMMAND`, but
   it cannot satisfy "failing test first" by construction.
3. `VERIFY_COMMAND` from docs / CI. Run `--help` on unfamiliar tools to discover
   flags. If none is found, fall back to the repo's documented test command.
4. Required commit trailers from the session context (a `Co-Authored-By:` line
   at minimum).

## Stage 1 — Build the task list  (agent: `product-manager`)

Spawn the `product-manager` to turn the `<goal>` into an ordered task list. Each
task is a discrete, testable change: a bug to fix, a small feature to add, or a
refactor that unblocks the next task. The task list is written to working memory
and echoed in the report.

- If the goal references GitHub issues, read them and convert their acceptance
  criteria into tasks.
- If the goal is a free-text objective, break it into 1–10 tasks that each fit
  a single red→green cycle.
- Record dependencies between tasks; a dependent task is never attempted before
  its dependency.

In `plan` mode, the run stops after this stage and prints the task list, the
discovered `VERIFY_COMMAND`, and the commit target. No file is changed.

## Stage 2 — TDD loop  (agents: `sdet`, `senior-engineer`)

While the task list is not empty and the task counter is below `STOP_AFTER`:

1. **Select the next task.** Prefer the first unblocked task in dependency order.
2. **Write or update a failing test.** Spawn the `sdet` to add or modify a test
   that asserts the correct behaviour for this task and **fails now**. If the
   repo has no tests, skip this sub-step and record that the task is
   unaccompanied by a failing test. The test must fail for the real reason,
   not for a typo.
3. **Implement the minimal change.** Spawn a `senior-engineer` to make the
   smallest change that makes the new test pass while keeping the existing
   suite green.
4. **Run `VERIFY_COMMAND`.** The `sdet` runs it against the working tree. Two
   outcomes:
   - **Green.** Proceed to commit.
   - **Red.** Try to fix within `MAX_RED_ROUNDS` by re-spawning the
     `senior-engineer` on the failure output. If a fix cannot be found, roll
     back the source change (and its test) for this task, append the task back
     to the list with a note, and move on. Never leave trunk in a red state.
5. **Commit to `COMMIT_TARGET`.** Only when `MODE=run`. Use a conventional commit
   message (`type(scope): description`) derived from the task. Include the
   required trailers. Do not commit in `plan` mode.
6. **Push, only when `PUSH_MODE=push`.** Only when `MODE=run`. No force push.
   If the remote rejects the push (protected branch, permission, or hook),
   stop loudly and report the exact error.
7. **Discover new issues.** If the change reveals a new bug, missing test, or
   follow-up refactor, append it to the task list with a link to the task that
   uncovered it. Do not silently expand scope; every discovery is logged.
8. **Report the iteration.** What changed, which test was added, the verify
   result, and any rollbacks.

If `COMMIT_TARGET` is protected and direct push is not possible, the run
degrades: cut a branch from the current trunk tip, commit there instead, open a
PR against `COMMIT_TARGET` at the end, and stop — never repeatedly attempt a
push that the repo has forbidden.

## Stage 3 — Stop & report

Stop when:
- the task list is empty, **and**
- `VERIFY_COMMAND` is green on the final tree, **and**
- every committed change in `run` mode has been pushed if `PUSH_MODE=push`.

Report:
- Mode that ran (`plan` or `run`).
- Tasks completed, rolled back, and newly discovered.
- Commits made (messages and SHAs) in `run` mode.
- Whether anything remains in the task list (including why it was not finished).
- Any blocked push, protection fallback, or escalated red task.

---

### Guardrails

- **No force push.** A trunk loop is only safe if history is not rewritten.
- **Plan by default.** `mode=run` and `push=push` are both opt-in; neither is
  the silent default.
- **Bounded loops.** `MAX_RED_ROUNDS` and `STOP_AFTER` are hard caps. When a cap
  is hit, stop and escalate to the user with a clear report.
- **Green trunk.** The loop never leaves `COMMIT_TARGET` red. A red task is
  rolled back, not half-committed.
- **Test-first, when tests exist.** A source change without a preceding failing
  test is only allowed in a no-test repo, and that fact is reported.
- **Captain-supplied `verify-command` is logged.** Because it executes arbitrary
  shell, the exact command is echoed in the report.
- **Protected-branch degrade.** Direct push to a protected default branch is
  detected up front; the run falls back to a branch+PR or stops loudly.
- **No merge as verification.** Do not merge to a shared branch "just to see if
  a CI/config fix works". Use a disposable branch/PR that is explicitly never
  merged.

## Parameters

| Name | Required | Token | Values | Default | Mutates | Help |
|------|----------|-------|--------|---------|---------|------|
| goal | yes | `<goal>` | free text | — | — | Natural-language objective; becomes the ordered task list. |
| mode | no | `mode=plan` / `mode=run` | plan / run | plan | yes | `run` commits to trunk; `plan` previews only. |
| push | no | `push=no-push` / `push=push` | no-push / push | no-push | yes | Whether to push commits to the remote; only honored when `mode=run`. |
| stop-after | no | `stop-after=<N>` | positive integer | 10 | — | Safety brake: max tasks before a forced pause. |
| verify-command | no | `verify-command=<cmd>` | shell string | repo default | — | Override the verification command; captain-supplied. |

## Runtime input

Parse `$ARGUMENTS` as follows: the first contiguous run of non-knob tokens is
the `<goal>`; recognized knob tokens (`mode=...`, `push=...`, `stop-after=...`,
`verify-command=...`) may appear anywhere. If the invocation is empty, stop and
ask for the goal.

Examples:
- `/shipmates-auto-trunk Fix every bug from the review in #42`
- `/shipmates-auto-trunk "Refactor the auth module to use the new policy API" mode=run push=push`
- `/shipmates-auto-trunk "Add dark mode toggle" mode=plan`
