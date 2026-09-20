#!/usr/bin/env bash
# Build global (platform-agnostic) artifacts with cargo-dist.
# Environment: TAG_FLAG, GITHUB_OUTPUT, BUILD_MANIFEST_NAME
set -euo pipefail

dist build "$TAG_FLAG" --output-format=json "--artifacts=global" > dist-manifest.json
echo "dist ran successfully"

{
  echo "paths<<EOF"
  jq --raw-output ".upload_files[]" dist-manifest.json
  echo "EOF"
} >> "$GITHUB_OUTPUT"

cp dist-manifest.json "$BUILD_MANIFEST_NAME"
