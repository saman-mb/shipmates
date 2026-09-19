#!/usr/bin/env bash
# Verify the antigravity harness install lands skills and agents on the shared tree.
set -euo pipefail

cargo run -- install --harness antigravity --dir "$RUNNER_TEMP/install-antigravity"
test -f "$RUNNER_TEMP/install-antigravity/.agents/skills/ship-issue/SKILL.md"
# agy discovers `{workspace}/.agents/agents/{agent_name}/` and reads the
# `agent.md` inside it, so a directory per agent is the only shape it
# loads. A flat `<name>.md` installs cleanly and is silently never
# read, so assert the shape, not just presence.
test -f "$RUNNER_TEMP/install-antigravity/.agents/agents/sdet/agent.md"
test ! -f "$RUNNER_TEMP/install-antigravity/.agents/agents/sdet.md"
test ! -d "$RUNNER_TEMP/install-antigravity/.gemini"
