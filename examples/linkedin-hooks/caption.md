# LinkedIn caption

**Your AI agent shouldn't be able to merge a PR it hasn't finished reviewing.**

Watch the gate work, step by step. Shipmates compiles the `/ship-issue` command
into a state machine, and wires a **pre-tool hook** into your coding harness.
Before *every* tool call, the hook asks `shipmates state gate` one question: *is
this tool allowed in the phase the run is actually in?*

- `git push` in the **build** phase → **allowed**.
- `gh pr merge` while still in **build** → **denied** — merge is gated to the
  `deliver` phase, after review.
- `gh pr merge` once the run reaches **deliver** → **allowed**.

Same tool, opposite answers — because the gate enforces *order*, not a blanket
block. It's a hard stop at the tool boundary, not a lint or a suggestion.

The diagram shows it on **Codex**. The same gate runs on every harness with a
blocking pre-tool hook — **Claude Code, Cursor, Windsurf, Antigravity, GitHub
Copilot, and opencode** — each translated into that harness's own deny channel.

And the animation itself? Rendered by Shipmates' own `diagram` tool — one JSON
spec, one command (`--animate reveal`), a deterministic on-brand animated GIF.
No design app.

#AICodingAgents #DeveloperTools #Codex #ClaudeCode #OpenSource
