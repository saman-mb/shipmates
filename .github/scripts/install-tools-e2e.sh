#!/usr/bin/env bash
# Verify tool installation end to end across harnesses (--with-tools selection).
set -euo pipefail

# a plain install ships every bundled tool under shipmates-* (aliases still
# select the namespaced skill; old short names must not land as a second skill)
test -f "$RUNNER_TEMP/install/.claude/skills/shipmates-termgif/SKILL.md"
test -f "$RUNNER_TEMP/install/.claude/skills/shipmates-termgif/termgif.py"
grep -q "user-invocable: false" "$RUNNER_TEMP/install/.claude/skills/shipmates-termgif/SKILL.md"
test ! -e "$RUNNER_TEMP/install/.claude/skills/termgif"
# crew-only escape hatch
cargo run -- install --harness claude-code --with-tools none --dir "$RUNNER_TEMP/install-crew-only"
test ! -e "$RUNNER_TEMP/install-crew-only/.claude/skills/shipmates-termgif"
test ! -e "$RUNNER_TEMP/install-crew-only/.claude/skills/termgif"
cargo run -- install --harness claude-code --with-tools termgif --dir "$RUNNER_TEMP/install-tool"
test -f "$RUNNER_TEMP/install-tool/.claude/skills/shipmates-termgif/SKILL.md"
test -f "$RUNNER_TEMP/install-tool/.claude/skills/shipmates-termgif/termgif.py"
grep -q "user-invocable: false" "$RUNNER_TEMP/install-tool/.claude/skills/shipmates-termgif/SKILL.md"
test ! -e "$RUNNER_TEMP/install-tool/.claude/skills/termgif"
# a named tool that is not termgif also installs correctly (agent-only skill + its script)
cargo run -- install --harness claude-code --with-tools pixelart --dir "$RUNNER_TEMP/install-tool-px"
test -f "$RUNNER_TEMP/install-tool-px/.claude/skills/shipmates-pixelart/SKILL.md"
test -f "$RUNNER_TEMP/install-tool-px/.claude/skills/shipmates-pixelart/pixelart.py"
test ! -e "$RUNNER_TEMP/install-tool-px/.claude/skills/shipmates-termgif"   # only the requested tool
test ! -e "$RUNNER_TEMP/install-tool-px/.claude/skills/pixelart"
# comma-separated selection installs exactly those tools
cargo run -- install --harness claude-code --with-tools scrub,badge --dir "$RUNNER_TEMP/install-tool-multi"
test -d "$RUNNER_TEMP/install-tool-multi/.claude/skills/shipmates-scrub"
test -d "$RUNNER_TEMP/install-tool-multi/.claude/skills/shipmates-badge"
test ! -e "$RUNNER_TEMP/install-tool-multi/.claude/skills/shipmates-pixelart"
test ! -e "$RUNNER_TEMP/install-tool-multi/.claude/skills/scrub"
# opencode gets a native code tool per selected tool, not a skill.
# .ts is named after the skill; bundled .py keeps the original asset filename.
cargo run -- install --harness opencode --with-tools all --dir "$RUNNER_TEMP/install-tool-oc"
for t in shipmates-termgif shipmates-social-card shipmates-pixelart shipmates-diagram shipmates-svgflow shipmates-badge shipmates-sparkline shipmates-scrub shipmates-fixtures shipmates-domaincheck shipmates-gh; do
  test -f "$RUNNER_TEMP/install-tool-oc/.opencode/tools/$t.ts"
done
for script in termgif.py social_card.py pixelart.py diagram.py svgflow.py badge.py sparkline.py scrub.py fixtures.py domaincheck.py gh.py; do
  test -f "$RUNNER_TEMP/install-tool-oc/.opencode/tools/$script"
done
test ! -d "$RUNNER_TEMP/install-tool-oc/.opencode/skills"
# shared-tree harness (codex): --with-tools all lands every tool in .agents/skills, not .codex
cargo run -- install --harness codex --with-tools all --dir "$RUNNER_TEMP/install-tool-cx"
for t in shipmates-termgif shipmates-social-card shipmates-pixelart shipmates-diagram shipmates-svgflow shipmates-badge shipmates-sparkline shipmates-scrub shipmates-fixtures shipmates-domaincheck shipmates-gh; do
  test -f "$RUNNER_TEMP/install-tool-cx/.agents/skills/$t/SKILL.md"
done
test ! -d "$RUNNER_TEMP/install-tool-cx/.codex/skills"
