#!/usr/bin/env bash
# Check every target payload against its reference digest.
set -euo pipefail

for target in claude-code opencode antigravity codex cursor github-copilot pi windsurf; do
  cargo run -- check --target "$target"
done
