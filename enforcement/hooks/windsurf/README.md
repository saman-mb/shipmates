# Windsurf/Devin FSM tool-gate hook

`fsm-gate.sh` is a Windsurf/Devin `pre_run_command` shim that binds a Shipmates
command run to its finite-state machine: it blocks a shell command a
`tool_gates` binding gates until the run has reached the required stage.

It is a thin wrapper over the engine — `shipmates state gate --run <N> --tool
"<command>"` (exit 0 allow / 1 deny / 2 error). The shim owns run discovery
(from the `feat/issue-<N>[-<slug>]` branch + `.shipmates/run-<N>.json`), the
fail-safe rules (allow anything it cannot unambiguously identify), and the
translation of a deny verdict into a blocked command.

## Deny form

`pre_run_command` is a shell-only allow/block hook with no rewrite channel, so a
deny is a bare **`exit 2` with no stdout**. Allow is a silent `exit 0`; an engine
error is a fail-safe allow with a stderr log.

> Caveat: the `pre_run_command` event-JSON shape and exit-2 block are
> format-verified but not yet proven against a running Windsurf/Devin.
> Installation writes the workspace hook config.

## Fail-safe rules

Allowed with no output (never blocked): `main`, a detached HEAD, a missing
`.shipmates/run-<N>.json`, a non-git cwd, or no active run. The installed
dispatcher uses Rust JSON parsing and requires `git` plus `shipmates`.

## Scope

The script is emitted into the Windsurf payload and installation registers it in
`.windsurf/hooks.json` for `pre_run_command`.
