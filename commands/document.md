---
description: Write or refresh documentation that actually works — the technical-writer drafts it from the real code, then a fresh reader agent follows the steps against the repo and must reach the stated result. Loops until the docs are drift-free and completable.
argument-hint: <what to document — a module, a feature, a public API, the README, the whole repo>
allowed-tools: Bash, Read, Write, Edit, Agent, Grep, Glob, WebSearch, WebFetch
---

# /document — draft from the code → fresh-reader test → fix drift

Produce docs that get a reader to *done*, gated on the one check that catches bad docs: a **fresh agent
with no prior context follows the instructions against the real repo and must reach the stated result**.
Docs that drift from the code or can't be completed are rejected and fixed. The gate is "a newcomer can
actually follow this," not "it reads well."

Input (**$ARGUMENTS**): what to document — a module, a feature, a public API/CLI, the README/getting-started,
a migration guide, or the whole repo. If empty, ask what to document and for whom.

---

## Config

- `WRITER` = `technical-writer`. `READER` = a **fresh** `general-purpose` (or `technical-writer`) agent that
  has NOT seen the drafting — it only gets the doc + the repo, like a real newcomer.
- `MAX_ROUNDS` = `3`. `MODE` = `edit-in-place` (default) or `pr` (worktree + CI-gated PR, like `/ship-issue`).
- **Audience, voice & format** = the repo's own (README tone, docs framework, terminology). Match it; don't
  invent a competing style.

## Stage 0 — Scope: audience + doc type

Decide **who reads this and what they're trying to do**, then the right **Diátaxis** type — tutorial
(first success), how-to (accomplish one task), reference (accurate/exhaustive), or explanation (the why).
Don't blend them. State the concrete "reader can now do X" success condition the fresh-reader test will check.

## Stage 1 — Draft from the actual code  (agent: `technical-writer`)

Spawn the `technical-writer` to write/refresh the doc **from the real source** — read the actual
signatures, flags, paths, config, and outputs first; every command, parameter, and result must match what
the repo does *today*. Minimal (least that gets to done), scannable, consistent terminology, runnable
examples, prerequisites stated up front. Writes in the repo's format.

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

`edit-in-place`: leave the finished docs in the repo and report. `pr`: commit on a worktree branch, run the
CI gate (docs build/link-check if the repo has one), open a PR. Report: what was documented, the doc type,
the fresh-reader's final result (in its words), rounds taken, and any follow-ups (things worth documenting
next, discovered gaps).

---

### Guardrails
- **The fresh-reader test is the gate** — accuracy is proven by someone completing the task, not by the
  writer asserting it's right. The reader must be a genuinely fresh agent, not the writer re-reading.
- **Zero drift:** every command/flag/path/output matches the current code — verified against source, not memory.
- Right doc for the reader's job (Diátaxis) — don't turn a how-to into a history lesson.
- Minimal and honest: cut filler; call out breaking changes and their upgrade path in changelogs/migration guides.
- Bounded loop; escalate with the reader's blockers rather than shipping docs that don't work.
- If a role doesn't resolve to a `.claude/agents/*.md`, fall back to `general-purpose` with the brief
  inlined and note it.
