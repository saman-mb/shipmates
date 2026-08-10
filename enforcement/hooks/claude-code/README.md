# Claude Code FSM tool-gate hook (reference)

`fsm-gate.sh` is the reference PreToolUse shim that binds a Shipmates command run
to its finite-state machine: it blocks a `Bash` tool call whose command a
`tool_gates` binding gates until the run has reached the required stage.

It is a thin wrapper over the engine — `shipmates state gate --run <N> --tool
"<command>"` (exit 0 allow / 1 deny / 2 error). The shim only owns run discovery
(from the `feat/issue-<N>[-<slug>]` branch + `.shipmates/run-<N>.json`), the
fail-safe rules (allow anything it cannot unambiguously identify), and the
translation of a deny verdict into Claude Code's permission-decision JSON.

## Wire it

Add a PreToolUse hook for the `Bash` matcher in your Claude Code settings
(`.claude/settings.json`), pointing at this script:

```json
{
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "Bash",
        "hooks": [
          { "type": "command", "command": "/absolute/path/to/fsm-gate.sh" }
        ]
      }
    ]
  }
}
```

Installed configurations set `SHIPMATES_NATIVE_HOOK=1`, routing through
`shipmates hook gate` so the production path needs only `git` and `shipmates` on
`PATH`; ordinary sessions with no identified run remain allowed. Manual wiring
without that flag uses the legacy shell parser and requires `jq`.

## Scope

This is the Claude Code reference implementation, and the pattern every other
harness's shim mirrors (`enforcement/hooks/<harness>/`). As of #216 the script
IS emitted into every harness's payload — each adapter's `build()` writes its
shim via `emit_hook_shim` (Claude Code lands it at `.claude/hooks/fsm-gate.sh`).
`shipmates install` now merges the registration into `settings.json` without
clobbering existing hooks, marks the shell shim executable, and uses the same
registration path on reinstall. `shipmates doctor` reports missing registration.
