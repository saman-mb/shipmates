#!/usr/bin/env bash
# cursor: agents directory reported but frontmatter schema unverified.
# windsurf: target identity under review since the Devin Desktop rename.
set -euo pipefail

cargo run -- install --harness cursor --dir "$RUNNER_TEMP/install-cursor"
# Cursor's slash picker only reads its first-party tree (#405).
test -f "$RUNNER_TEMP/install-cursor/.cursor/skills/ship-issue/SKILL.md"
test ! -d "$RUNNER_TEMP/install-cursor/.agents/skills"
test ! -d "$RUNNER_TEMP/install-cursor/.cursor/agents"
