---
name: polish
description: Iterate a visual / UI / output artifact to a specialist's sign-off — produce → critique → fix, looping until the art-director, ux-ui-designer, or product-manager is genuinely happy (or a round cap).
argument-hint: <what to polish — a screen, an asset, a rendered surface> [reviewer: art-director|ux-ui-designer|product-manager]
allowed-tools: Bash, Read, Write, Edit, Agent, Grep, Glob, WebSearch, WebFetch
disable-model-invocation: true
---

# /polish — iterate to a specialist's sign-off

Take a visual / UI / output artifact and refine it in a **loop**: produce it, have the right
specialist critique what they actually SEE (not the source that made it), apply the concrete fixes,
re-produce, re-review — until that specialist **genuinely signs off** or a round cap is hit. This is
the render → critique → refine loop, formalised.

Input (**$ARGUMENTS**): what to polish (a screen/panel, a generated art asset, a rendered game view,
a chart, a piece of output…) and optionally which reviewer. If it's empty, ask what to polish.

---

## Config

- `REVIEWER` — chosen by the artifact's domain (or named in `$ARGUMENTS`):
  - rendered visual **art** (game world, sprites, shaders, generative imagery, brand/motion) → `art-director`
  - on-screen application **UI** (screens, HUD, panels, components) → `ux-ui-designer`
  - general **output quality** / does-it-meet-the-goal (copy, a data view, a non-visual deliverable) → `product-manager`

  Infer from the repo's README/CLAUDE.md domain; when genuinely ambiguous, ask the user which reviewer.
- `MAX_ROUNDS` = 5 — loop cap before escalating to the user with the current state and the reviewer's
  outstanding notes. Never loop forever; never declare a sign-off the reviewer didn't give.
- `BUILDER` = `senior-engineer` — applies the reviewer's fixes each round.
- `MODE` = `pr` (default) — run the loop in a worktree on its own branch and hand back a CI-gated
  PR, reusing `/ship-issue`'s isolate stage, its commit-push-PR stage, and its CI gate; the caller's
  checkout is never written to. `edit-in-place` refines the working tree directly — still available,
  but ask for it.
- Under `MODE=pr`: `BASE_BRANCH` = the repo's default branch.
  `WORKTREE_DIR` = `../<repo>--polish-<slug>`. `BRANCH` = `polish/<slug>`.
  `MERGE_MODE` = `manual` (stop at a reviewed PR; `auto` opt-in). The orchestrator owns all git/gh;
  agents never push. If there is no remote for `gh` to open a PR against, stop at the branch and say
  so — never silently downgrade to writing in the tree.
  **The guard:** the real question isn't which branch you're on, it's whether you're already inside
  an isolated worktree — typically the one `/ship-issue` just left behind, which is how `/polish` is
  usually chained. Detect it properly, don't infer it from a branch name:
  ```bash
  [ "$(git rev-parse --git-dir)" != "$(git rev-parse --git-common-dir)" ]   # true inside a linked worktree
  ```
  Inside a linked worktree: stay put, do not cut a second one. In the caller's primary checkout: cut
  a fresh worktree per Stage 0, whatever branch they happen to be on.
- The renders are **evidence, not deliverables.** The fixes land in tracked source; the captured
  artifact is often gitignored build output. Never force-add an ignored render to the branch — cite
  its path in the report and the PR body instead.
- **Quality bar** = the project's stated visual/UX bar (README/CLAUDE.md), passed to the reviewer.

---

## Stage 0 — Isolate  (`MODE=pr` only — orchestrator, deterministic, no agent)

Resolve `MODE` and the guard above first.

If the guard finds you already inside a linked worktree, confirm it's clean before touching it —
`git -C <repo> status --porcelain`. If it's dirty, stop and say so; round 0 must not fold someone
else's unrelated, uncommitted work into the polish commit.

Otherwise, cut the worktree from your current `HEAD`, not `origin/<BASE_BRANCH>` — precisely so it
contains the work you were asked to polish, whatever your branch has become:

```bash
git -C <repo> rev-parse --abbrev-ref HEAD          # the branch you are on
git -C <repo> worktree add <WORKTREE_DIR> -b <BRANCH> HEAD
```

Every round — the harness, the renders, the fixes — happens inside `<WORKTREE_DIR>`. Under
`MODE=edit-in-place`, or when the guard keeps you inside an existing worktree, work where you are.
Either way, name the location in the report **before** the first round runs, so nobody discovers
after five rounds where the edits went.

## Stage 1 — Secure a way to SEE the output  (the loop can't converge without it)

The reviewer must judge the ACTUAL produced artifact, not the code. Find the project's way to produce
it headlessly — a render / screenshot / build / export / preview harness (check its tools/scripts/
docs). If none exists and one is feasible, have a `senior-engineer` build a **minimal** one (e.g. a
headless script that writes a PNG, or a preview export). If it is genuinely impossible to produce or
view the artifact, say so plainly, fall back to a single static review flagged **"needs a human visual
pass,"** and tell the user the loop cannot truly converge blind — don't fake iterations.

## Stage 2 — Baseline

Produce the artifact once (run the harness) and capture it (PNG / output file). Call this round 0 and
keep it, so the final report can show a before → after.

## Stage 3 — The loop  (repeat up to `MAX_ROUNDS`)

Each round:
1. **Critique** — spawn the `REVIEWER` against the CURRENT produced artifact (the actual image/output
   file, which it opens and inspects — not the source). It returns a decisive verdict —
   `ACCEPT` (sign-off) / `ACCEPT-WITH-NITS` / `REJECT` — with SPECIFIC, plug-in fixes (values, not
   "make it nicer"), blockers separated from nits. Instruct it explicitly not to rubber-stamp to end
   the loop.
2. **Signed off?** `ACCEPT` → leave the loop. `ACCEPT-WITH-NITS` → leave the loop too (the nits become
   follow-ups) unless the caller asked to resolve nits as well. `REJECT` → continue.
3. **Fix** — spawn a `senior-engineer` with the reviewer's exact blocker list; apply the changes where
   Stage 0 put you — the worktree branch under `MODE=pr`, the working tree under `MODE=edit-in-place`.
   Keep the change scoped to the notes — no unrelated drift.
4. **Re-produce** — re-run the harness and capture the new artifact.
5. Keep a one-line changelog per round so the trajectory is visible.

If `MAX_ROUNDS` is reached without a sign-off, **STOP** and hand the user the current artifact plus the
reviewer's remaining notes. Escalate; don't spin.

## Stage 4 — Report

Show the user the final artifact (path / screenshot), the reviewer's verdict in its own words, the
number of rounds, and a short before → after of what changed. Optionally file any allowed nits as
follow-up issues. Under `MODE=pr`, commit the rounds on the branch — staging only the paths the
rounds actually touched, never `git add -A`, since the tree may hold unrelated uncommitted work —
then run the CI gate: poll `gh pr checks` until nothing is pending; if red, pull the failing log, fix,
re-push, re-poll. If the branch already has an open PR (the usual case when chained after
`/ship-issue`), push to it and add a comment with the before → after renders cited by path rather than
opening a second one; otherwise open a new PR with the same renders cited by path. Stop there unless
`MERGE_MODE=auto`.

---

### Guardrails
- The reviewer judges the PRODUCED OUTPUT every round — never the source. Code that looks right can
  render wrong; that's the whole point of the loop.
- A reviewer that `ACCEPT`s round 0 with zero changes gets a sanity check — make sure it actually
  inspected the artifact and isn't rubber-stamping.
- Bounded by `MAX_ROUNDS` — escalate rather than loop forever.
- Scope each fix round to the reviewer's notes; the `senior-engineer` doesn't refactor or wander.
- The sign-off is the REVIEWER's to give, and the final report states what the reviewer actually said
  — not an optimistic paraphrase. A "needs a human visual pass" fallback is a real outcome, not a fail.
- Reviewer choice follows the project's domain: `art-director` for art, `ux-ui-designer` for UI,
  `product-manager` for general output. When ambiguous, ask.
- Runs standalone, or as the visual pass inside/after `/ship-issue` on a UI/visual story.
- **The loop runs on its own branch by default.** Five rounds of edits belong in a diff a human can
  read, not in someone's checkout. `MODE=edit-in-place` is an explicit request — except when you're
  already inside an isolated worktree, where staying put *is* the isolation.
- If a role doesn't resolve to a `.claude/agents/*.md`, fall back to `general-purpose` with the role's
  brief inlined, and note the fallback.
