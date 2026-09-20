#!/usr/bin/env bash
# Build platform-specific artifacts with cargo-dist.
# Environment: TAG_FLAG, DIST_ARGS
set -euo pipefail

# DIST_ARGS is a space-separated flag list (e.g. "--artifacts=local --target=..."),
# so word splitting is the point — quoting it would pass one nonsense argument.
# shellcheck disable=SC2086
dist build "$TAG_FLAG" --print=linkage --output-format=json ${DIST_ARGS} > dist-manifest.json
echo "dist ran successfully"
