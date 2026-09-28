#!/usr/bin/env bash
# Build platform-specific artifacts with cargo-dist.
# Environment: TAG_FLAG, DIST_ARGS
set -euo pipefail

# TAG_FLAG and DIST_ARGS are space-separated flag lists (e.g. "--tag=... --force-tag"
# and "--artifacts=local --target=..."), so word splitting is intentional.
# shellcheck disable=SC2086
dist build ${TAG_FLAG} --print=linkage --output-format=json ${DIST_ARGS} > dist-manifest.json
echo "dist ran successfully"
