---
name: migrate
description: Run a mechanical migration across a whole codebase — discover every call site, transform each in isolation, verify per-site, and gate on a clean sweep (no old-pattern remnants) with the suite green. For API/dependency/pattern/framework migrations.
argument-hint: <from → to — e.g. "moment.js → date-fns" or "callback API → async/await">
allowed-tools: Bash, Read, Write, Edit, Agent, Grep, Glob, WebSearch, WebFetch
disable-model-invocation: true
---
# /migrate — discover → transform each → verify → sweep clean
<!-- shipmates:command-preamble -->

Carry a repeated, mechanical change across an entire codebase without missing a site or leaving it
half-migrated. Discover **every** occurrence, transform each independently in isolation, verify per site,
and close only when a **grep for the old pattern comes back empty** and the suite is green. The gate is
"zero remnants + still green," and nothing is dropped silently.

The migration comes from the Runtime input section at the end of this workflow.

---

## Config

- `BASE_BRANCH` = default branch. `WORKTREE_LAYOUT` = `nested` (default) —
  `<repo>/.shipmates/worktrees/`; runtime guidance **`worktree-root=sibling`** selects legacy
  `../<repo>--…` paths. `WORKTREE_DIR` — **nested:** `<repo>/.shipmates/worktrees/migrate-<slug>`;
  **sibling:** `../<repo>--migrate-<slug>`. Re-runs reuse the same path. `BRANCH` = `chore/migrate-<slug>`.
- `TRANSFORMER` = `senior-engineer`. `MAX_FIX_ROUNDS` = `3`. `MERGE_MODE` = `manual` (`auto` opt-in).
- `BATCH` = group call sites by module/ownership so parallel transformers don't touch the same files.
- **Correctness bar / test commands** = the repo's own. Read them first. Orchestrator owns all git/gh.

## Stage 0 — Discover every call site  ⛔ the migration is only as good as this census

Exhaustively find all occurrences of the old pattern — `grep`/`glob` across the repo for the symbol,
import, signature, or idiom (account for aliases, re-exports, string references, config, and docs). Produce
a **complete inventory** with counts and a per-file list. This census is the definition of "done": every
item on it must end migrated or explicitly excluded. Note any ambiguous/hand-judgement sites separately.

## Stage 1 — Plan the transform

Define the exact old→new transformation and the **per-site verification** (the tests/build that prove a
site still works). Identify sites that are *not* mechanical (semantics differ, not just syntax) and flag
them for careful individual handling rather than a blind sweep. Batch the inventory by module/ownership.

## Stage 2 — Isolate

1. **Resolve `<WORKTREE_DIR>`** from Config. Parse **`worktree-root=sibling`** from runtime guidance
   before resolving.
2. **Gitignore the worktree root** when `WORKTREE_LAYOUT=nested` — idempotently ensure
   `.shipmates/worktrees/` is in `<repo>/.gitignore` (append only when missing; create with a one-line
   Shipmates comment if absent). Never rewrite unrelated rules.
3. **Sync base ref** — `BASE_REF=origin/<BASE_BRANCH>`. **`git -C <repo> fetch origin`** is required;
   stop if fetch fails. Fresh `worktree add … origin/<BASE_BRANCH>` is pull-latest.
4. **Resume / reuse** — re-fetch; rebase onto `BASE_REF` when behind (sync conflicts → `MAX_FIX_ROUNDS`).
5. **Create the worktree** (when branch/worktree do not yet exist):

```bash
mkdir -p "$(dirname "<WORKTREE_DIR>")"
git -C <repo> worktree add <WORKTREE_DIR> -b <BRANCH> origin/<BASE_BRANCH>
```
All transforms land in the worktree; the base branch stays clean.

## Stage 3 — Transform each batch  (agents: `senior-engineer` × N, parallel, non-overlapping files)

Spawn one `TRANSFORMER` per batch **in a single message** (concurrent), each owning a disjoint file set
(use `isolation: worktree` if they'd otherwise collide). Each applies the exact transform to its sites,
preserves behaviour, matches surrounding style, and reports the sites it changed. **Handle the flagged
non-mechanical sites individually** — never blind-replace where semantics differ.

## Stage 4 — Verify + sweep clean  ⛔ HARD GATE  (agent: `sdet`)

1. `sdet` runs the full suite/build on the worktree — everything green.
2. **Sweep:** re-grep for the old pattern across the whole repo. It must come back **empty** (bar the
   sites you *explicitly* excluded in Stage 0, which you list). Any straggler → back to Stage 3.
3. Push and run the **CI gate** (poll `gh pr checks`; red → pull log, fix, re-push, re-poll — bounded by
   `MAX_FIX_ROUNDS`). Never advance red.
> If any sites are intentionally left un-migrated, **log them loudly** in the report and PR — a silent
> partial migration reads as "done" when it isn't.

## Stage 5 — Review & deliver  (agents, on the pushed PR head)

<!-- shipmates:acceptance-board -->

**Command-specific seats** (in addition to the mandatory PE+PO core):

- `sdet` (always): suite green on the PR head; sweep confirmed clean.
- `senior-engineer` or `architect` (fresh): spot-checks a sample of transformed sites for correctness and
  the non-mechanical sites in full — confirms behaviour is preserved, not just that it compiles.

Any `REJECT`/`FAIL` → fixer loop, re-push, re-gate (bounded), then **Retry** the board from the
fixer delta (shared rule), then escalate. Open (or, `auto`, merge) the
PR: body lists the census counts, sites migrated, sites excluded (with reasons), and the green-CI link.

## Stage 6 — Report

The inventory (found / migrated / excluded), the clean-sweep confirmation, review verdicts, fix rounds,
and the PR link. Be explicit about anything deliberately left behind.

---

### Guardrails
- **The census in Stage 0 is the contract** — every discovered site ends migrated or explicitly excluded;
  the clean re-grep is the proof.
- **No silent truncation.** Excluded/skipped sites are listed loudly, never quietly dropped.
- Mechanical ≠ semantic: sites where meaning (not just syntax) changes get individual human-grade handling.
- Isolated worktrees + non-overlapping ownership so parallel transforms don't corrupt each other.
- Bounded loops; never advance a red PR; the reviewer is a **fresh** agent.
- If a role doesn't resolve to an `{{agents-glob}}`, fall back to `{{general-purpose}}` with the brief
  inlined and note it.

## Runtime input

`$ARGUMENTS` describes the `from → to` migration: an API/signature change, dependency swap,
language/framework idiom, config format, or renamed symbol. If empty, ask what is migrating to what.
