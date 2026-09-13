# ADR 0002 — Discovering the available model pool before routing a tier to a model

**Status:** Accepted
**Date:** 2026-09-13
**Decided by:** `/ship-issue` #434
**Bundles:** #434 (pool discovery + resolution order + audit), sibling to #296 (neutral spawn-hint rendering)

---

## Context

Shipmates routes work by difficulty: a role sets a baseline **tier**, and a unit's complexity scales it.
Three decisions already shipped around that:

- **#205** — no adapter stamps a model into a crew file. A model is chosen at spawn, never installed.
- **#210** — the neutral tier vocabulary (`mechanical` / `judgment`, scaled by `trivial` / `standard` /
  `complex`) and the cross-harness effort scale.
- **#296** — the boundary this ADR shares: #296 owns neutral spawn-hint **rendering**; #434 owns pool
  **discovery** and the record that describes it.

What none of them settled is the question a spawn actually has to answer: **which models can this user
run on the harness in front of them, and which of those is cheap-capable or top-available?** Without an
answer, "pick the cheapest capable model" degrades into guessing a product name — the exact failure the
repo's own contributor rules forbid, because a written model name is wrong within a release cycle and
meaningless on a harness the user is not signed into.

Three inputs exist on a running harness, and they are not the same thing:

1. **Enumeration** — a way to ask which models the account can reach. Some harnesses document a
   non-interactive command; some do not.
2. **A relative ranking** — which of those is *cheapest capable* and which is *top*. No harness
   documents one for any target.
3. **An override surface** — where a chosen model and effort are written: a per-spawn argument, a static
   agent file, a session flag, or nothing.

The design has to hold across all eight targets in `tools/harness_matrix.json` and degrade to a *named*
fallback rather than a hard failure, because an installed command is a snapshot and the user's harness
is not.

## Evidence

Every cell below was read from the harness vendor's own documentation (or its own shipped repository)
on **2026-09-13**. Naming is described as a **scheme**; no document in this ADR contains a concrete
model identifier. The full per-harness record — with the exact command strings, the enumeration notes
and the `verified_on` date — lives in `tools/harness_matrix.json` under `model_surface`.

| Target | Discovery tier | Enumeration (scheme) | Override kind | Effort surface | Declared pool |
|---|---|---|---|---|---|
| claude-code | declared | none documented as a command; an interactive picker, and a gateway deployment can feed it from the gateway's endpoint | per-spawn, over a session/frontmatter/env chain | separate key; 5-step depth scale plus a non-model orchestration pseudo-level; unsupported levels clamp down | harness-native `availableModels` + `enforceAvailableModels`, per-surface mixed reject/replace/fallback |
| opencode | query | a listing subcommand taking an optional provider, with a cache-refresh and a verbose flag | static agent file, plus global config and a session flag | separate key plus a run-level variant preset; provider-defined, no fixed enum | provider-level only (`enabled_providers` / `disabled_providers`, denylist wins, silent exclusion) |
| antigravity | query | a listing subcommand that prints the available slugs | session-level; no per-agent key | run-level; a 3-value flag, separate from the tier folded into the slug | none documented; an unknown run value aborts non-zero and prints the list |
| codex | query | a debug subcommand that prints the raw catalog as JSON | per-spawn, with an agent-defaults table and a static agent file beneath | separate key; 6-step scale, gated on model support | none documented (open vendor request) |
| cursor | query | a session flag that lists all models, plus a listing subcommand | static agent file per subagent (`inherit` is the default), plus a session-wide flag | folded into the model string as a bracketed parameter; values are model-defined | none documented; documented instead are fallback conditions → the model is substituted |
| github-copilot | declared | none documented as a command; an interactive picker plus a static reference table | per-spawn, plus a static agent file and a settings override map | separate key with three mismatched first-party vocabularies (flag / settings key / free-string agent field) | repo-root allow-list file (globs plus one fallback directive) and an agent policy key; abort |
| pi | query | a listing flag with an optional fuzzy search; a catalog-refresh subcommand | session-level; the target ships no built-in subagents by design | separate key; 7-step scale with a per-model tristate support map | scoping, not enforcement (an enabled-models key and a models-pattern flag) |
| windsurf | inherit | none on the surface we target; a companion CLI documents a family-grouped JSON listing we do not drive | session-level on the surface we target | none; only an interactive shortcut-bound cycle | admin-side only; restriction and a team default, no abort |

Reading: **five `query`, two `declared`, one `inherit`** — and every one of the five query targets still
needs a declared tiering, because enumeration answers *what exists*, never *what is cheap*.

### Corrections and confirmations — where first-party docs changed or upheld the issue's table

| Cell | Claimed | Verified 2026-09-13 | First-party source |
|---|---|---|---|
| codex enumeration | no CLI to list models | **contradicted** — a debug subcommand prints the raw catalog as JSON | `developers.openai.com/codex/cli/reference.md` |
| cursor enumeration | not enumerated | **contradicted** — a `--list-models` flag and a `models` subcommand are documented | `cursor.com/docs/cli/reference/parameters` |
| github-copilot enumeration | not enumerated | **confirmed** — no listing command; an interactive picker plus a static table | `docs.github.com/en/copilot/reference/copilot-cli-reference/cli-command-reference` |
| github-copilot effort | no per-agent effort field | **contradicted** — the CLI reference documents an agent-frontmatter reasoning-effort key (plus a flag with five names and a settings key with four, vocabularies that do not match) | `docs.github.com/en/copilot/reference/copilot-cli-reference/cli-command-reference`; `.../cli-config-dir-reference` |
| github-copilot declared pool | not mentioned | **contradicted** — a repo-root allow-list file with a single fallback directive, and an agent policy key | `docs.github.com/en/copilot/reference/copilot-cli-reference/cli-config-dir-reference` |
| claude-code declared pool | not mentioned | **contradicted** — a harness-native allow-list key pair with per-surface enforcement | `code.claude.com/docs/en/model-config` |
| antigravity / github-copilot / windsurf effort | absent | **conflated** — static per-agent effort is absent on all three (the CI-enforced `effort` flag stays `false`), but a routing-usable override is documented on the first two: a run-level flag on antigravity, a separate agent-frontmatter key with three mismatched vocabularies on github-copilot; only the third has none | `antigravity.google/docs/cli/headless`; `docs.github.com/en/copilot/reference/copilot-cli-reference/{cli-command-reference,cli-config-dir-reference}`; `docs.devin.ai/cli/models` |
| opencode / pi effort surface | a separate static key on opencode; a run-level override on pi | **partly confirmed, partly reclassified** — opencode does document a separate per-agent pass-through key (the story's mapping holds), with a run-level variant preset on top; pi's effort is a separate per-session key on a 7-step scale, not the run-level override the story lists | `opencode.ai/docs/models`; `opencode.ai/docs/agents`; `opencode.ai/docs/cli`; pi's own `docs/usage.md`, `docs/settings.md`, `docs/models.md` |
| pi declared pool | a document-enforced allow-list that **aborts** (`enforce: true`, `allow: [...]`) — the story's reference semantics | **contradicted** — scoping is documented (`enabledModels` / a models-pattern flag) and an unknown override key is **ignored**; no allow-list with abort semantics is documented for the shipped product. The aborting shape the story cites is a *subagent example extension* in the vendor repo, not a built-in surface | pi's own `docs/usage.md`, `docs/settings.md`, `docs/models.md`; the extension README in the vendor repository |
| claude-code subagent-model environment override | reported, not documented | **re-verified** — the environment override is first-party documented (with a force variant), so the mechanism is no longer a report; only the reported absence of an effective-model field in the tool result remains unverified | `code.claude.com/docs/en/model-config`; `code.claude.com/docs/en/sub-agents` |
| github-copilot agent `model` property | no `model` property in the custom-agents reference | **contradicted** — the CLI reference documents a `model` property that inherits the default when unset, alongside a priority-ordered array form and an agent policy key | `docs.github.com/en/copilot/reference/copilot-cli-reference/cli-command-reference` |
| windsurf enumeration | queryable | **qualified** — the listing belongs to a companion CLI whose surface we do not target | `docs.devin.ai/cli/reference/commands` |

Two of the eight rows (claude-code, github-copilot) genuinely lack enumeration, so the design constraint
the issue named is real for them — but the durable constraint is stronger and applies to all eight: **no
harness tiers its own pool**, so a declared ranking is required regardless of queryability.

## Options considered

**A — Query-only.** Discover the pool by running the enumeration command; where there is none, inherit.
*Rejected.* Three of eight targets cannot reach a pool this way on the surface we ship, and enumeration
still never answers *which model is cheap-capable* — so the "cheapest capable" rule would resolve to an
arbitrary member of a listing, which is guessing with extra steps.

**B — Declared-only.** Skip enumeration entirely; route strictly from the user's declared pool.
*Rejected.* It handles the ranking problem but throws away a real capability: on five targets the
enumeration command is a cheap, authoritative **filter** that stops the orchestrator proposing a model
the account cannot reach. It also loses the ability to verify a surface exists before relying on it.

**C — Three-tier degrade: query → declared → inherit.** **Chosen.**
Enumeration narrows *candidates*; the declared pool supplies *rank*; `inherit` is the terminal fallback
that is always available. Each tier is independently useful, each has a named degradation, and a target
we have never seen recorded is handled by the same ladder (no row → declared → `inherit`).

## Decision

**Adopt C.** The algorithm is stated once, canonically, in `docs/COST.md` under `## Model routing` and
expanded from the shared cost-discipline preamble into **every** command — the ruleset is global, not
a two-command special case — so no command can drift from it. The repo-side capability record is
`tools/harness_matrix.json` → `model_surface`.

1. **Tiers.** Two neutral model tiers — `mechanical` (cheapest capable) and `judgment` (top available) —
   plus complexity scaling (`trivial` / `standard` / `complex`) and `inherit`. Effort is a **separate**
   decision from tier. No adapter resolves a tier to a concrete model; the tier is resolved at spawn.
2. **Discovery ladder.** `query` (the target's documented enumeration command — candidates only) →
   `declared` (the target's own native allow-list where documented, otherwise the user's
   `model-pool.json`) → `inherit`.
3. **Declared-pool shape.** The project file `<repo>/.shipmates/model-pool.json` wins over the user file
   `~/.shipmates/model-pool.json`. Keys: `schema_version`, `tiers.mechanical[]`, `tiers.judgment[]`,
   and optional `effort.mechanical` / `effort.judgment`. Entries are **patterns the target's own model
   surface accepts**, never a Shipmates-owned identifier. **Shipmates never writes a value into either
   file**, and no key has a model-name default, example or illustration.
4. **Resolution order, stated once** — explicit spawn value → declared default → parent/session value →
   the model's own effort default. A model chosen without an effort gets **that model's own default
   effort**; the parent's effort is never carried across a model change, because one model's effort
   scale does not describe another's.
5. **Enforcement.** An identity outside the resolved pool is **refused**, not silently clamped —
   resolve a different candidate, or stop and report. Targets whose documented mechanism is coarser
   (provider-level exclusion, scoping-only, admin-side filtering) have their gap named in the table and
   in the matrix record rather than being papered over.
6. **Audit.** One compact `MODEL ROUTING:` line per spawn, in the run report: tier, pool source
   (`project` / `user` / `harness-native` / `inherit`), the identity **as the harness accepts it**,
   effort requested and resolved, and `honoured` / `substituted` / `inherit`. Substitution is reported
   **as** substitution. #187 owns the structured cost line; this ADR adds no field to
   `EPIC_UNIT_RECORD`.
7. **Ownership.** The pool **shape** (paths, keys, ladder, resolution order, enforcement, audit) is
   install-time canonical content. The pool **values** are runtime and per-user, and Shipmates never
   writes one into an installed file. The per-harness record is repo-side only and is never installed.

## Design questions — answered

### 1. What is the source of truth per harness?

The three-tier ladder, one row per target, recorded in `tools/harness_matrix.json` under
`model_surface` with a `discovery_tier` of `query`, `declared` or `inherit` and a `verified_on` date. A
harness with no discoverable tiering is a stated verdict (`inherit`), never an untested cell — the
`test_matrix_model_surface_is_complete` guard enforces the closed enums and forbids a blank cell.

### 2. What is the shape of the pool, and where does it live?

The pool is a **ranking over user-declared patterns**, keyed by the neutral tiers, not a list of owned
identifiers. Listing is orthogonal to ranking: enumeration narrows candidates, the declared pool supplies
rank. Paths and keys are in the Decision above. The adapter layer never resolves a tier to a model.

### 3. What is the precedence order?

One order, every target: explicit spawn value → declared default → parent/session value → the model's
own effort default. Per-target absences are stated in the record rather than assumed: opencode, cursor
and windsurf document no per-spawn argument; pi, antigravity and windsurf document no per-agent file we
emit; and where a level does not exist, the next level in the order applies.

### 4. How is the pool enforced?

**Abort/refuse.** An out-of-pool identity is refused, using the strictest documented semantics in the
set as the reference shape. Three targets cannot honour strict enforcement, and each is recorded as a
finding rather than approximated: opencode is provider-granularity only with silent exclusion, pi
documents scoping rather than enforcement, and windsurf's restriction lives in an admin console with no
user file.

### 5. How is a routing decision audited?

One compact `MODEL ROUTING:` line per spawn in the run report, carrying the pool **source** so a
substituted model is visible as substituted. The line is additive: the existing cost accounting and the
`EPIC_UNIT_RECORD` schema are unchanged, and #187 keeps ownership of the structured cost line.

### 6. Where is the ownership boundary?

The pool **shape** is install-time canonical content (`docs/COST.md` → every command, via one marker).
The pool **values** are runtime and per-user; Shipmates never writes one into an installed file and
never ships a model-name default. The per-harness `model_surface` record is repo-side only.

## Per-target behaviour where a level is absent

- **No enumeration (claude-code, github-copilot).** The ladder starts at the declared tier: the
  harness-native allow-list where one is documented (both of these document one), else the user's pool.
- **No declared allow-list (opencode, antigravity, codex, cursor, pi).** Provider-level scoping
  (opencode) and scoping-only model sets (pi) are not a ranking: they fall through to the user's pool,
  and a target with neither falls to `inherit`.
- **No per-spawn argument (opencode, cursor, windsurf).** The override is a static agent file (the first
  two) or session level (the third); the orchestrator uses what the target documents and records the
  substitution when the harness's own rules win.
- **No per-agent file we emit (pi, antigravity, windsurf).** Session-level resolution, stated in the
  record — never invented as a frontmatter key.
- **No routing-usable effort (windsurf).** Effort is `none`; the tier still applies to the model, and the
  audit line records the effort as unresolved rather than guessing a value.
- **No row at all (a harness newer than the installed payload).** Treated as no-enumeration → declared →
  `inherit`, which is the same degradation the drift rule names.

## Consequences

**What this commits us to**

- Every spawn has a *named* source for its model: a candidate filter, a user ranking, or `inherit`. A
  model name can no longer appear in canonical content as a default, an example or a fallback.
- The per-harness model surface is a maintained record with a `verified_on` date and a mechanical
  completeness gate, so the next harness change has a place to land and a check that fails loudly.
- The audit line makes substitution visible, so "we chose the cheap model" is falsifiable in the report.

**What becomes harder**

- A user who declares no pool gets `inherit` everywhere: correct, but it means the tiering only pays off
  once a pool exists. That is a deliberate trade — we would rather inherit than guess.
- Three targets can never honour strict enforcement, so the invariant is "refused *or* the gap is
  reported", not "always refused". The report carries the honesty the mechanism cannot.
- The declaration file is user-owned, so its correctness is outside our test surface; the shape is fixed
  and the values are not.

## Open gaps

1. **Enumeration is unconfirmed by execution on claude-code and github-copilot.** Both are recorded as
   "no listing command" from the absence of one in their first-party flag tables. That is strong but it
   is absence of evidence; the next step is running each binary's help output and diffing it against the
   documented tables.
2. **Three effort vocabularies inside one harness (github-copilot)** — a flag, a settings key and a
   free-string agent field whose value sets do not match. Recorded as a finding; the docs do not
   reconcile it, so we do not pick a winner.
3. **A display-name/slug conflict on one target (antigravity)** — the headless page calls the run value a
   slug while a tracker thread reports it is the display name. Recorded, not resolved.
4. **Reported-but-undocumented items are recorded as reports.** The tracker threads behind them were
   re-linked, not re-fetched; the `reported_gaps` entries say so and must never be promoted to fact.
5. **Windsurf's identity** — the doc host now serves a differently-named product and the CLI surface is
   named differently from the adapter target. The record states the mismatch; the identity question
   belongs to #168.

## Follow-ups

- **Copilot effort emission.** The model surface now documents an agent-frontmatter reasoning-effort
  key; the adapter still emits none and the CI-enforced `effort` flag stays `false`, because that flag
  asserts emission. Aligning emission with the documented key is a separate story.
- **A model-docs watch entry.** `tools/harness_watch.json` should carry a per-harness *model-docs* URL,
  so model-surface drift is caught the same way skill-path drift is.
- **A reader-facing routing page** on the site, generated or hand-authored per that page's rules.
- **#296 is a sibling, not a duplicate.** #296 owns neutral spawn-hint rendering and consumes this
  record — including its `runtime_model_override` sub-key. It must not add a second record.
- **#168 owns the windsurf identity question** (rename, retarget or remove the target); this ADR records
  the mismatch rather than pre-empting that call.
