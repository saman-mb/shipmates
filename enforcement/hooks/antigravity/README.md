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
> assumed from the docs, not yet verified against a running Antigravity — the shim
> parses defensively and fails safe on any unrecognised shape. That verify-live
> pass, with all install-time wiring, is #217.

> Verified: the actual agy event uses `.toolCall.name` for the tool name and
> `.toolCall.args.CommandLine` for the command (not `.tool_input.command` as
> originally assumed). The shim now handles both formats defensively.

## Fail-safe rules

Allowed with no output (never blocked): a non-shell tool, `main`, a detached
HEAD, a `feat/bundle-*` branch, a missing `.shipmates/run-<N>.json`, a non-git
cwd, no `shipmates` on `PATH`, or an engine error (exit 2). The shim never fails
open — it only ever denies on a definite engine deny (exit 1). Requires `jq` and
`git`.

## Scope

The script is emitted into the Antigravity payload by #216. Install-time config
wiring — registering it in `.agents/hooks.json` for `PreToolUse` with a
tool-name regex matcher — is #217.
