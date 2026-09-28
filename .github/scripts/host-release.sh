#!/usr/bin/env bash
# Upload artifacts and create the GitHub Release with cargo-dist.
# Environment: TAG_FLAG, GITHUB_OUTPUT
set -euo pipefail

# TAG_FLAG is a space-separated flag list ("--tag=... --force-tag" or empty),
# so word splitting is intentional.
# shellcheck disable=SC2086
dist host ${TAG_FLAG} --steps=upload --steps=release --output-format=json > dist-manifest.json
echo "artifacts uploaded and released successfully"
cat dist-manifest.json
echo "manifest=$(jq -c "." dist-manifest.json)" >> "$GITHUB_OUTPUT"
