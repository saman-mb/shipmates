#!/usr/bin/env bash
# Remove granular dist-manifest files before creating the GitHub Release.
set -euo pipefail

rm -f artifacts/*-dist-manifest.json
