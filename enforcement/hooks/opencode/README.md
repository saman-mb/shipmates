# opencode FSM tool-gate plugin

`fsm-gate.ts` is an opencode `tool.execute.before` plugin that binds a Shipmates
command run to its finite-state machine: it blocks a shell (`bash`) tool call a
`tool_gates` binding gates until the run has reached the required stage.

Unlike the other harnesses, opencode has no exit-code hook contract — a plugin
**denies a tool call by throwing an Error** from `tool.execute.before`. This
plugin replicates the reference shim's run discovery (parse the
`feat/issue-<N>[-<slug>]` or `feat/bundle-<N>[-<slug>]` branch → read
`.shipmates/run-<N>.json`) and shells out
to `shipmates state gate` via Bun's `$`, throwing on a deny.

## Deny form

`throw new Error(reason)` from `tool.execute.before`. Allow is a plain `return`;
an engine error is a fail-safe allow (return, no throw).

## Known gap — opencode #5894 (subagent bypass)

`tool.execute.before` does **not** fire for tool calls made inside a spawned
subagent, so a builder subagent's `bash` calls are not gated by this plugin. The
gate covers the primary agent only until opencode propagates tool hooks into
subagents. Tracked upstream at <https://github.com/sst/opencode/issues/5894>.

## Fail-safe rules

Allowed (never blocked): a non-`bash` tool, `main`, a detached HEAD, a
`feat/bundle-*` branch, a missing `.shipmates/run-<N>.json`, a non-git dir, no
`shipmates` on `PATH`, or an engine error (exit 2). The plugin never fails open —
it only ever throws on a definite engine deny (exit 1). Requires `git` and a
`shipmates` on `PATH`; runs on opencode's Bun runtime.

## Scope

The plugin is emitted into the opencode payload and installed under
`.opencode/plugins/`, which opencode auto-loads. No separate registration file is
needed.
