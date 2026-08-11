# Codex FSM tool-gate hook (experimental)

`fsm-gate.sh` is a Codex `PreToolUse` shim that binds a Shipmates command run to
its finite-state machine: it blocks a shell command a `tool_gates` binding gates
until the run has reached the required stage.

It is a thin wrapper over the engine — `shipmates state gate --run <N> --tool
"<command>"` (exit 0 allow / 1 deny / 2 error). The shim owns run discovery
(from the `feat/issue-<N>[-<slug>]` branch + `.shipmates/run-<N>.json`), the
fail-safe rules (allow anything it cannot unambiguously identify), gating ONLY
the shell tool, and the translation of a deny verdict into Codex's decision JSON.

## Deny form

On a deny the shim writes JSON with `hookSpecificOutput` containing
`permissionDecision: "deny"` (plus a `permissionDecisionReason`) to stdout and
exits 0, so Codex reads the decision rather than a hook fault. Allow is a silent
`exit 0`; an engine error is a fail-safe allow with a stderr log.

> Caveat: Codex hooks are experimental, and this `PreToolUse` event-JSON shape and
> deny response are format-verified but not yet proven against a running Codex.
> Installation enables the canonical `[features].hooks = true` flag and writes
> the project hook config.

## Fail-safe rules

Allowed with no output (never blocked): a non-shell tool, `main`, a detached
HEAD, a missing `.shipmates/run-<N>.json`, a non-git cwd, or no active run. The
installed dispatcher uses Rust JSON parsing and requires `git` plus `shipmates`.

## Scope

The script is emitted into the Codex payload. Codex hooks remain experimental and
shell-tool-only; installation registers `.codex/hooks.json` and sets the
canonical `config.toml [features].hooks = true` flag.
Codex may require a one-time `/hooks` review/trust action before a changed
project hook executes.
