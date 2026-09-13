# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versioning follows
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
