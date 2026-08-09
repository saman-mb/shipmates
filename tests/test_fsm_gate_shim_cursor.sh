#!/usr/bin/env bash
#
# End-to-end test for the Cursor `beforeShellExecution` FSM tool-gate shim
# (enforcement/hooks/cursor/fsm-gate.sh). It feeds the script simulated event
# payloads on stdin against a throwaway git repo + run file, with a STUBBED
# `shipmates` on PATH whose exit code we control (0 allow / 1 deny / 2 error),
# and asserts Cursor's deny form.
#
# Deny form (Cursor): stdout `{"permission":"deny"}` AND exit 2 (failClosed).
#
#   bash tests/test_fsm_gate_shim_cursor.sh
#
# Exit 0 = all passed, 1 = at least one failure. Requires jq and git.

set -uo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SHIM="$REPO/enforcement/hooks/cursor/fsm-gate.sh"

WORK="$(mktemp -d)"
BINDIR="$(mktemp -d)"
trap 'rm -rf "$WORK" "$BINDIR"' EXIT

# Stub `shipmates`: a greppable reason on stderr, exit code from $STUB_EXIT.
cat > "$BINDIR/shipmates" <<'STUB'
#!/usr/bin/env bash
echo "gate: stub deny reason for $*" >&2
exit "${STUB_EXIT:-0}"
STUB
chmod +x "$BINDIR/shipmates"
export PATH="$BINDIR:$PATH"

PASS=0; FAIL=0
ok()  { PASS=$((PASS+1)); printf 'ok   %s\n' "$1"; }
bad() { FAIL=$((FAIL+1)); printf 'FAIL %s\n' "$1"; }

# A throwaway git repo on a gated branch with a run file present.
GIT="$WORK/repo"
mkdir -p "$GIT/.shipmates"
git -C "$GIT" init -q
git -C "$GIT" config user.email t@t.t
git -C "$GIT" config user.name t
git -C "$GIT" commit -q --allow-empty -m init
git -C "$GIT" branch -m feat/issue-1
echo '{}' > "$GIT/.shipmates/run-1.json"

payload() { jq -cn --arg cmd "$1" --arg cwd "$GIT" '{command:$cmd, cwd:$cwd}'; }

# Run the shim; sets OUT (stdout), ERR (stderr), RC (exit code).
run_shim() {
    local err; err="$(mktemp)"
    OUT="$(payload "$1" | bash "$SHIM" 2>"$err")"; RC=$?
    ERR="$(cat "$err")"; rm -f "$err"
}

# (a) engine deny (exit 1) → {"permission":"deny"} on stdout AND exit 2.
export STUB_EXIT=1
run_shim 'gh pr merge --squash'
if printf '%s' "$OUT" | grep -q '"permission":"deny"' && [ "$RC" -eq 2 ]; then
    ok "(a) deny → {\"permission\":\"deny\"} + exit 2"
else
    bad "(a) expected permission=deny + exit 2, got rc=$RC out=$OUT"
fi

# (b) engine allow (exit 0) → silent allow (no stdout, exit 0).
export STUB_EXIT=0
run_shim 'git push -u origin HEAD'
[ -z "$OUT" ] && [ "$RC" -eq 0 ] && ok "(b) allow → silent, exit 0" || bad "(b) expected silent allow, got rc=$RC out=$OUT"

# (c) engine error (exit 2) → fail-safe allow + stderr log.
export STUB_EXIT=2
run_shim 'gh pr merge --squash'
if [ -z "$OUT" ] && [ "$RC" -eq 0 ] && printf '%s' "$ERR" | grep -q 'errored'; then
    ok "(c) engine error → fail-safe allow + stderr log"
else
    bad "(c) expected fail-safe allow, got rc=$RC out=$OUT err=$ERR"
fi

# (d) fail-safe: `main` is never gated even on a deny verdict.
export STUB_EXIT=1
git -C "$GIT" branch -m main
run_shim 'gh pr merge --squash'
[ -z "$OUT" ] && [ "$RC" -eq 0 ] && ok "(d) main → allow (no output)" || bad "(d) expected allow on main, got rc=$RC out=$OUT"
git -C "$GIT" branch -m feat/issue-1

# (e) fail-safe: missing run file → allow.
export STUB_EXIT=1
mv "$GIT/.shipmates/run-1.json" "$GIT/.shipmates/run-1.json.bak"
run_shim 'gh pr merge --squash'
[ -z "$OUT" ] && [ "$RC" -eq 0 ] && ok "(e) missing run file → allow" || bad "(e) expected allow with no run file, got rc=$RC out=$OUT"
mv "$GIT/.shipmates/run-1.json.bak" "$GIT/.shipmates/run-1.json"

printf '\n%d passed, %d failed\n' "$PASS" "$FAIL"
[ "$FAIL" -eq 0 ]
