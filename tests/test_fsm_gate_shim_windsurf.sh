#!/usr/bin/env bash
#
# End-to-end test for the Windsurf/Devin `pre_run_command` FSM tool-gate shim
# (enforcement/hooks/windsurf/fsm-gate.sh). Simulated event payloads on stdin
# against a throwaway git repo + run file, with a STUBBED `shipmates` on PATH
# whose exit code we control (0 allow / 1 deny / 2 error).
#
# Deny form (Windsurf): exit 2, NO stdout JSON (allow/block only, no rewrite).
#
#   bash tests/test_fsm_gate_shim_windsurf.sh
#
# Exit 0 = all passed, 1 = at least one failure. Requires jq and git.

set -uo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SHIM="$REPO/enforcement/hooks/windsurf/fsm-gate.sh"

WORK="$(mktemp -d)"
BINDIR="$(mktemp -d)"
trap 'rm -rf "$WORK" "$BINDIR"' EXIT

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

GIT="$WORK/repo"
mkdir -p "$GIT/.shipmates"
git -C "$GIT" init -q
git -C "$GIT" config user.email t@t.t
git -C "$GIT" config user.name t
git -C "$GIT" commit -q --allow-empty -m init
git -C "$GIT" branch -m feat/issue-1
echo '{}' > "$GIT/.shipmates/run-1.json"

payload() { jq -cn --arg cmd "$1" --arg cwd "$GIT" '{command:$cmd, cwd:$cwd}'; }

run_shim() {
    local err; err="$(mktemp)"
    OUT="$(payload "$1" | bash "$SHIM" 2>"$err")"; RC=$?
    ERR="$(cat "$err")"; rm -f "$err"
}

# (a) engine deny (exit 1) → exit 2, no stdout.
export STUB_EXIT=1
run_shim 'gh pr merge --squash'
if [ -z "$OUT" ] && [ "$RC" -eq 2 ] && printf '%s' "$ERR" | grep -q 'denied'; then
    ok "(a) deny → exit 2, no stdout, stderr reason"
else
    bad "(a) expected exit 2 with no stdout, got rc=$RC out=$OUT err=$ERR"
fi

# (b) engine allow (exit 0) → silent allow.
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

# (d) fail-safe: `main` is never gated.
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
