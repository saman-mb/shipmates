#!/usr/bin/env bash
# End-to-end proof that an installed shim delegates to the Rust dispatcher.

set -uo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SHIM="$REPO/enforcement/hooks/claude-code/fsm-gate.sh"
BIN="$REPO/target/debug/shipmates"
if [ ! -x "$BIN" ]; then
    (cd "$REPO" && cargo build --quiet) || exit 1
fi

WORK="$(mktemp -d)"
BINDIR="$(mktemp -d)"
trap 'rm -rf "$WORK" "$BINDIR"' EXIT
ln -s "$BIN" "$BINDIR/shipmates"
export PATH="$BINDIR:$PATH"

GIT="$WORK/repo"
mkdir -p "$GIT"
git -C "$GIT" init -q
git -C "$GIT" config user.email test@example.invalid
git -C "$GIT" config user.name test
git -C "$GIT" commit -q --allow-empty -m init
git -C "$GIT" branch -m feat/issue-1-native

"$BIN" state init --dir "$GIT" --run 1 --command ship-issue >/dev/null
"$BIN" state advance --dir "$GIT" --run 1 --to isolate >/dev/null
"$BIN" state advance --dir "$GIT" --run 1 --to build >/dev/null

stop="$(printf '{"cwd":"%s","stop_hook_active":false}\n' "$GIT" | "$BIN" hook stop --harness claude-code)"
printf '%s' "$stop" | python3 -c 'import json,sys; assert json.load(sys.stdin)["decision"] == "block"'
codex_stop="$(printf '{"cwd":"%s","stop_hook_active":false}\n' "$GIT" | "$BIN" hook stop --harness codex)"
printf '%s' "$codex_stop" | python3 -c 'import json,sys; assert json.load(sys.stdin)["continue"] is False'

context="$(printf '{"cwd":"%s"}\n' "$GIT" | "$BIN" hook context --harness claude-code --event SessionStart)"
printf '%s' "$context" | python3 -c 'import json,sys; value=json.load(sys.stdin); assert "phase `build`" in value["hookSpecificOutput"]["additionalContext"]'
printf '{"tool_name":"Bash","cwd":"%s"}\n' "$GIT" | "$BIN" hook record --harness claude-code --event PostToolUse
python3 - "$GIT/.shipmates/run-1.json" <<'PY'
import json
import sys
with open(sys.argv[1]) as f:
    run = json.load(f)
assert run["events"][-1]["event"] == "PostToolUse"
assert run["events"][-1]["tool"] == "Bash"
PY

payload_at() {
    python3 -c 'import json,sys; print(json.dumps({"tool_name":"Bash","tool_input":{"command":sys.argv[1]},"cwd":sys.argv[2]}))' "$1" "$2"
}

payload() {
    payload_at "$1" "$GIT"
}

out="$(payload 'gh pr merge --squash' | SHIPMATES_NATIVE_HOOK=1 bash "$SHIM")"
decision="$(printf '%s' "$out" | python3 -c 'import json,sys; print(json.load(sys.stdin)["hookSpecificOutput"]["permissionDecision"])')"
[ "$decision" = deny ] || { printf 'expected native deny, got: %s\n' "$out"; exit 1; }

mkdir -p "$GIT/subdir"
out="$(payload_at 'gh pr merge --squash' "$GIT/subdir" | SHIPMATES_NATIVE_HOOK=1 bash "$SHIM")"
decision="$(printf '%s' "$out" | python3 -c 'import json,sys; print(json.load(sys.stdin)["hookSpecificOutput"]["permissionDecision"])')"
[ "$decision" = deny ] || { printf 'expected subdirectory deny, got: %s\n' "$out"; exit 1; }

# A base-session command with an active sibling worktree is discovered from git's
# worktree list even when the hook event cwd is still the base checkout.
BASE="$WORK/base"
WT="$WORK/worktree"
mkdir -p "$BASE"
git -C "$BASE" init -q
git -C "$BASE" config user.email test@example.invalid
git -C "$BASE" config user.name test
git -C "$BASE" commit -q --allow-empty -m init
git -C "$BASE" branch -m main
git -C "$BASE" worktree add -q -b feat/issue-2-native "$WT" HEAD
"$BIN" state init --dir "$WT" --run 2 --command ship-issue >/dev/null
"$BIN" state advance --dir "$WT" --run 2 --to isolate >/dev/null
"$BIN" state advance --dir "$WT" --run 2 --to build >/dev/null
out="$(payload_at 'gh pr merge --squash' "$BASE" | SHIPMATES_NATIVE_HOOK=1 bash "$SHIM")"
decision="$(printf '%s' "$out" | python3 -c 'import json,sys; print(json.load(sys.stdin)["hookSpecificOutput"]["permissionDecision"])')"
[ "$decision" = deny ] || { printf 'expected sibling-worktree deny, got: %s\n' "$out"; exit 1; }

# Bundle branches use the same first issue run and must not silently bypass gates.
git -C "$GIT" branch -m feat/bundle-1-native
out="$(payload 'gh pr merge --squash' | SHIPMATES_NATIVE_HOOK=1 bash "$SHIM")"
decision="$(printf '%s' "$out" | python3 -c 'import json,sys; print(json.load(sys.stdin)["hookSpecificOutput"]["permissionDecision"])')"
[ "$decision" = deny ] || { printf 'expected bundle deny, got: %s\n' "$out"; exit 1; }

"$BIN" state advance --dir "$GIT" --run 1 --to verify >/dev/null
"$BIN" state advance --dir "$GIT" --run 1 --to review >/dev/null
"$BIN" state advance --dir "$GIT" --run 1 --to deliver >/dev/null
out="$(payload 'gh pr merge --squash' | SHIPMATES_NATIVE_HOOK=1 bash "$SHIM")"
decision="$(printf '%s' "$out" | python3 -c 'import json,sys; print(json.load(sys.stdin)["hookSpecificOutput"]["permissionDecision"])')"
[ "$decision" = deny ] || { printf 'expected unattested-merge deny, got: %s\n' "$out"; exit 1; }

copilot="$(printf '{"toolName":"bash","toolArgs":{"command":"gh pr merge --squash"},"cwd":"%s"}\n' "$GIT" | "$BIN" hook gate --harness github-copilot)"
printf '%s' "$copilot" | python3 -c 'import json,sys; assert json.load(sys.stdin)["permissionDecision"] == "deny"'
codex="$(payload 'gh pr merge --squash' | "$BIN" hook gate --harness codex)"
printf '%s' "$codex" | python3 -c 'import json,sys; assert json.load(sys.stdin)["hookSpecificOutput"]["permissionDecision"] == "deny"'

# Corrupt active state denies rather than silently bypassing the active run.
printf '{"broken":true}\n' > "$GIT/.shipmates/run-1.json"
out="$(payload 'gh pr merge --squash' | SHIPMATES_NATIVE_HOOK=1 bash "$SHIM")"
decision="$(printf '%s' "$out" | python3 -c 'import json,sys; print(json.load(sys.stdin)["hookSpecificOutput"]["permissionDecision"])')"
[ "$decision" = deny ] || { printf 'expected corrupt-state deny, got: %s\n' "$out"; exit 1; }

printf 'native hook dispatcher: 4 passed\n'
