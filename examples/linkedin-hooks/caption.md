# LinkedIn caption

**Your AI coding agent shouldn't be able to merge a PR it hasn't finished reviewing.**

Watch the gate work. Shipmates compiles the `/ship-issue` command into a state
machine and wires a **pre-tool hook** into your coding harness. Before *every*
tool call, the hook asks `shipmates state gate` one question — *is this tool
allowed in the phase the run is actually in?* — and translates the answer into
the harness's own allow/deny.

- `git push` in the **build** phase → **allowed**.
- `gh pr merge` while still in **build** → **denied**. Merge is gated to
  `deliver`, after review.
- `gh pr merge` once the run reaches **deliver** → **allowed**.

Same tool, opposite answers — because the gate enforces *order*, not a blanket
block. A hard stop at the tool boundary, not a lint, not a suggestion.

Shown on **Codex**. The same gate runs on every harness with a blocking
pre-tool hook — **Claude Code, Cursor, Windsurf, Antigravity, GitHub Copilot,
and opencode** — each translated into that harness's own deny channel.

#AICodingAgents #DeveloperTools #Codex #ClaudeCode #OpenSource
