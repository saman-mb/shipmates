#!/usr/bin/env bash
# Resolve the release context: tag, flags, publishing state, and binary-affected gate.
# Environment: GITHUB_EVENT_NAME, GITHUB_REF_TYPE, GITHUB_REF_NAME, GITHUB_SHA,
#              GH_TOKEN, GITHUB_OUTPUT, GITHUB_STEP_SUMMARY
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=release-paths.sh
# SC1091 is expected: lint-shell.sh runs shellcheck without -x over every script
# in this directory, and release-paths.sh is linted as an input of its own.
# shellcheck disable=SC1091
. "$SCRIPT_DIR/release-paths.sh"

if [[ "$GITHUB_EVENT_NAME" == "pull_request" ]]; then
  TAG=""
  FLAG=""
  PUBLISH="false"
elif [[ "$GITHUB_REF_TYPE" == "tag" ]]; then
  TAG="$GITHUB_REF_NAME"
  FLAG="--tag=${TAG} --force-tag"
  PUBLISH="true"
else
  VERSION="$(grep -m1 '^version' Cargo.toml | sed 's/.*= *"\(.*\)"/\1/')"
  TAG="v${VERSION}"
  if gh release view "${TAG}" >/dev/null 2>&1; then
    FLAG=""
    PUBLISH="false"
  else
    FLAG="--tag=${TAG} --force-tag"
    PUBLISH="true"
  fi
fi

# --- Path-filter gate: skip binary builds when only docs/site/tests change. ---
# PRs always run the full pipeline for CI feedback.
BINARY_AFFECTED="false"

if [[ "$GITHUB_EVENT_NAME" == "pull_request" ]]; then
  BINARY_AFFECTED="true"
else
  if [[ "$GITHUB_REF_TYPE" == "tag" ]]; then
    # Ensure tag history is available in the shallow clone for base resolution.
    git fetch --tags --depth=1 origin 2>/dev/null || true
    BASE_REF=$(git describe --tags --abbrev=0 "${GITHUB_SHA}^" 2>/dev/null || echo "")
  else
    BASE_REF="${GITHUB_EVENT_BEFORE:-}"
  fi

  if [[ -z "$BASE_REF" || "$BASE_REF" == "0000000000000000000000000000000000000000" ]]; then
    BINARY_AFFECTED="true"
  else
    # Ensure the base commit is available in the shallow clone.
    git fetch --depth=1 origin "$BASE_REF" 2>/dev/null || true
    if ! CHANGED_FILES=$(git diff --name-only "$BASE_REF" "$GITHUB_SHA" 2>/dev/null); then
      # Fail-open: if diff fails (e.g. unreachable base), run the full pipeline.
      BINARY_AFFECTED="true"
    else
      while IFS= read -r file; do
        # One definition of "release-affecting", shared with the pull-request gate
        # in check-release-bump.sh. Two globs for one rule drift apart silently,
        # and the drift surfaces as a change that is release-affecting to the
        # trigger but not to the gate — which is exactly the silent no-op below.
        if is_release_path "$file"; then
          BINARY_AFFECTED="true"
        fi
      done <<< "$CHANGED_FILES"
    fi
  fi
fi

{
  echo "tag=${TAG}"
  echo "tag-flag=${FLAG}"
  echo "publishing=${PUBLISH}"
  echo "binary-affected=${BINARY_AFFECTED}"
} >> "$GITHUB_OUTPUT"
echo "resolved tag=${TAG:-<none>} publishing=${PUBLISH} binary-affected=${BINARY_AFFECTED}"

# Released content reached `main` with nothing new to publish: the version was
# never bumped, so the release workflow finds a release already there and
# publishes nothing. Every check is green in that moment, which is the whole
# problem — the work is on `main` and no captain can install it. The PR gate
# normally stops this before it lands (and a `no-release` label lets it through
# on purpose); when it happens anyway, say it in the run rather than leaving it
# to be discovered by someone wondering why the tap never moved.
if [[ "$PUBLISH" == "false" && "$BINARY_AFFECTED" == "true" && "$GITHUB_EVENT_NAME" != "pull_request" ]]; then
  {
    echo "### Nothing published from this push"
    echo
    echo "Released content changed on \`main\`, but \`${TAG}\` already has a release, so"
    echo "this run published nothing and captains will not see the change. Bump"
    echo "\`Cargo.toml\` and add its \`CHANGELOG.md\` entry to ship it."
  } >> "${GITHUB_STEP_SUMMARY:-/dev/null}"
  echo "::notice::released content changed but ${TAG} is already published — nothing shipped; bump Cargo.toml to release it"
fi
