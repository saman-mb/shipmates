#!/usr/bin/env bash
#
# End-to-end test for the Claude Code PreToolUse FSM tool-gate shim
# (enforcement/hooks/claude-code/fsm-gate.sh). It feeds the script simulated
# PreToolUse payloads on stdin, against a throwaway git repo + run file, with the
# freshly built `shipmates` on PATH, and asserts the emitted permission decision.
#
# Cases:
#   (a) `gh pr merge` on a `feat/issue-1` branch at phase=build → deny JSON
#   (b) same on `main`                                          → allow (no output)
#   (c) `gh pr merge` with no run file                          → allow (no output)
#   (d) a non-Bash tool                                         → allow (no output)
#   (e) `git push` at phase=build (satisfied gate)              → allow (no output)
#   (f) `feat/bundle-*` branch (not gated in this slice)        → allow (no output)
#
#   bash tests/test_fsm_gate_shim.sh
#
# Exit 0 = all passed, 1 = at least one failure. Requires jq, git, and a built
# `shipmates` binary (target/debug or target/release).

set -uo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SHIM="$REPO/enforcement/hooks/claude-code/fsm-gate.sh"

# Locate the built binary; build it if neither profile is present.
BIN=""
for cand in "$REPO/target/debug/shipmates" "$REPO/target/release/shipmates"; do
    [ -x "$cand" ] && BIN="$cand" && break
done
if [ -z "$BIN" ]; then
    ( cd "$REPO" && cargo build --quiet ) || { echo "cargo build failed"; exit 1; }
    BIN="$REPO/target/debug/shipmates"
fi
# Put the binary on PATH under the name the shim calls (`shipmates`).
BINDIR="$(mktemp -d)"
ln -sf "$BIN" "$BINDIR/shipmates"
export PATH="$BINDIR:$PATH"

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK" "$BINDIR"' EXIT

PASS=0; FAIL=0
ok()  { PASS=$((PASS+1)); printf 'ok   %s\n' "$1"; }
bad() { FAIL=$((FAIL+1)); printf 'FAIL %s\n' "$1"; }

# --- a throwaway git repo with a ship-issue run file at phase=build ---
GIT="$WORK/repo"
mkdir -p "$GIT"
git -C "$GIT" init -q
git -C "$GIT" config user.email t@t.t
git -C "$GIT" config user.name t
git -C "$GIT" commit -q --allow-empty -m init
git -C "$GIT" branch -m feat/issue-1
"$BIN" state init --run 1 --command ship-issue >/dev/null 2>&1
# `state init` wrote .shipmates/run-1.json in the CWD, not the repo — move it.
mkdir -p "$GIT/.shipmates"
mv ".shipmates/run-1.json" "$GIT/.shipmates/run-1.json"
# Advance the run to `build` inside the repo.
( cd "$GIT" && "$BIN" state advance --run 1 --to isolate >/dev/null 2>&1 \
             && "$BIN" state advance --run 1 --to build   >/dev/null 2>&1 )

# Build a PreToolUse payload for a Bash command in $GIT.
bash_payload() { # command
    jq -cn --arg cmd "$1" --arg cwd "$GIT" \
        '{tool_name:"Bash", tool_input:{command:$cmd}, cwd:$cwd}'
}

run_shim() { bash "$SHIM"; } # reads stdin

# (a) gh pr merge on feat/issue-1 at build → deny JSON.
out="$(bash_payload 'gh pr merge --squash' | run_shim)"
decision="$(printf '%s' "$out" | jq -r '.hookSpecificOutput.permissionDecision // empty' 2>/dev/null)"
reason="$(printf '%s' "$out" | jq -r '.hookSpecificOutput.permissionDecisionReason // empty' 2>/dev/null)"
if [ "$decision" = "deny" ] && printf '%s' "$reason" | grep -q 'gate: gh pr merge requires phase>=deliver'; then
    ok "(a) gh pr merge at build → deny JSON with greppable reason"
else
    bad "(a) expected deny JSON, got: $out"
fi

# (b) same command on `main` → allow (no output).
git -C "$GIT" branch -m main
out="$(bash_payload 'gh pr merge --squash' | run_shim)"
[ -z "$out" ] && ok "(b) gh pr merge on main → allow (no output)" || bad "(b) expected no output, got: $out"
git -C "$GIT" branch -m feat/issue-1

# (c) no run file → allow (no output).
mv "$GIT/.shipmates/run-1.json" "$GIT/.shipmates/run-1.json.bak"
out="$(bash_payload 'gh pr merge --squash' | run_shim)"
[ -z "$out" ] && ok "(c) no run file → allow (no output)" || bad "(c) expected no output, got: $out"
mv "$GIT/.shipmates/run-1.json.bak" "$GIT/.shipmates/run-1.json"

# (d) a non-Bash tool → allow (no output).
out="$(jq -cn --arg cwd "$GIT" '{tool_name:"Read", tool_input:{file_path:"/etc/hosts"}, cwd:$cwd}' | run_shim)"
[ -z "$out" ] && ok "(d) non-Bash tool → allow (no output)" || bad "(d) expected no output, got: $out"

# (e) git push at build (satisfied gate) → allow (no output).
out="$(bash_payload 'git push -u origin HEAD' | run_shim)"
[ -z "$out" ] && ok "(e) git push at build → allow (no output)" || bad "(e) expected no output, got: $out"

# (f) feat/bundle-* branch is not gated in this slice → allow (no output).
git -C "$GIT" branch -m feat/bundle-1-x
out="$(bash_payload 'gh pr merge --squash' | run_shim)"
[ -z "$out" ] && ok "(f) feat/bundle-* branch → allow (no output)" || bad "(f) expected no output, got: $out"

# (g) real slugged branch `feat/issue-1-<slug>` resolves to run 1 → deny.
#     (ship-issue names branches feat/issue-<N>-<slug>, not the bare form.)
git -C "$GIT" branch -m feat/issue-1-gate-core
out="$(bash_payload 'gh pr merge --squash' | run_shim)"
decision="$(printf '%s' "$out" | jq -r '.hookSpecificOutput.permissionDecision // empty' 2>/dev/null)"
[ "$decision" = "deny" ] && ok "(g) feat/issue-1-slug → deny (run discovered by number)" || bad "(g) expected deny, got: $out"

printf '\n%d passed, %d failed\n' "$PASS" "$FAIL"
[ "$FAIL" -eq 0 ]
