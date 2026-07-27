---
name: ship-issue
description: Take a GitHub issue/story from open → reviewed PR (→ merged, opt-in) autonomously — worktree, subagent build, CI gate, specialist acceptance board, follow-up issues.
argument-hint: <issue-or-story-number> [optional extra guidance]
allowed-tools: Bash, Read, Write, Edit, Agent, Grep, Glob, WebSearch, WebFetch
---

# /ship-issue — autonomous ticket delivery

Take **issue / story `#<issue>`** from open all the way to a **reviewed, CI-green pull request** on the
base branch, autonomously — using an isolated git worktree and a board of specialist subagents.
Merging is gated by `MERGE_MODE` (see Config): by default the run stops with the PR open for a
human to merge; set `MERGE_MODE=auto` for fully hands-off delivery.

Input (**$ARGUMENTS**): the first whitespace-delimited token is the issue / story number (`<issue>`
below); everything after it is extra guidance from the caller (may be empty).

---

## Config (defaults — override only if the repo clearly needs it)

- `BASE_BRANCH` = the repo's default branch (`gh repo view --json defaultBranchRef -q .defaultBranchRef.name`).
- `MERGE_STRATEGY` = `--squash --delete-branch`
- `MERGE_MODE` = `manual` — `manual`: stop after the acceptance board with a green, reviewed PR
  open for a human to merge. `auto`: squash-merge automatically once every gate passes. Start with
  `manual`; opt into `auto` only in a repo where unattended merges to the base branch are acceptable.
- `MAX_FIX_ROUNDS` = `3`  (acceptance→fix→re-acceptance loops before escalating to the user)
- `WORKTREE_DIR` = a sibling of the repo root: `../<repo>--issue-<issue>`
- `BRANCH` = `feat/issue-<issue>-<short-slug>`
- **Quality bar** = whatever the repo's `README` / `CLAUDE.md` / contributing docs state. Read it at
  the start and pass it to every reviewer — the `product-manager` (and the visual specialists)
  enforce THAT bar, not just "it runs."

Required commit trailers (append to every commit and the PR body — read them from the harness /
session context; do not invent them). At minimum a `Co-Authored-By:` line for the agent.

### The reviewer/builder pool

Every specialist below is a **named subagent** shipped alongside this command (`.claude/agents/*.md`,
installed globally or per-project) and invoked by its `subagent_type` — NOT a `general-purpose`
agent with a persona pasted inline. The pool:

| `subagent_type`   | Used for |
|--------------------|----------|
| `senior-engineer`  | Building, fixing, remediation (Stages 2, 3, 4.5, 6) |
| `sdet`             | Test / build / validation runs (Stages 3, 5) |
| `product-manager`  | Acceptance vs. criteria + the quality bar (Stage 5) |
| `architect`        | Structural / schema review — gated by `IS_ARCH_SIGNIFICANT` (Stages 1.5, 5) |
| `ux-ui-designer`   | On-screen UI design + review — gated by `IS_UI_STORY` (Stages 1.5, 5) |
| `art-director`     | Visual-art direction + review — **art-producing domains only**, gated by `IS_VISUAL_STORY` (Stages 1.5, 5) |
| `security-engineer`| Threat-model + security review — gated by `IS_SECURITY_SENSITIVE` (Stage 5) |
| `devops-engineer`  | Delivery-system review: pipeline/build definitions, images, IaC, environment parity, toolchain pinning — gated by `IS_DELIVERY_SENSITIVE` (Stage 5) |

These agents are **generic** (domain-neutral); the project-specific standard they enforce comes
from your repo's README / CLAUDE.md, passed at spawn — not baked into the role. Which specialists a
story needs is decided by the Planner's classification flags, so the board is **context-aware to
the story's domain** (a pure-logic story pulls no designer/art-director; a UI story pulls the designer; a
rendered-art story pulls the art-director; a schema story pulls the architect). If a referenced role does
not resolve to a `.claude/agents/*.md`, fall back to `general-purpose` with the role's brief inlined,
and note the fallback in the final report — never silently skip a gated review.

---

## Stage 0 — Intake & plan  (agent: `Plan`)

1. Resolve the number: `gh issue view <issue> --json number,title,body,labels,url`. If `<issue>` is a
   "story number" that differs from the GitHub issue number, map it via the epic checklist issue or
   the README scope table, then confirm the real issue number.
2. Spawn ONE **Planner** (`subagent_type: Plan`). Give it the full issue body + repo README +
   CLAUDE.md. Ask it to return, as structured data:
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
     - `IS_SECURITY_SENSITIVE = yes/no` — does it touch authn/authz, untrusted input, secrets/credentials,
       crypto, file/network/OS access, or dependencies? Gates `security-engineer`.
     - `IS_DELIVERY_SENSITIVE = yes/no` — does it change how the project is built, packaged, configured
       or shipped (pipeline/CI definitions, build scripts, image or environment definitions,
       infrastructure-as-code, dependency or toolchain pins)? Gates `devops-engineer`.
   This flag vocabulary is shared with `/review`, which classifies a PR diff the same way — a new flag
   must be added to both files.
3. If the plan reveals the issue is too big/ambiguous to finish autonomously, stop and tell the user
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

- Spawn one **Builder** (`subagent_type: senior-engineer`) per independent work unit from the plan,
  **in a single message** so they run concurrently. Each Builder is told: its exact file ownership,
  the acceptance criteria it must satisfy, the worktree path, any Stage 1.5 spec that governs its
  files, and to match existing code style/idioms.
- Builders write code only — they do **not** commit, push, or open PRs (the orchestrator owns git).
- After they report done, **verify the files on disk yourself** (Read/Grep). Never trust a "done"
  report blindly.

## Stage 3 — Self-check before PR  (agent: `sdet`)

- Spawn the **SDET** (`subagent_type: sdet`) to run the test/validation plan against the worktree:
  unit tests, linters, type-checks, and — if the toolchain exists — a real build/compile step
  (whatever this repo uses: e.g. `npm test && npm run build`, `cargo test`, `pytest -q`,
  `go build ./...`, `make check`). If the toolchain is absent, it does a rigorous **static** pass
  and says so explicitly in the PR.
- If self-check fails, loop a **Fixer** (`subagent_type: senior-engineer`) until green (counts
  toward `MAX_FIX_ROUNDS`). Only open a PR once self-check passes — never open a known-red PR.

## Stage 4 — Commit, push, open PR  (orchestrator)

```bash
git -C <WORKTREE_DIR> add -A
git -C <WORKTREE_DIR> commit -m "<type>: <summary> (#<issue>)"   # + required trailers
git -C <WORKTREE_DIR> push -u origin <BRANCH>
gh pr create --base <BASE_BRANCH> --head <BRANCH> \
  --title "<type>: <issue title> (#<issue>)" \
  --body  "<what changed, how validated, 'Closes #<issue>', trailers>"
```
The PR body must include: summary, the acceptance criteria as a checklist, how it was validated
(and any validation that could NOT be run locally), and `Closes #<issue>`.

## Stage 4.5 — CI gate: wait for green, fix if red  (orchestrator + Fixer)  ⛔ HARD GATE

**When there is no local toolchain to fully validate, CI is the ONLY real runtime gate — the Stage 3
SDET pass is static and WILL miss things (parse errors, lint-as-error, dependency/version drift). You
MUST confirm CI is green on the pushed head before the acceptance board runs. Never assume a push is
green.** (If the repo has no CI, say so and treat the SDET pass as the gate, explicitly noting the
reduced assurance.)

1. **Wait for the checks to finish** on the PR head (poll, don't guess):
   ```bash
   until s=$(gh pr checks <PR#> 2>&1 | head -1); st=$(echo "$s" | awk '{print $2}'); \
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
3. **Dispatch a Fixer** (`subagent_type: senior-engineer`; or fix directly if it's a trivial,
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
  repo's README/CLAUDE.md). Returns `ACCEPT` / `ACCEPT-WITH-NITS` / `REJECT` with specifics per criterion.
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
- **`security-engineer`** (only if `IS_SECURITY_SENSITIVE`): threat-models the change (STRIDE / OWASP) —
  broken access control, injection, secrets/crypto, vulnerable deps — and returns severity-ranked findings
  with the exploit path; a credible Critical/High is a blocking defect.
- **`devops-engineer`** (only if `IS_DELIVERY_SENSITIVE`): reviews the delivery definitions for
  reproducibility (same commit → same artifact), toolchain/base pinning, environment parity, config and
  secret plumbing, and whether the pipeline actually gates. Defers rollout/rollback and migration safety
  to `site-reliability-engineer`, and the severity of secret exposure or dependency trust to
  `security-engineer`.

Decision (each specialist participates only when its flag is set):
- **All spawned reviewers ACCEPT/PASS (nits allowed)** → go to Stage 7.
- **Any REJECT / FAIL** → Stage 6.

## Stage 6 — Remediation loop  (agent: `senior-engineer` as Fixer)

- Spawn a **Fixer** (`subagent_type: senior-engineer`) to address every blocking item (REJECT reasons
  from any Stage 5 reviewer + FAIL defects). Commit + push to the same branch. Then **re-run Stage 5**
  on the new head.
- Repeat up to `MAX_FIX_ROUNDS`. If still not green after that, **stop and escalate to the user** with
  the outstanding blockers — do not merge a failing PR.

## Stage 7 — Follow-up issues  (orchestrator)

- Take every **non-blocking nit** (from any "WITH-NITS" verdict and low-severity SDET findings) and
  file each as its own GitHub issue: `gh issue create` with a `priority:low` / `tech-debt` label
  (create the label if missing), a clear title, context, and a link back to this PR.
- Do NOT let nits block delivery; they become tracked follow-ups.

## Stage 8 — Deliver  (orchestrator)

- **If `MERGE_MODE=manual`** (default): stop here. Post a completion comment on the PR (what shipped,
  how validated, the green CI link, follow-ups filed) and hand the user the PR link to merge. Leave
  the worktree in place, or remove it and keep the branch — your choice, state which.
- **If `MERGE_MODE=auto`**: `gh pr merge <BRANCH> --squash --delete-branch`, then confirm the issue
  auto-closed (else `gh issue close`), tick the epic checklist box if any, remove the worktree
  (`git -C <repo> worktree remove <WORKTREE_DIR>`), and post the completion comment.

## Final report to the user

One concise summary: PR link (and merge state), commit(s), which specialists reviewed it and their
verdicts, number of fix rounds, follow-up issues filed (with links), the confirmed-green CI link, and
anything that could only be validated statically.

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
  bodies, or logs — assume the repo is public. When the change touches auth, input handling, crypto,
  or dependencies, the `security-engineer` (gated by `IS_SECURITY_SENSITIVE`) threat-models it and the
  `sdet` also flags secret leakage; a real leak or a clear injection path is a blocking defect.
- **Be resumable.** A re-run may find the worktree, branch, or PR already exists — reuse them rather
  than erroring or duplicating work. Every stage should be safe to repeat.
- Static review cannot verify pixels. When neither the `ux-ui-designer` (UI) nor the `art-director`
  (visual-art) could actually render and inspect the result, surface their **"needs human visual
  pass"** flag in the final report rather than implying the visuals are confirmed. Both are auto-gated
  by the Planner's `IS_UI_STORY` / `IS_VISUAL_STORY` flags, as `architect` is by `IS_ARCH_SIGNIFICANT`.
