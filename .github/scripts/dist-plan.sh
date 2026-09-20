#!/usr/bin/env bash
# Run dist plan or dist host create, emitting a JSON manifest.
# Environment: PUBLISHING, TAG, GITHUB_OUTPUT
set -euo pipefail

if [ "$PUBLISHING" == "true" ]; then
  dist host --steps=create --tag="$TAG" --force-tag --output-format=json > plan-dist-manifest.json
else
  dist plan --output-format=json > plan-dist-manifest.json
fi
echo "dist ran successfully"
cat plan-dist-manifest.json
echo "manifest=$(jq -c "." plan-dist-manifest.json)" >> "$GITHUB_OUTPUT"
