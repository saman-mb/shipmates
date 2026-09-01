---
name: pr-review
description: Run the specialist acceptance board against an existing pull request the crew didn't author — classify the diff, pull the right reviewers, and return one consolidated verdict. Read-only by default: it reports, it never repairs.
argument-hint: <pr-number or PR url> [optional emphasis passed to every reviewer — e.g. "weight the schema change"]
allowed-tools: Bash, Read, Agent, Grep, Glob, WebSearch, WebFetch
disable-model-invocation: true
---
# /pr-review — classify → convene the board → consolidate
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

Point the crew at a pull request **somebody else wrote** — a teammate's, an outside contributor's, a
dependency bump, or one you opened by hand before you thought to use Shipmates. Every other command
assumes the crew authored the change; this one doesn't, and that single fact sets its shape: **no
worktree, no build, no fix loop.** You don't own the branch, so the deliverable is findings, not
commits.

The PR and optional focus hint come from the Runtime input section at the end of this workflow.

---

## Config (override only if the repo needs it)

- `MODE` = `report` — `report`: return the verdict to the caller only. `post`: also publish it with
  `gh pr review`. Posting onto someone else's PR is a **social side effect and irreversible**, so it
  is opt-in, never the default.
- `RUN_TESTS` = `no` for a PR from a **fork**, `ask` otherwise. See the trust boundary below — this
  command is the only one that executes code the crew did not write.
- **Quality bar** = whatever the target repo's `README` / `CLAUDE.md` / contributing docs state. Read
  it first and pass it to every reviewer — they enforce *that* bar, not a generic one.

## Shell safety — untrusted GitHub data

`<PR#>` is interpolated into every `gh pr` call below, and the PR's title, body, diff and review
comments are untrusted input — anyone who opened the PR controls them. Apply these rules, the same
ones `/ship-issue` applies to its issue tokens:

1. **Validate `<PR#>` first.** It must match `^[0-9]+$` or be a full GitHub PR URL (`gh` accepts
   either everywhere a number works). Anything else — stop and ask the user; never pass a raw token
   to `gh` or `git`.
2. **Never inline untrusted fields.** Capture PR-sourced fields (title, body, diff, comments) into
   variables with command substitution — `TITLE=$(gh pr view <PR#> --json title -q .title)` — then
   quote the variable at point of use. Never interpolate a field straight into a command string.
3. **Every body goes through a file.** Write the review body to a temp file and use
   `--body-file <file>` (see Stage 4). Not "never `--body` with interpolated content" — never
   `--body`, full stop, even quoted and even for text you wrote: nobody reading one line can tell
   which variable holds your text and which holds the PR's, and `tools/validate_skills.py` fails
   the build on any `--body` in a shell fence for exactly that reason. Keep the path itself a
   literal or a plain variable — `--body-file "$(…)"` is the same defect under a safer name.

## Stage 0 — Intake & classify

Pull the change itself, not a description of it:

```bash
gh pr view <PR#> --json number,title,body,author,headRefOid,isCrossRepository,files,url
gh pr diff <PR#>
```

Then read the repo's `README` / `CLAUDE.md` for the bar. Set the **same classification flags
`/ship-issue` uses** — with one deliberate exception, `IS_SECURITY_SENSITIVE` (see below) —
but derive them from the **diff**, not from an issue body. That is the real difference: there are no
stated acceptance criteria here, so the criteria are *the repo's own bar plus what the PR claims to
do*. Where the PR description and the diff disagree, that mismatch is itself a finding.

- `IS_UI_STORY` — does the diff create or change on-screen UI? Gates `ux-ui-designer`.
- `IS_VISUAL_STORY` — is the project's deliverable rendered visual **art**, and does this touch it?
  Almost always `no` for conventional apps — prefer `IS_UI_STORY`. Gates `art-director`.
- `IS_ARCH_SIGNIFICANT` — new subsystem, changed persisted schema, or a cross-cutting change a narrow
  read would miss? Gates `architect`.
- `IS_SECURITY_SENSITIVE` — authn/authz, untrusted input, secrets, crypto, file/network/OS access, or
  dependencies? Gates `security-engineer`.
- `IS_DELIVERY_SENSITIVE` — does the diff change how the project is built, packaged, configured or
  shipped (pipeline/build definitions, image or environment definitions, infrastructure-as-code,
  dependency or toolchain pins)? Gates `devops-engineer`.
- `IS_DOCS_AFFECTING` — does the change touch documented behaviour, flags, commands, config, or public
  API/CLI surface that user- or agent-facing docs describe? Gates `technical-writer`.
- `IS_RELEASE_AFFECTING` — does merging this PR to the repo's release branch require a new published
  version for users to receive the change (commands, tools, crew, install payload, CLI/API surface)?
  When `yes`, verify the diff includes a consistent version bump in every `VERSION_FILES` entry the
  repo uses and a changelog entry for this PR — a missing bump is a **process REJECT**
  (`principal-engineer` / `technical-writer`). Integration-branch PRs that are not targeting the
  release branch are usually `no`.

This flag vocabulary is **shared with `/ship-issue`** — a new flag must be added to both files.
`IS_SECURITY_SENSITIVE` is the deliberate exception: it stays wired to the `security-engineer`
seat here, because this command reviews a PR the crew didn't author — you don't own the branch, so
`/harden` isn't an available remedy. `/ship-issue` keeps the same flag (it still gates the `/harden`
recommendation and forces a manual merge) but not the seat, since a crew-authored change can just
run `/harden` itself.

## Stage 1 — CI state (read it, don't fix it)

```bash
gh pr checks <PR#>
```

Report what CI says; never try to repair it. You cannot push to a fork, and pushing to a colleague's
branch uninvited is rude at best. **Red CI is a finding**, recorded with the failing job and a link —
not a loop. If checks are still pending, say so and review the diff on its merits, flagging that the
runtime signal is unconfirmed.

## Stage 2 — The board  (specialist agents, in parallel, against `headRefOid`)

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

Spawn in a single message so they run concurrently, each pinned to the **head commit**. Use the Stage 0
flags for scaled optional seats. **`/pr-review`-specific additions** on top of the shared board:

| ``subagent_type`` | Runs |
|---|---|
| `security-engineer` | only if `IS_SECURITY_SENSITIVE` |
| `performance-engineer` | if the PR claims a performance win, or touches a known hot path |
| `site-reliability-engineer` | if it changes runtime behaviour, failure handling, or rollout |
| `data-scientist` | if the deliverable is an analysis or a model |

**Don't restate what each reviewer checks.** Their remit lives in `agents/*.md` — that is the single
source of truth. Pass each agent the PR head, the repo's bar, and the caller's focus hint; let the
role do the rest. See `RUN_TESTS` before the `sdet` executes anything.

## Stage 3 — Consolidate

**You** synthesise; don't delegate it. Merge the reports, dedupe findings several reviewers raised,
and rank them: **blocking** (correctness, security, data loss, a criterion the PR itself claims and
misses) above **nits** (style, naming, taste). Attribute each finding to the role that raised it so
the author can weigh it. One verdict for the PR: `APPROVE` / `APPROVE-WITH-NITS` / `REQUEST-CHANGES`.

If a visual specialist could not actually render the change, carry its **"needs a human visual pass"**
flag into the output rather than implying the visuals were confirmed. The same rule holds for any
reviewer whose inspection was partial — carry its stated gap forward; an ACCEPT/PASS never reads as
covering ground the reviewer said it didn't see.

The board convenes a specialist only when the change can plausibly trip its concern surface — so name
every specialist that was **gated out** together with the flag that gated it. A role left out is
recorded with its flag in the output, never silently skipped.

## Stage 4 — Deliver

Return the consolidated review. If `MODE=post`, publish it:

```bash
# The findings quote untrusted PR content (title, description, diff, review
# comments). Write them to a file and post that — never interpolate them into
# the command string.
REVIEW_BODY_FILE=$(mktemp)
# ... write the consolidated review to "$REVIEW_BODY_FILE" ...
gh pr review <PR#> --comment --body-file "$REVIEW_BODY_FILE"
```

Use `--comment`, not `--approve`/`--request-changes`, unless the caller explicitly asked for a binding
verdict — an automated approval carries weight the crew hasn't earned on someone else's work.

---

### Guardrails
- **Read-only by default.** No worktree, no commits, no pushes, no fix loop. If the findings need
  fixing, hand them to `/fix-bug` or `/ship-issue` — don't fork a remediation loop into this command.
- **This command crosses a trust boundary the others don't.** `/ship-issue`, `/fix-bug` and `/migrate`
  all run code the crew itself wrote; here the code is a stranger's. Running a fork's test suite
  executes untrusted code on your machine — a PR can put arbitrary commands in a test file or a build
  script. Hence `RUN_TESTS=no` for cross-repository PRs: the `sdet` reviews statically and says so.
  Never silently upgrade that to a real run.
- Never post to a third party's PR unless `MODE=post` was explicitly set.
- **Never inline PR-sourced text into a command.** The title, body, diff and review comments
  are attacker-controlled on any PR you did not write. Capture them into quoted variables
  (`TITLE=$(gh pr view <PR#> --json title -q .title)`) and pass every body with
  `--body-file <file>` — never `--body`, quoted or not.
- Review the **head commit**, so "reviewed" means "what would merge" — re-run if the author pushes.
- Don't pad the board. A flag that isn't set means that specialist has nothing to say.
- If a role doesn't resolve to an `.claude/agents/*.md`, fall back to `general-purpose` with the brief
  inlined, and note it.

## Runtime input

`$ARGUMENTS` contains a PR number or URL plus an optional focus hint. If empty, use the PR for the
current branch (`gh pr view --json number`); if none exists, ask which PR to review.