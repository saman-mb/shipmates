#!/usr/bin/env bash
# The paths whose contents ship to a captain: compiled into the binary by
# build.rs, or deciding how that payload is built.
#
# Two decisions read this list — whether a push is worth building and publishing
# (resolve-release.sh) and whether a pull request had to bump the version
# (check-release-bump.sh). It lives here as one function so the two can never
# disagree about what "release-affecting" means: a gate and a trigger written
# against two globs drift apart silently, and the drift looks like a bug in
# whichever one fired first.
#
# Sourced, not executed.

is_release_path() {
  case "$1" in
    src/* | Cargo.toml | Cargo.lock | build.rs | crew/* | commands/* | toolbox/* | docs/COST.md | steering/* | dist-workspace.toml)
      return 0
      ;;
    *)
      return 1
      ;;
  esac
}
