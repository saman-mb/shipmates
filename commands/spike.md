---
description: De-risk an open technical decision with a time-boxed spike — frame the question, prototype several approaches in parallel as throwaways, judge them against the project's constraints, and return a recommendation as an ADR. Produces a decision, not production code.
argument-hint: <the open question or decision — e.g. "which queue for the job system" or "SSR vs SPA">
allowed-tools: Bash, Read, Write, Edit, Agent, Grep, Glob, WebSearch, WebFetch
---

# /spike — prototype the options, recommend with an ADR

Answer a hard "which way should we build this?" question with evidence instead of a hunch. Frame the
decision, have engineers prototype **several approaches in parallel as disposable spikes**, judge them
against the project's real constraints, and record the outcome as an **ADR** (Architecture Decision
Record). The output is a *decision with reasoning*, not shippable code — the prototypes are throwaway.

Input (**$ARGUMENTS**): the open question or decision to resolve. If empty, ask what decision is blocked.

---

## Config

- `APPROACHES` = 2–4 — the distinct options to prototype (derive from the question; name them up front).
- `JUDGE` = `architect` (structural/quality-attribute trade-offs) — add `security-engineer`,
  `performance-engineer`, or `data-scientist` as extra judges when the decision hinges on their axis.
- `PROTOTYPER` = `senior-engineer`. `TIME_BOX` = keep each spike minimal — just enough to answer the
  question, thrown away after. `ISOLATION` = each prototype in its own throwaway worktree/branch so they
  don't collide and nothing lands on the base branch.
- **Constraints & priorities** = from the repo (quality attributes that matter here — performance,
  simplicity, reversibility, team familiarity, cost) plus anything in `$ARGUMENTS`.

## Stage 0 — Frame the decision

State the question sharply: what we're deciding, why now, the constraints, and — crucially — the
**criteria** each option will be judged on (the quality attributes that matter for *this* decision, e.g.
latency, operational simplicity, migration cost, reversibility). Enumerate the `APPROACHES` as distinct,
named options. A good frame makes the eventual recommendation obvious in hindsight.

## Stage 1 — Prototype in parallel  (agents: `senior-engineer` × N, one per approach)

Spawn one `senior-engineer` per approach **in a single message** (concurrent), each in its own isolated
worktree, to build the **smallest prototype that answers the question** — a spike, not a feature: enough
to measure the criteria (does it work, how complex, how fast, how much migration). Each returns: what
they built, what they learned, evidence against the criteria (numbers where measurable), and the sharp
edges they hit. Explicitly disposable — no polish, no tests-for-keeps.

## Stage 2 — Judge  (agent: `architect` + any axis-specific judges)

Spawn the `JUDGE`(s) to score every prototype against the Stage 0 criteria **side by side** — honest
trade-offs, not a favourite. Weigh **reversibility** heavily: a two-way-door choice (easy to undo) can be
made fast; a one-way door (a persisted format, a public API, a framework lock-in) demands more certainty.
Note essential vs accidental complexity. Output: a ranked comparison with the reasoning, and the runner-up's
best ideas worth grafting onto the winner.

## Stage 3 — Recommend as an ADR

Write an **ADR** to the repo's decision-records location (or `docs/adr/NNNN-<slug>.md` if none exists):
**Context** (the question + constraints), **Options considered** (each with its trade-offs and the spike
evidence), **Decision** (the recommendation), **Consequences** (what it commits us to, what becomes
harder, how reversible it is), and **Status** (proposed). Tear down the throwaway worktrees.

## Stage 4 — Report & hand off

Summarise the recommendation and why, link the ADR, and offer the next step: `/plan-epics` to turn the
chosen direction into a backlog, or `/ship-issue` if it's already a single unit of work. Optionally file
the ADR as an issue/PR for the team to ratify.

---

### Guardrails
- **Prototypes are disposable** — this command answers a question; it does not ship a feature. Don't let a
  spike quietly become the implementation without a proper `/ship-issue` pass.
- Judge against the criteria set in Stage 0 — decide on evidence, not the newest/most-familiar tech.
- Weigh **reversibility**: spend certainty on one-way doors; move fast on two-way doors.
- The decision and its trade-offs are **recorded** (the ADR) so the "why" survives — an unrecorded decision
  gets re-litigated in six months.
- Keep spikes minimal and time-boxed; more prototypes ≠ better if they don't sharpen the criteria.
- If a role doesn't resolve to a `.claude/agents/*.md`, fall back to `general-purpose` with the brief
  inlined and note it.
