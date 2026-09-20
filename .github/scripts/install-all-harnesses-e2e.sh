#!/usr/bin/env bash
# Verify a multi-harness install (--harness all) lands every tree correctly under one root.
set -euo pipefail

cargo run -- install --harness all --dir "$RUNNER_TEMP/install-all"
# crew-bearing target and a skill-only target both land under one root
test -f "$RUNNER_TEMP/install-all/.claude/agents/sdet.md"
test -f "$RUNNER_TEMP/install-all/.agents/skills/shipmates-issue/SKILL.md"
# grok-build is crew-bearing on its own tree, not the shared one
test -f "$RUNNER_TEMP/install-all/.grok/skills/shipmates-issue/SKILL.md"
test -f "$RUNNER_TEMP/install-all/.grok/agents/sdet.md"
# The four harnesses on the open tree share ONE .agents/skills rendering,
# so a multi-harness install must NOT collide: canonical prose names no
# harness-specific crew tree at all. It cannot — one shared file is
# rendered for four harnesses whose crews live in four different trees,
# so naming one is wrong for three of them (it was `.agents/agents/*.md`,
# correct only for antigravity). Assert absence, not shape.
if grep -qE '\.(agents|codex|cursor|github|pi)/agents' "$RUNNER_TEMP/install-all/.agents/skills/shipmates-issue/SKILL.md"; then
  echo "FAIL: shared skills tree names a harness-specific crew tree" >&2
  exit 1
fi
grep -q 'shipped crew role' "$RUNNER_TEMP/install-all/.agents/skills/shipmates-issue/SKILL.md"
# cursor owns its own tree for slash discovery (#405); the other
# shared harnesses write no private skills tree.
test -d "$RUNNER_TEMP/install-all/.cursor/skills"
test ! -d "$RUNNER_TEMP/install-all/.codex/skills"
test ! -d "$RUNNER_TEMP/install-all/.github/skills"
