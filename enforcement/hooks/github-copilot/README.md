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

> Caveat: the Copilot deny channel is format-verified but not yet proven live.
> Installation writes a project hook file under `.github/hooks/`.

## Fail-safe rules

Allowed with no output (never blocked): a non-shell tool, `main`, a detached
HEAD, a missing `.shipmates/run-<N>.json`, a non-git cwd, or no active run. The
installed dispatcher uses Rust JSON parsing and requires `git` plus `shipmates`.

## Scope

The script is emitted into the Copilot payload and installation registers it
under `.github/hooks/` for `preToolUse`.
