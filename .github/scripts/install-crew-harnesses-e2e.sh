#!/usr/bin/env bash
# Codex agents are TOML and Copilot needs the .agent.md double extension.
# Both are easy to regress into a plain <name>.md that installs cleanly
# and is silently never loaded, so assert the format, not just presence.
set -euo pipefail

cargo run -- install --harness codex --dir "$RUNNER_TEMP/install-codex"
# skills follow the open Agent Skills standard (.agents/skills), crew stay Codex-native (.codex/agents)
test -f "$RUNNER_TEMP/install-codex/.agents/skills/shipmates-issue/SKILL.md"
test ! -d "$RUNNER_TEMP/install-codex/.codex/skills"
test -f "$RUNNER_TEMP/install-codex/.codex/agents/sdet.toml"
test ! -f "$RUNNER_TEMP/install-codex/.codex/agents/sdet.md"
cargo run -- install --harness github-copilot --dir "$RUNNER_TEMP/install-copilot"
# Copilot skills follow the open standard (.agents/skills); crew stay .github-native
test -f "$RUNNER_TEMP/install-copilot/.agents/skills/shipmates-issue/SKILL.md"
test ! -d "$RUNNER_TEMP/install-copilot/.github/skills"
test -f "$RUNNER_TEMP/install-copilot/.github/agents/sdet.agent.md"
test ! -f "$RUNNER_TEMP/install-copilot/.github/agents/sdet.md"
