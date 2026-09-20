#!/usr/bin/env bash
# Verify a claude-code install lands crew agents and skills end to end.
set -euo pipefail

cargo run -- install --harness claude-code --dir "$RUNNER_TEMP/install"
test -f "$RUNNER_TEMP/install/.claude/skills/ship-issue/SKILL.md"
test -f "$RUNNER_TEMP/install/.claude/agents/sdet.md"
