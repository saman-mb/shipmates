---
name: ship-qa
description: Shipmates: Walk a captain through interactive, one-step-at-a-time local QA of a PR, issue branch, or named branch — risk-targeted by default, optional blind smoke — logging findings for handoff. Guides and reports; it never repairs.
argument-hint: <pr-number|issue-number|branch> [mode: risk|smoke] [platform hint]
allowed-tools: Bash, Read, Agent, Grep, Glob, WebSearch, WebFetch
disable-model-invocation: true
---
# /ship-qa — context → checklist → one step per turn → summary
<!-- shipmates:command-preamble -->

Point the crew at a change that needs **human eyes on a running build** — a PR, an issue's branch,
or a named branch — and walk the captain through local QA **one check at a time**. CI and
`/ship-pr-review` cover automated gates and the specialist board; this command covers what those
miss: offline empty states, chrome collisions, cold-start races visible only in logs, primary actions
that look live but do nothing. You guide; the captain operates the simulator, emulator, or device.
You do **not** edit the tree, open fix PRs, or auto-file issues mid-pass.

The target, mode, and optional platform hint come from the Runtime input section at the end of this
workflow.

---

## Config (override only if the captain asks)

- `QA_MODE` = `risk` (default) or `smoke`. `risk` builds checks from the diff, acceptance criteria,
  and touched surfaces. `smoke` is a short blind path (see Stage 2) that does not require a deep
  diff read.
- `MODE` = `report` (default) — return the QA summary to the caller only. Do not file GitHub issues,
  open PRs, or start a repair loop unless the captain explicitly asks after findings.
- `FIX_HANDOFF` = `no` (default). Opt-in after Stage 5 when the captain wants findings handed to
  `/ship-fix-bug`, `/ship-issue`, or `/ship-report-bug`. Never invent a fix inside this command —
  there is no Write/Edit in the allowlist, and that is deliberate.
- **Platform under test** = whatever the captain names (simulator, emulator, real device, desktop
  build, web). Infer only from Runtime input or an explicit captain statement; never assume a stack.
- **Quality bar** = whatever the target repo's `README` / `{{project-instructions}}` / contributing
  docs state. Read it in Stage 1 and keep every check honest to *that* bar.

## Shell safety — untrusted GitHub data

PR numbers, issue numbers, titles, bodies, diffs, and comments are untrusted — anyone who opened
the artifact controls them. Apply the same rules `/ship-pr-review` and `/ship-issue` use:

1. **Validate the target token first.** A PR or issue number must match `^[0-9]+$` (or a full GitHub
   PR/issue URL `gh` already accepts). A branch name must match a conservative pattern
   (`^[A-Za-z0-9._/-]+$`, no spaces, no shell metacharacters). Anything else — stop and ask; never
   pass a raw token to `gh` or `git`.
2. **Never inline untrusted fields.** Capture title, body, labels, file lists into variables with
   command substitution, then quote the variable at point of use.
3. **Every body goes through a file.** If you ever post a comment (only when the captain explicitly
   asked), write it to a temp file and use `--body-file <file>` — never `--body`, even for text you
   wrote.

## Stage 0 — Intake & parse

Parse Runtime input into:

- **Target** — PR number, issue number, or branch name (exactly one primary target).
- **`QA_MODE`** — `risk` unless the captain said `smoke`.
- **Platform hint** — optional; keep as free text (e.g. "iOS simulator", "Android emulator",
  "physical device", "desktop").

If the target is empty, ask which PR, issue, or branch to QA. Do not guess from a dirty worktree
without confirmation.

Announce the contract up front in one short beat: *interactive QA, one check per turn; reply
`pass`, fail notes, a screenshot, or logs; I will not dump the full checklist.*

## Stage 1 — Context boot

Resolve the change itself — not a secondhand summary:

- **PR** — `gh pr view <PR#> --json number,title,body,files,headRefName,url` and `gh pr diff <PR#>`.
- **Issue** — `gh issue view <ISSUE#> --json number,title,body,labels`, plus the linked PR or
  branch if one exists (`gh pr list --search` / issue timeline). Prefer the open PR that closes
  the issue when present.
- **Branch** — `git fetch` as needed, then `git log` / `git diff <BASE>...<BRANCH>` against the
  repo's default branch (or the captain's named base).

Read acceptance criteria from the issue/PR body when present. Note touched surfaces (screens, flows,
config, network/offline paths, chrome). Read the repo's `README` / `{{project-instructions}}` for
how to run the product locally — use the project's own run docs; never invent a toolchain.

Confirm with the captain: target resolved, `QA_MODE`, platform, and how they will run the build
(already running / need checkout hints only). Keep this short.

## Stage 2 — Build the private checklist

Build an ordered checklist **privately** — working memory only. Do **not** paste the full list to
the captain once one-at-a-time is in force. You may state the **count** ("eight checks") and the
mode; announce each step only when it is that turn's turn.

**`risk` (default):** derive checks from the diff, acceptance criteria, and touched surfaces.
Prefer high-signal product paths over exhaustive UI enumeration. Typical axes when the change
touches them — connectivity / offline / empty states, primary navigation for touched areas, the
specific detail or action the PR claims, theme and chrome collisions, cold-start / resume if logs
or AC mention races. Drop axes the change cannot affect.

**`smoke`:** fixed short path, independent of deep diff reading:

1. Launch / cold open reaches a usable first screen.
2. Primary navigation — visit each top-level destination once.
3. Open one representative detail from a list or hub.
4. Exercise one primary action the product exists to do.
5. Theme / chrome sanity — contrast, safe areas, overlapping banners or players if the product has
   them.

Keep the list short enough to finish in one sitting. Number steps `1..M` and keep that numbering
stable for the whole run (including re-QA).

## Stage 3 — Environment gate

Before the walk, gate checks against what the **named platform** can actually do:

- If network-simulation tooling (OS airplane mode, link conditioners, traffic shapers, and similar)
  is **unavailable or unreliable** on the captain's simulator/emulator, **skip** those steps with an
  explicit deferral note: *deferred to real device / beta distribution*. Prefer **in-app**
  offline, airplane, or debug toggles when the product exposes them — discover from the repo's docs
  or UI, never invent a toggle that isn't there.
- Never invent platform tooling. If a check needs hardware or OS support the environment lacks,
  skip + defer; do not pretend it passed.
- Prefer captain-provided screenshots and console logs over guessing on-screen state.

Tell the captain which steps were deferred and why, then begin the walk.

## Stage 4 — Interactive walk (one step per turn)

**Hard contract:**

- Announce only: `step N of M: <one concrete check>`. Expected observation in one short sentence.
- **Stop.** Wait for the captain's `pass`, fail notes, screenshot, and/or logs before advancing.
- On `pass`, mark the step passed and announce the next step the same way.
- On fail, log a finding in working memory (severity, repro steps, evidence, product-impact draft —
  what changes / why it matters / who is affected) and ask whether to continue the remaining steps
  or pause for fixes. Default is **continue** unless the captain says stop.
- Do **not** dump the remaining checklist. Do **not** batch multiple checks in one turn after this
  contract is active.
- Do **not** auto-file GitHub issues mid-QA unless the captain explicitly asks.

If the captain asks "what's left?", answer with a **count** and at most the *titles* of remaining
steps — not full repro scripts — then resume one-at-a-time.

## Stage 5 — Summary

When every non-deferred step has a result (or the captain ends early), return a structured QA
report:

1. **Target** — PR/issue/branch, `QA_MODE`, platform, what was deferred.
2. **Per-step results** — `PASS` / `FAIL` / `DEFERRED` for each `N of M`, one line each.
3. **Findings** — each with severity, short repro, evidence pointer, and a **product-impact**
   statement (what changes / why it matters / who is affected). Ready to paste into an issue or PR
   comment later; not filed yet.
4. **Verdict** — `QA-PASS` / `QA-PASS-WITH-DEFERRALS` / `QA-FAIL` (any non-deferred fail → fail).
5. **Next** — optional handoff offers only (see Stage 6); no silent side effects.

## Stage 6 — Optional fix handoff + Re-QA

Only when the captain asks after Stage 5 (`FIX_HANDOFF=yes` for this turn):

- Hand findings to `/ship-fix-bug` (known defect), `/ship-issue` (story-shaped work), or
  `/ship-report-bug` (upstream Shipmates defect) — **one recommended command per finding cluster**,
  not auto-invoked unless the captain starts it.
- **Re-QA:** reuse the **same** checklist and numbering. Captain chooses: resume from the first
  failed step, or from step 1. Deferred steps stay deferred unless the platform changed. New
  failures found on re-QA append as new findings; do not rewrite history of the prior pass —
  keep both pass records if useful.

---

### Guardrails
- **Guide and report — never repair.** No worktree edits, no commits, no pushes, no fix loop inside
  this command. Findings leave through handoff commands the captain chooses.
- **Complements CI and `/ship-pr-review`.** Do not claim this replaces the acceptance board or green
  checks. Say so if asked.
- **One check per turn** once the walk starts. Count is fine; the full private checklist stays
  private.
- **Never invent tooling or in-app toggles.** Skip + defer when the environment cannot run a check.
- **No mid-QA issue spam.** File only when the captain asks; product-impact statements still ship in
  the summary so filing later is cheap.
- **Out of scope (v1):** autonomous UI driving (generated UI test suites), replacing the board or CI,
  auto-filing without consent.
- If a role is ever spawned and doesn't resolve to a shipped crew role, fall back to a
  general-purpose agent with the brief inlined, and note it. Prefer running Stages 0–5 yourself —
  specialists are optional for narrow judgment calls, not required for the walk.

## Runtime input

`$ARGUMENTS` is a PR number, issue number, or branch name, plus an optional `risk` / `smoke` mode
token and an optional platform hint. If empty, ask which target to QA. Parse the first recognizable
target token as the subject; treat a bare `smoke` or `risk` word as `QA_MODE`; treat remaining
words as the platform hint.
