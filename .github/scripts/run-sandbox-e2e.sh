#!/usr/bin/env bash
# Run the sandbox CLI e2e suite with bats.
# Environment: RUNNER_TEMP, SHIPMATES_BIN
set -euo pipefail

mkdir -p "$RUNNER_TEMP/reports"
bats --print-output-on-failure --timing \
  --report-formatter junit --output "$RUNNER_TEMP/reports" \
  tests/e2e/cli
