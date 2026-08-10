#!/usr/bin/env bash
#
# Shipmates FSM tool-gate — GitHub Copilot `preToolUse` hook.
#
# Turns a `shipmates state gate` verdict into a Copilot permission decision.
# Copilot `preToolUse` fires for every tool, so the shim gates ONLY the shell
# tool and stays silent on everything else. It only ever GATES a run it can
# unambiguously identify; in every other case it stays silent and allows, so it
# can never block work it does not understand.
#
#   Discovery:  the run is the issue number N in a `feat/issue-<N>[-<slug>]`
#               branch with a `.shipmates/run-<N>.json` file. Anything else —
#               `main`, a detached HEAD, a `feat/bundle-*` branch, a missing run
#               file, a non-git cwd, or no `shipmates` on PATH — is ALLOWED with
#               no output. Never block an ambiguous/unknown run (fail-safe).
#
#   Verdict:    `shipmates state gate` exits 0 allow / 1 deny / 2 error.
#               deny  → emit `permissionDecision: "deny"` on stdout (exit 0, so
#                       the decision is honoured rather than read as a hook fault).
#               allow → exit 0, no output.
#               error → allow + log to stderr (fail-safe; an engine fault must
#                       not wedge the session).
#
# Deny form: stdout JSON with `permissionDecision: "deny"` (and a
# `permissionDecisionReason`), exit 0.
#
# ASSUMPTION (flag for the board): the `preToolUse` event JSON is parsed
# defensively. The shell tool is matched by name against `execute`/`shell`/
# `bash`, and the command is read from `.tool_input.command`. If Copilot's real
# event shape differs, an unrecognised tool-name or missing command falls through
# to a fail-safe ALLOW rather than a wrong block. The deny channel itself is not
# yet verified live (`.github/hooks/` registration + a real block) — that
# verify-live pass, along with all install-time wiring, is #217.
#
# Requires `jq` and `git`.

set -u

if [ "${SHIPMATES_NATIVE_HOOK:-}" = "1" ]; then
    exec shipmates hook gate --harness github-copilot
fi

payload="$(cat)"

# Extract with jq; a missing field yields "" via `// empty`.
jqr() { printf '%s' "$payload" | jq -r "$1" 2>/dev/null; }

# Only the shell tool is gated. Any other tool (or an absent/unknown tool-name,
# which we cannot confirm is a shell) → allow.
tool_name="$(jqr '.tool_name // .tool // empty')"
case "$tool_name" in
    execute | shell | bash | Bash) ;;
    *) exit 0 ;;
esac

command="$(jqr '.tool_input.command // .command // empty')"
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
        # Deny — emit Copilot's permission decision on stdout. Exit 0 so Copilot
        # honours the decision rather than treating a non-zero exit as a fault.
        jq -cn --arg reason "$reason" '{
            permissionDecision: "deny",
            permissionDecisionReason: $reason
        }'
        exit 0
        ;;
    *)
        # Engine error (exit 2) — fail safe: allow, but log so the fault is visible.
        printf 'fsm-gate: shipmates state gate errored (exit %s); allowing: %s\n' "$code" "$reason" >&2
        exit 0
        ;;
esac
