---
name: shipmates-document
description: Shipmates: Write or refresh documentation that actually works — the technical-writer drafts it from the real code, then a fresh reader agent follows the steps against the repo and must reach the stated result. Loops until the docs are drift-free and completable.
argument-hint: <what to document — a module, a feature, a public API, the README, the whole repo> [sequential]
allowed-tools: Bash, Read, Write, Edit, Agent, Grep, Glob, WebSearch, WebFetch
disable-model-invocation: true
---
# /shipmates-document — draft from the code → fresh-reader test → fix drift
<!-- shipmates:command-preamble -->

Produce docs that get a reader to *done*, gated on the one check that catches bad docs: a **fresh agent
with no prior context follows the instructions against the real repo and must reach the stated result**.
Docs that drift from the code or can't be completed are rejected and fixed. The gate is "a newcomer can
actually follow this," not "it reads well."

Input: what to document — a module, a feature, a public API/CLI, the README/getting-started,
a migration guide, or the whole repo — and for whom (see Parameters).

---

## Config

- `WRITER` = `technical-writer`. `READER` = a **fresh** general-purpose agent (or `technical-writer`) agent that
  has NOT seen the drafting — it only gets the doc + the repo, like a real newcomer.
- `EXECUTION` = `fanout` — how independent doc surfaces are drafted. `fanout` (default): when the
  scope covers several file-disjoint docs, spawn Writers concurrently up to `MAX_CONCURRENT_WORKERS`.
  Guidance `sequential` sets `EXECUTION=sequential` to draft them one at a time.
- `MAX_CONCURRENT_WORKERS` = `5` — concurrency cap in `fanout` mode.
- `MAX_ROUNDS` = `3` — the fresh-reader fix loop cap (Stage 3). `MODE` = `pr` (default) — a worktree,
  a branch and a CI-gated PR, reusing `/shipmates-issue`'s isolate stage and its commit-push-PR stage;
  your checkout is left exactly as you left it. `edit-in-place` writes the docs straight into the
  working tree — still available, but ask for it.
- Under `MODE=pr`: `BASE_BRANCH` = the repo's default branch — the PR's target, not what the
  worktree is cut from (that's current `HEAD`, so the draft has your latest work in it).
  `WORKTREE_LAYOUT` = `nested` (default) — `<repo>/.shipmates/worktrees/`; runtime guidance
  **`worktree-root=sibling`** selects legacy `../<repo>--…` paths. `WORKTREE_DIR` — **nested:**
  `<repo>/.shipmates/worktrees/docs-<slug>`; **sibling:** `../<repo>--docs-<slug>`. Re-runs reuse the
  same path. `BRANCH` = `docs/<slug>`.
  Default worktree cut is from local **`HEAD`**. Runtime guidance **`sync-base`** fetches and cuts
  from `origin/<BASE_BRANCH>` when remote-latest is required.
  `MERGE_MODE` = `manual` (stop at a reviewed PR; `auto` opt-in). `MAX_FIX_ROUNDS` = `2` — a
  separate cap, on CI-fix rounds at Stage 4, so a permanently-red check escalates to the user
  instead of looping. The orchestrator owns all git/gh; agents never push. If there is no remote for
  `gh` to open a PR against, stop at the branch and say so — never silently downgrade to writing in
  the tree.
- **Audience, voice & format** = the repo's own (README tone, docs framework, terminology). Match it; don't
  invent a competing style.

## Stage 0 — Scope: audience + doc type

Decide **who reads this and what they're trying to do**, then the right **Diátaxis** type — tutorial
(first success), how-to (accomplish one task), reference (accurate/exhaustive), or explanation (the why).
Don't blend them. State the concrete "reader can now do X" success condition the fresh-reader test will check.

## Stage 0.5 — Isolate  (`MODE=pr` only — orchestrator, deterministic, no agent)

The worktree exists before anything writes. First check `git -C <repo> status --porcelain`; if the
caller's tree is dirty, **stop and say so** — a worktree cut from `HEAD` holds committed work only,
so a draft written against it would silently miss whatever the caller hasn't committed yet; tell them
to commit or stash first. Otherwise, exactly as `/shipmates-issue`'s isolate stage, but cut from current
`HEAD` rather than `origin/<BASE_BRANCH>`, so it contains the work being documented. Resolve
`<WORKTREE_DIR>`, gitignore `.shipmates/worktrees/` when nested (once, idempotently), then:

```bash
mkdir -p "$(dirname "<WORKTREE_DIR>")"
git -C <repo> worktree add <WORKTREE_DIR> -b <BRANCH> HEAD
```

Drafting, the fresh-reader run and every fix round happen inside `<WORKTREE_DIR>`. Under
`MODE=edit-in-place`, skip this stage and work in the repo as it stands.

## Stage 1 — Draft from the actual code  (agent: `technical-writer`)

Under `EXECUTION=fanout` (default), when the scope covers several file-disjoint docs, spawn one
`technical-writer` per doc concurrently in a single message (up to `MAX_CONCURRENT_WORKERS`), each with
a machine-checkable **owned-paths manifest** (diffed against `git status` and `git diff --name-only` upon
return to prevent collision); under `EXECUTION=sequential`, draft them one at a time. Each writer works
**from the real source** — verify citations (actual signatures, flags, paths, config, and outputs) with
`grep` or inspection first; every command, parameter,
and result must match what the repo does *today*. Minimal (least that gets to done), scannable, consistent
terminology, runnable examples, prerequisites stated up front. Writes in the repo's format.

## Stage 2 — Fresh-reader test  ⛔ HARD GATE  (agent: fresh `READER`)

Spawn a **fresh** agent that gets ONLY the doc and the repo (not the writer's context). It plays the
newcomer: follows the steps **literally**, runs the commands/examples as written, and reports where it
got stuck, what didn't match reality, what was assumed-but-unstated, and whether it reached the Stage 0
success condition. This is the real gate — a doc the reader can't complete has failed, however polished.

## Stage 3 — Fix drift & loop

Feed the reader's failures back to the `technical-writer`; fix the drift, the broken example, the missing
prerequisite, the undefined jargon. **Re-run the fresh-reader test** (Stage 2) on the revised doc. Loop up
to `MAX_ROUNDS`. If it still can't be completed after that, **STOP** and hand the user the doc plus the
reader's outstanding blockers — don't ship docs that don't work.

## Stage 4 — Deliver

`pr` (the default): commit on the worktree branch — staging only the paths this run produced, never
`git add -A` — push, and open the PR against `BASE_BRANCH`.

<!-- shipmates:why-merge-pr -->

Then gate on CI: poll `gh pr checks` until nothing is pending; a red check means pulling the failing
log, fixing it, re-pushing, and re-polling — bounded by `MAX_FIX_ROUNDS`, after which you stop and
escalate to the user with the failing log rather than looping. Never advance a red PR. Stop there
unless `MERGE_MODE=auto`, in which case merge the PR and remove `<WORKTREE_DIR>`; the manual default
leaves the worktree in place with the PR open. `edit-in-place`: leave the finished docs in the working
tree and report. Report: what was documented, the doc type, the fresh-reader's final result (in its
words), rounds taken, and any follow-ups (things worth documenting next, discovered gaps).

---

### Guardrails
- **The fresh-reader test is the gate** — accuracy is proven by someone completing the task, not by the
  writer asserting it's right. The reader must be a genuinely fresh agent, not the writer re-reading.
- **Zero drift:** every command/flag/path/output matches the current code — verified against source, not memory.
- Right doc for the reader's job (Diátaxis) — don't turn a how-to into a history lesson.
- Minimal and honest: cut filler; call out breaking changes and their upgrade path in changelogs/migration guides.
- Bounded loop; escalate with the reader's blockers rather than shipping docs that don't work.
- **Docs are source, so they get a branch.** A doc rewrite lands as a diff a human can read, not as
  a surprise in someone's checkout. `MODE=edit-in-place` is an explicit request, never an assumption.
- If a role doesn't resolve to a shipped crew role, fall back to a general-purpose agent with the brief
  inlined and note it.

## Parameters

| Name | Required | Token | Values | Default | Help |
|------|----------|-------|--------|---------|------|
| target_and_audience | yes | free text | text | — | What to document and for whom — module, feature, public API/CLI, README, migration guide, or whole repo, plus audience. |
| sequential | no | `sequential` | sequential  /  (absent→fanout) | fanout | Draft independent doc surfaces one at a time instead of fan-out. |
| mode | no | `edit-in-place` / `MODE=edit-in-place` | pr  /  edit-in-place | pr | `pr` drafts in a worktree and opens a PR; `edit-in-place` writes into the working tree. |
| worktree_root | no | `worktree-root=sibling` | nested  /  sibling | nested | Under `MODE=pr`, use legacy sibling worktree paths. |
| sync_base | no | `sync-base` | on  /  off | off | Under `MODE=pr`, cut the worktree from `origin/<BASE_BRANCH>` instead of local `HEAD`. |
| merge_mode | no | `MERGE_MODE=auto` / `auto` | manual  /  auto | manual | Under `MODE=pr`, merge the PR when CI is green. |

## Runtime input

Read `## Parameters` first. `$ARGUMENTS` is the captain-supplied or post-intake token string; parse Tokens/Values from that table (required then optionals). If intake ran, treat the restated invocation as authoritative for this run.

Treat the free-text remainder as the documentation target and audience hint; strip guidance tokens (`sequential`, `edit-in-place`, `worktree-root=sibling`, `sync-base`) when present.
