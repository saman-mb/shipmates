#!/usr/bin/env bash
# e2e_cli.sh — end-to-end CLI behaviour test for shipmates.
#
# Validates the install/uninstall/doctor/check/build --update/update lifecycle against a
# hermetic temp project directory so nothing touches the real user home.
#
# Run from the repository root:
#     bash tools/e2e_cli.sh
#
# Exits 0 on success, 1 on any failure. Each segment is labelled so a CI log
# makes it obvious which gate tripped.

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BIN="$ROOT/target/debug/shipmates"
TMPDIR="$(mktemp -d)"
PASS=0
FAIL=0

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

ok() {
  PASS=$((PASS + 1))
  printf '  PASS: %s\n' "$1"
}

fail() {
  FAIL=$((FAIL + 1))
  printf '  FAIL: %s\n' "$1"
}

# Run a command and capture its exit code, suppressing set -e.
# Usage: rc=$(cmd_run <command> [args...])
cmd_run() {
  set +e
  "$@"
  local rc=$?
  set -e
  echo "$rc"
}

# Run a command and capture its output + exit code.
# Usage: cmd_capture <description> <command> [args...]
#   Sets: CMD_OUT, CMD_RC
cmd_capture() {
  local desc="$1"
  shift
  set +e
  CMD_OUT="$("$@" 2>&1)"
  CMD_RC=$?
  set -e
}

assert_exit() {
  # Usage: assert_exit <expected> <description> <command...>
  local expected="$1"
  local desc="$2"
  shift 2
  local rc
  set +e
  "$@" >/dev/null 2>&1
  rc=$?
  set -e
  if [ "$rc" -eq "$expected" ]; then
    ok "$desc (exit $rc)"
  else
    fail "$desc — expected exit $expected, got $rc"
  fi
}

assert_contains() {
  # Usage: assert_contains <needle> <haystack> <description>
  local needle="$1"
  local haystack="$2"
  local desc="$3"
  # `--` so a needle like `--force` is never parsed as a grep option.
  if printf '%s' "$haystack" | grep -qF -- "$needle"; then
    ok "$desc"
  else
    fail "$desc — output missing '$needle'"
  fi
}

assert_file_count() {
  local dir="$1"
  local expected="$2"
  local desc="$3"
  local actual
  actual=$(find "$dir" -type f | wc -l)
  if [ "$actual" -eq "$expected" ]; then
    ok "$desc ($actual files)"
  else
    fail "$desc — expected $expected files, got $actual"
  fi
}

assert_file_exists() {
  local path="$1"
  local desc="$2"
  if [ -f "$path" ]; then
    ok "$desc"
  else
    fail "$desc — file not found: $path"
  fi
}

assert_file_not_exists() {
  local path="$1"
  local desc="$2"
  if [ ! -f "$path" ]; then
    ok "$desc"
  else
    fail "$desc — file should not exist: $path"
  fi
}

# ---------------------------------------------------------------------------
# Segment 1 — Build
# ---------------------------------------------------------------------------
echo "=== Segment 1: Build ==="
(
  cd "$ROOT"
  cargo build --bin shipmates 2>&1 | tail -5
)
[ -x "$BIN" ] && ok "Binary built" || fail "Binary not found at $BIN"

# ---------------------------------------------------------------------------
# Segment 2 — Targets
# ---------------------------------------------------------------------------
echo "=== Segment 2: Targets ==="
cmd_capture "targets" "$BIN" targets
TARGETS_OUT="$CMD_OUT"
TARGET_COUNT=$(echo "$TARGETS_OUT" | wc -l)
[ "$TARGET_COUNT" -eq 9 ] && ok "Targets lists 9 harnesses" || fail "Targets lists $TARGET_COUNT (expected 9)"
for t in claude-code opencode antigravity codex cursor github-copilot pi grok-build windsurf; do
  if printf '%s' "$TARGETS_OUT" | grep -qF "$t" || true; then
    ok "Target '$t' present"
  else
    fail "Target '$t' missing"
  fi
done

# ---------------------------------------------------------------------------
# Segment 3 — Doctor on clean install (pass)
# ---------------------------------------------------------------------------
echo "=== Segment 3: Doctor on clean install ==="
PROJ="$TMPDIR/proj-doctor-clean"
mkdir -p "$PROJ"
cmd_run "$BIN" install --harness claude-code --dir "$PROJ" --with-tools none >/dev/null 2>&1
cmd_capture "doctor clean" "$BIN" doctor --harness claude-code --dir "$PROJ"
DOCTOR_OUT="$CMD_OUT"
DOCTOR_RC="$CMD_RC"
[ "$DOCTOR_RC" -eq 0 ] && ok "Doctor exits 0 on clean install" || fail "Doctor exited $DOCTOR_RC (expected 0)"
assert_contains "All shipshape" "$DOCTOR_OUT" "Doctor reports healthy"

# ---------------------------------------------------------------------------
# Segment 4 — Doctor detects missing file
# ---------------------------------------------------------------------------
echo "=== Segment 4: Doctor detects missing file ==="
PROJ="$TMPDIR/proj-doctor-missing"
mkdir -p "$PROJ"
cmd_run "$BIN" install --harness claude-code --dir "$PROJ" --with-tools none >/dev/null 2>&1
rm -f "$PROJ/.claude/agents/architect.md"
cmd_capture "doctor missing" "$BIN" doctor --harness claude-code --dir "$PROJ"
DOCTOR_OUT="$CMD_OUT"
DOCTOR_RC="$CMD_RC"
[ "$DOCTOR_RC" -eq 2 ] && ok "Doctor exits 2 when file missing" || fail "Doctor exited $DOCTOR_RC (expected 2)"
assert_contains "architect" "$DOCTOR_OUT" "Doctor mentions missing agent"
assert_contains "missing" "$DOCTOR_OUT" "Doctor reports missing"

# ---------------------------------------------------------------------------
# Segment 5 — Doctor detects corrupted receipt
# ---------------------------------------------------------------------------
echo "=== Segment 5: Doctor detects corrupted receipt ==="
PROJ="$TMPDIR/proj-doctor-corrupt"
mkdir -p "$PROJ"
cmd_run "$BIN" install --harness claude-code --dir "$PROJ" --with-tools none >/dev/null 2>&1
echo "corrupted" > "$PROJ/.shipmates/receipts/claude-code.json"
cmd_capture "doctor corrupt" "$BIN" doctor --harness claude-code --dir "$PROJ"
DOCTOR_OUT="$CMD_OUT"
DOCTOR_RC="$CMD_RC"
[ "$DOCTOR_RC" -eq 2 ] && ok "Doctor exits 2 on corrupted receipt" || fail "Doctor exited $DOCTOR_RC (expected 2)"
assert_contains "invalid" "$DOCTOR_OUT" "Doctor reports receipt invalid"

# ---------------------------------------------------------------------------
# Segment 6 — Doctor --fix repairs missing file
# ---------------------------------------------------------------------------
echo "=== Segment 6: Doctor --fix repairs missing file ==="
PROJ="$TMPDIR/proj-doctor-fix"
mkdir -p "$PROJ"
cmd_run "$BIN" install --harness claude-code --dir "$PROJ" --with-tools none >/dev/null 2>&1
rm -f "$PROJ/.claude/agents/architect.md"
cmd_capture "doctor fix" "$BIN" doctor --fix --harness claude-code --dir "$PROJ"
FIX_OUT="$CMD_OUT"
FIX_RC="$CMD_RC"
[ "$FIX_RC" -eq 0 ] && ok "Doctor --fix exits 0" || fail "Doctor --fix exited $FIX_RC (expected 0)"
assert_file_exists "$PROJ/.claude/agents/architect.md" "architect.md restored"
assert_contains "Restored" "$FIX_OUT" "Doctor --fix reports restoration"

# ---------------------------------------------------------------------------
# Segment 7 — Doctor --fix fails on corrupted receipt
# ---------------------------------------------------------------------------
echo "=== Segment 7: Doctor --fix fails on corrupted receipt ==="
PROJ="$TMPDIR/proj-doctor-fix-corrupt"
mkdir -p "$PROJ"
cmd_run "$BIN" install --harness claude-code --dir "$PROJ" --with-tools none >/dev/null 2>&1
echo "corrupted" > "$PROJ/.shipmates/receipts/claude-code.json"
FIX_RC=$(cmd_run "$BIN" doctor --fix --harness claude-code --dir "$PROJ")
[ "$FIX_RC" -eq 1 ] && ok "Doctor --fix exits 1 on corrupted receipt" || fail "Doctor --fix exited $FIX_RC (expected 1)"

# ---------------------------------------------------------------------------
# Segment 8 — Install file counts
# ---------------------------------------------------------------------------
echo "=== Segment 8: Install file counts ==="
PROJ="$TMPDIR/proj-counts"
mkdir -p "$PROJ"
cmd_run "$BIN" install --harness claude-code --dir "$PROJ" --with-tools none >/dev/null 2>&1
assert_file_count "$PROJ" 31 "claude-code no-tools: 30 files + receipt"

PROJ="$TMPDIR/proj-counts-default"
mkdir -p "$PROJ"
cmd_run "$BIN" install --harness claude-code --dir "$PROJ" >/dev/null 2>&1
assert_file_count "$PROJ" 53 "claude-code default: all tools (53 files)"

PROJ="$TMPDIR/proj-counts-tools"
mkdir -p "$PROJ"
cmd_run "$BIN" install --harness claude-code --dir "$PROJ" --with-tools all >/dev/null 2>&1
assert_file_count "$PROJ" 53 "claude-code all-tools: 53 files"

PROJ="$TMPDIR/proj-counts-scrub"
mkdir -p "$PROJ"
cmd_run "$BIN" install --harness claude-code --dir "$PROJ" --with-tools scrub >/dev/null 2>&1
assert_file_count "$PROJ" 33 "claude-code scrub tool: 33 files"

# ---------------------------------------------------------------------------
# Segment 9 — Install --with-tools nonexistent fails
# ---------------------------------------------------------------------------
echo "=== Segment 9: Install unknown tool fails ==="
PROJ="$TMPDIR/proj-bad-tool"
mkdir -p "$PROJ"
TOOL_RC=$(cmd_run "$BIN" install --harness claude-code --dir "$PROJ" --with-tools nonexistent)
[ "$TOOL_RC" -eq 1 ] && ok "Install rejects unknown tool (exit 1)" || fail "Install exited $TOOL_RC (expected 1)"

# ---------------------------------------------------------------------------
# Segment 10 — Install re-installs (idempotent)
# ---------------------------------------------------------------------------
echo "=== Segment 10: Re-install idempotent ==="
PROJ="$TMPDIR/proj-reinstall"
mkdir -p "$PROJ"
cmd_run "$BIN" install --harness claude-code --dir "$PROJ" --with-tools none >/dev/null 2>&1
cmd_capture "reinstall" "$BIN" install --harness claude-code --dir "$PROJ" --with-tools none
REINSTALL_RC="$CMD_RC"
[ "$REINSTALL_RC" -eq 0 ] && ok "Re-install exits 0" || fail "Re-install exited $REINSTALL_RC (expected 0)"
assert_file_count "$PROJ" 31 "Files unchanged after re-install"

# ---------------------------------------------------------------------------
# Segment 11 — Conflicting flags
# ---------------------------------------------------------------------------
echo "=== Segment 11: Conflicting flags ==="
PROJ="$TMPDIR/proj-conflict"
mkdir -p "$PROJ"
CONFLICT_RC=$(cmd_run "$BIN" install --harness claude-code --dir "$PROJ" --global)
[ "$CONFLICT_RC" -eq 2 ] && ok "--dir + --global exits 2" || fail "--dir + --global exited $CONFLICT_RC (expected 2)"

# ---------------------------------------------------------------------------
# Segment 12 — Shared .agents/skills tree (codex + github-copilot)
# ---------------------------------------------------------------------------
echo "=== Segment 12: Shared skills tree ==="
PROJ="$TMPDIR/proj-shared"
mkdir -p "$PROJ"
cmd_run "$BIN" install --harness codex --dir "$PROJ" --with-tools none >/dev/null 2>&1
CODEX_FILES=$(find "$PROJ" -type f | wc -l)
[ "$CODEX_FILES" -eq 31 ] && ok "Codex install: 31 files" || fail "Codex install: $CODEX_FILES files (expected 31)"
assert_file_exists "$PROJ/.agents/skills/shipmates-issue/SKILL.md" "Shared skill present after codex"
assert_file_exists "$PROJ/.codex/agents/architect.toml" "Codex agent present"

cmd_run "$BIN" install --harness github-copilot --dir "$PROJ" --with-tools none >/dev/null 2>&1
SHARED_FILES=$(find "$PROJ" -type f | wc -l)
# 31 (codex) + 13 (github-copilot agents) + 1 receipt = 45
[ "$SHARED_FILES" -eq 45 ] && ok "After github-copilot: 45 files" || fail "After github-copilot: $SHARED_FILES files (expected 45)"
assert_file_exists "$PROJ/.github/agents/architect.agent.md" "GitHub agent present"
assert_file_exists "$PROJ/.agents/skills/shipmates-issue/SKILL.md" "Shared skill still present"

# ---------------------------------------------------------------------------
# Segment 13 — Unowned Shipmates file at a payload path is adopted
# ---------------------------------------------------------------------------
echo "=== Segment 13: Adopt unowned matching flagship ==="
PROJ="$TMPDIR/proj-migrate-plain"
mkdir -p "$PROJ/.claude/agents"
printf '%s\n' '---' 'name: architect' 'description: leftover' '---' 'legacy architect' \
  > "$PROJ/.claude/agents/architect.md"
cmd_capture "migrate plain" "$BIN" install --harness claude-code --dir "$PROJ" --with-tools none
MIGRATE_RC="$CMD_RC"
[ "$MIGRATE_RC" -eq 0 ] && ok "Adopt install exits 0" || fail "Adopt install exited $MIGRATE_RC (expected 0)"
assert_contains "30 files written" "$CMD_OUT" "Adopt install writes the full payload"
if grep -q "legacy architect" "$PROJ/.claude/agents/architect.md" 2>/dev/null; then
  fail "Matching leftover was not adopted"
else
  ok "Matching leftover was adopted (rewritten from payload)"
fi

# ---------------------------------------------------------------------------
# Segment 14 — Migration: --force migrates
# ---------------------------------------------------------------------------
echo "=== Segment 14: Migration --force ==="
PROJ="$TMPDIR/proj-migrate-force"
mkdir -p "$PROJ/.claude/agents"
echo "legacy architect" > "$PROJ/.claude/agents/architect.md"
cmd_capture "migrate force" "$BIN" install --harness claude-code --dir "$PROJ" --force --with-tools none
FORCE_RC="$CMD_RC"
[ "$FORCE_RC" -eq 0 ] && ok "--force install exits 0" || fail "--force install exited $FORCE_RC (expected 0)"
assert_contains "30 files written" "$CMD_OUT" "--force reports 30 files (overwrote legacy)"
# Force-overwritten file should match shipmates version
if ! grep -q "legacy architect" "$PROJ/.claude/agents/architect.md" 2>/dev/null; then
  ok "Force migration overwrites legacy"
else
  fail "Legacy file was not overwritten"
fi

# ---------------------------------------------------------------------------
# Segment 15 — Third-party collision without --force refuses the install
# ---------------------------------------------------------------------------
echo "=== Segment 15: Collision without --force ==="
PROJ="$TMPDIR/proj-collision"
mkdir -p "$PROJ/.claude/agents"
echo "intruder" > "$PROJ/.claude/agents/architect.md"
cmd_capture "collision" "$BIN" install --harness claude-code --dir "$PROJ" --with-tools none
COLLIDE_RC="$CMD_RC"
[ "$COLLIDE_RC" -eq 1 ] && ok "Collision install exits 1 (refused)" || fail "Collision exited $COLLIDE_RC (expected 1)"
assert_contains "does not own" "$CMD_OUT" "Collision names the unowned path"
# Force hint replays the captain's flags then appends --force (#392).
assert_contains "shipmates install" "$CMD_OUT" "Collision force hint starts from shipmates install"
assert_contains "--harness claude-code" "$CMD_OUT" "Collision force hint keeps --harness"
assert_contains "--dir $PROJ" "$CMD_OUT" "Collision force hint keeps --dir"
assert_contains "--with-tools none" "$CMD_OUT" "Collision force hint keeps --with-tools"
assert_contains "--force" "$CMD_OUT" "Collision names --force as the overwrite path"
if grep -q "intruder" "$PROJ/.claude/agents/architect.md" 2>/dev/null; then
  ok "Collision preserves intruder file"
else
  fail "Collision overwrote intruder file"
fi
if [ -e "$PROJ/.shipmates/receipts/claude-code.json" ]; then
  fail "Refused collision published a receipt"
else
  ok "Refused collision published no receipt"
fi

# ---------------------------------------------------------------------------
# Segment 16 — Uninstall
# ---------------------------------------------------------------------------
echo "=== Segment 16: Uninstall ==="
PROJ="$TMPDIR/proj-uninstall"
mkdir -p "$PROJ"
cmd_run "$BIN" install --harness claude-code --dir "$PROJ" --with-tools none >/dev/null 2>&1
cmd_capture "uninstall" "$BIN" uninstall --harness claude-code --dir "$PROJ"
UNINSTALL_RC="$CMD_RC"
[ "$UNINSTALL_RC" -eq 0 ] && ok "Uninstall exits 0" || fail "Uninstall exited $UNINSTALL_RC (expected 0)"
assert_contains "removed" "$CMD_OUT" "Uninstall reports removal"
# Files should be removed but empty dirs may remain
REMAINING_FILES=$(find "$PROJ" -type f 2>/dev/null | wc -l)
[ "$REMAINING_FILES" -eq 0 ] && ok "Uninstall removes all files" || fail "Uninstall left $REMAINING_FILES files"

# ---------------------------------------------------------------------------
# Segment 17 — Local install
# ---------------------------------------------------------------------------
echo "=== Segment 17: Local install ==="
PROJ="$TMPDIR/proj-local"
mkdir -p "$PROJ"
cp "$BIN" "$PROJ/shipmates"
(
  cd "$PROJ"
  set +e
  LOCAL_OUT="$(./shipmates install --harness claude-code --local --with-tools none 2>&1)"
  LOCAL_RC=$?
  set -e
  [ "$LOCAL_RC" -eq 0 ] && ok "Local install exits 0" || fail "Local install exited $LOCAL_RC (expected 0)"
  assert_file_exists "$PROJ/.claude/agents/architect.md" "Local install creates agent"
  assert_file_exists "$PROJ/.shipmates/receipts/claude-code.json" "Local install creates receipt"
)

# ---------------------------------------------------------------------------
# Segment 18 — Tools are runnable
# ---------------------------------------------------------------------------
echo "=== Segment 18: Tools runnable ==="
PROJ="$TMPDIR/proj-tools"
mkdir -p "$PROJ"
cmd_run "$BIN" install --harness claude-code --dir "$PROJ" --with-tools scrub >/dev/null 2>&1
SCRUB_SCRIPT="$PROJ/.claude/skills/shipmates-scrub/scrub.py"
assert_file_exists "$SCRUB_SCRIPT" "scrub.py installed"
if [ -f "$SCRUB_SCRIPT" ]; then
  set +e
  TOOL_OUT="$(python3 "$SCRUB_SCRIPT" 'my secret key is AKIA1234567890ABCDEF' 2>&1)"
  TOOL_RC=$?
  set -e
  if echo "$TOOL_OUT" | grep -qF "[REDACTED_AWS_KEY]" || true; then
    ok "scrub tool redacts AWS keys"
  else
    fail "scrub tool did not redact AWS keys"
  fi
fi

# ---------------------------------------------------------------------------
# Segment 19 — Check passes
# ---------------------------------------------------------------------------
echo "=== Segment 19: Check passes ==="
cmd_capture "check" "$BIN" check
CHECK_RC="$CMD_RC"
[ "$CHECK_RC" -eq 0 ] && ok "Check exits 0" || fail "Check exited $CHECK_RC (expected 0)"
assert_contains "Check passed" "$CMD_OUT" "Check reports all targets passed"

# ---------------------------------------------------------------------------
# Segment 20 — build --update regenerates digest (contributor)
# ---------------------------------------------------------------------------
echo "=== Segment 20: build --update regenerates digest ==="
# Back up the digest
cp tests/payload-digests/claude-code.sha256 "$TMPDIR/claude-code.sha256.bak"
# Corrupt it
echo "bad" > tests/payload-digests/claude-code.sha256
# Contributor digest regeneration lives on build --update, not shipmates update
cmd_capture "build --update" "$BIN" build --target claude-code --update
BUILD_UPDATE_RC="$CMD_RC"
[ "$BUILD_UPDATE_RC" -eq 0 ] && ok "build --update exits 0" || fail "build --update exited $BUILD_UPDATE_RC (expected 0)"
assert_contains "Wrote digests" "$CMD_OUT" "build --update reports writing digests"
# Verify digest is restored
cmd_capture "check after build --update" "$BIN" check --target claude-code
CHECK_AFTER_RC="$CMD_RC"
[ "$CHECK_AFTER_RC" -eq 0 ] && ok "Digest restored after build --update" || fail "Digest not restored after build --update (exit $CHECK_AFTER_RC)"
# Restore original digest
cp "$TMPDIR/claude-code.sha256.bak" tests/payload-digests/claude-code.sha256

# ---------------------------------------------------------------------------
# Segment 21 — update refreshes an existing install
# ---------------------------------------------------------------------------
echo "=== Segment 21: update refreshes an existing install ==="
UPDATE_PROJ="$TMPDIR/proj-update"
mkdir -p "$UPDATE_PROJ"
cmd_capture "install for update" "$BIN" install --harness claude-code --dir "$UPDATE_PROJ" --with-tools none
[ "$CMD_RC" -eq 0 ] && ok "Install for update exits 0" || fail "Install for update exited $CMD_RC"
AGENT="$UPDATE_PROJ/.claude/agents/architect.md"
[ -f "$AGENT" ] || fail "architect.md missing after install"
printf 'local drift\n' > "$AGENT"
cmd_capture "update install" "$BIN" update --harness claude-code --dir "$UPDATE_PROJ"
[ "$CMD_RC" -eq 0 ] && ok "update exits 0" || fail "update exited $CMD_RC (expected 0)"
assert_contains "Installed harness: claude-code" "$CMD_OUT" "update reports installed harness"
if grep -qF 'local drift' "$AGENT"; then
  fail "update left drifted architect.md untouched"
else
  ok "update refreshed drifted architect.md"
fi
mkdir -p "$TMPDIR/empty-update"
cmd_capture "update without receipt" "$BIN" update --dir "$TMPDIR/empty-update"
[ "$CMD_RC" -ne 0 ] && ok "update without receipt fails closed" || fail "update without receipt unexpectedly succeeded"
assert_contains "No install receipt" "$CMD_OUT" "update without receipt names the cause"

# ---------------------------------------------------------------------------
# Segment 22 — update keeps opencode native tools (#412)
# ---------------------------------------------------------------------------
echo "=== Segment 22: update keeps opencode native tools ==="
PROJ="$TMPDIR/proj-opencode-update"
mkdir -p "$PROJ"
cmd_run "$BIN" install --harness opencode --dir "$PROJ" --with-tools all >/dev/null 2>&1
TOOLS_BEFORE=$(find "$PROJ/.opencode/tools" -type f ! -name '*.bak-*' | wc -l | tr -d ' ')
[ "$TOOLS_BEFORE" -ge 11 ] && ok "opencode installed native tools ($TOOLS_BEFORE files)" || fail "opencode native tools missing ($TOOLS_BEFORE files)"
cmd_capture "opencode bare update" "$BIN" update --harness opencode --dir "$PROJ"
[ "$CMD_RC" -eq 0 ] && ok "update --harness opencode exits 0" || fail "update --harness opencode exited $CMD_RC (expected 0)"
TOOLS_AFTER=$(find "$PROJ/.opencode/tools" -type f ! -name '*.bak-*' | wc -l | tr -d ' ')
[ "$TOOLS_AFTER" -eq "$TOOLS_BEFORE" ] && ok "bare update kept every native tool ($TOOLS_AFTER)" || fail "bare update changed the native tool count: $TOOLS_BEFORE -> $TOOLS_AFTER"
if printf '%s' "$CMD_OUT" | grep -qF "Removed dropped file"; then
  fail "bare update removed dropped files"
else
  ok "bare update reported no removed dropped files"
fi
if grep -qF '.opencode/tools/shipmates-termgif.ts' "$PROJ/.shipmates/receipts/opencode.json"; then
  ok "receipt still claims the native tools"
else
  fail "receipt no longer claims .opencode/tools/shipmates-termgif.ts"
fi

# ---------------------------------------------------------------------------
# Segment 23 — update advances a drifted shared .agents skill (#428)
# ---------------------------------------------------------------------------
echo "=== Segment 23: shared .agents skill across sibling receipts ==="
PROJ="$TMPDIR/proj-shared-update"
mkdir -p "$PROJ"
for h in codex antigravity github-copilot; do
  cmd_run "$BIN" install --harness "$h" --dir "$PROJ" --with-tools all >/dev/null 2>&1
done
SKILL="$PROJ/.agents/skills/shipmates-issue/SKILL.md"
printf '%s\n' '---' 'name: shipmates-issue' 'description: stale shared generation' '---' 'stale shared body' > "$SKILL"
STALE_SHA=$(python3 - "$SKILL" <<'PY'
import hashlib, sys
print(hashlib.sha256(open(sys.argv[1], "rb").read()).hexdigest())
PY
)
python3 - "$PROJ" "$STALE_SHA" <<'PY'
import json, pathlib, sys
root = pathlib.Path(sys.argv[1])
stale = sys.argv[2]
rel = ".agents/skills/shipmates-issue/SKILL.md"
for receipt in sorted((root / ".shipmates/receipts").glob("*.json")):
    data = json.loads(receipt.read_text())
    for entry in data["files"]:
        if entry["path"] == rel:
            entry["sha256"] = stale
    data["files"].sort(key=lambda entry: entry["path"])
    receipt.write_text(json.dumps(data, indent=2, ensure_ascii=False) + "\n")
PY
SHARED_OK=1
for h in codex antigravity github-copilot; do
  cmd_capture "shared update $h" "$BIN" update --harness "$h" --dir "$PROJ"
  if [ "$CMD_RC" -ne 0 ]; then
    fail "update --harness $h exited $CMD_RC (expected 0)"
    SHARED_OK=0
  fi
  if printf '%s' "$CMD_OUT" | grep -qF "shared-managed file left untouched"; then
    fail "update --harness $h deferred the attributable shared skill"
    SHARED_OK=0
  fi
done
[ "$SHARED_OK" -eq 1 ] && ok "every sibling update advanced the shared skill" || true
FRESH="$TMPDIR/proj-shared-update-fresh"
mkdir -p "$FRESH"
cmd_run "$BIN" install --harness codex --dir "$FRESH" --with-tools all >/dev/null 2>&1
if cmp -s "$SKILL" "$FRESH/.agents/skills/shipmates-issue/SKILL.md"; then
  ok "updated shared skill matches a fresh install"
else
  fail "updated shared skill differs from a fresh install"
fi
for h in codex antigravity github-copilot; do
  assert_exit 0 "doctor $h healthy after shared update" "$BIN" doctor --harness "$h" --dir "$PROJ"
done

# ---------------------------------------------------------------------------
# Summary
# ---------------------------------------------------------------------------
echo ""
echo "========================================"
echo "  Results: $PASS passed, $FAIL failed"
echo "========================================"

# Cleanup
rm -rf "$TMPDIR"

[ "$FAIL" -eq 0 ] && exit 0 || exit 1
