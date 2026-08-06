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

Requires `jq`, `git`, and `shipmates` on `PATH`. If any is missing, or the run
cannot be identified, the shim allows the call — it never blocks an ambiguous or
unknown run.

## Scope

This is the Claude Code reference implementation. Emitting a per-harness shim for
the other supported harnesses (and wiring it into each adapter's install output)
is a follow-up (#217); nothing installs this script yet.
