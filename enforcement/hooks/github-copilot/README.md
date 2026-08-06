# GitHub Copilot FSM tool-gate hook

`fsm-gate.sh` is a GitHub Copilot `preToolUse` shim that binds a Shipmates
command run to its finite-state machine: it blocks a shell command a `tool_gates`
binding gates until the run has reached the required stage.

It is a thin wrapper over the engine — `shipmates state gate --run <N> --tool
"<command>"` (exit 0 allow / 1 deny / 2 error). The shim owns run discovery
(from the `feat/issue-<N>[-<slug>]` branch + `.shipmates/run-<N>.json`), the
fail-safe rules (allow anything it cannot unambiguously identify), gating ONLY
the shell tool, and the translation of a deny verdict into Copilot's decision
JSON.

## Deny form

On a deny the shim writes JSON with `permissionDecision: "deny"` (plus a
`permissionDecisionReason`) to stdout and exits 0, so Copilot reads the decision
rather than a hook fault. Allow is a silent `exit 0`; an engine error is a
fail-safe allow with a stderr log.

> Caveat: the Copilot deny channel is not yet verified live — the exact
> `preToolUse` response shape that produces a real block still needs a
> verify-live pass. That, with all install-time wiring, is #217.

## Fail-safe rules

Allowed with no output (never blocked): a non-shell tool, `main`, a detached
HEAD, a `feat/bundle-*` branch, a missing `.shipmates/run-<N>.json`, a non-git
cwd, no `shipmates` on `PATH`, or an engine error (exit 2). The shim never fails
open — it only ever denies on a definite engine deny (exit 1). Requires `jq` and
`git`.

## Scope

The script is emitted into the Copilot payload by #216. Install-time config
wiring — registering it under `.github/hooks/` for `preToolUse` — is #217.
