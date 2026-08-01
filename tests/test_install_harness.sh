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

# opencode's payload is the other layout: agents/*.md plus flat commands/*.md
# (its skills are model-invoked, and every Shipmates command is user-invoked
# only), so its expected counts come from its own payload.
OC_PAYLOAD="$WORK/payload-opencode"
python3 "$REPO/tools/export.py" build --target opencode --root "$REPO" --out "$OC_PAYLOAD" >/dev/null 2>&1
OC_TREE="$OC_PAYLOAD/harnesses/opencode"
N_OC_AGENTS=$(find "$OC_TREE/agents" -maxdepth 1 -name '*.md' 2>/dev/null | wc -l)
N_OC_COMMANDS=$(find "$OC_TREE/commands" -maxdepth 1 -name '*.md' 2>/dev/null | wc -l)

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

# --- 2b. opencode: the flat commands/ layout (project scope) ------------------

h=opencode
p="$WORK/per-$h"
run --harness "$h" --project "$p" >/dev/null 2>&1
root="$(project_root_for "$h")"
assert "$h: agents installed at $root/agents"      test -f "$p/$root/agents/architect.md"
assert "$h: all agents installed" \
  test "$N_OC_AGENTS" -eq "$(find "$p/$root/agents" -name '*.md' | wc -l)"
assert "$h: commands installed flat at $root/commands" test -f "$p/$root/commands/ship-issue.md"
assert "$h: all commands installed" \
  test "$N_OC_COMMANDS" -eq "$(find "$p/$root/commands" -maxdepth 1 -name '*.md' | wc -l)"
# The safety property: opencode skills are model-invoked and every Shipmates
# command is user-invoked only, so nothing of ours may land in skills/.
assert "$h: no skills/ dir created"                test ! -e "$p/$root/skills"
assert "$h: manifest at $root/shipmates"           test -f "$p/$root/shipmates/manifest"
assert "$h: manifest lists commands/, not skills/" \
  bash -c "grep -q '^file=commands/ship-issue.md ' '$p/$root/shipmates/manifest' && ! grep -q '^file=skills/' '$p/$root/shipmates/manifest'"

run --harness "$h" --project "$p" --uninstall >/dev/null 2>&1
assert "$h: uninstall removed commands/"  test ! -e "$p/$root/commands"
assert "$h: uninstall removed agents/"    test ! -e "$p/$root/agents"
assert "$h: uninstall removed manifest"   test ! -e "$p/$root/shipmates/manifest"

# --- 2c. harnesses with no adapter refuse without a payload -------------------

# The capability matrix refuses to build for harnesses with no user-invoked-only
# equivalent — so install.sh refuses too, with a clear message.
for h in github-copilot codex cursor gemini windsurf zed; do
  p="$WORK/per-$h"
  if run --harness "$h" --project "$p" >/dev/null 2>"$WORK/$h.err"; then
    bad "$h: refuses without payload (exits non-zero)"
  else
    ok  "$h: refuses without payload (exits non-zero)"
  fi
  assert "$h: refusal mentions exporter" grep -q "exporter failed\|not implemented" "$WORK/$h.err"
  assert "$h: nothing created" test ! -e "$p"
done

# --- 3. --harness all installs every buildable harness, skips the rest ----------

# 'all' means "every harness that can be built", so a harness with no adapter is
# skipped with a note rather than aborting the run. The distinction matters:
# ALL_HARNESSES lists opencode second-to-last, so aborting on the first
# adapterless harness (github-copilot, 2nd of 8) silently excluded it.
# Naming a harness explicitly still hard-fails — that is section 2c.
if run --harness all --project "$WORK/all" >/dev/null 2>"$WORK/all.err"; then
  ok  "all: succeeds, skipping harnesses without an adapter"
else
  bad "all: succeeds, skipping harnesses without an adapter"
fi
assert "all: claude-code installed" test -f "$WORK/all/.claude/skills/ship-issue/SKILL.md"
assert "all: opencode agents installed" test -f "$WORK/all/.opencode/agents/architect.md"
assert "all: opencode commands installed" test -f "$WORK/all/.opencode/commands/ship-issue.md"
assert "all: opencode has no skills/ dir" test ! -e "$WORK/all/.opencode/skills"
assert "all: adapterless harness left untouched" test ! -e "$WORK/all/.cursor"

# A harness WITH an adapter that fails to build must stop the run even under
# 'all' — otherwise a corrupt canonical tree (or a truncated curl|bash
# download) reports every harness as "no adapter yet" and exits 0 having
# installed nothing. "No adapter" is queried from the exporter, never inferred
# from a build failure.
BROKEN="$WORK/broken-repo"
cp -R "$REPO" "$BROKEN"
printf '{ broken' > "$BROKEN/canonical/manifest.json"
if bash "$BROKEN/install.sh" --harness all --project "$WORK/broken" >/dev/null 2>&1; then
  bad "all: a broken canonical tree fails the run"
else
  ok  "all: a broken canonical tree fails the run"
fi
assert "all: broken tree installs nothing" test ! -e "$WORK/broken/.claude"

# --dir pins one root, so two harnesses would overwrite each other's payload
# and orphan the first one's manifest entries. Refuse rather than corrupt.
if run --harness all --dir "$WORK/dircollide" >/dev/null 2>&1; then
  bad "all + --dir is refused"
else
  ok  "all + --dir is refused"
fi
assert "all + --dir creates nothing" test ! -e "$WORK/dircollide"


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

# claude-code and opencode are the harnesses with an adapter; the opencode
# uninstall is covered in 2b, this is the nested-layout half. A harness with
# no adapter would fail at the same payload gate.
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

# Only the harnesses with an adapter succeed; the rest refuse at the payload gate.
run --harness claude-code >/dev/null 2>&1
assert "global: claude-code → ~/.claude" test -f "$HOME/.claude/skills/ship-issue/SKILL.md"
run --harness opencode >/dev/null 2>&1
assert "global: opencode → ~/.config/opencode" test -f "$HOME/$(global_root_for opencode)/commands/ship-issue.md"
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
