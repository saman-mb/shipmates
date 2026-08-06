# LinkedIn caption

**Your AI agent shouldn't be able to merge a PR it hasn't finished reviewing.**

Shipmates compiles each command into a small state machine, and wires a
**pre-tool hook** into your coding harness. Before *every* tool call, the hook
asks `shipmates state gate` one question: *is this tool allowed in the phase the
run is actually in?* An out-of-phase `gh pr merge` — before the work has passed
review and reached the `deliver` phase — is denied at the tool boundary. Not a
lint. Not a suggestion. A hard stop.

The diagram shows it on **Codex**. The same gate runs on every harness with a
blocking pre-tool hook — **Claude Code, Cursor, Windsurf, Antigravity, GitHub
Copilot, and opencode** — each translated into that harness's own deny channel.
(A harness without a scriptable hook is out of scope, by design.)

And the diagram itself? Rendered by Shipmates' own `diagram` tool — one JSON
spec, one command, a deterministic on-brand animated GIF. No design app.

#AICodingAgents #DeveloperTools #Codex #ClaudeCode #OpenSource
