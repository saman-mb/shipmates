# ADR 0003 — The orchestrator judges the model; there is no declared pool

**Status:** Accepted
**Date:** 2026-09-21
**Decided by:** captain, in session
**Supersedes:** the declared-pool half of [ADR 0002](0002-available-model-pool-discovery.md) — its
"a pool is required even when the pool is enumerable" rule, and the captain-authored `model-pool.json`
that rule depended on. The discovery ladder, the per-harness evidence, the enforcement contract and the
`tools/harness_matrix.json` record all stand.

---

## Context

ADR 0002 (#434) gave a spawn two ways to learn a model ranking: a captain-authored pool file
(`<repo>/model-pool.json`, then `~/.shipmates/model-pool.json`) whose entries ranked the neutral tiers,
or `inherit`. It argued the file was **necessary**, because no harness documents a cheap→best ladder, so
the orchestrator could not rank what it found.

Two problems surfaced in use:

1. **A static file cannot be right across targets.** Nine harnesses are supported, and the models that
   exist differ per harness, per plan and per sign-in. A pool written for one target is wrong or empty on
   another — and an empty pool is not a smaller ranking, it is no ranking at all, so every spawn fell to
   `inherit`. The feature's failure mode was indistinguishable from the feature's absence.
2. **It was invisible work with a sharp edge.** A repository that already owned a root `model-pool.json`
   meaning something else made the captain's own file unusable (#449, #486), and a captain had to learn a
   schema, two paths and a precedence order before a single per-role model was chosen.

## Decision

**The orchestrator is the ranking step.** The declared pool is removed — no project file, no user file,
no schema, no refusal rule — and the routing doctrine states one mechanism, exercised at spawn:

1. **Ask the target what exists.** Where the target documents a non-interactive listing command, run it.
   The answer is candidates only: it says what exists, never which is cheap or best.
2. **Where there is no listing command, read what the target documents.** A native allow-list (a managed
   settings key, or a repository-root allow-list file) offers what it permits, alongside the model the
   session is already on. A target with neither offers nothing to read.
3. **Then judge**, per role: cheapest capable for `mechanical`, top available for `judgment`, at the
   effort the unit needs.
4. **Record the call** on the `MODEL ROUTING:` line, with whether the identity was observed on the target
   or inherited.
5. **`inherit` when you cannot decide** — nothing to read, a listing that fails or will not parse, or a
   call you are not confident in.

Two constraints keep that a judgment rather than a guess: an identity may only be one **the target itself
offered** — never recalled from a price page, a release note or a naming convention — and the choice is
**recorded**, so a wrong one is visible and correctable. A captain who wants a specific model can still
name it in the run's own guidance; that is an explicit spawn value and outranks the orchestrator.

### What this costs

- **A captain cannot pin a model in a file.** Pinning becomes per-run guidance rather than stored
  configuration. That is the point of the decision, and it is the part to revisit first if a durable pin
  is ever genuinely needed.
- **The judgment itself is not mechanically verifiable.** No test can prove a model choice was wise. What
  is guarded instead is the rule's presence, the removed concept's absence, and that no canonical file
  still describes a pool.
- **`inherit` carries more traffic** on the three targets with nothing to read: claude-code and
  github-copilot document only an allow-list, and windsurf documents neither.

## Consequences

- `docs/COST.md`'s routing block drops from 8,377 to 7,347 bytes, and its ceiling from 8,400 to 7,500 —
  the block is inlined into all seventeen commands on all nine targets.
- `tools/harness_matrix.json` **keeps** `declared_pool`, because that records each *target's own* native
  allow-list mechanism — a capability fact, not a captain-authored config. Its `empty_pool_fallback` key
  is renamed `empty_surface_fallback` for the same reason.
- The per-target table is unchanged: `discovery_tier` still separates a listing command (`query`), an
  allow-list with no command (`declared`) and neither (`inherit`), and the guards binding the table to
  the record still hold.
- ADR 0002 remains the record of the discovery ladder and the per-harness evidence; only its declared-pool
  half is superseded.
