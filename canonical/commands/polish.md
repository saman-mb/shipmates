---
name: polish
description: Iterate a visual / UI / output artifact to a specialist's sign-off — produce → critique → fix, looping until the art-director, ux-ui-designer, or product-manager is genuinely happy (or a round cap).
argument-hint: <what to polish — a screen, an asset, a rendered surface> [reviewer: art-director|ux-ui-designer|product-manager]
allowed-tools: Bash, Read, Write, Edit, Agent, Grep, Glob, WebSearch, WebFetch
disable-model-invocation: true
source: skills/polish/SKILL.md
arguments: target
loop_max: 3
stages: [{"order":1,"stage":"inspect","roles":["ux-ui-designer"],"gate":"render-available","max_loops":1},{"order":2,"stage":"iterate","roles":["senior-engineer"],"gate":"polish-improved","max_loops":3},{"order":3,"stage":"sign-off","roles":["art-director"],"gate":"visual-accepted","max_loops":1}]
invocation: @{{role}}({{target}})
board: native
---
# /polish — iterate to a specialist's sign-off

Take a visual / UI / output artifact and refine it in a **loop**: produce it, have the right
specialist critique what they actually SEE (not the source that made it), apply the concrete fixes,
re-produce, re-review — until that specialist **genuinely signs off** or a round cap is hit. This is
the render → critique → refine loop, formalised.

Input (**{{target}}**): what to polish (a screen/panel, a generated art asset, a rendered game view,
a chart, a piece of output…) and optionally which reviewer. If it's empty, ask what to polish.

---

## Config

- `REVIEWER` — chosen by the artifact's domain (or named in `{{target}}`):
  - rendered visual **art** (game world, sprites, shaders, generative imagery, brand/motion) → `art-director`
  - on-screen application **UI** (screens, HUD, panels, components) → `ux-ui-designer`
  - general **output quality** / does-it-meet-the-goal (copy, a data view, a non-visual deliverable) → `product-manager`

  Infer from the repo's README/AGENTS.md domain; when genuinely ambiguous, ask the user which reviewer.
- `MAX_ROUNDS` = 5 — loop cap before escalating to the user with the current state and the reviewer's
  outstanding notes. Never loop forever; never declare a sign-off the reviewer didn't give.
- `MAX_FIX_ROUNDS` = `2` — bounds the Stage 4 CI-fix loop only; separate from `MAX_ROUNDS`, which
  bounds the Stage 3 critique loop.
- `BUILDER` = `senior-engineer` — applies the reviewer's fixes each round.
- `DESTINATION` = `reused-worktree`, `existing-pr`, or `new-branch` — resolved once by Stage 0, named
  in the report before round 0 runs, and consumed by Stage 4 to pick the push target and whether
  `MERGE_MODE` applies.
- `MODE` = `pr` (default) — run the loop in a worktree on its own branch and hand back a CI-gated
  PR, reusing `/ship-issue`'s CI gate; the isolate and commit-push-PR stages diverge on purpose (see
  Stage 0 and Stage 4) — the caller's checkout is never written to. `edit-in-place` refines the
  working tree directly — still available, but ask for it.
- Under `MODE=pr`: `BASE_BRANCH` = the repo's default branch — the PR's target, not what the
  worktree is cut from (that's current `HEAD`; see Stage 0).
  `WORKTREE_DIR` = `../<repo>--polish-<slug>`. `BRANCH` = `polish/<slug>`.
  `MERGE_MODE` = `manual` (stop at a reviewed PR; `auto` opt-in). The orchestrator owns all git/gh;
  agents never push. If there is no remote for `gh` to open a PR against, stop at the branch and say
  so — never silently downgrade to writing in the tree.
  **The guard:** the real question isn't which branch you're on, it's where the polish should
  land — resolve it by destination, not by standing position. Being inside a linked worktree left
  behind by `/ship-issue` resolves to `DESTINATION` = `reused-worktree`; the caller's own feature
  branch with an open PR resolves to `existing-pr`; a fresh branch is the `new-branch` fallback.
  Don't infer any of it from a branch name — Stage 0 spells out the order:
  ```bash
  # --path-format needs git >= 2.31 — without it, a primary checkout entered from a
  # subdirectory reports an absolute --git-dir vs a relative --git-common-dir and false-positives
  [ "$(git rev-parse --path-format=absolute --git-dir)" != "$(git rev-parse --path-format=absolute --git-common-dir)" ]   # true inside a linked worktree
  ```
  Inside a linked worktree: stay put, do not cut a second one. Otherwise Stage 0 decides between the
  caller's own branch (if it already has an open PR) and a fresh `polish/<slug>` branch.
- The renders are **evidence, not deliverables.** The fixes land in tracked source; the captured
  artifact is often gitignored build output. Never force-add an ignored render to the branch — cite
  its path in the report and the PR body instead.
- **Quality bar** = the project's stated visual/UX bar (README/AGENTS.md), passed to the reviewer.

---

## Stage 0 — Isolate  (`MODE=pr` only — orchestrator, deterministic, no agent)

Resolve `MODE` and the guard above first — decide the destination, don't infer it from where you
happen to be standing:

1. **Already inside a linked worktree** (the guard above). Confirm *that worktree* — not `<repo>`,
   the primary checkout — is clean before touching it: run `git status --porcelain` with no `-C`, so
   it targets the tree you're already standing in. If it's dirty, stop and say so; round 0 must not
   fold someone else's unrelated, uncommitted work into the polish commit. Otherwise stay put and
   reuse it — do not cut a second one. `DESTINATION` = `reused-worktree`.
2. **Otherwise:** first check `git -C <repo> status --porcelain`; if the caller's tree is dirty,
   **stop and say so** — a worktree cut from `HEAD` holds committed work only, so round 0 would
   render and critique a version of the artifact the caller isn't looking at. Otherwise resolve
   whether the current branch already has an open PR:
   ```bash
   git -C <repo> rev-parse --abbrev-ref HEAD                  # the branch you are on
   gh pr list --head <branch> --state open --json number
   ```
   - **A PR exists:** cut a **detached** worktree at `HEAD`, so the caller's checkout is never
     written to, and run the rounds there:
     ```bash
     git -C <repo> worktree add --detach <WORKTREE_DIR> HEAD
     ```
     At Stage 4, push back onto `<branch>` and its existing PR — never open a second one.
     `DESTINATION` = `existing-pr`.
   - **No PR:** cut `<BRANCH>` = `polish/<slug>` from your current `HEAD`, not
     `origin/<BASE_BRANCH>` — precisely so it contains the work you were asked to polish:
     ```bash
     git -C <repo> worktree add <WORKTREE_DIR> -b <BRANCH> HEAD
     ```
     At Stage 4, open a new PR for it. `DESTINATION` = `new-branch`.

Every round — the harness, the renders, the fixes — happens inside `<WORKTREE_DIR>`. Under
`MODE=edit-in-place`, or when the guard keeps you inside an existing worktree, work where you are.
Either way, name `DESTINATION` and why it was chosen in the report **before** the first round runs,
so nobody discovers after five rounds where the edits went.

## Stage 1 — Secure a way for the REVIEWER to SEE the output  (the loop can't converge without it)

The `REVIEWER` must be able to open and inspect the ACTUAL produced artifact itself — not a
description of it relayed by the orchestrator, and not the code. Find the project's way to produce it
headlessly — a render / screenshot / build / export / preview harness (check its tools/scripts/docs).
If none exists and one is feasible, have a `senior-engineer` build a **minimal** one (e.g. a headless
script that writes a PNG, or a preview export) that hands the reviewer a file it can open directly. If
it is genuinely impossible to produce the artifact or to give the reviewer direct access to it, say so
plainly, fall back to a single static review flagged **"needs a human visual pass,"** name exactly what
the reviewer could not see, and tell the user the loop cannot truly converge blind — don't fake
iterations.

Resolve a **capture matrix** before round 0: every section × the chosen breakpoints — a narrow and a
desktop viewport at minimum, 375 / 768 / 1280 a common ladder — plus every distinct page template (N
generated pages sharing one template need one capture; zero is not acceptable). **Settle animation
before capturing** — wait it out or force a stable frame; unsettled motion is the leading cause of a
flaky review (two captures of a 38-frame GIF both landing on frame 0 read as a capture artifact once —
it was a real defect).

## Stage 2 — Baseline

Produce the artifact once (run the harness) and capture it (PNG / output file). Call this round 0 and
keep it, so the final report can show a before → after.

## Stage 3 — The loop  (repeat up to `MAX_ROUNDS`)

Each round:
1. **Critique** — spawn the `REVIEWER` against the CURRENT produced artifact (the actual image/output
   file, which it opens and inspects — not the source). It returns a decisive verdict —
   `ACCEPT` (sign-off) / `ACCEPT-WITH-NITS` / `REJECT` — with SPECIFIC, plug-in fixes (values, not
   "make it nicer"), blockers separated from nits, and a statement of which capture-matrix cells it
   actually reviewed this round — the verdict covers only those. Instruct it explicitly not to
   rubber-stamp to end the loop.
2. **Signed off?** `ACCEPT` → leave the loop. `ACCEPT-WITH-NITS` → leave the loop too (the nits become
   follow-ups) unless the caller asked to resolve nits as well. `REJECT` → continue.
3. **Fix** — spawn a `senior-engineer` with the reviewer's exact blocker list; apply the changes where
   Stage 0 put you — the worktree branch under `MODE=pr`, the working tree under `MODE=edit-in-place`.
   Keep the change scoped to the notes — no unrelated drift.
4. **Re-produce** — re-run the harness and capture the new artifact.
5. Keep a one-line changelog per round, naming that round's capture-matrix coverage, so the trajectory
   and the coverage are both visible.

If `MAX_ROUNDS` is reached without a sign-off, **STOP** and hand the user the current artifact plus the
reviewer's remaining notes. Escalate; don't spin.

## Stage 4 — Report

Show the user the final artifact (path / screenshot), the reviewer's verdict in its own words, the
number of rounds, and a short before → after of what changed. Optionally file any allowed nits as
follow-up issues. Under `MODE=pr`, commit the rounds — staging only the paths the rounds actually
touched, never `git add -A`, since the tree may hold unrelated uncommitted work — then push per
`DESTINATION`:
- `reused-worktree` / `existing-pr`: push onto that branch and add a comment on its existing PR
  with the before → after renders cited by path. Never run `gh pr create` here — that would open a
  second PR against work that already has one.
  ```bash
  git -C <WORKTREE_DIR> push origin HEAD:<branch>
  ```
  `MERGE_MODE=auto` does not apply here — the PR belongs to the caller's feature work, not to this
  run. Push, comment, and stop regardless; say so in the report. Leave `<WORKTREE_DIR>` in place
  either way — it isn't polish's PR to merge or clean up.
- `new-branch`: push `<BRANCH>` and open a new PR with the same renders cited by path.

Either way, then run the CI gate: poll `gh pr checks` until nothing is pending; a red check means
pulling the failing log, fixing it, re-pushing, and re-polling — bounded by `MAX_FIX_ROUNDS`, after
which you stop and escalate to the user with the failing log rather than looping. Never advance a
red PR. Under `new-branch`, stop there unless `MERGE_MODE=auto`, in which case merge the PR and
remove `<WORKTREE_DIR>`; the manual default leaves the worktree in place with the PR open.

---

### Guardrails
- The reviewer judges the PRODUCED OUTPUT every round — never the source. Code that looks right can
  render wrong; that's the whole point of the loop.
- A reviewer that `ACCEPT`s round 0 with zero changes gets a sanity check — make sure it actually
  inspected the artifact and isn't rubber-stamping.
- Bounded by `MAX_ROUNDS` — escalate rather than loop forever.
- **Be resumable.** A re-run may find the worktree, branch, or PR for this slug already exists —
  reuse them rather than erroring or duplicating work.
- Scope each fix round to the reviewer's notes; the `senior-engineer` doesn't refactor or wander.
- The sign-off is the REVIEWER's to give, and the final report states what the reviewer actually said
  — not an optimistic paraphrase. A "needs a human visual pass" fallback is a real outcome, not a fail.
- Reviewer choice follows the project's domain: `art-director` for art, `ux-ui-designer` for UI,
  `product-manager` for general output. When ambiguous, ask.
- Runs standalone, or as the visual pass inside/after `/ship-issue` on a UI/visual story.
- **The loop runs on its own branch by default.** Five rounds of edits belong in a diff a human can
  read, not in someone's checkout. `MODE=edit-in-place` is an explicit request — except when you're
  already inside an isolated worktree, where staying put *is* the isolation.
- If a role doesn't resolve to an `agent-files/*.md`, fall back to `general-purpose` with the role's
  brief inlined, and note the fallback.
