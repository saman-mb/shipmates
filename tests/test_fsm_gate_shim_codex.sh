#!/usr/bin/env bash
#
# End-to-end test for the Codex `PreToolUse` FSM tool-gate shim
# (enforcement/hooks/codex/fsm-gate.sh). Simulated event payloads on stdin
# against a throwaway git repo + run file, with a STUBBED `shipmates` on PATH
# whose exit code we control (0 allow / 1 deny / 2 error).
#
# Deny form (Codex): stdout JSON with `permissionDecision: "deny"`, exit 0.
#
#   bash tests/test_fsm_gate_shim_codex.sh
#
# Exit 0 = all passed, 1 = at least one failure. Requires jq and git.

set -uo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SHIM="$REPO/enforcement/hooks/codex/fsm-gate.sh"

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

# Codex PreToolUse fires for every tool; the shim gates only the shell tool.
shell_payload() { jq -cn --arg cmd "$1" --arg cwd "$GIT" '{tool_name:"shell", tool_input:{command:$cmd}, cwd:$cwd}'; }
other_payload() { jq -cn --arg cwd "$GIT" '{tool_name:"read_file", tool_input:{path:"/etc/hosts"}, cwd:$cwd}'; }

run_shell() {
    local err; err="$(mktemp)"
    OUT="$(shell_payload "$1" | bash "$SHIM" 2>"$err")"; RC=$?
    ERR="$(cat "$err")"; rm -f "$err"
}

# (a) engine deny (exit 1) → permissionDecision:deny on stdout, exit 0.
export STUB_EXIT=1
run_shell 'gh pr merge --squash'
if printf '%s' "$OUT" | jq -e '.permissionDecision == "deny"' >/dev/null 2>&1 && [ "$RC" -eq 0 ]; then
    ok "(a) deny → permissionDecision:deny, exit 0"
else
    bad "(a) expected permissionDecision=deny, got rc=$RC out=$OUT"
fi

# (b) engine allow (exit 0) → silent allow.
export STUB_EXIT=0
run_shell 'git push -u origin HEAD'
[ -z "$OUT" ] && [ "$RC" -eq 0 ] && ok "(b) allow → silent, exit 0" || bad "(b) expected silent allow, got rc=$RC out=$OUT"

# (c) engine error (exit 2) → fail-safe allow + stderr log.
export STUB_EXIT=2
run_shell 'gh pr merge --squash'
if [ -z "$OUT" ] && [ "$RC" -eq 0 ] && printf '%s' "$ERR" | grep -q 'errored'; then
    ok "(c) engine error → fail-safe allow + stderr log"
else
    bad "(c) expected fail-safe allow, got rc=$RC out=$OUT err=$ERR"
fi

# (d) non-shell tool → allow even on a deny verdict (only the shell tool gates).
export STUB_EXIT=1
OUT="$(other_payload | bash "$SHIM")"; RC=$?
[ -z "$OUT" ] && [ "$RC" -eq 0 ] && ok "(d) non-shell tool → allow" || bad "(d) expected allow for non-shell tool, got rc=$RC out=$OUT"

# (e) fail-safe: `main` is never gated.
export STUB_EXIT=1
git -C "$GIT" branch -m main
run_shell 'gh pr merge --squash'
[ -z "$OUT" ] && [ "$RC" -eq 0 ] && ok "(e) main → allow (no output)" || bad "(e) expected allow on main, got rc=$RC out=$OUT"
git -C "$GIT" branch -m feat/issue-1

# (f) fail-safe: missing run file → allow.
export STUB_EXIT=1
mv "$GIT/.shipmates/run-1.json" "$GIT/.shipmates/run-1.json.bak"
run_shell 'gh pr merge --squash'
[ -z "$OUT" ] && [ "$RC" -eq 0 ] && ok "(f) missing run file → allow" || bad "(f) expected allow with no run file, got rc=$RC out=$OUT"
mv "$GIT/.shipmates/run-1.json.bak" "$GIT/.shipmates/run-1.json"

printf '\n%d passed, %d failed\n' "$PASS" "$FAIL"
[ "$FAIL" -eq 0 ]
