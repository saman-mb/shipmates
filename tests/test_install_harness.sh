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
N_AGENTS=$(find "$REPO/agents" -maxdepth 1 -name '*.md' | wc -l)
N_SKILLS=$(find "$REPO/skills" -maxdepth 1 -mindepth 1 -type d | wc -l)

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

# --- 2. each harness individually (project scope) -----------------------------

for h in $ALL; do
  p="$WORK/per-$h"
  run --harness "$h" --project "$p" >/dev/null 2>&1
  root="$(project_root_for "$h")"
  assert "$h: skills installed at $root/skills"    test -f "$p/$root/skills/ship-issue/SKILL.md"
  assert "$h: manifest at $root/shipmates"         test -f "$p/$root/shipmates/manifest"
  if [ "$h" = "claude-code" ]; then
    assert "$h: agents installed" test "$N_AGENTS" -eq "$(find "$p/$root/agents" -name '*.md' | wc -l)"
  else
    assert "$h: agents skipped" test ! -d "$p/$root/agents"
  fi
done

# --- 3. --harness all ---------------------------------------------------------

run --harness all --project "$WORK/all" >/dev/null 2>&1
for h in $ALL; do
  root="$(project_root_for "$h")"
  assert "all: $h root present" test -f "$WORK/all/$root/skills/ship-issue/SKILL.md"
done

# --- 4. repeated --harness flags ----------------------------------------------

run --harness cursor --harness codex --project "$WORK/repeat" >/dev/null 2>&1
assert "repeat: cursor installed"   test -f "$WORK/repeat/.cursor/skills/ship-issue/SKILL.md"
assert "repeat: codex installed"    test -f "$WORK/repeat/.agents/skills/ship-issue/SKILL.md"
assert "repeat: gemini NOT installed" test ! -d "$WORK/repeat/.gemini"

# --- 5. --project + --harness compose -----------------------------------------

run --project "$WORK/compose" --harness gemini >/dev/null 2>&1
assert "compose: gemini under project path" test -f "$WORK/compose/.gemini/skills/ship-issue/SKILL.md"
assert "compose: no .claude created"        test ! -d "$WORK/compose/.claude"

# --dir pins the root exactly; the harness only governs the agents skip rule.
run --harness cursor --dir "$WORK/dir-cursor" >/dev/null 2>&1
assert "compose --dir: skills at explicit root" test -f "$WORK/dir-cursor/skills/ship-issue/SKILL.md"
assert "compose --dir: agents skipped for cursor" test ! -d "$WORK/dir-cursor/agents"

# --- 6. --uninstall per harness ------------------------------------------------

p="$WORK/uninst"
run --harness cursor --project "$p" >/dev/null 2>&1
run --harness cursor --project "$p" --uninstall >/dev/null 2>&1
assert "uninstall: skills removed"   test ! -d "$p/.cursor/skills"
assert "uninstall: manifest removed" test ! -e "$p/.cursor/shipmates/manifest"

# Uninstalling one harness leaves a sibling harness untouched.
p="$WORK/uninst-sibling"
run --harness cursor --harness gemini --project "$p" >/dev/null 2>&1
run --harness gemini --project "$p" --uninstall >/dev/null 2>&1
assert "uninstall: gemini removed"        test ! -d "$p/.gemini/skills"
assert "uninstall: cursor left intact"    test -f "$p/.cursor/skills/ship-issue/SKILL.md"

# --- 7. overwrite backup per harness -------------------------------------------

p="$WORK/backup"
run --harness gemini --project "$p" >/dev/null 2>&1
edited="$p/.gemini/skills/ship-issue/SKILL.md"
printf '# hand edit\n' >> "$edited"
run --harness gemini --project "$p" >/dev/null 2>&1
assert "backup: .bak-* kept for hand edit" bash -c "ls '$edited'.bak-* >/dev/null 2>&1"
assert "backup: payload restored over edit" cmp -s "$REPO/skills/ship-issue/SKILL.md" "$edited"

# --- 8. unknown harness errors non-zero, touches nothing -----------------------

if run --harness bogus --dir "$WORK/bogus" >/dev/null 2>"$WORK/bogus.err"; then
  bad "unknown harness: exits non-zero"
else
  ok  "unknown harness: exits non-zero"
fi
assert "unknown harness: error message" grep -q "unknown harness 'bogus'" "$WORK/bogus.err"
assert "unknown harness: nothing created" test ! -e "$WORK/bogus"

# --- 9. global roots land under the right per-harness homes -------------------

run --harness all >/dev/null 2>&1
for h in $ALL; do
  root="$(global_root_for "$h")"
  assert "global: $h → ~/$root" test -f "$HOME/$root/skills/ship-issue/SKILL.md"
done

# --- summary -------------------------------------------------------------------

echo
echo "passed: $PASS, failed: $FAIL"
[ "$FAIL" -eq 0 ]
