# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versioning follows
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
