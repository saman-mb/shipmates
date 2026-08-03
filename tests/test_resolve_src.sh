#!/usr/bin/env bash
#
# Regression test for the `shipmates install` command: it must drop the
# harness's own tree (`.claude/`, `.opencode/`, `.codex/`, …) at the target
# root, not the `harnesses/<target>/` container the build layout uses. The
# harness reads its tree from its own root, so installing the container would
# install nothing any harness loads.
#
# Also gates the documented harness surface: every advertised target installs,
# skill-only targets emit no agent files, and an unknown target is refused.
#
#   bash tests/test_resolve_src.sh
#
# Exit 0 = all passed, 1 = at least one failure.

set -uo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

PASS=0; FAIL=0
ok()  { PASS=$((PASS+1)); printf 'ok   %s\n' "$1"; }
bad() { FAIL=$((FAIL+1)); printf 'FAIL %s\n' "$1"; }

assert() {
  local desc="$1"; shift
  if "$@" >/dev/null 2>&1; then ok "$desc"; else bad "$desc"; fi
}

install_to() { # harness dest  -- runs the local CLI, not a stale installed copy
  local harness="$1" dest="$2"
  ( cd "$REPO" && cargo run --quiet -- install --harness "$harness" --dir "$dest" )
}

# --- claude-code: agents + skills land under .claude/ ---
D="$WORK/claude"
assert "claude-code: install exits 0" install_to claude-code "$D"
assert "claude-code: skill under .claude/skills" test -f "$D/.claude/skills/ship-issue/SKILL.md"
assert "claude-code: agent under .claude/agents" test -f "$D/.claude/agents/sdet.md"
assert "claude-code: no harnesses/ container leaks" test ! -d "$D/harnesses"

# --- opencode: commands + agents land under .opencode/ ---
D="$WORK/opencode"
assert "opencode: install exits 0" install_to opencode "$D"
assert "opencode: command under .opencode/commands" test -f "$D/.opencode/commands/ship-issue.md"
assert "opencode: agent under .opencode/agents" test -f "$D/.opencode/agents/sdet.md"

# --- antigravity: agents + skills land under .agents/ ---
D="$WORK/antigravity"
assert "antigravity: install exits 0" install_to antigravity "$D"
assert "antigravity: skill under .agents/skills" test -f "$D/.agents/skills/ship-issue/SKILL.md"
assert "antigravity: agent under .agents/agents" test -f "$D/.agents/agents/sdet.md"

# --- crew-bearing targets whose agent format is not Claude's ---
# Codex agents are TOML, not Markdown; Copilot needs the .agent.md double
# extension or the file is not discovered. Both are easy to regress into a
# plain <name>.md that installs cleanly and is silently never loaded.
D="$WORK/codex"
assert "codex: install exits 0" install_to codex "$D"
# Codex reads skills from the open Agent Skills standard (.agents/skills), NOT
# .codex/skills; only its crew are Codex-native (.codex/agents).
assert "codex: skill under .agents/skills" test -f "$D/.agents/skills/ship-issue/SKILL.md"
assert "codex: no skills under .codex" test ! -d "$D/.codex/skills"
assert "codex: agent is TOML under .codex/agents" test -f "$D/.codex/agents/sdet.toml"
assert "codex: agent is not markdown" test ! -f "$D/.codex/agents/sdet.md"

# Copilot reads Agent Skills from the open .agents/skills tree; only its crew
# are .github-native (.github/agents/*.agent.md).
D="$WORK/github-copilot"
assert "github-copilot: install exits 0" install_to github-copilot "$D"
assert "github-copilot: skill under .agents/skills" test -f "$D/.agents/skills/ship-issue/SKILL.md"
assert "github-copilot: no skills under .github" test ! -d "$D/.github/skills"
assert "github-copilot: agent uses .agent.md" test -f "$D/.github/agents/sdet.agent.md"
assert "github-copilot: bare .md is not emitted" test ! -f "$D/.github/agents/sdet.md"

# --- skill-only targets on the open Agent Skills tree: skills only, no crew ---
# cursor: reads .agents/skills natively (first-party peer of .cursor/skills).
# windsurf: keeps its canonical .windsurf/skills (.agents/skills is only a
#   secondary compat scan there — do not move it off its documented path).
# zed: genuinely has no agents directory — ACP, not files.
for pair in "cursor:.agents" "windsurf:.windsurf" "zed:.agents"; do
  harness="${pair%%:*}"
  dirname="${pair##*:}"
  D="$WORK/$harness"
  assert "$harness: install exits 0" install_to "$harness" "$D"
  assert "$harness: skill under $dirname/skills" test -f "$D/$dirname/skills/ship-issue/SKILL.md"
  assert "$harness: no agent files emitted" test ! -d "$D/$dirname/agents"
done
# Cursor and Zed read the open tree; neither writes its own harness dotdir.
assert "cursor: no .cursor tree" test ! -d "$WORK/cursor/.cursor"
assert "zed: no .zed tree" test ! -d "$WORK/zed/.zed"

# --- unknown target is refused, not silently ignored ---
assert "unknown target exits non-zero" bash -c "cd '$REPO' && ! cargo run --quiet -- install --harness nope --dir '$WORK/nope' 2>/dev/null"

# --- embedded sources: a project dir has no crew/ or commands/, so the CLI
# must fall back to the payload compiled into the binary by build.rs ---
EMBED="$WORK/embedded"
mkdir -p "$EMBED"
assert "embedded: install from empty cwd exits 0" bash -c "cd '$EMBED' && cargo run --quiet --manifest-path '$REPO/Cargo.toml' -- install --harness claude-code --dir '$EMBED'"
assert "embedded: skill from embedded payload" test -f "$EMBED/.claude/skills/ship-issue/SKILL.md"
assert "embedded: agent from embedded payload" test -f "$EMBED/.claude/agents/sdet.md"
assert "embedded: twelve skills emitted" test "$(ls "$EMBED/.claude/skills" | wc -l | tr -d ' ')" -eq 12
assert "embedded: twelve agents emitted" test "$(ls "$EMBED/.claude/agents" | wc -l | tr -d ' ')" -eq 12

# --- summary ---

echo
echo "passed: $PASS, failed: $FAIL"
[ "$FAIL" -eq 0 ]
