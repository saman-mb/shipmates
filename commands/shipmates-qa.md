---
name: shipmates-qa
description: Shipmates: Walk a captain through interactive, one-step-at-a-time local QA of a PR, issue branch, or named branch — risk-targeted by default, optional blind smoke — logging findings for handoff. Guides and reports; it never repairs.
argument-hint: <pr-number|issue-number|branch> [mode: risk|smoke] [tester: human|agent] [platform hint]
allowed-tools: Bash, Read, Agent, Grep, Glob, WebSearch, WebFetch
disable-model-invocation: true
---
# /shipmates-qa — context → checklist → one step per turn → summary

<!-- shipmates:bootstrap -->
Point the crew at a change that needs **local QA on a running build** — a PR, an issue's branch,
or a named branch. By default, walk the captain through local QA **one check at a time** (human-as-tester);
opt in to `tester=agent` when the agent can operate the running build directly. CI and
`/ship-pr-review` cover automated gates and the specialist board; this command covers what those
miss: offline empty states, chrome collisions, cold-start races visible only in logs, primary actions
that look live but do nothing. In human mode, you guide; the captain operates the simulator, emulator,
or device. In agent mode, you drive the running build using the repo's documented tooling.
You do **not** edit the tree, open fix PRs, or auto-file issues mid-pass.

The target, mode, tester, and optional platform hint come from the Runtime input section at the end of
this workflow.

---

## Config (override only if the captain asks)

- `QA_MODE` = `risk` (default) or `smoke`. `risk` builds checks from the diff, acceptance criteria,
  and touched surfaces. `smoke` is a short blind path (see Stage 2) that does not require a deep
  diff read.
- `TESTER` = `human` (default) or `agent`. `human`: walk the captain one check at a time; the
  captain operates the build. `agent`: the agent drives the running build directly, captures evidence,
  and signs off each step with dual verdicts.
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
- **`TESTER`** — `human` (default) unless the captain specified `tester=agent` or `agent`.
- **Platform hint** — optional; keep as free text (e.g. "mobile simulator", "emulator",
  "physical device", "desktop", "web").

If the target is empty, ask which PR, issue, or branch to QA. Do not guess from a dirty worktree
without confirmation.

Announce the contract up front in one short beat:
- If `TESTER=human`: *interactive QA, one check per turn; reply `pass`, fail notes, a screenshot, or logs; I will not dump the full checklist.*
- If `TESTER=agent`: *agent-as-tester QA; discovering driving tooling from repo docs, executing checks directly with dual SDET+PO verdicts and evidence on file.*

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

When `TESTER=agent`, discover available driving tooling from the target repo's own docs — `README`,
`{{project-instructions}}`, contributing docs, or repo-local QA tooling docs. Those docs are the
sole source of truth for what can drive the app (commands, keys, selectors, screenshot captures, and
log streams). If the repo publishes no driving tooling or lacks tooling for a requested surface,
defer those steps to human QA (Stage 3) rather than guessing or inventing tooling.

Confirm with the captain: target resolved, `QA_MODE`, `TESTER`, platform, and how the build runs
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

Before the walk, gate checks against what the **named platform** and available driving tooling can
actually do:

- If network-simulation tooling (OS airplane mode, link conditioners, traffic shapers, and similar)
  is **unavailable or unreliable** on the simulator/emulator, **skip** those steps with an explicit
  deferral note: *deferred to real device / beta distribution*. Prefer **in-app** offline, airplane,
  or debug toggles when the product exposes them — discover from the repo's docs or UI, never invent a
  toggle that isn't there.
- When `TESTER=agent`: strictly gate checks against what the discovered repo tooling can operate. Any
  scenario requiring physical hardware, biometrics, purchases, unmocked external credentials,
  multi-device sync, an unsupported OS, or any action without documented driving tooling is
  **deferred to human / real device with an explicit reason**; never fake driving or pretend an
  unexecuted step passed.
- Never invent platform tooling. If a check needs hardware, OS support, or tooling the environment
  lacks, skip + defer; do not pretend it passed.
- Prefer captain-provided screenshots and console logs in human mode; in agent mode, capture
  screenshots and logs directly via documented repo tooling.

Tell the captain which steps were deferred and why, then begin the walk.

## Stage 4 — Interactive walk (one step per turn)

Branch on `TESTER`:

**When `TESTER=human` (default — interactive walk one step per turn):**

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

**When `TESTER=agent` (autonomous driving with dual sign-off):**

The agent operates the running app directly using the driving tooling discovered in Stage 1:

- For each non-deferred step `N of M`:
  1. **Drive:** Execute the actions (navigate, interact, input, scroll, read on-screen state) via the
     repo's documented tooling.
  2. **Capture evidence:** Record an evidence artifact for each check — screenshot file path,
     captured log slice, or assertion output. **No sign-off without evidence on file.**
  3. **Dual verdicts:** Evaluate the result through two perspectives:
     - **SDET verdict** (`PASS` / `FAIL`): Does the feature function correctly at the technical
       layer? If `FAIL`, capture exact reproduction steps and error output.
     - **PO verdict** (`PASS` / `FAIL`): Is the acceptance criterion met with acceptable quality
       and user experience? What is the product impact?
     Both must pass for the step result to be `PASS`. If either fails, the step is `FAIL`.
  4. On failure, log the finding in working memory with severity, reproduction steps, evidence
     pointer, and a product-impact draft (what changes / why it matters / who is affected).
     Continue to the next step unless a fatal blocker halts execution.
- If a step encounters an undrivable action or missing tooling mid-walk, do not fake execution — mark
  the step `DEFERRED` with the reason and proceed.

## Stage 5 — Summary

When every non-deferred step has a result (or the captain ends early), return a structured QA
report:

1. **Target** — PR/issue/branch, `QA_MODE`, `TESTER` (human or agent), platform, what was deferred.
2. **Per-step results** — `PASS` / `FAIL` / `DEFERRED` for each `N of M`, one line each. When
   `TESTER=agent`, cite the evidence pointer and dual SDET + PO sign-off per step.
3. **Findings** — each with severity, short repro, evidence pointer, and a **product-impact**
   statement (what changes / why it matters / who is affected). Ready to paste into an issue or PR
   comment later. Until the captain asks to file it, the disposition below is declined.
4. **Verdict** — `QA-PASS` / `QA-PASS-WITH-DEFERRALS` / `QA-FAIL` (any non-deferred fail → fail).
5. **Next** — optional handoff offers only (see Stage 6); no silent side effects.

Every finding in this Stage 5 summary has a disposition: fixed, filed, or declined with a reason,
not only a count. The default is declined — not filed, and not left unrecorded — unless the captain
asks to file it. Findings stay in working memory until that summary. No ledger file — do not write a
per-run findings ledger under `.shipmates/` or anywhere else in the repository (`.shipmates/worktrees/`
stays allowed). A docs or visual step names the rendered artifact inspected, or states inspection was
unavailable and does not pass that step.

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
- **One check per turn in human mode.** Once the human walk starts, keep the contract. Count is fine;
  the full private checklist stays private.
- **Generic and stack-agnostic.** `shipmates-qa` must never hardcode or assume any specific framework,
  toolchain, or platform driver. All driving tooling, launch commands, selectors, and screenshot
  mechanics come strictly from the target repo's own documentation (`README`, `{{project-instructions}}`,
  or QA docs).
- **Never invent tooling, selectors, or in-app toggles.** If the repo publishes no driving tooling, or
  if an action lacks documented tooling or hardware support, skip + defer; never fake driving,
  hallucinate selectors, or pretend an unexecuted step passed.
- **Evidence or no sign-off.** Under `tester=agent`, no step may pass without an evidence artifact on
  file (screenshot path, captured log, or assertion output) and dual SDET + PO sign-off.
- **No mid-QA issue spam.** File only when the captain asks; product-impact statements still ship in
  the summary so filing later is cheap.
- **Out of scope:** generating UI test suite code in the repository, replacing the board or CI,
  auto-filing without consent.
- If a role is ever spawned and doesn't resolve to a shipped crew role, fall back to a
  general-purpose agent with the brief inlined, and note it. Prefer running Stages 0–5 yourself —
  specialists are optional for narrow judgment calls, not required for the walk.

## Parameters

| Name | Required | Token | Values | Default | Help |
|------|----------|-------|--------|---------|------|
| target | yes | first recognizable token | PR number / issue number / branch name | — | What to QA locally (PR, issue branch, or named branch). |
| qa_mode | no | `risk` / `smoke` | `risk` / `smoke` | `risk` | `risk` builds checks from the diff; `smoke` is a short blind path. |
| tester | no | `tester=human` / `tester=agent` | human / agent | human | Driver: captain operates the app (`human`, default), or the agent operates the running build (`agent`). |
| platform | no | remaining words | free text | — | Simulator, emulator, device, desktop, or web hint for the walk. |
| filing | no | (session only; default report) | report | report | Return the QA summary only; do not file issues mid-pass unless the captain asks later. |
| fix_handoff | no | (after Stage 5, captain ask) | no / yes | no | Opt-in handoff to a fix command after findings; never invent a fix inside this command. |

## Runtime input

Read `## Parameters` first. `$ARGUMENTS` is the captain-supplied or post-intake token string; parse
Tokens/Values from that table (required then optionals). If intake ran, treat the restated
invocation as authoritative for this run.

Parse the first recognizable target token as the subject; treat a bare `smoke` or `risk` word as
`QA_MODE`; treat `tester=agent`, `tester=human`, or a bare `agent` / `human` word as `TESTER`
(`human` by default); treat remaining words as the platform hint.
