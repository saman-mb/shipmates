---
name: ship-issue
description: Take one or more GitHub issues/stories from open → reviewed PR (→ merged, opt-in) autonomously — worktree, subagent build, CI gate, specialist acceptance board, follow-up issues.
argument-hint: <issue-number>... [optional extra guidance]
allowed-tools: Bash, Read, Write, Edit, Agent, Grep, Glob, WebSearch, WebFetch
disable-model-invocation: true
arguments: issue, guidance
loop_max: 3
stages: [{"order":1,"stage":"plan","roles":["product-manager"],"gate":"plan-ready","max_loops":1},{"order":2,"stage":"isolate","roles":["senior-engineer"],"gate":"isolated-worktree","max_loops":1},{"order":3,"stage":"build","roles":["senior-engineer"],"gate":"implementation-complete","max_loops":3},{"order":4,"stage":"verify","roles":["sdet"],"gate":"tests-green","max_loops":3},{"order":5,"stage":"review","roles":["product-manager"],"gate":"board-accepted","max_loops":3},{"order":6,"stage":"deliver","roles":["senior-engineer"],"gate":"pr-ready","max_loops":1}]
invocation: @{{role}}({{issue}})
board: native
---
# /ship-issue — autonomous ticket delivery

Take **one or more issues / stories (`#<issue>`..)** from open all the way to a **reviewed, CI-green
pull request** on the base branch, autonomously — using an isolated git worktree and a board of
specialist subagents. Merging is gated by `MERGE_MODE` (see Config): by default the run stops with
the PR open for a human to merge; set `MERGE_MODE=auto` for fully hands-off delivery.

Input (**{{issue}}**): whitespace-delimited tokens. The issue / story numbers (`<issue>` below) are
the **leading run** of numeric tokens; the first non-numeric token begins the extra guidance, which
runs to the end. So `104` is one issue with no guidance, `104 105` is two, and `104 focus on retries`
is issue 104 with the guidance `focus on retries`. Never extend the run past the first non-numeric
token — a digit later in the guidance is guidance, not an issue. When the guidance itself starts with
a number, separate it explicitly with `--`, which ends the issue list wherever it appears:
`104 -- 2 fix rounds max`.

## Bundling — the token-efficient default

Most of a run's token cost is **fixed overhead paid once per invocation**: the Planner pass, the
acceptance board (two core reviewers + any gated specialists), the CI poll loop, and worktree setup.
That overhead barely grows with diff size, so shipping several **small, cohesive** issues in one run
is far cheaper than one run each — the board reads one combined diff instead of re-paying the whole
board N times. **So bundling cohesive issues is the recommended default** (`BUNDLE=recommend`, see
Config): given a single small ticket, Stage 0 looks for cohesive siblings and proposes a bundle before
building. Bundling is right only when the issues are *cohesive and cheap* — it is wrong when combining
them would muddy the review or let one failure sink the rest, so the Stage 0 **cohesion test** is the
gate. Never bundle merely to save tokens; bundle when the tickets genuinely belong in one PR.

---

## Config (defaults — override only if the repo clearly needs it)

- `BASE_BRANCH` = the repo's default branch (`gh repo view --json defaultBranchRef -q .defaultBranchRef.name`).
- `MERGE_STRATEGY` = `--squash --delete-branch`
- `MERGE_MODE` = `manual` — `manual`: stop after the acceptance board with a green, reviewed PR
  open for a human to merge. `auto`: squash-merge automatically once every gate passes. Start with
  `manual`; opt into `auto` only in a repo where unattended merges to the base branch are acceptable.
  If Stage 0 set `IS_SECURITY_SENSITIVE`, `MERGE_MODE` is forced to `manual` for this run regardless
  of the configured default — a security-sensitive change must not auto-merge past the `/harden`
  recommendation.
- `MAX_FIX_ROUNDS` = `3`  (acceptance→fix→re-acceptance loops before escalating to the user)
- `BUNDLE` = `recommend` — the token-efficient default (see **Bundling** above). `recommend`: when the
  leading issue is small/low-risk, Stage 0 scans for cohesive sibling issues and **proposes** a bundle
  before building, letting the user choose. `auto`: bundle cohesive siblings without asking — for
  unattended / non-interactive runs only. `off`: ship exactly the issues passed, never suggest more. A
  multi-issue invocation is already an explicit bundle, so it skips the recommendation (but still gets
  the cohesion warning).
- `WORKTREE_DIR` = a sibling of the repo root: `../<repo>--issue-<first-issue>` (single issue)
  or `../<repo>--bundle-<first-issue>-<short-slug>` (multiple issues)
- `BRANCH` = `feat/issue-<first-issue>-<short-slug>` (single) or `feat/bundle-<first-issue>-<short-slug>` (multiple)
- **Quality bar** = whatever the repo's `README` / `AGENTS.md` / contributing docs state. Read it at
  the start and pass it to every reviewer — the `product-manager` (and the visual specialists)
  enforce THAT bar, not just "it runs."

Required commit trailers (append to every commit and the PR body — read them from the harness /
session context; do not invent them). At minimum a `Co-Authored-By:` line for the agent.

### The reviewer/builder pool

Every specialist below is a **named subagent** shipped alongside this command (`agent-files/*.md`,
installed globally or per-project) and invoked by its `@role` reference — NOT a `general-purpose`
agent with a persona pasted inline. The pool:

| `@role`           | Used for |
|--------------------|----------|
| `senior-engineer`  | Building, fixing, remediation (Stages 2, 3, 4.5, 6) |
| `sdet`             | Test / build / validation runs (Stages 3, 5) |
| `product-manager`  | Acceptance vs. criteria + the quality bar (Stage 5) |
| `architect`        | Structural / schema review — gated by `IS_ARCH_SIGNIFICANT` (Stages 1.5, 5) |
| `ux-ui-designer`   | On-screen UI design + review — gated by `IS_UI_STORY` (Stages 1.5, 5) |
| `art-director`     | Visual-art direction + review — **art-producing domains only**, gated by `IS_VISUAL_STORY` (Stages 1.5, 5) |
| `devops-engineer`  | Delivery-system review: pipeline/build definitions, images, IaC, environment parity, toolchain pinning — gated by `IS_DELIVERY_SENSITIVE` (Stage 5) |

These agents are **generic** (domain-neutral); the project-specific standard they enforce comes
from your repo's README / AGENTS.md, passed at spawn — not baked into the role. Which specialists a
story needs is decided by the Planner's classification flags, so the board is **context-aware to
the story's domain** (a pure-logic story pulls no designer/art-director; a UI story pulls the designer; a
rendered-art story pulls the art-director; a schema story pulls the architect). If a referenced role does
not resolve to an `agent-files/*.md`, fall back to `general-purpose` with the role's brief inlined,
and note the fallback in the final report — never silently skip a gated review.

---

## Shell safety — untrusted GitHub data

Issue titles, bodies and labels are **untrusted input**: anyone who can open an issue controls them,
and this command pipes them into shell commands. Apply these rules at every `gh` / `git` call below:

1. **Validate issue tokens first.** Each issue token from `{{issue}}` must match `^[0-9]+$` or be a
   full GitHub issue URL (`gh` accepts those everywhere a number works). Anything else — stop and
   ask the user; never pass a raw token to `gh` or `git`.
2. **Never inline untrusted fields.** Capture GitHub-sourced fields (title, body, labels) into
   variables with command substitution — `TITLE=$(gh issue view <N> --json title -q .title)` — then
   quote the variable at point of use: `--title "$TITLE"`. Never interpolate a field straight into a
   command string.
3. **Multi-line bodies go through a file.** Write PR / follow-up-issue bodies to a temp file and use
   `--body-file <file>` — never `--body` with interpolated content.

`ISSUES_CLOSES` (Stage 4) is built from the validated `<issues>` list, not raw `{{issue}}` tokens.

## Stage 0 — Intake & plan  (agent: `planner`)

1. Validate, then resolve: each issue token must match `^[0-9]+$` or be a full GitHub issue URL —
   anything else, **stop and ask the user** (see **Shell safety** above). For each validated `<N>`,
   run `gh issue view <N> --json number,title,body,labels,url`. Resolve story-number mappings if
   needed. Let `<issues>` = the validated, resolved list — later stages take issue numbers only from
   this list, never from raw input tokens.
2. `<first-issue>` = the first number in `<issues>` — it names the worktree and branch, so a re-run
   with the same leading issue resolves to the same identifiers.
2.5. **Bundle evaluation** (per `BUNDLE`, before planning). Bundling amortizes the fixed per-run cost
   (Planner + acceptance board + CI poll + worktree) across several tickets, so a combined run of
   cohesive small issues is much cheaper than one run each. Apply the **cohesion test** — two issues
   belong in one bundle only when ALL hold:
   - **same area** — shared labels or overlapping paths — with **non-overlapping file ownership** (so
     builders still parallelize without collisions);
   - **each small and low-risk** — never fold an `IS_ARCH_SIGNIFICANT` or `IS_SECURITY_SENSITIVE`
     change in with unrelated work; it needs its own review and its own merge story;
   - **independent** — neither needs the other merged first;
   - **still one reviewable PR** — the combined diff reads cleanly as a single change (cap a bundle at
     ~4 issues / a diff a human would still review in one sitting).

   Then act by `BUNDLE`:
   - **multi-issue input** — already an explicit bundle: run the cohesion test and, if it fails,
     **warn** (don't block), naming what makes them a poor bundle, then proceed as asked.
   - **single issue, `BUNDLE=recommend`** (default) — if the issue is small/low-risk, run ONE cheap
     `gh issue list --state open --label <its-labels> --json number,title,labels` for cohesive
     candidates. If any pass the test, **recommend a bundle to the user** — list the candidates and the
     token rationale — and let them choose which (if any) to fold in. In a non-interactive run, proceed
     solo. Never widen the run without the user's ok on this path.
   - **`BUNDLE=auto`** — fold in the passing candidates without asking (unattended runs only).
   - **`BUNDLE=off`** — ship exactly the issues passed.

   Add any accepted issues to `<issues>` (re-sort so `<first-issue>` is unchanged). Whatever the
   bundle, Stage 4 already repeats `Closes #<N>` for every issue, so if the board later rejects one, it
   can be dropped from the bundle — revert its files and omit its `Closes` — rather than sinking the rest.
3. Spawn ONE **Planner** (`@role(planner)`). Give it all issue bodies + repo README +
   AGENTS.md. Ask it to return, as structured data:
   - a **build plan** broken into independent work units with **non-overlapping file ownership**
     (so builders can run in parallel without collisions),
   - **explicit, checkable acceptance criteria** (functional + the quality bar above), including the
     project's **Definition of Done** where it states one (tests, docs/changelog, non-functional bars),
   - a **test/validation plan** (the commands the SDET should run: unit tests, lint, type-check,
     build/compile/import — whatever this repo uses),
   - a list of files expected to change,
   - **domain classification flags** that decide which specialists the board pulls (set each
     independently — a story can trip more than one):
     - `IS_UI_STORY = yes/no` — does it create/modify on-screen UI (screens, HUD, panels, overlays,
       menus, components, styling)? Gates `ux-ui-designer`.
     - `IS_VISUAL_STORY = yes/no` — gate the `art-director` on the PROJECT'S DOMAIN, not merely on whether
       a story touches pixels. Set it only when the project's actual deliverable is rendered visual
       *art* — a game's world/sprites/shaders, illustration/brand/motion assets, generative imagery —
       and this story touches it. A conventional app (finance, media, SaaS, dev-tooling) whose only
       visual surface is its interface is a **UI** story (`ux-ui-designer`), NOT an art story: the
       art-director reviews pictures judged *as art*, not application chrome. Most projects never set this —
       when in doubt, prefer `IS_UI_STORY` and leave the art-director out.
     - `IS_ARCH_SIGNIFICANT = yes/no` — does it add a new subsystem, change a persisted data/schema
       format, or cross-cut many modules in a way a narrow code review would miss? Gates `architect`.
     - `IS_SECURITY_SENSITIVE = yes/no` — does it touch authn/authz, untrusted input, secrets, crypto,
       file/network/OS access, or dependencies? Does **not** gate a reviewer seat — security review
       lives in `/harden`, which this command doesn't run. It gates two things instead: the final
       report must carry the `/harden` recommendation (mechanical, not a judgment call made while
       writing the summary), and it forces `MERGE_MODE=manual` for this run (see Config).
     - `IS_DELIVERY_SENSITIVE = yes/no` — does it change how the project is built, packaged, configured
       or shipped (pipeline/CI definitions, build scripts, image or environment definitions,
       infrastructure-as-code, dependency or toolchain pins)? Gates `devops-engineer`.
   This flag vocabulary is shared with `/pr-review`, which classifies a PR diff the same way — a new flag
   must be added to both files. `IS_SECURITY_SENSITIVE` is the deliberate exception: here it gates the
   `/harden` recommendation and `MERGE_MODE` above, never a reviewer seat, because this command owns
   the branch and can just run `/harden` itself. `/pr-review` keeps the same flag wired to a
   `security-engineer` seat, because it reviews a PR the crew didn't author, where `/harden` isn't
   available — you don't own that branch.
4. If the plan reveals any issue is too big/ambiguous to finish autonomously, stop and tell the user
   what's blocking — otherwise continue.

## Stage 1.5 — Design specs  (specialist agents — each only if its flag is set)

BEFORE any Builder runs, spawn whichever apply, **in parallel** (a story can need more than one).
Each writes NO code — it returns a spec document, fed verbatim to every Stage 2 Builder whose files
its work governs. Skip this stage entirely when none of the flags are set.

- **`ux-ui-designer`** (only if `IS_UI_STORY`) — a UI design spec: wireframes, a shared-token/theme
  plan, responsive container layout (no hardcoded positions), focus order + interaction states, and
  accessibility/contrast, consistent with the project's design system.
- **`art-director`** (only if `IS_VISUAL_STORY`) — a concrete, numeric art direction (palette values,
  dimensions, light angle/ratio — whatever the medium needs) held to the project's visual bar, so a
  Builder implements what's meant, not a guess.
- **`architect`** (only if `IS_ARCH_SIGNIFICANT`) — the structural approach: module boundaries and
  ownership, and the schema/versioning/migration strategy if persisted data changes.

## Stage 1 — Isolate  (orchestrator, deterministic — no agent)

```bash
git -C <repo> fetch origin
git -C <repo> worktree add <WORKTREE_DIR> -b <BRANCH> origin/<BASE_BRANCH>
```
All build/fix work happens **inside `<WORKTREE_DIR>`** so the base branch and the user's checkout
stay clean. Pass the absolute worktree path to every agent.

## Stage 2 — Build  (agents: `senior-engineer` × N, parallel)

- Spawn one **Builder** (`@role(senior-engineer)`) per independent work unit from the plan,
  **in a single message** so they run concurrently. Each Builder is told: its exact file ownership,
  the acceptance criteria it must satisfy, the worktree path, any Stage 1.5 spec that governs its
  files, and to match existing code style/idioms.
- Builders write code only — they do **not** commit, push, or open PRs (the orchestrator owns git).
- After they report done, **verify the files on disk yourself** (Read/Grep). Never trust a "done"
  report blindly.

## Stage 3 — Self-check before PR  (agent: `sdet`)

- Spawn the **SDET** (`@role(sdet)`) to run the test/validation plan against the worktree:
  unit tests, linters, type-checks, and — if the toolchain exists — a real build/compile step
  (whatever this repo uses: e.g. `npm test && npm run build`, `cargo test`, `pytest -q`,
  `go build ./...`, `make check`). If the toolchain is absent, it does a rigorous **static** pass
  and says so explicitly in the PR.
- If self-check fails, loop a **Fixer** (`@role(senior-engineer)`) until green (counts
  toward `MAX_FIX_ROUNDS`). Only open a PR once self-check passes — never open a known-red PR.

## Stage 4 — Commit, push, open PR  (orchestrator)

```bash
git -C <WORKTREE_DIR> add -A
# <summary> derives from the issue title — untrusted. Capture once into a variable, quote at point
# of use; never inline it.
TITLE="<type>: <summary> (#<first-issue>)"
git -C <WORKTREE_DIR> commit -m "$TITLE"   # + required trailers
git -C <WORKTREE_DIR> push -u origin <BRANCH>
# ISSUES_CLOSES comes from the validated <issues> list, not raw {{issue}}.
# separator substitution first: "Closes #" contains a space, so prefixing first would rewrite it too
ISSUES_CLOSES=$(echo "<issues>" | sed 's/ / · Closes #/g; s/^/Closes #/')
# the body carries untrusted issue text — write it to a temp file, never pass it inline
BODY_FILE=$(mktemp)
# ... write summary, acceptance checklist, validation, ${ISSUES_CLOSES}, trailers to "$BODY_FILE" ...
gh pr create --base <BASE_BRANCH> --head <BRANCH> \
  --title "$TITLE" \
  --body-file "$BODY_FILE"
```
The PR body must include: summary, the acceptance criteria as a checklist, how it was validated
(and any validation that could NOT be run locally), and a `Closes #<issue>` keyword repeated for
every issue in `<issues>` — GitHub only auto-closes the ones it's told individually, so a single
comma-separated `Closes #1, #2, #3` silently leaves all but the first open.

## Stage 4.5 — CI gate: wait for green, fix if red  (orchestrator + Fixer)  ⛔ HARD GATE

**When there is no local toolchain to fully validate, CI is the ONLY real runtime gate — the Stage 3
SDET pass is static and WILL miss things (parse errors, lint-as-error, dependency/version drift). You
MUST confirm CI is green on the pushed head before the acceptance board runs. Never assume a push is
green.** (If the repo has no CI, say so and treat the SDET pass as the gate, explicitly noting the
reduced assurance.)

1. **Wait for the checks to finish** on the PR head (poll, don't guess):
   ```bash
   until s=$(gh pr checks <PR#> 2>&1 | head -1); st=$(echo "$s" | cut -f2); \
     [ "$st" != "pending" ]; do sleep 15; done; echo "$s"
   ```
   (Long-running: launch as a background command / until-loop so you're notified on completion — do
   not chain foreground sleeps.)
2. **If any check FAILS**, pull the actual failure log — do not speculate:
   ```bash
   gh run view <run-id> --log-failed | grep -iE "FAIL|error|Parse|::error" | head -60
   ```
   Diagnose the real cause from the log. Before pushing a fix, **grep the whole changeset for sibling
   instances of the same failure class** so you fix them all in one round instead of burning several.
3. **Dispatch a Fixer** (`@role(senior-engineer)`; or fix directly if it's a trivial,
   unambiguous one-liner you've root-caused from the log) in the worktree, commit with the required
   trailers, push to the same branch. This counts toward `MAX_FIX_ROUNDS`.
4. **Re-poll CI** (back to step 1) on the new head. Repeat until green or `MAX_FIX_ROUNDS` is
   exhausted — if still red after that, **stop and escalate to the user** with the failure log; do
   not proceed to acceptance on a red PR.
5. Only once **CI is green** do you proceed to Stage 5. Carry the confirmed-green run link into the
   final report.

## Stage 5 — Acceptance board  (specialist agents, reviewing the PUSHED PR head)

Spawn these **in parallel** against the PR head commit (they review exactly what will merge). The two
core reviewers always run; each specialist runs only when its flag is set:

- **`product-manager`** (always): checks every acceptance criterion AND the quality bar (from the
  repo's README/AGENTS.md). Returns `ACCEPT` / `ACCEPT-WITH-NITS` / `REJECT` with specifics per criterion.
- **`sdet`** (always): re-runs the validation plan against the PR branch; returns `PASS` / `FAIL`
  with a severity-tagged defect list.
- **`ux-ui-designer`** (only if `IS_UI_STORY`): reviews the pushed head against the Stage 1.5 spec and
  the UI bar — shared-token/theme usage (not per-component override sprawl), responsive layout, focus
  navigation, interaction states, contrast/readability. If it cannot actually render the UI, it
  reviews statically and MUST flag the PR **"needs a human visual pass"** — carry that into the report.
- **`art-director`** (only if `IS_VISUAL_STORY`): renders the actual change via the project's render/preview
  harness (if one exists) and reviews the produced output against the Stage 1.5 direction and the
  visual bar — the render, not the source. If it cannot render, it says so and flags **"needs a human
  visual pass"**.
- **`architect`** (only if `IS_ARCH_SIGNIFICANT`): reviews structural fit, coupling/blast radius, and
  schema/migration safety.
- **`devops-engineer`** (only if `IS_DELIVERY_SENSITIVE`): reviews the delivery definitions for
  reproducibility (same commit → same artifact), toolchain/base pinning, environment parity, config and
  secret plumbing, and whether the pipeline actually gates. Defers rollout/rollback and migration safety
  to `site-reliability-engineer`.

Decision (each specialist participates only when its flag is set):
- **All spawned reviewers ACCEPT/PASS (nits allowed)** → go to Stage 7.
- **Any REJECT / FAIL** → Stage 6.

## Stage 6 — Remediation loop  (agent: `senior-engineer` as Fixer)

- Spawn a **Fixer** (`@role(senior-engineer)`) to address every blocking item (REJECT reasons
  from any Stage 5 reviewer + FAIL defects). Commit + push to the same branch. Then **re-run Stage 5**
  on the new head.
- Repeat up to `MAX_FIX_ROUNDS`. If still not green after that, **stop and escalate to the user** with
  the outstanding blockers — do not merge a failing PR.

## Stage 7 — Follow-up issues  (orchestrator)

- Take every **non-blocking nit** (from any "WITH-NITS" verdict and low-severity SDET findings) and
  file each as its own GitHub issue: capture title and body into quoted variables / a body file per
  **Shell safety** above (titles may quote text from the source issues — untrusted), then
  `gh issue create --title "$ISSUE_TITLE" --body-file "$ISSUE_BODY_FILE"` with a `priority:low` /
  `tech-debt` label (create the label if missing), a clear title, context, and a link back to this PR.
- Do NOT let nits block delivery; they become tracked follow-ups.

## Stage 8 — Deliver  (orchestrator)

- **If `MERGE_MODE=manual`** (default): stop here. Post a completion comment on the PR (what shipped,
  how validated, the green CI link, follow-ups filed) and hand the user the PR link to merge. Leave
  the worktree in place, or remove it and keep the branch — your choice, state which. Nothing closes
  the issues on this path: the repeated `Closes` keywords in the PR body do that when a human merges,
  so name every issue the PR will close in the completion comment.
- **If `MERGE_MODE=auto`**: `gh pr merge <BRANCH> --squash --delete-branch`, then confirm all issues
  auto-closed (for each issue in `<issues>`: `gh issue close <N>` if not already closed), tick the
  epic checklist box if any, remove the worktree (`git -C <repo> worktree remove <WORKTREE_DIR>`),
  and post the completion comment.

## Final report to the user

One concise summary: PR link (and merge state), commit(s), which specialists reviewed it and their
verdicts, number of fix rounds, follow-up issues filed (with links), the confirmed-green CI link,
anything that could only be validated statically, and — when `IS_SECURITY_SENSITIVE` was set at
Stage 0 — the `/harden` recommendation, carried here mechanically rather than decided now.

---

### Guardrails
- The orchestrator owns **all** git/gh actions; agents never push or merge.
- Reviewers always evaluate the **pushed PR head**, so "accepted" == "what merges".
- Never open or advance a red PR. Never skip the SDET run. Never silently drop a nit — file it.
- **Never assume a push is green.** After every push (Stage 4 and every Stage 6 fix), run the Stage
  4.5 CI gate: poll `gh pr checks` until done, and if red pull `gh run view --log-failed`, fix,
  re-push, re-poll. The static SDET pass does NOT substitute for a confirmed-green CI run.
- Keep the base branch and the user's working tree untouched throughout (all work in the worktree).
- Respect `MERGE_MODE` — do not auto-merge unless it is explicitly set to `auto`.
- If genuinely blocked (ambiguous scope, unsatisfiable hard gate, a missing toolchain the test plan
  requires), stop and surface it — autonomy does not mean forcing a bad merge.
- **Secrets & security hygiene.** Never write secrets, tokens, or credentials into commits, PR/issue
  bodies, or logs — assume the repo is public. The `sdet` flags secret leakage as a defect; a real
  leak is blocking. Deeper security work is not this command's job — see the next bullet.
- **Security review lives in `/harden`, not here.** This command does not threat-model. When
  `IS_SECURITY_SENSITIVE` is set, the final report must carry the `/harden` recommendation and the
  run stays on `MERGE_MODE=manual` (Stage 0, Config) — the flag is the trigger, not a judgment call
  made while writing the summary.
- **Be resumable.** A re-run may find the worktree, branch, or PR already exists — reuse them rather
  than erroring or duplicating work. Every stage should be safe to repeat.
- Static review cannot verify pixels, and no reviewer can verify what it didn't examine. When neither
  the `ux-ui-designer` (UI) nor the `art-director` (visual-art) could actually render and inspect the
  result, surface their **"needs human visual pass"** flag in the final report rather than implying
  the visuals are confirmed. The same holds for any Stage 5 reviewer that names a gap in what it
  covered — carry it into the final report; never let an ACCEPT/PASS silently stand in for ground it
  didn't see. Both visual roles are auto-gated by the Planner's `IS_UI_STORY` / `IS_VISUAL_STORY`
  flags, as `architect` is by `IS_ARCH_SIGNIFICANT`.
