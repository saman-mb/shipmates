---
name: onboard
description: Read an unfamiliar repo and write the agent-facing context file every other order depends on — conventions, commands, boundaries and the quality bar, proven by running them. Gated on a fresh agent answering the crew's real questions from the file alone.
argument-hint: [path to the repo — defaults to the current one]
allowed-tools: Bash, Read, Write, Edit, Agent, Grep, Glob
disable-model-invocation: true
arguments: path
loop_max: 2
stages: [{"order":1,"stage":"survey","roles":["architect"],"gate":"repo-understood","max_loops":1},{"order":2,"stage":"draft","roles":["technical-writer"],"gate":"context-written","max_loops":1},{"order":3,"stage":"verify","roles":["sdet"],"gate":"fresh-reader-complete","max_loops":2}]
invocation: @{{role}}({{path}})
board: native
---
# /onboard — recon → draft → prove it answers

Every role in this crew is told to hold the work to the standard in *your* repo's `README` /
`AGENTS.md`. Nothing produces that file. On a repo without a good one the whole crew quietly degrades
to generic advice — and the failure is silent, because a vague context file still yields confident
output. This order writes it, and **gates on a fresh agent being able to answer the crew's actual
questions from the file alone**.

Input (**{{path}}**): a path to a repo. Empty means the current one.

**This is not `/document`.** The difference is the *audience*, not the topic. `/document` writes for
**humans** and gates on a fresh reader completing a task. `/onboard` writes the **agent-facing
contract** that every other order loads at run time, and gates on a fresh agent answering the crew's
questions correctly. Same philosophy, different question — so neither forks the other. If what you
want is a README or a tutorial, stop and run `/document`.

---

## Config (override only if the repo needs it)

- `MODE` = `pr` (default) or `edit-in-place` — where the result lands. `pr` opens a worktree, a
  branch and a CI-gated PR rather than writing to the tree, reusing `/ship-issue`'s isolate stage and
  its commit-push-PR stage. This file is the contract every later run inherits, so it earns a diff
  and a human's eye before it lands; `edit-in-place` is an explicit request. `SURVEY` (`create` /
  `refresh`) is set by Stage 0 and describes what was *found* — it is a separate axis and never
  overwrites `MODE`.
- Under `MODE=pr`: `BASE_BRANCH` = the repo's default branch — the PR's target, not what the
  worktree is cut from (that's current `HEAD`, so Stage 0's survey sees your actual checkout).
  `WORKTREE_DIR` = `../<repo>--onboard-<SURVEY>`. `BRANCH` = `docs/onboard-context-file-<SURVEY>` —
  onboard has no topic slug to build a name from (it always produces the one context file), so the
  identifier is `SURVEY` instead: a still-open `create` PR then can't collide with a later `refresh`
  run, which is the failure this suffix exists to prevent. `MERGE_MODE` = `manual` (stop at a
  reviewed PR; `auto` opt-in). `MAX_FIX_ROUNDS` = `2` — a separate cap, on CI-fix rounds at Stage 4,
  so a permanently-red check escalates to the user instead of looping, distinct from the
  verification-loop `MAX_ROUNDS` below. A repo with no remote to open a PR against is the one
  fallback: build the branch, stop there, and report the branch as the undo path — never quietly
  write to the tree instead.
- `TARGET` = auto-detected. `TARGET.md` if one exists, else `AGENTS.md` if one exists, else
  `TARGET.md`. **Never write both** — see below.
- `MAX_ROUNDS` = `3` verification loops before escalating (Stage 3).

## Stage 0 — Survey & mode

Detect what already exists before writing anything:

- **Neither file exists** → `SURVEY=create`.
- **One exists** → `SURVEY=refresh`. **Never blind-overwrite.** Under `MODE=edit-in-place`, back it
  up first, reusing the installer's own convention: `<file>.bak-<timestamp>`. Under `MODE=pr` the
  branch *is* the undo path, so don't drop a backup into a tree you were asked not to touch. Read
  the existing file either way, and treat every hand-written rule in it as authoritative unless the
  repo contradicts it — a human wrote that for a reason you can't see from the code.
- **Both exist** → they are two sources of truth for one contract, which is the exact failure this
  order exists to prevent. Keep the richer one, and reduce the other to a one-line pointer at it.

Also read the repo's `README`, contributing docs, CI config and any existing rules files, so the
context file agrees with them instead of competing.

## Stage 0.5 — Isolate  (`MODE=pr` only — orchestrator, deterministic, no agent)

The branch exists before the context file does. First check `git -C <repo> status --porcelain`; if
the caller's tree is dirty, **warn loudly** — a worktree cut from `HEAD` holds committed work only,
so an uncommitted rule Stage 0's survey just saw won't carry into the draft — then proceed. Exactly
as `/ship-issue`'s isolate stage, but cut from current `HEAD` rather than `origin/<BASE_BRANCH>` — so
an unpushed `TARGET.md`/`AGENTS.md` that Stage 0's survey already saw is actually present in the
worktree the draft and refresh work against:

```bash
git -C <repo> worktree add <WORKTREE_DIR> -b <BRANCH> HEAD
```

Recon reads the repo either way; the draft and every verification round write inside
`<WORKTREE_DIR>`. Under `MODE=edit-in-place`, skip this stage — the Stage 0 backup is the undo path
instead.

## Stage 1 — Recon  (agents, in parallel)

Spawn these in a single message. Each returns findings, not prose:

- **`architect`** — the real structure: module boundaries, layering, where business logic lives,
  which invariants matter, and what a newcomer would break first.
- **`sdet`** — **runs** the build, test, lint and type-check commands. This is the point of the
  stage: the commands that end up in the file must be *proven*, not inferred from a config file.
  Anything it couldn't run is recorded as unverified rather than guessed at.
- **`devops-engineer`** (only if the repo has a pipeline, image, or infrastructure definition) — how
  the project actually builds and ships, the toolchain and version pins, and what a contributor needs
  installed before anything works.

## Stage 2 — Draft  (agent: `technical-writer`)

Hand the recon findings to the `technical-writer` and let the role do its job — don't restate writing
principles here. Brief it that the reader is **an agent about to change code**, not a newcomer
browsing, so the file must be dense and decision-shaped: stack and layout, the commands to run,
architectural non-negotiables, testing expectations, what's generated and must never be hand-edited,
what's off-limits, and the quality bar a change is held to. Short, checkable statements beat prose.

Record what could not be verified as unverified. An honest gap is safe; a confident wrong instruction
is not — every later run inherits it.

## Stage 3 — Verification  ⛔ HARD GATE  (fresh agent)

Spawn a **fresh** agent and give it the generated file and **nothing else** — no repo access while it
answers. Ask it exactly what the crew asks on a real run:

1. What command builds this? What runs the tests? What lints it?
2. Where does this kind of change belong, and what must it not touch?
3. What is the quality bar a change is held to before it can merge?
4. What is generated, and what must never be hand-edited?

Then verify every answer against the source yourself. A question it can't answer, or answers wrongly,
is a **gap in the file**, not a failure of the agent — send it back to Stage 2. Loop to `MAX_ROUNDS`,
then escalate with the unanswerable questions listed.

## Stage 4 — Deliver

Write to `TARGET` — inside `<WORKTREE_DIR>` under `MODE=pr` (the default), in the repo itself under
`MODE=edit-in-place`. If a file was replaced, **show the diff** — a human must be able to see what was
changed on their behalf. Under `MODE=pr`, commit on the branch — staging only the paths this run
produced, never `git add -A` — push, and open the PR against `BASE_BRANCH`. Then gate on CI: poll
`gh pr checks` until nothing is pending; a red check means pulling the failing log, fixing it,
re-pushing, and re-polling — bounded by `MAX_FIX_ROUNDS`, after which you stop and escalate to the
user with the failing log rather than looping. Never advance a red PR. Stop there unless `MERGE_MODE=auto`,
in which case merge the PR and remove `<WORKTREE_DIR>`; the manual default leaves the worktree in
place with the PR open. The context file only exists on that branch until it's merged, though — merge
or check out `BRANCH` before running other commands in this session if you need it in place now, or
pass `MODE=edit-in-place` up front instead. Report: the file written, the undo path (the backup, or
the branch), which commands were proven versus recorded as unverified, and the verification round it
passed on.

---

### Guardrails
- **An undo path is mandatory, not best-effort.** A bad context file degrades every future run in
  that repo with no failing signal, so the way back has to exist before the write does — the branch
  itself under `MODE=pr` (the default), a backup under `MODE=edit-in-place`.
- **One context file, never two.** Two sources of truth for the quality bar is the problem, not a
  tidy outcome.
- Proven over plausible: a command that wasn't run is labelled unverified, never presented as fact.
- **Be resumable.** A re-run may find the worktree, branch, or PR already exists — reuse them rather
  than erroring or duplicating work.
- Preserve hand-written rules on a refresh. You are augmenting someone's judgement, not replacing it.
- Don't write a README. If the content is for humans, it belongs in `/document`.
- If a role doesn't resolve to an `agent-files/*.md`, fall back to `general-purpose` with the brief
  inlined, and note it.
