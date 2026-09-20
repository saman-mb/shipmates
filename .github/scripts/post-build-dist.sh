#!/usr/bin/env bash
# Parse built artifacts from dist manifest and stage them for upload.
# Environment: BUILD_MANIFEST_NAME, GITHUB_OUTPUT
set -euo pipefail

{
  echo "paths<<EOF"
  dist print-upload-files-from-manifest --manifest dist-manifest.json
  echo "EOF"
} >> "$GITHUB_OUTPUT"

cp dist-manifest.json "$BUILD_MANIFEST_NAME"
