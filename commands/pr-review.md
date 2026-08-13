---
name: pr-review
description: Run the specialist acceptance board against an existing pull request the crew didn't author — classify the diff, pull the right reviewers, and return one consolidated verdict. Read-only by default: it reports, it never repairs.
argument-hint: <pr-number or PR url> [optional emphasis passed to every reviewer — e.g. "weight the schema change"]
allowed-tools: Bash, Read, Agent, Grep, Glob, WebSearch, WebFetch
disable-model-invocation: true
arguments: pull-request
invocation: @{{role}}({{pull-request}})
board: native
---
# /pr-review — classify → convene the board → consolidate

Point the crew at a pull request **somebody else wrote** — a teammate's, an outside contributor's, a
dependency bump, or one you opened by hand before you thought to use Shipmates. Every other command
assumes the crew authored the change; this one doesn't, and that single fact sets its shape: **no
worktree, no build, no fix loop.** You don't own the branch, so the deliverable is findings, not
commits.

Input (**{{pull-request}}**): a PR number or URL, plus an optional focus hint. If it's empty, default to the
PR for the current branch (`gh pr view --json number`); if there isn't one, ask which PR to review.

---

## Config (override only if the repo needs it)

- `MODE` = `report` — `report`: return the verdict to the caller only. `post`: also publish it with
  `gh pr review`. Posting onto someone else's PR is a **social side effect and irreversible**, so it
  is opt-in, never the default.
- `RUN_TESTS` = `no` for a PR from a **fork**, `ask` otherwise. See the trust boundary below — this
  command is the only one that executes code the crew did not write.
- **Quality bar** = whatever the target repo's `README` / `{{project-instructions}}` / contributing docs state. Read
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

Then read the repo's `README` / `{{project-instructions}}` for the bar. Set the **same classification flags
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

Spawn these in a single message so they run concurrently, each pinned to the **head commit** so they
review exactly what would merge. Two always run; the rest only when their flag is set:

| `{{role-reference}}` | Runs |
|---|---|
| `product-manager` | always — does it solve the stated problem, and does it clear the repo's bar? |
| `sdet` | always — test coverage and quality of the change (see `RUN_TESTS` before executing anything) |
| `architect` | only if `IS_ARCH_SIGNIFICANT` |
| `security-engineer` | only if `IS_SECURITY_SENSITIVE` |
| `devops-engineer` | only if `IS_DELIVERY_SENSITIVE` |
| `ux-ui-designer` | only if `IS_UI_STORY` |
| `art-director` | only if `IS_VISUAL_STORY` |
| `technical-writer` | only if `IS_DOCS_AFFECTING` |
| `performance-engineer` | if the PR claims a performance win, or touches a known hot path |
| `site-reliability-engineer` | if it changes runtime behaviour, failure handling, or rollout |
| `data-scientist` | if the deliverable is an analysis or a model |

**Don't restate what each reviewer checks.** Their remit lives in `agents/*.md` — that is the single
source of truth, and duplicating it here is how the two boards drift apart. Pass each agent the PR
head, the repo's bar, and the caller's focus hint; let the role do the rest.

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
- If a role doesn't resolve to an `{{agents-glob}}`, fall back to `{{general-purpose}}` with the brief
  inlined, and note it.
