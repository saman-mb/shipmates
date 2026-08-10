# Cursor FSM tool-gate hook

`fsm-gate.sh` is a Cursor `beforeShellExecution` shim that binds a Shipmates
command run to its finite-state machine: it blocks a shell command a
`tool_gates` binding gates until the run has reached the required stage.

It is a thin wrapper over the engine — `shipmates state gate --run <N> --tool
"<command>"` (exit 0 allow / 1 deny / 2 error). The shim owns run discovery
(from the `feat/issue-<N>[-<slug>]` branch + `.shipmates/run-<N>.json`), the
fail-safe rules (allow anything it cannot unambiguously identify), and the
translation of a deny verdict into Cursor's shell-permission response.

## Deny form

`beforeShellExecution` is shell-only, so there is no tool-name to match. On a
deny the shim writes `{"permission":"deny"}` to stdout **and exits 2**; Cursor is
configured `failClosed`, so the deny is honoured. Allow is a silent `exit 0`; an
engine error is a fail-safe allow with a stderr log.

> Caveat: the `beforeShellExecution` event-JSON shape and this deny response are
> format-verified but not yet proven against a running Cursor. Installation sets
> `failClosed`; the Rust dispatcher parses the event defensively.

## Fail-safe rules

Allowed with no output (never blocked): `main`, a detached HEAD, a
`feat/bundle-*` branch, a missing `.shipmates/run-<N>.json`, a non-git cwd, no
`shipmates` on `PATH`, or an engine error (exit 2). The shim never fails open —
it only ever emits a deny on a definite engine deny (exit 1).

Cursor invokes hooks with a fresh cwd per call, so discovery keys off the event's
own `cwd`. Installed hooks use Rust JSON parsing and require `git` plus
`shipmates` on `PATH`.

## Scope

The script is emitted into the Cursor payload and installation registers it in
`.cursor/hooks.json` for `beforeShellExecution` with `failClosed`.
