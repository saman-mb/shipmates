# Sample — "Codex blocks an out-of-phase merge" (LinkedIn asset)

A worked example of the Shipmates **`diagram`** tool generating a shippable,
on-brand **animated GIF** for a social post — dogfooding the tool that ADR 0001
introduced (#221/#222).

## What it shows
The Shipmates hooks/enforcement system, with **Codex** as the worked example: a
per-harness `PreToolUse` hook calls `shipmates state gate` before every tool
call, and an out-of-phase tool (here `gh pr merge` before the run reaches the
`deliver` phase) is **hard-denied** — the workflow is enforced, not merely
suggested.

## How it was generated (the whole point — one command, no design app)
```
python3 toolbox/diagram/diagram.py \
  --spec examples/linkedin-hooks/hooks-codex.json \
  --out  examples/linkedin-hooks/hooks-codex.gif \
  --animate --scale 3
```
- `hooks-codex.json` — the sequence spec (actors + messages).
- `hooks-codex.gif` — the animated, hi-res (1764×1224, `--scale 3`) LinkedIn asset.
- `hooks-codex.png` — a static preview / fallback.

Deterministic and self-contained: the same spec always renders the same bytes,
with the theme baked in — safe to regenerate and commit.
