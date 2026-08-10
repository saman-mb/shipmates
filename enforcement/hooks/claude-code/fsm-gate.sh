#!/usr/bin/env bash
#
# Shipmates FSM tool-gate — Claude Code PreToolUse hook (reference shim).
#
# Turns a `shipmates state gate` verdict into a Claude Code permission decision.
# It reads the PreToolUse event JSON on stdin, and only ever GATES a `Bash` tool
# whose run it can unambiguously identify; in every other case it stays silent
# and allows, so it can never block work it does not understand.
#
#   Discovery:  the run is the issue number N in a `feat/issue-<N>[-<slug>]`
#               branch with a `.shipmates/run-<N>.json` file. Anything else —
#               `main`, a detached HEAD, a `feat/bundle-*` branch, a missing run
#               file, a non-git cwd, or no `shipmates` on PATH — is ALLOWED with
#               no output. Never block an ambiguous/unknown run (fail-safe).
#
#   Verdict:    `shipmates state gate` exits 0 allow / 1 deny / 2 error.
#               deny  → emit the deny JSON (exit 0, so the decision is honoured).
#               allow → exit 0, no output.
#               error → allow + log to stderr (fail-safe; an engine fault must
#                       not wedge the session).
#
# Install: wire this as a PreToolUse hook for the `Bash` matcher (see
# enforcement/hooks/claude-code/README.md). Requires `jq` and `git`.

set -u

# Installed registrations set this flag so the dependency-free Rust dispatcher
# owns production parsing. Manual wiring without the flag retains the reference
# shell implementation below for compatibility with the documented example.
if [ "${SHIPMATES_NATIVE_HOOK:-}" = "1" ]; then
    exec shipmates hook gate --harness claude-code
fi

payload="$(cat)"

# Extract with jq; a missing field yields "" via `// empty`.
jqr() { printf '%s' "$payload" | jq -r "$1" 2>/dev/null; }

# Only Bash commands are gated. Any other tool (or malformed payload) → allow.
tool_name="$(jqr '.tool_name // empty')"
[ "$tool_name" = "Bash" ] || exit 0

command="$(jqr '.tool_input.command // empty')"
[ -n "$command" ] || exit 0

# Where the tool would run. Claude Code sends `cwd`; fall back to $PWD.
cwd="$(jqr '.cwd // empty')"
[ -n "$cwd" ] || cwd="$PWD"

# The engine must be reachable; degrade to allow if it is not installed.
command -v shipmates >/dev/null 2>&1 || exit 0

# Discover the run from the branch name. Not a git repo, or rev-parse fails → allow.
branch="$(git -C "$cwd" rev-parse --abbrev-ref HEAD 2>/dev/null)" || exit 0

# Only `feat/issue-<N>[-<slug>]` is a gated run. `feat/bundle-*`, `main`, a
# detached HEAD ("HEAD"), and everything else fall through to allow.
case "$branch" in
    feat/issue-*) rest="${branch#feat/issue-}" ;;
    *) exit 0 ;;
esac
# N is the leading numeric segment, up to the first `-` (so `feat/issue-216` and
# `feat/issue-216-gate-core` both yield 216). A non-numeric segment → allow.
n="${rest%%-*}"
case "$n" in
    '' | *[!0-9]*) exit 0 ;;
esac

# The run file must exist for this issue, or there is nothing to gate → allow.
[ -f "$cwd/.shipmates/run-$n.json" ] || exit 0

# Ask the engine. `--dir "$cwd"` points it at the worktree's `.shipmates/` (no
# `cd` needed). Capture stderr (the greppable reason) and discard the stdout
# verdict JSON.
reason="$(shipmates state gate --dir "$cwd" --run "$n" --tool "$command" 2>&1 >/dev/null)"
code=$?

case "$code" in
    0)
        # Allow — the tool is ungated, or the run has reached the required stage.
        exit 0
        ;;
    1)
        # Deny — emit the Claude Code permission decision. Exit 0 so Claude Code
        # honours the decision rather than treating a non-zero exit as a hook error.
        jq -cn --arg reason "$reason" '{
            hookSpecificOutput: {
                hookEventName: "PreToolUse",
                permissionDecision: "deny",
                permissionDecisionReason: $reason
            }
        }'
        exit 0
        ;;
    *)
        # Engine error (exit 2) — fail safe: allow, but log so the fault is visible.
        printf 'fsm-gate: shipmates state gate errored (exit %s); allowing: %s\n' "$code" "$reason" >&2
        exit 0
        ;;
esac
