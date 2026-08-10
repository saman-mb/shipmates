# Antigravity FSM tool-gate hook

`fsm-gate.sh` is an Antigravity (`agy`) `PreToolUse` shim that binds a Shipmates
command run to its finite-state machine: it blocks a shell command a
`tool_gates` binding gates until the run has reached the required stage.

It is a thin wrapper over the engine — `shipmates state gate --run <N> --tool
"<command>"` (exit 0 allow / 1 deny / 2 error). The shim owns run discovery
(from the `feat/issue-<N>[-<slug>]` branch + `.shipmates/run-<N>.json`), the
fail-safe rules (allow anything it cannot unambiguously identify), gating ONLY
the shell tool (`run_command`), and the translation of a deny verdict into
Antigravity's decision JSON.

## Deny form

On a deny the shim writes JSON with `decision: "deny"` (plus a `reason`) to
stdout and exits 0, so agy reads the decision rather than a hook fault. Allow is
a silent `exit 0`; an engine error is a fail-safe allow with a stderr log.

> Caveat: the `PreToolUse` event-JSON shape and this `decision:deny` response are
> format-verified but not yet proven against a running Antigravity. Installation
> registers the project hook; the dispatcher remains inactive outside a run.

## Fail-safe rules

Allowed with no output (never blocked): a non-shell tool, `main`, a detached
HEAD, a missing `.shipmates/run-<N>.json`, a non-git cwd, or no active run. The
installed dispatcher uses Rust JSON parsing and requires `git` plus `shipmates`.

## Scope

The script is emitted into the Antigravity payload and installation registers it
in `.agents/hooks.json` for `PreToolUse` with a tool-name matcher.
