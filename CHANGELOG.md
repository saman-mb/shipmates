# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versioning follows
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
