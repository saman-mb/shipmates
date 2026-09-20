#!/usr/bin/env bash
# Verify the opencode harness install lands commands and agents.
set -euo pipefail

cargo run -- install --harness opencode --dir "$RUNNER_TEMP/install-opencode"
test -f "$RUNNER_TEMP/install-opencode/.opencode/commands/ship-issue.md"
test -f "$RUNNER_TEMP/install-opencode/.opencode/agents/sdet.md"
