#!/usr/bin/env bash
#
# Tests for install.sh's --harness flag (#73/#93 bundle).
#
# Covers: default install (no --harness, byte-identical layout), each harness
# individually, --harness all, repeated flags, --project/--dir composition,
# per-harness --uninstall, per-harness overwrite backups, and unknown-harness
# rejection. Everything runs under a mktemp workdir with a fake $HOME, so the
# suite is CI-safe: no network (install.sh resolves its payload from the repo
# it lives in), no prompts, nothing touches the real home directory.
#
#   bash tests/test_install_harness.sh
#
# Exit 0 = all passed, 1 = at least one failure.

set -uo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
INSTALLER="$REPO/install.sh"

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

# Every install below runs against a fake home; global-scope harness roots
# resolve under it and nothing leaks to the real $HOME.
export HOME="$WORK/home"
mkdir -p "$HOME"
unset CLAUDE_CONFIG_DIR  # a user-set value would redirect the claude-code global root

PASS=0; FAIL=0
ok()  { PASS=$((PASS+1)); printf 'ok   %s\n' "$1"; }
bad() { FAIL=$((FAIL+1)); printf 'FAIL %s\n' "$1"; }

# assert <description> <cmd...> — passes when the command exits 0.
assert() {
  local desc="$1"; shift
  if "$@" >/dev/null 2>&1; then ok "$desc"; else bad "$desc"; fi
}

# Independent restatement of install.sh's harness root table — deliberately
# duplicated here so the tests verify the table instead of echoing it.
project_root_for() {
  case "$1" in
    claude-code)    printf '.claude' ;;
    github-copilot) printf '.github' ;;
    codex|zed)      printf '.agents' ;;
    cursor)         printf '.cursor' ;;
    gemini)         printf '.gemini' ;;
    windsurf)       printf '.windsurf' ;;
    opencode)       printf '.opencode' ;;
    *) return 1 ;;
  esac
}
global_root_for() {
  case "$1" in
    claude-code)    printf '.claude' ;;
    github-copilot) printf '.copilot' ;;
    codex|zed)      printf '.agents' ;;
    cursor)         printf '.cursor' ;;
    gemini)         printf '.gemini' ;;
    windsurf)       printf '.codeium/windsurf' ;;
    opencode)       printf '.config/opencode' ;;
    *) return 1 ;;
  esac
}

ALL="claude-code github-copilot codex cursor gemini windsurf zed opencode"

# Generate the canonical payload to count expected files (no committed harnesses/).
PAYLOAD="$WORK/payload"
python3 "$REPO/tools/export.py" build --target claude-code --root "$REPO" --out "$PAYLOAD" >/dev/null 2>&1
PAYLOAD_TREE="$PAYLOAD/harnesses/claude-code"
N_AGENTS=$(find "$PAYLOAD_TREE/agents" -maxdepth 1 -name '*.md' | wc -l)
N_SKILLS=$(find "$PAYLOAD_TREE/skills" -maxdepth 1 -mindepth 1 -type d | wc -l)

run() { bash "$INSTALLER" "$@"; }

# --- 1. default install (no --harness) is the pre-harness behaviour ---------

out="$(run --dir "$WORK/default" 2>&1)"
assert "default: agents/ installed"            test "$N_AGENTS" -eq "$(find "$WORK/default/agents" -name '*.md' | wc -l)"
assert "default: skills/ installed"            test "$N_SKILLS" -eq "$(find "$WORK/default/skills" -mindepth 1 -maxdepth 1 -type d | wc -l)"
assert "default: manifest written"             test -f "$WORK/default/shipmates/manifest"
if printf '%s' "$out" | grep -q '(harness:'; then
  bad "default: no harness tag in banner"
else
  ok  "default: no harness tag in banner"
fi

# --harness claude-code explicitly must produce the same tree as the default
# (manifests compared with the informational fields stripped).
run --harness claude-code --dir "$WORK/explicit-cc" >/dev/null 2>&1
assert "claude-code: same tree as default" diff -r --exclude=manifest "$WORK/default" "$WORK/explicit-cc"
norm_manifest() { grep -v -E '^(installed_at|version)=' "$1"; }
assert "claude-code: same manifest as default" \
  diff <(norm_manifest "$WORK/default/shipmates/manifest") <(norm_manifest "$WORK/explicit-cc/shipmates/manifest")

# --- 2. claude-code harness individually (project scope) ---------------------

h=claude-code
p="$WORK/per-$h"
run --harness "$h" --project "$p" >/dev/null 2>&1
root="$(project_root_for "$h")"
assert "$h: skills installed at $root/skills"    test -f "$p/$root/skills/ship-issue/SKILL.md"
assert "$h: manifest at $root/shipmates"         test -f "$p/$root/shipmates/manifest"
assert "$h: agents installed" test "$N_AGENTS" -eq "$(find "$p/$root/agents" -name '*.md' | wc -l)"

# --- 2b. non-claude-code harnesses refuse without a payload -------------------

# The capability matrix refuses to build for harnesses with no user-invoked-only
# equivalent — so install.sh refuses too, with a clear message.
for h in github-copilot codex cursor gemini windsurf zed opencode; do
  p="$WORK/per-$h"
  if run --harness "$h" --project "$p" >/dev/null 2>"$WORK/$h.err"; then
    bad "$h: refuses without payload (exits non-zero)"
  else
    ok  "$h: refuses without payload (exits non-zero)"
  fi
  assert "$h: refusal mentions exporter" grep -q "exporter failed\|not implemented" "$WORK/$h.err"
  assert "$h: nothing created" test ! -e "$p"
done

# --- 3. --harness all fails on first refused harness ----------------------------

# 'all' expands to every harness; the first refused one (github-copilot) stops
# the run before anything is installed — same trust posture as a single-target
# install.
if run --harness all --project "$WORK/all" >/dev/null 2>"$WORK/all.err"; then
  bad "all: fails on refused harness (exits non-zero)"
else
  ok  "all: fails on refused harness (exits non-zero)"
fi
assert "all: refusal mentions exporter" grep -q "exporter failed\|not implemented" "$WORK/all.err"

# --- 4. repeated --harness flags ----------------------------------------------

# First refused harness stops the run.
if run --harness cursor --harness codex --project "$WORK/repeat" >/dev/null 2>"$WORK/repeat.err"; then
  bad "repeat: fails on refused harness (exits non-zero)"
else
  ok  "repeat: fails on refused harness (exits non-zero)"
fi
assert "repeat: refusal mentions exporter" grep -q "exporter failed\|not implemented" "$WORK/repeat.err"

# --- 5. --project + --harness compose -----------------------------------------

# Refused harness fails before touching the project path.
if run --project "$WORK/compose" --harness gemini >/dev/null 2>"$WORK/compose.err"; then
  bad "compose: fails on refused harness (exits non-zero)"
else
  ok  "compose: fails on refused harness (exits non-zero)"
fi
assert "compose: refusal mentions exporter" grep -q "exporter failed\|not implemented" "$WORK/compose.err"

# --dir pins the root exactly; the harness only governs the agents skip rule.
# Refused harness fails before touching the --dir path.
if run --harness cursor --dir "$WORK/dir-cursor" >/dev/null 2>"$WORK/dir-cursor.err"; then
  bad "compose --dir: fails on refused harness (exits non-zero)"
else
  ok  "compose --dir: fails on refused harness (exits non-zero)"
fi
assert "compose --dir: refusal mentions exporter" grep -q "exporter failed\|not implemented" "$WORK/dir-cursor.err"

# --- 6. --uninstall per harness (claude-code only for now) -------------------

# Only claude-code installs succeed today; uninstall tests use it as the
# working harness. Non-claude-code uninstall would fail at the same gate.
p="$WORK/uninst"
run --harness claude-code --project "$p" >/dev/null 2>&1
run --harness claude-code --project "$p" --uninstall >/dev/null 2>&1
assert "uninstall: skills removed"   test ! -d "$p/.claude/skills"
assert "uninstall: manifest removed" test ! -e "$p/.claude/shipmates/manifest"

# --- 7. overwrite backup (claude-code) -----------------------------------------

p="$WORK/backup"
run --harness claude-code --project "$p" >/dev/null 2>&1
edited="$p/.claude/skills/ship-issue/SKILL.md"
printf '# hand edit\n' >> "$edited"
run --harness claude-code --project "$p" >/dev/null 2>&1
assert "backup: .bak-* kept for hand edit" bash -c "ls '$edited'.bak-* >/dev/null 2>&1"
# Payload is generated fresh each install; verify the installed file matches the golden output.
assert "backup: payload restored over edit" diff -q "$PAYLOAD_TREE/skills/ship-issue/SKILL.md" "$edited"

# --- 8. unknown harness errors non-zero, touches nothing -----------------------

if run --harness bogus --dir "$WORK/bogus" >/dev/null 2>"$WORK/bogus.err"; then
  bad "unknown harness: exits non-zero"
else
  ok  "unknown harness: exits non-zero"
fi
assert "unknown harness: error message" grep -q "unknown harness 'bogus'" "$WORK/bogus.err"
assert "unknown harness: nothing created" test ! -e "$WORK/bogus"

# --- 9. global roots land under the right per-harness homes -------------------

# Only claude-code succeeds; the rest refuse at the payload gate.
run --harness claude-code >/dev/null 2>&1
assert "global: claude-code → ~/.claude" test -f "$HOME/.claude/skills/ship-issue/SKILL.md"
if run --harness github-copilot >/dev/null 2>"$WORK/global-copilot.err"; then
  bad "global: github-copilot refuses (exits non-zero)"
else
  ok  "global: github-copilot refuses (exits non-zero)"
fi
assert "global: refusal mentions exporter" grep -q "exporter failed\|not implemented" "$WORK/global-copilot.err"

# --- summary -------------------------------------------------------------------

echo
echo "passed: $PASS, failed: $FAIL"
[ "$FAIL" -eq 0 ]
