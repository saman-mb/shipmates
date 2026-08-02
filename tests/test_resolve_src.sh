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

# --- gemini (+ antigravity alias resolves to the same tree) ---
D="$WORK/gemini"
assert "gemini: install exits 0" install_to gemini "$D"
assert "gemini: skill under .gemini/skills" test -f "$D/.gemini/skills/ship-issue/SKILL.md"
assert "gemini: agent under .gemini/agents" test -f "$D/.gemini/agents/sdet.md"
assert "gemini: antigravity alias installs the gemini tree" install_to antigravity "$D"

# --- skill-only targets: skills only, no agent files ---
for pair in "codex:.codex" "cursor:.cursor" "github-copilot:.github" "windsurf:.windsurf" "zed:.zed"; do
  harness="${pair%%:*}"
  dirname="${pair##*:}"
  D="$WORK/$harness"
  assert "$harness: install exits 0" install_to "$harness" "$D"
  assert "$harness: skill under $dirname/skills" test -f "$D/$dirname/skills/ship-issue/SKILL.md"
  assert "$harness: no agent files emitted" test ! -d "$D/$dirname/agents"
done

# --- unknown target is refused, not silently ignored ---
assert "unknown target exits non-zero" bash -c "cd '$REPO' && ! cargo run --quiet -- install --harness nope --dir '$WORK/nope' 2>/dev/null"

# --- summary ---

echo
echo "passed: $PASS, failed: $FAIL"
[ "$FAIL" -eq 0 ]
