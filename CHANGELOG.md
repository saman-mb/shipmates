# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versioning follows
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.10.4] - 2026-09-21

### Fixed

- **`/shipmates-consolidate-issues` Stage 3 reattach is symmetric for existing epics.** Checklist
  backfill and `Part of #<epic>` on the child are unconditional (not fallback-only); post-attach
  verification checks membership in the epic's pre-existing child set rather than exact equality;
  and a story already labeled `Part of #<epic>` but not yet graph-linked stays a migrate case
  (#517, follow-up to #519 / #516).

## [0.10.3] - 2026-09-21

### Added

- **PR bodies require a maintainer-facing Why merge this block** on every command
  that opens a pull request. The block answers the product-impact bar (What
  changes / Why it matters / Who is affected) — not a file list or bare
  `Closes #n` — and is shared from `docs/COST.md` via
  `<!-- shipmates:why-merge-pr -->` (#530).

## [0.10.2] - 2026-09-21

### Fixed

- **`--from-cwd` and `SHIPMATES_SRC` together are a hard error** (two explicit
  sources). `--from-cwd` and the contributor loop walk up from a nested cwd to
  the catalog root; the contributor loop still exact-matches that root against
  this binary's checkout (#394).
- **Intentional `update --with-tools none` no longer leaves tool bak sidecars or
  empty husk dirs** that doctor would treat as an interrupted update forever.
  Mid-write overwrite backups are unchanged (#418).
- **Collision refuse and doctor foreign-collision messages replay the captain's
  install flags** (`--harness`, `--dir`/`--local`/`--global`, `--with-tools`)
  plus `--force` — never a bare `shipmates install --force` (#392).
- **Foreign collisions are advisory (`warn`, unfixable)** so doctor can go
  green without implying `doctor --fix` will overwrite the captain's file; the
  detail line still names the force hint (#393).
- **`doctor --fix` skip stdout includes `mv <bak> <dest>`** when a non-matching
  sibling bak blocks restore of an unowned missing path (#361).
- **Pi contributor steering** is recorded in the capability registry and
  steering path table; contributor-tree `install --harness pi` writes
  `.shipmates/contributor-steering.md` and claims it on the receipt (#482, #483).

### Changed

- Identity-rename delete-after-write keeps the safer rename rollback (delete the
  half-written new path); documented in code, with CLI lifecycle coverage for
  install and `doctor --fix` identity paths (#379).
- Install and troubleshooting docs describe interrupted-update bak restore and
  the non-matching `mv` hint (#362).

## [0.10.1] - 2026-09-21

### Fixed

- **Verifying a CI or config fix must not use a merge to a shared branch as the
  test.** `/shipmates-issue` and `/shipmates-epic` now say a merge is never itself
  the verification step — use a local simulation or a disposable branch/PR that is
  never merged. Stage 4.5 names a permanently empty check suite as its own failure
  mode (not pending, not red). Stage 0.5 cheaply confirms a `pull_request` event
  actually produces checks on the epic branch. Citation verification covers
  third-party platform claims, not only in-repo `file:line` citations (#480).
- **`/shipmates-consolidate-issues` reattaches dangling issues to existing open epics
  before inventing a new bundle.** Stage 0 inventories the open-epic set; Stage 2
  matches unmatched keep-candidates against it (`target_epic`); Stage 3 reattaches;
  only the leftovers reach Stage 4's themed bundles. The report counts reattachments
  vs fresh bundles (#516).
- **A global Pi install no longer writes command skills.** Pi loads `~/.pi/agent/skills`
  and project `.agents/skills` in the same session, so a home install plus a project
  (or sibling shared-tree) install printed `[Skill conflicts]` for every `ship-*`
  name. Project Pi still shares `.agents/skills` with sibling harnesses (one copy).
  A home install keeps crew at `~/.pi/agent/agents/` and omits command/tool skills.
  `doctor --harness pi` warns when the same Shipmates skill name is present in more
  than one tree Pi would load (#513).

## [0.10.0] - 2026-09-20

### Changed

- **All seventeen commands (except the repo-internal `/ship-qa` and `/ship-deslop`) carry
  the `shipmates-` prefix.** They shipped under `ship-` (`/ship-issue`, `/ship-epic`, …); the home
  page's "THE COMMANDS" section always showed the plain, unprefixed title and never that prefix, so
  a captain typing what the site showed got "command not found". The `shipmates-` prefix is the one
  every install now uses, matching the CLI's own name — an existing install migrates automatically,
  with the old skill renamed in place, its previous bytes backed up, and the receipt rewritten
  (#502).

### Added

- **xAI's Grok Build is a target: `shipmates install --harness grok-build`.** The ninth harness — and
  the seventh to take the full crew — installs into the harness's own native tree rather than the
  shared one: thirteen crew at `.grok/agents/<name>.md`, and the seventeen commands plus all eleven
  toolbox tools at `.grok/skills/<name>/SKILL.md`. It is the one non-Claude target that keeps the
  native `disable-model-invocation` guard, so the seventeen stay captain-invoked by the same key
  Claude Code uses; it enforces a per-agent tool allowlist, so least privilege holds without
  opencode's inverted `deny`-first map; and it carries static per-role `effort` on the harness's own
  scale. Project steering lands at `.grok/rules/shipmates-contributor.md` and global steering at
  `~/.grok/AGENTS.md`. The payload's format was verified against Grok Build's own first-party docs and
  its digest is gate-checked in CI; no live run is recorded, so its `runtime_verified` status is
  `none` — `tools/harness_matrix.json` says exactly that, and nothing claims more (#509).
- **One recorded gap on Grok Build: the toolbox tools are typeable.** Its skills are model-invoked,
  and the harness offers no way to hide one from the slash menu without hiding it from the model — so
  a tool arrives agent-invoked *and* still reachable by typing its name. Recorded in the adapter and
  the harness matrix rather than papered over (#509).

## [0.9.3] - 2026-09-20

### Changed

- **The seventeenth command is now `/ship-deslop`.** It shipped in 0.9.0 as
  `/ship-deslop-codebase`; the shorter name is the one it keeps. An existing install migrates
  automatically — the old skill is renamed in place with the previous bytes backed up and the receipt
  rewritten — and the old site page keeps a redirect, so nothing breaks for a captain who already has
  it (#505).
- **The census gained a twelfth finding class: structural placement.** Files and symbols sitting in the
  wrong module, layer or folder for the repository's own stated architecture — a layer importing inward
  past its boundary, logic in a presentation file that belongs in the application layer, cross-cutting
  policy parked inside one feature, a catch-all `utils` bucket, single-file folders, sibling modules
  disagreeing about their own layout. The repository's committed convention is the oracle: the class
  quotes it from a committed file before recording a finding, and degrades to observed-majority-layout
  findings where none exists, rather than inventing a taxonomy the project never chose. A move is not a
  delete, so a pure relocation grades at least `review` even with no call-site change — moves break
  downstream patches, `git blame` continuity and path-based tooling — and the moved path is grepped
  across CI config, ownership rules and docs so the blast radius is in the finding rather than found in CI.
  The coverage footer now also names the classes that were *not* looked for, so a run cannot read as a
  clean bill of health for something it never asked about. The command was also swept for stack and
  domain assumptions — it no longer presumes a compiler, a lockfile, a type system or an async runtime,
  and its examples are shapes of debt rather than any one ecosystem's (#505).
- **Pruned the dependency set.** `flate2`, `reqwest` and `tar` were declared but never referenced
  anywhere in the source, and `serde_yaml` was reachable only from the test suite. The first three
  are removed and `serde_yaml` moved to `[dev-dependencies]`, shrinking the published dependency
  closure by roughly 150 packages and dropping an async HTTP stack from a CLI that never makes a
  network request (#496).
- **The binary no longer compiles the module tree a second time.** `src/main.rs` re-declared the ten
  modules `src/lib.rs` already exposes, so every unit test ran twice and fourteen dead-code warnings
  came from the library's copy of items only the binary calls. The binary now consumes the library
  instead of duplicating it: unit tests run once (the duplicated 184-run set is gone), warnings drop
  from 14 to 2, and the refactor changed no rendered output — all eight target digests were unmoved by
  it. Two items in `src/installer/manifest_db.rs` widened from `pub(crate)` to `pub`, since
  `pub(crate)` does not cross the lib/bin boundary (#495).
- **`/ship-deslop` now names its crew at every spawn.** A live run resolved its workers to
  general-purpose agents instead of the crew, because the command described "workers" without ever
  naming a role — the harness had no identity to look up. The command now carries a crew roster and
  names the role at each spawn, and `validate_skills.py` fails any command whose fan-out section binds
  no crew role and names none — so the defect cannot recur silently. The command's
  `technical-writer` seat is explicitly briefed to report only: unlike the other six roles it is a
  writing role by trade, and an audit must not carry a write path.

## [0.9.2] - 2026-09-19

### Changed

- **Runtime verification is structured, not prose-only.** `tools/harness_matrix.json`
  now carries a per-harness `runtime_verified` record (`full` / `partial` / `none`) with
  distinct cells for crew resolve, argument passing, command E2E, and the commands
  exercised. Claude Code stays `full`; antigravity, cursor, pi and opencode are captain-
  attested `partial` live runs (#497); codex, GitHub Copilot and windsurf remain `none`.
  Scope & honesty, README, and the site roadmap read from that record instead of claiming
  only Claude Code has ever been run.

### Fixed

- **Every command that files an epic now spells out the `gh` sub-issue attach step.**
  `/ship-deslop-codebase` delegated its filing mechanics to `/ship-plan-epics` by reference,
  so a run could produce an epic body with a tidy `- [ ] #N` checklist and **no parent/child
  graph** — the epic reads as filed, but `/ship-epic` and every dependency walk that reads
  `subIssues` sees an empty parent. The command now states the sequence inline (`gh issue
  create` the epic, `gh issue create` each sub-issue, `gh issue edit <epic> --add-sub-issue
  <sub>`, then re-fetch `subIssues` to verify), and `/ship-consolidate-issues` carries the
  same explicit attach-and-verify step when a migration turns an issue into an epic with
  stories. The sub-issue graph is the relationship source of truth; the checklist is display.

## [0.9.0] - 2026-09-15

### Added

- **`/ship-deslop-codebase` — discovery-driven codebase simplification.** A seventeenth command
  that audits a repo for the health debt a linter alone misses: dead code, duplication,
  redundancy, machinery built far beyond what the product actually needs, inconsistent
  patterns, complexity hotspots, dependency and config rot, and documentation drift. Every
  finding is graded `safe` / `review` / `architectural` / `protected`, so nothing reachable by
  reflection, a migration, an error path or a public API is deleted on the strength of a grep.
  The command never edits your code: it files the findings as an epic with sub-issues, stops at
  a human gate where you confirm the breakdown is sound, and then hands the scope you pick —
  the whole epic, a single sub-issue, or a bundle — to the workflow that ships it (#488).

## [0.8.2] - 2026-09-15

### Changed

- **Non-blocking review nits are absorbed into the shipping PR by default.** `/ship-issue` Stage 7 is
  now **nit disposition** (`NITS_MODE=absorb`): cheap in-scope findings are fixed on the same branch
  (bounded by `MAX_ABSORB_NITS` / `MAX_ABSORB_LOC` / `ABSORB_FIX_ROUNDS`), taste items become PR notes,
  and backlog tickets are capped (`MAX_FOLLOWUP_ISSUES`) and theme-batched with dedupe — not one issue
  per nit. Captain overrides: `nits absorb` / `nits file` / `nits pr-comment`. Sibling PR-raising
  commands (`/ship-fix-bug`, `/ship-harden`, `/ship-refactor`, `/ship-polish`, `/ship-pr-review`) and
  `/ship-epic` unit accounting follow the same ladder so captains stop drowning in `priority:low`
  tech-debt tickets that never get picked up (#490).

## [0.8.1] - 2026-09-15

### Fixed

- **`shipmates install` no longer auto-installs every detected harness.** Omitting `--harness`
  prints any detections as a hint, then prompts in a terminal (Enter = `claude-code`) or installs
  `claude-code` only when non-interactive. Pass `--harness NAME` or `--harness all` to opt in (#489).
- **Detection markers no longer treat every GitHub repo as Copilot, or a shared skills tree as
  Antigravity.** Project detection requires `.github/agents` and `.agents/agents` respectively;
  home detection dropped bare `~/.github` and bare `~/.gemini` in favour of harness-owned config
  roots. Receipts are read from `.shipmates/receipts/` (the path install actually writes) (#489).
- **Project `--local` / `--dir` installs no longer rewrite home instruction files.** Canonical
  global steering installs only when the target is the home directory (default / `--global`).
  `shipmates uninstall` now strips the managed steering block / Tier-A file for that harness (#489).
- **Symlink skips no longer claim a full tool install.** Skipped paths are reported as an incomplete
  install, and the tools count / returned tool set match what was actually written (#489).
- **Cursor's install picker blurb names `.cursor/skills`**, matching where the payload lands (#405,
  #489).
- **Uninstall no longer spam-warns `cannot read dir` after a sibling already removed that directory**
  (#489).

## [0.8.0] - 2026-09-15

### Changed

- **The project model pool moved to the repository root.** The project file is now
  `<repo>/model-pool.json`, and `<repo>` is defined as the **run's repository root** — the checkout the
  run was started from, so a spawn running in a cut worktree still reads the run's checkout, never its
  own. A captain's pool becomes a file the repository can commit, which is what lets it reach every clone
  and every worktree cut from one; the retired `<repo>/.shipmates/model-pool.json` sat under the
  project's `.shipmates/` tree, which is Shipmates' own install state — per-machine wherever a repository
  ignores it, as this one does. The user file is unchanged at `~/.shipmates/model-pool.json`. Captains
  upgrading from v0.7.x: move `model-pool.json` to the repository root (#449).
- **The `MODEL ROUTING:` line names a pool it did not consult.** Its `pool` field carries the source in
  force (`project` / `user` / `inherit`) and, only when no project pool is in force, **at most one**
  condition, chosen in the stated order — `pool unusable`, `pool out of scope`, `no pool` — so a project
  pool the run does not resolve, a worktree's own copy included, is a named line rather than a silent
  absence (#449).
- **The per-target table in `## Model routing` is trimmed to the cells the orchestrator acts on.** Each
  row's discovery tier, override kind and enforcement are the capability record's own values, and the
  effort cell is the record's effort kind plus at most one clamp clause; the restated glosses are gone.
  The rendered block drops from 8,809 to 8,260 bytes on every command on all eight targets, and guards
  now cap both the block and its table so the glosses cannot creep back (#450).

### Fixed

- **A project model pool the run did not resolve was ignored in silence.** A pool declared inside a
  worktree, or left at the project's old in-tree `.shipmates/` path, was never the pool in force and the
  report said nothing about it. Every such case is now `pool out of scope` on the spawn's audit line —
  never fatal, never quiet — and a selected file that cannot be read stays `inherit (pool unusable)` with
  no fall-through to the other file (#449).

## [0.7.4] - 2026-09-15

### Fixed

- **pi contributor steering parity.** The pi adapter now emits `.shipmates/contributor-steering.md`
  like the other seven harnesses, and receipts/doctor accept that path for pi (#442).
- **pi home-install guidance.** A `--global` / `$HOME` pi install prints a project-local preference
  note: pi resolves the nearest ancestor with `.pi/` or `.agents/`, so a home install can be shadowed
  (#454). Doctor-side detection remains #453.

## [0.7.3] - 2026-09-15

### Fixed

- **Gated command composition mid-run.** `/ship-epic` (and other orchestrators) compose `/ship-issue`
  and `/ship-report-bug` by Reading the installed command file and executing its stages in-session —
  not via Skill / `skill` (which Claude Code delists when `disable-model-invocation: true`). The
  user-invoked-only gate stays on; the contradictory "never reimplement stages inline" guardrail is
  rewritten so Stage 2 is implementable (#473).

## [0.7.2] - 2026-09-15

### Added

- **Interactive argument intake as the catalogue default.** Every command declares a `## Parameters`
  catalogue (each captain-facing knob is its own row with a Default) and the shared command-preamble
  in `docs/COST.md` runs a **defaults-first** intake when `$ARGUMENTS` is empty or missing a required
  parameter: show **every** Parameters row (finite enums print the default **and** the other
  choices), ask OK or type what to change, then proceed — no long form each run. Fully specified
  invocations still skip straight to the workflow; non-interactive runs fail closed on missing
  requireds and never hang (#467).

## [0.7.1] - 2026-09-13

### Fixed

- **`doctor --harness opencode` reported a `[fix]` that `--fix` could never clear.** opencode's `write`
  tool was invisible to two places that agreed with each other: `capability_registry.json` recorded
  `write` as folded into `edit`, and the checker's vocabulary list had no `write` — while the adapter
  emitted `write` correctly, because two roles carry an explicit `tool-order` naming it. A false
  `Problem` on every opencode install trains a captain to ignore the check, which is the failure mode
  the check exists to prevent (#468).

  opencode's `write` is first-class alongside `edit` — verified against the shipped binary, which
  defines both in one tool table next to `bash`. The registry and the vocabulary now agree with the
  adapter, and a new test asserts that **every tool name the registry documents resolves in the
  vocabulary `doctor` checks that harness against**, so the two can no longer drift apart in silence.

- The `edit` capability now reaches opencode's `write` tool as well as `edit`. `edit` can only modify
  a file that exists, so a builder given `edit` alone cannot create one; Claude Code and Antigravity
  both map this capability to two tools for that reason. The gap was latent — both edit-capable roles
  declare an explicit `tool-order` naming `write` — and is pinned by a test before the next role
  omits one.

## [0.7.0] - 2026-09-13

### Added

- **`/ship-qa` — interactive local QA walkthrough.** A sixteenth command that walks a captain
  through one-step-at-a-time human QA of a PR, issue branch, or named branch — risk-targeted by
  default, optional blind smoke — with simulator/emulator environment gates, in-app toggle
  preference, product-impact-ready findings, and re-QA that reuses the same checklist. Complements
  `/ship-pr-review` and CI; it guides and reports, it never repairs (#463).

## [0.6.4] - 2026-09-13

### Fixed

- **Every shared-tree command told the model to resolve a crew role from the wrong tree.** Canonical
  prose named `.agents/agents/*.md` as the crew location, and that claim is rendered *once* and shared
  by four harnesses whose crews live in four different trees — correct only for Antigravity. pi reads
  `.pi/agents/`, Codex `.codex/agents/`, Copilot `.github/agents/`. An agent that believed it would
  conclude the role had not resolved and fall back to a general-purpose agent, quietly downgrading a
  specialist seat to a generic one (#455).

  The fallback instruction now names a *shipped crew role* and a *general-purpose agent* — true for
  every harness — instead of asserting a path that can only be right for one. `{{agents-glob}}` and
  `{{general-purpose}}` remain part of the render vocabulary, because a harness rendering through its
  own dialect may still use them correctly; what changes is that canonical content may not, and a test
  enforces it so the next author cannot reintroduce it. Two commands also carried a bare
  `general-purpose` in the same instruction and are corrected the same way.

- `doctor`'s foreign-crew check is now driven by a documented `FOREIGN_CREW_READS` table rather than a
  hardcoded `if harness == "pi"`, and it resolves each foreign crew's `tools:` against the *reader's*
  vocabulary. Ownership was never the hard half — the failure is on the reader's side, and a hardcoded
  reader is how the next one goes unnoticed (#453).

## [0.6.3] - 2026-09-13

### Fixed

- **One symlinked config path no longer aborts the whole install.** Sharing a single skills or agents
  tree across harnesses is a normal thing to want, and it is exactly what a dotfiles repo or a skills
  manager produces. Shipmates refused to write *through* a symlink — a containment property that is
  not negotiable — but it refused the entire payload to do it, so an install that could place forty
  files placed none, on a path the captain had deliberately linked and that was already correct
  (#462).

  A symlinked payload path is now skipped and named in the install summary, and the rest of the
  payload installs normally. `doctor` reports them under a `Symlinked paths` check instead of failing
  the whole diagnosis, and `doctor --fix` leaves them exactly as found — never written, never
  repaired, never removed. An *unsafe* path (absolute, or containing `..`) is still a hard error: that
  is a programming fault, not a fact about the captain's environment.

## [0.6.2] - 2026-09-13

### Fixed

- **Antigravity's crew were never loaded.** The adapter emitted a flat
  `.agents/agents/<name>.md`, but Antigravity discovers `{workspace}/.agents/agents/{agent_name}/`
  and reads the `agent.md` inside that directory — so the crew installed cleanly and were invisible,
  in both scopes. Verified against the shipped `agy` binary, which embeds that path template and whose
  release notes describe custom agents as `agent.md` files. The legacy flat shape stays loadable on the
  receipt side, so an existing install remains upgradable and removable (#460).
- **Global installs wrote workspace paths into `$HOME`.** A global install (`--global`, the default)
  joined the *workspace* tree to the home directory. For four harnesses that is not where the harness
  reads: Antigravity's global tree is `~/.gemini/config/`, pi's is `~/.pi/agent/`, Codex's is
  `$CODEX_HOME` (`~/.codex`), and Copilot's is `~/.copilot`. All four now land there, and the plan, the
  receipt, the migration table and `doctor` all agree on the relocated paths. The payload and its
  committed digests stay scope-invariant — relocation happens once, at write time — so no second
  payload was needed. This also removes the collisions that made the shared tree a hazard: a
  mis-shaped Antigravity or Codex crew sitting in `~/.agents/` is read by pi as a legacy agent
  location, where it outranks pi's own crew and yields an empty toolset (#437, #458).
- Reinstalling a relocated harness now clears the copies the old layout left behind, because a payload
  path a previous receipt owned and the new payload does not is removed. On a real machine this took
  `~/.agents/skills` from 34 entries to the captain's own 8, which also ends the skill-collision
  warning pi printed for every shipmates skill.
- The `doctor` shared-tree check scanned `.agents/agents/` non-recursively, so it could not see a
  foreign crew in Antigravity's actual shape — the nest was exactly what it needed to find. It now
  walks the tree the way the readers do.
- Two further defects found by verifying rather than trusting a green run: the tool payload was not
  relocated with the rest of a global install (it is a separate payload, and its files landed where
  the harness never looks), and `doctor` compared that payload against workspace-shaped paths and so
  reported every installed tool as orphaned.

## [0.6.1] - 2026-09-13

### Added

- **Rework elimination via shared preamble (`docs/COST.md`) and global steering (`steering/global.md`):** Extended the shared command preamble (`<!-- command-preamble:start -->`) and global steering (`steering/global.md`) with the rework cost clause ("Cost is seats × model plus rework"), automatically compiled into all 15 commands to eliminate rework without manual per-command copy-paste. Encoded five core guardrails for complex and multi-unit runs: machine-checkable per-unit owned-paths manifests replacing repeated prose scope fences and diffed against `git status` / `git diff --name-only`, citation verification (`grep` on `file:line` and counting claims) before design specs become binding, routing empirical questions to builders/code-executors rather than speculative design conditionals, plan-time blast-radius greps (`grep -rl`) for shared providers/APIs, and recording shared repo facts once at plan/recon time (#452).

## [0.6.0] - 2026-09-13

### Added

- **Model-pool discovery and spawn-time routing.** The orchestrator now resolves a spawn's model
  tier against a pool it can actually see, instead of falling back to `inherit` for want of one. A
  three-tier ladder — query the harness's documented enumeration command, then a declared pool, then
  `inherit` — is stated once in `docs/COST.md` (`## Model routing`) and expanded from the shared
  cost-discipline preamble into **every** command, so the ruleset is global rather than a two-command
  special case and no command can drift from it. The ladder
  records that enumeration answers *what exists*, never *what is cheap*, so a declared ranking is
  required even where the pool is enumerable, and an unknown or empty pool always produces a named
  `inherit` — never a guessed model name (#434).
- **A declared-pool shape the captain owns.** `model-pool.json` (project `.shipmates/`, then user
  `~/.shipmates/`) maps the neutral `mechanical` / `judgment` tiers to patterns the harness's own
  model surface accepts. Shipmates never writes a value into it and ships no model-name default,
  example or fallback (#434).
- **A `MODEL ROUTING:` audit line per spawn** in the run report, carrying the pool source, the
  identity the harness accepted, the effort requested and resolved, and `honoured` / `substituted` /
  `inherit` — so a model the harness silently substituted is visible as substituted (#434).
- **A per-harness model-surface record** in `tools/harness_matrix.json` (`model_surface`):
  enumeration command or an explicit "none", the model-identity scheme, the per-spawn vs static
  override, the effort surface with its clamp, and the declared-pool mechanism — every cell stated,
  with a `verified_on` date and a mechanical completeness guard in the test suite (#434).
- **ADR 0002 — discovering the available model pool before routing a tier to a model**, recording
  the verified per-harness evidence, the corrections it forced against the story's own table, and
  the six design answers behind the decision (#434).
- **Acceptance-board retry costs less and reports consistently.** A seat that accepted is carried when
  the fixer delta implements that seat's own finding — asking a reviewer to re-approve the change they
  requested is a predictable green — and a seat whose verdict came from running the gates is re-covered
  by re-running them rather than by re-seating. Scaled seats now damp by artifact rather than by flag
  count, so a change confined to one file cluster pulls at most one specialist beyond the mandatory
  PE+PO core, and the board's SDET seat is skipped outright when the pre-PR pass already covered the
  same tree and CI re-runs those gates. The retry report vocabulary is uniform (`re-run` / `carried
  ACCEPT` / `newly seated` / `still gated`), and the "accepted == what merges" guardrail is scoped to
  seated or re-run reviewers so it no longer contradicts a carried ACCEPT (#370 #372 #445).

## [0.5.0] - 2026-09-13

### Added

- **Intelligent harness auto-detection on `shipmates install`:** `shipmates install` without
  `--harness` now discovers which supported harnesses are actually installed on the system (via
  PATH binaries, home configuration directories, project markers, and install receipts) and
  configures an optimal native setup for each detected harness — native dialect, tool vocabulary,
  least-privilege permissions, and canonical user-scope steering in 1 command, 1 time, perfectly (#438).
- **Canonical user-scope steering (`steering/global.md`) installation:** `shipmates install` installs
  domain-neutral global heuristics into user-scope instruction files across supported harnesses:
  Tier A for Cursor (`~/.cursor/rules/shipmates.mdc`) and Tier B for Claude Code (`~/.claude/CLAUDE.md`),
  Codex (`~/.codex/AGENTS.md`), OpenCode (`~/.config/opencode/AGENTS.md`), Antigravity (`~/.gemini/GEMINI.md`),
  and Pi (`~/.pi/agent/AGENTS.md`) using `<!-- shipmates:global-steering -->` managed blocks with atomic
  writes and idempotency (#417, #430, #438).
- **`shipmates doctor` vocabulary and foreign agent checks:** Doctor now validates that every installed
  agent's declared tools resolve within that harness's first-party tool vocabulary, verifies required
  least-privilege keys (such as OpenCode's catch-all `*: deny`), flags foreign agent vocabularies in
  shared trees (`.agents/agents/`) that would cause silent zero-tool seats on Pi, and reports user-scope
  global steering status (#438).

## [0.4.1] - 2026-09-13

### Added

- **Pi receives the crew.** `shipmates install --harness pi` now installs the thirteen specialists as
  pi-native agents at `.pi/agents/<name>.md`, alongside the fifteen commands on the shared
  `.agents/skills/` tree. Pi's crew mechanic comes from the third-party `pi-subagents` extension rather
  than core pi, so that dependency is recorded in `tools/harness_matrix.json` and stated in the README
  instead of being glossed over. Pi's per-role reasoning effort is emitted as `thinking:` (#437).

### Fixed

- **Pi crew seats spawned with no tools at all, silently.** Pi reads the shared `.agents/agents/` tree
  as a *legacy* agent location, and its frontmatter reader is a line-based parser rather than a YAML
  loader — so Antigravity's YAML-list `tools:` was read as a single unmatchable tool name. On any
  machine where the Antigravity crew was installed, every pi crew seat resolved with an empty toolset:
  reviewers could not read the diff, builders could not edit, and nothing errored at discovery time.
  Pi's crew now ship to `.pi/agents/` in pi's own dialect, with `tools:` as the comma-separated scalar
  its parser expects — the path that outranks the legacy `.agents/` tree within whichever directory pi
  resolves as its project root, so a project-local install resolves correctly. A keyless regression
  test asserts the path, the scalar shape, and that pi never writes crew into the shared tree (#437).
  A `--global` install resolves only when no nearer ancestor carries `.pi/` or `.agents/`; that
  residual is tracked separately rather than papered over.
- `tools/harness_matrix.json` no longer claims Pi ships no subagents; the `agents` and `effort` flags
  are now enforced against the adapter's real output, and Pi's registry entry records its tool
  vocabulary and the `pi-web-access` dependency its `web` capability names (#437).

## [0.4.0] - 2026-09-13

### Added

- **`/ship-epic` fan-out execution mode (default):** Added `EPIC_EXECUTION=fanout` by default (`sequential` available as opt-in) and `MAX_CONCURRENT_WORKERS=5`, allowing independent, file-disjoint units to run concurrently in dedicated worktrees. Encodes wave-based DAG partitioning, in-flight rebase conflict handling, post-wave integration CI checks on `<EPIC_BRANCH>`, and single final acceptance board review over the integrated epic PR. A rebased unit waits for green CI on the post-rebase head before it merges, and shared-branch regressions are fixed through CI-gated fixer PRs rather than direct commits (#432).
- **Cross-command fan-out execution mode:** File-disjoint work units run concurrently up to `MAX_CONCURRENT_WORKERS` (5), with a `sequential` opt-out, across `/ship-issue`, `/ship-fix-bug`, `/ship-refactor`, `/ship-harden`, `/ship-migrate`, `/ship-document`, and `/ship-polish`. `/ship-issue`, `/ship-fix-bug`, `/ship-refactor`, and `/ship-migrate` accept the two shared acceptance-board delegation modes — `board=epic-deferred` (a deferral to a guaranteed milestone board, never a cancel) and `board=off` (explicit captain opt-out with no deferral target) — defined once in `docs/COST.md` (#432).
- **Canonical global steering ruleset (`steering/global.md`):** Authored the strictly domain-neutral steering content — repo-context precedence, routing across all 15 commands, the product impact bar, backlog/ticket hygiene, worktree, git and shell safety, multi-perspective acceptance, and execution efficiency / review amortization. The file is the payload for the user-scope install tracked in #417 and is **not yet wired into the installer** — that wiring (and its automated domain-neutrality gate) is tracked in #417 / #438 (#430).

## [0.3.1] - 2026-09-13

### Fixed

- `update` no longer removes a harness's optional tools when `--with-tools` is
  omitted. Native tool payloads (for example opencode's `.opencode/tools/*.ts`)
  now map back to their tool identity, so the documented "omit = keep what the
  receipt claims" contract holds, and the completion line reports the tool
  count and delta (#412).
- `doctor` no longer reports a clean "no optional tools installed" state over
  tool files a previous update removed leaving only installer backups; it names
  the removed tools and the repair command. A receipt-claimed tool file that is
  missing is a problem, not a shrug (#412).
- `update` now refreshes drifted files in the shared `.agents/skills/` tree
  (codex, antigravity, github-copilot). A co-owned path advances when its bytes
  match any claiming receipt's recorded digest, so the latest payload lands
  without waiting for `doctor --fix`; bytes that match no recorded digest — a
  captain's edit — are still preserved byte-identical with a warning (#428).

## [0.3.0] - 2026-09-13

### Added

- **Pi Agent harness support:** Added adapter to emit the 15 commands as skills to `.agents/skills/`.

## [0.2.1] - 2026-09-12

### Fixed

- Generated skill frontmatter is valid strict YAML: free-text scalars such as
  `description` are double-quoted and escaped at the serialization boundary, so
  a description like `Shipmates: …` no longer breaks the whole frontmatter
  block. Strict YAML loaders — including the one Cursor uses for skills — now
  accept every emitted skill, with no captain-side rewrite, and a cross-target
  integration test parses every emitted frontmatter block to keep it that way
  (#407).

## [0.2.0] - 2026-09-12

### Changed

- **Every command now carries the `ship-` prefix, and the catalogue is uniform.**
  `/ship-issue` and `/ship-epic` are joined by `/ship-fix-bug`, `/ship-harden`,
  `/ship-migrate`, `/ship-document`, `/ship-release`, `/ship-polish`,
  `/ship-spike`, `/ship-onboard`, `/ship-refactor`, `/ship-plan-epics`,
  `/ship-pr-review`, `/ship-report-bug`, and `/ship-consolidate-issues`.
  The four workflow names that previously had no prefix (`plan-epics`,
  `pr-review`, `report-bug`, `consolidate-issues`) and the nine that carried
  `shipmates-` all move. An install migrates existing trees automatically — a
  bare `polish` and a `shipmates-polish` both become `ship-polish`, with the
  old bytes backed up and receipts rewritten — and every renamed page on the
  site keeps a redirect. Tools keep their `shipmates-` names.

## [0.1.20] - 2026-09-12

### Fixed

- `install` reclaims a pre-prefix skill left by an older release when the file itself
  declares the matching identity, instead of requiring a receipt claim and leaving a
  duplicate slash command beside the `shipmates-` prefixed one. A third-party skill that
  merely sits at one of those names still fails closed and is left untouched (#403).
- `install` no longer refuses silently when the prefixed path already holds a foreign
  file; it reports the conflict and leaves both files in place (#403).
- `install --force` stopped warning that it left a file untouched when it went on to
  overwrite and claim it. The unmanaged scan now runs after the write loop and is keyed
  on what the run actually published (#404).
- `install` no longer reports its own `.bak-<secs>-<pid>-<n>` backups as unmanaged
  files. A user file such as `notes.md.bak-mine` is still reported (#404).
- `cursor` installs skills to its first-party `.cursor/skills/` tree, which is the only
  one the slash-command picker reads, so `/ship-issue` and friends now appear in Cursor.
  One copy, never two: shipping to `.agents/skills/` as well would list every command
  twice and pay for the payload twice. The shared tree is unchanged for `codex`,
  `antigravity` and `github-copilot` (#405).
- `doctor` gained a Hygiene check and no longer reports "All shipshape" over leftover
  install backups or emptied pre-prefix skill directories (#406).

### Added

- `doctor --fix` prunes recognised install backups and bak-only pre-prefix husk
  directories. It never touches a backup whose live file is missing or drifted, because
  that backup is the interrupted-install undo, and never touches a directory holding any
  user file or a skill outside the rename table (#406).

## [0.1.19] - 2026-09-01

### Fixed

- `/ship-epic` Stage 0.5 runs the identical-tip kickoff whenever `<EPIC_PR>` is unset —
  including when the integration branch already exists on the remote — and pins the empty
  commit to a dedicated worktree, not the captain's checkout (#397).
- `/ship-epic` Stage 3.5 surfaces `/shipmates-harden` from delegated unit records via a
  `HARDEN:` field on `EPIC_UNIT_RECORD` (#398).
- `/ship-issue` documents merge-mode behaviour in caller terms — explicit `MERGE_MODE=manual`
  from `/ship-epic` delegation, not `/ship-epic`-only guidance tokens (#399).

## [0.1.18] - 2026-09-01

### Fixed

- `/ship-epic` auto-merges green unit PRs into the epic branch even when a story is
  `IS_SECURITY_SENSITIVE`; the harden recommendation and the epic PR stay the human
  gate. Standalone `/ship-issue` still forces `MERGE_MODE=manual` for that flag (#382).
- `/ship-epic` Stage 0.5 writes an empty `chore: epic kickoff` commit when the epic
  branch tip still matches main, so the epic PR is always creatable (#383).

## [0.1.17] - 2026-09-01

### Fixed

- `/plan-epics` attaches each story to its epic as a GitHub **sub-issue** (the
  checklist stays as progress copy), `/ship-epic` reads story membership as the
  union of the sub-issue graph and the checklist, and `shipmates-gh` gains
  validated `issue.sub_issue_add` / `issue.sub_issue_list` /
  `issue.sub_issue_remove` ops (#388).

## [0.1.16] - 2026-09-01

### Fixed

- `install` no longer aborts on a lived-in harness root: the unmanaged-file scan
  is bounded to the payload's own subtrees, skips symlinks outright, and never
  walks `node_modules`. `--harness all` continues past a failed harness, prints a
  per-harness summary and exits non-zero (#384).
- A released binary installs the payload compiled into it. An on-disk `crew/` +
  `commands/` source is used only for the in-repo dev loop or when asked for with
  `--from-cwd` / `SHIPMATES_SRC=<dir>`; a stale checkout in the current directory
  gets one loud warning instead of silently shadowing the embed (#385).
- A Shipmates file at a payload path that no receipt claims is adopted — backed
  up, rewritten from the payload and claimed — by both `install` and
  `doctor --fix`, so a flagship can no longer stay stale across an upgrade. A
  file Shipmates does not own is refused instead, naming
  `shipmates install --force`; third-party skills beside ours are untouched (#386).

## [0.1.15] - 2026-09-01

### Changed

- Generic commands and every tool install as `shipmates-*` (`/shipmates-polish`,
  `shipmates-gh`, …). Flagships (`/ship-issue`, `/ship-epic`, `/plan-epics`,
  `/pr-review`, `/report-bug`, `/consolidate-issues`) keep their names. `install`
  and `doctor --fix` migrate owned old paths; `--no-migrate` skips the sweep
  (#373).

## [0.1.14] - 2026-09-01

### Changed

- After a board REJECT, retry re-selects seats from the fixer delta — failers must
  sit; prior ACCEPTs are carried unless the delta can invalidate them (#203).

## [0.1.13] - 2026-09-01

### Fixed

- `/ship-epic` and `/ship-issue` treat harness "end your turn" / backgrounded
  builders as in-flight work, not a hard-limit pause (#351).
- `doctor --fix` restores a missing payload file from a sibling
  `{name}.bak-<secs>-<pid>-<n>` left by an interrupted install (#352).

## [0.1.12] - 2026-08-31

### Changed

- Mutating commands document explicit fetch + `origin/<BASE>` sync and resume rebase behaviour (#325).
- `/ship-epic` convenes an integration acceptance board on the epic PR before captain merge (#324).

## [0.1.11] - 2026-08-31

### Changed

- Mutating commands default to nested git worktrees under `<repo>/.shipmates/worktrees/` with
  idempotent `.gitignore` hygiene; `worktree-root=sibling` restores legacy `../<repo>--…` paths (#322).

## [0.1.10] - 2026-08-31

### Changed

- Plain `shipmates install` now includes all bundled tools by default; use
  `--with-tools none` for crew-only installs (#320).

## [0.1.9] - 2026-08-31

### Changed

- `/ship-epic` always opens a captain-reviewable epic PR; removes `epic merge auto`;
  reconstructs integration when units mis-targeted the default branch; crew-complete
  still leaves the epic PR open for human merge (#318).

## [0.1.8] - 2026-08-31

### Changed

- `/ship-epic` no longer pauses on owner-only remainders (DNS, registrar, deploy
  console); crew-complete terminal report instead of `/ship-epic resume` (#315).

## [0.1.7] - 2026-08-31

### Added

- `/report-bug` command for structured upstream bug reports (#308, #311).
- `gh` toolbox tool — JSON-spec wrapper around GitHub CLI (#309, #312).

### Changed

- `/ship-epic` skip/resume gates for partial epics (#307, #310).
- `/ship-issue` now requires release version bumps in the same PR when work is
  release-affecting (`IS_RELEASE_AFFECTING`); `/pr-review` and `/release` scope
  updated accordingly (#313).
