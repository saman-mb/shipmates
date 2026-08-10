#!/usr/bin/env bash
#
# Shipmates FSM tool-gate — Windsurf/Devin `pre_run_command` hook.
#
# Turns a `shipmates state gate` verdict into a Windsurf command decision.
# `pre_run_command` is a shell-only hook, so there is no tool-name to check:
# every invocation is a command about to run. The shim only ever GATES a run it
# can unambiguously identify; in every other case it stays silent and allows, so
# it can never block work it does not understand.
#
#   Discovery:  the run is the issue number N in a `feat/issue-<N>[-<slug>]`
#               branch with a `.shipmates/run-<N>.json` file. Anything else —
#               `main`, a detached HEAD, a `feat/bundle-*` branch, a missing run
#               file, a non-git cwd, or no `shipmates` on PATH — is ALLOWED with
#               no output. Never block an ambiguous/unknown run (fail-safe).
#
#   Verdict:    `shipmates state gate` exits 0 allow / 1 deny / 2 error.
#               deny  → exit 2 with NO stdout. Windsurf's `pre_run_command` is
#                       allow/block only (no rewrite channel), so a non-zero exit
#                       is the whole deny signal.
#               allow → exit 0, no output.
#               error → allow + log to stderr (fail-safe; an engine fault must
#                       not wedge the session).
#
# Deny form: exit 2, no stdout JSON. (Windsurf/Devin `pre_run_command` supports
# only allow/block, not a rewritten command, so there is no decision payload.)
#
# ASSUMPTION (flag for the board): the `pre_run_command` event JSON field names
# are parsed defensively — `.command` for the shell command and `.cwd` for the
# working directory, each with a `.tool_input.*` fallback. If Windsurf's real
# event shape differs, the missing-field paths all fall through to a fail-safe
# ALLOW rather than a wrong block. Live wiring (`.windsurf/hooks.json`) is #217.
#
# Requires `jq` and `git`.

set -u

if [ "${SHIPMATES_NATIVE_HOOK:-}" = "1" ]; then
    exec shipmates hook gate --harness windsurf
fi

payload="$(cat)"

# Extract with jq; a missing field yields "" via `// empty`.
jqr() { printf '%s' "$payload" | jq -r "$1" 2>/dev/null; }

# The shell command about to run. Empty/malformed payload → allow.
command="$(jqr '.command // .tool_input.command // empty')"
[ -n "$command" ] || exit 0

# Where the tool would run. Fall back to $PWD if no cwd is supplied.
cwd="$(jqr '.cwd // .tool_input.cwd // empty')"
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
# N is the leading numeric segment, up to the first `-`. A non-numeric segment → allow.
n="${rest%%-*}"
case "$n" in
    '' | *[!0-9]*) exit 0 ;;
esac

# The run file must exist for this issue, or there is nothing to gate → allow.
[ -f "$cwd/.shipmates/run-$n.json" ] || exit 0

# Ask the engine. Capture stderr (the greppable reason); discard stdout verdict.
reason="$(shipmates state gate --dir "$cwd" --run "$n" --tool "$command" 2>&1 >/dev/null)"
code=$?

case "$code" in
    0)
        # Allow — the tool is ungated, or the run has reached the required stage.
        exit 0
        ;;
    1)
        # Deny — Windsurf blocks on a non-zero exit; no stdout payload. Log the
        # reason to stderr so the block is explicable.
        printf 'fsm-gate: denied: %s\n' "$reason" >&2
        exit 2
        ;;
    *)
        # Engine error (exit 2) — fail safe: allow, but log so the fault is visible.
        printf 'fsm-gate: shipmates state gate errored (exit %s); allowing: %s\n' "$code" "$reason" >&2
        exit 0
        ;;
esac
