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

> Caveat: the `pre_run_command` event-JSON shape and this exit-2 block are assumed
> from the docs, not yet verified against a running Windsurf/Devin — the shim
> parses defensively and fails safe on any unrecognised shape. That verify-live
> pass, with all install-time wiring, is #217.

## Fail-safe rules

Allowed with no output (never blocked): `main`, a detached HEAD, a
`feat/bundle-*` branch, a missing `.shipmates/run-<N>.json`, a non-git cwd, no
`shipmates` on `PATH`, or an engine error (exit 2). The shim never fails open —
it only ever blocks on a definite engine deny (exit 1). Requires `jq` and `git`.

## Scope

The script is emitted into the Windsurf payload by #216. Install-time config
wiring — registering it in `.windsurf/hooks.json` for `pre_run_command` — is
#217.
