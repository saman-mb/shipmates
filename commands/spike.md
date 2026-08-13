---
name: spike
description: De-risk an open technical decision with a time-boxed spike — frame the question, prototype several approaches in parallel as throwaways, judge them against the project's constraints, and return a recommendation as an ADR. Produces a decision, not production code.
argument-hint: <the open question or decision — e.g. "which queue for the job system" or "SSR vs SPA">
allowed-tools: Bash, Read, Write, Edit, Agent, Grep, Glob, WebSearch, WebFetch
disable-model-invocation: true
---
# /spike — prototype the options, recommend with an ADR
<!-- shipmates:command-preamble -->

Answer a hard "which way should we build this?" question with evidence instead of a hunch. Frame the
decision, have engineers prototype **several approaches in parallel as disposable spikes**, judge them
against the project's real constraints, and record the outcome as an **ADR** (Architecture Decision
Record). The output is a *decision with reasoning*, not shippable code — the prototypes are throwaway.

The open question comes from the Runtime input section at the end of this workflow.

---

## Config

- `APPROACHES` = 2–4 — the distinct options to prototype (derive from the question; name them up front).
- `JUDGE` = `architect` (structural/quality-attribute trade-offs) — add `security-engineer`,
  `performance-engineer`, or `data-scientist` as extra judges when the decision hinges on their axis.
- `PROTOTYPER` = `senior-engineer`. `TIME_BOX` = keep each spike minimal — just enough to answer the
  question, thrown away after. `ISOLATION` = each prototype in its own throwaway worktree/branch so they
  don't collide and nothing lands on the base branch.
- **Constraints & priorities** = from the repo (quality attributes that matter here — performance,
  simplicity, reversibility, team familiarity, cost) plus anything in the validated runtime input.
- **ADR delivery** — `MODE` = `pr` (default) or `edit-in-place`: where the **ADR** lands. This is a
  different axis from `ISOLATION` above: `ISOLATION` governs the throwaway prototype worktrees in
  Stage 1, which always exist and are always torn down; `MODE` governs only where the *deliverable*
  (the ADR) ends up, and defaults to a reviewed PR. Under `MODE=pr`: `BASE_BRANCH` = the repo's
  default branch — the PR's target, not what the worktree is cut from (that's current `HEAD`).
  `WORKTREE_DIR` = `../<repo>--adr-<slug>`. `BRANCH` = `docs/adr-<slug>`. `MERGE_MODE` = `manual`
  (stop at a reviewed PR; `auto` opt-in). `MAX_FIX_ROUNDS` = `2` — the cap on CI-fix rounds at
  Stage 3.5, so a permanently-red check escalates to the user instead of looping. A repo with no
  remote to open a PR against is the one fallback: build the branch, stop there, and report the
  branch as the undo path — never quietly write to the tree instead.

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

## Stage 2.5 — Isolate the ADR worktree  (`MODE=pr` only — orchestrator, deterministic, no agent)

The branch exists before the ADR does. First check `git -C <repo> status --porcelain`; if the
caller's tree is dirty, **warn loudly** that a worktree cut from `HEAD` holds committed work only, so
any uncommitted context won't carry into the ADR — then proceed. Exactly as `/ship-issue`'s isolate
stage, but cut from current `HEAD` rather than `origin/<BASE_BRANCH>`:

```bash
git -C <repo> worktree add <WORKTREE_DIR> -b <BRANCH> HEAD
```

This is separate from the `ISOLATION` worktrees Stage 1 used for the disposable prototypes — those
get torn down in Stage 3; this one holds the ADR that's about to be written and reviewed. Under
`MODE=edit-in-place`, skip this stage and write straight into the repo.

## Stage 3 — Recommend as an ADR

Write an **ADR** to the repo's decision-records location (or `docs/adr/NNNN-<slug>.md` if none
exists) — inside `<WORKTREE_DIR>` under `MODE=pr` (the default), or directly in the repo under
`MODE=edit-in-place`: **Context** (the question + constraints), **Options considered** (each with its
trade-offs and the spike evidence), **Decision** (the recommendation), **Consequences** (what it
commits us to, what becomes harder, how reversible it is), and **Status** (proposed). Tear down the
throwaway prototype worktrees from Stage 1 — the ADR's own worktree, if any, stays until Stage 3.5
delivers it.

## Stage 3.5 — Deliver the ADR  (`MODE=pr` only — orchestrator, deterministic, no agent)

Commit on the ADR branch — staging only the ADR file this run produced, never `git add -A` — push,
and open the PR against `BASE_BRANCH`. Then gate on CI: poll `gh pr checks` until nothing is pending;
a red check means pulling the failing log, fixing it, re-pushing, and re-polling — bounded by
`MAX_FIX_ROUNDS`, after which you stop and escalate to the user with the failing log rather than
looping. Never advance a red PR. Stop there unless `MERGE_MODE=auto`, in which case merge the PR and
remove `<WORKTREE_DIR>`; the manual default leaves the worktree in place with the PR open. Under
`MODE=edit-in-place`, there's nothing to deliver — the ADR is already in the tree.

## Stage 4 — Report & hand off

Summarise the recommendation and why, link the ADR — and, under `MODE=pr`, the PR it's waiting for
review on — and offer the next step: `/plan-epics` to turn the chosen direction into a backlog, or
`/ship-issue` if it's already a single unit of work.

---

### Guardrails
- **Prototypes are disposable** — this command answers a question; it does not ship a feature. Don't let a
  spike quietly become the implementation without a proper `/ship-issue` pass.
- Judge against the criteria set in Stage 0 — decide on evidence, not the newest/most-familiar tech.
- Weigh **reversibility**: spend certainty on one-way doors; move fast on two-way doors.
- The decision and its trade-offs are **recorded** (the ADR) so the "why" survives — an unrecorded decision
  gets re-litigated in six months.
- Keep spikes minimal and time-boxed; more prototypes ≠ better if they don't sharpen the criteria.
- **The ADR is the deliverable, so it gets a branch.** Prototypes stay disposable; the ADR itself
  lands as a diff a human can read, not a surprise in someone's checkout. `MODE=edit-in-place` is an
  explicit request, never an assumption — and it's a different switch from `ISOLATION`, which only
  governs the throwaway prototype worktrees.
- If a role doesn't resolve to an `{{agents-glob}}`, fall back to `{{general-purpose}}` with the brief
  inlined and note it.
- **Be resumable.** A re-run may find the worktree, branch, or PR already exists — reuse them rather
  than erroring or duplicating work.

## Runtime input

`$ARGUMENTS` is the open question or decision to resolve. If empty, ask what decision is blocked.
