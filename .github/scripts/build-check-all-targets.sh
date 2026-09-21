#!/usr/bin/env bash
# Build and check every target payload on the minimum supported environment.
set -euo pipefail

for target in claude-code opencode antigravity codex cursor github-copilot pi grok-build devin; do
  cargo run -- check --target "$target"
  cargo run -- build --target "$target" --out "$RUNNER_TEMP/floor"
done
