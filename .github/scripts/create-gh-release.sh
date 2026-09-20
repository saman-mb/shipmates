#!/usr/bin/env bash
# Create the GitHub Release from artifacts.
# Environment: RELEASE_TAG, PRERELEASE_FLAG, ANNOUNCEMENT_TITLE, ANNOUNCEMENT_BODY
set -euo pipefail

# Write and read notes from a file to avoid quoting breaking things
echo "$ANNOUNCEMENT_BODY" > "$RUNNER_TEMP/notes.txt"

# PRERELEASE_FLAG is empty or "--prerelease" — it must expand to nothing when empty,
# so word splitting is the point.
# shellcheck disable=SC2086
gh release create "$RELEASE_TAG" --target "$GITHUB_SHA" \
  ${PRERELEASE_FLAG} --title "$ANNOUNCEMENT_TITLE" \
  --notes-file "$RUNNER_TEMP/notes.txt" artifacts/*
