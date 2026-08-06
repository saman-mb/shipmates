# Sample — "One gate, every tool call" (LinkedIn asset)

A worked example of the Shipmates **`diagram`** tool generating a shippable,
on-brand **animated GIF** for a social post — dogfooding the tool ADR 0001
introduced (#221/#222/#235).

## What it shows
The `/ship-issue` enforcement FSM, grounded in the command's real `stages`
(plan→isolate→build→verify→review→deliver) and `tool_gates` (`git push`→build,
`gh pr merge`→deliver), with **Codex** as the worked example. A per-harness
`PreToolUse` hook calls `shipmates state gate` before every tool call:
`git push` is allowed in build, an early `gh pr merge` is denied (it needs
`deliver`), and the same `gh pr merge` is allowed once the run reaches `deliver`.
The gate enforces order, not a blanket block.

## How it was generated (one command, no design app)
```
python3 diagram.py \
  --spec examples/linkedin-hooks/hooks-codex.json \
  --out  examples/linkedin-hooks/hooks-codex.gif \
  --animate reveal --scale 3
```
- `hooks-codex.json` — the sequence spec (`"animation": "reveal"`).
- `hooks-codex.gif` — the **reveal-animated**, hi-res (1764×2316, `--scale 3`)
  LinkedIn asset: the exchange builds up message-by-message, then holds.
- `hooks-codex.png` — a static final-frame preview.

Deterministic and self-contained: the same spec always renders the same bytes,
theme baked in — safe to regenerate and commit.
