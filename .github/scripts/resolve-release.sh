#!/usr/bin/env bash
# Resolve the release context: tag, flags, publishing state, and binary-affected gate.
# Environment: GITHUB_EVENT_NAME, GITHUB_REF_TYPE, GITHUB_REF_NAME, GITHUB_SHA,
#              GH_TOKEN, GITHUB_OUTPUT
set -euo pipefail

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
        case "$file" in
          src/*|Cargo.toml|Cargo.lock|build.rs|crew/*|commands/*|toolbox/*|docs/COST.md|steering/*|dist-workspace.toml)
            BINARY_AFFECTED="true" ;;
        esac
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
