# Sample — "Every tool call clears the gate" (LinkedIn motion asset)

A premium, on-brand **motion-graphics** explainer of the Shipmates
hooks/enforcement system, for a LinkedIn post — reproducible from one script.

## What it shows (grounded in the real `/ship-issue` command)
`/ship-issue` compiles into a state machine (phases plan→isolate→build→verify→
review→deliver) with tool-gates `git push`→`build` and `gh pr merge`→`deliver`.
A per-harness `PreToolUse` hook calls `shipmates state gate` before **every**
tool call. The animation stages three beats, with **Codex** as the worked
example:

1. `git push` during **build** → the gate opens **green** (allowed).
2. `gh pr merge` during **build** → the gate slams **red** and the call is
   **repelled** (merge is gated to `deliver`).
3. the phase rail advances to **deliver**; the same `gh pr merge` is retried →
   the gate opens **green**. Same call, opposite answers — the gate enforces
   *order*, not a blanket block.

## Files
- `render_hooks_animation.py` — the self-contained generator (Pillow frames →
  ffmpeg). Deterministic: grain is seeded per frame, so a re-render matches.
  Flags: `--fast` (quick low-res iteration), `--frame N` (dump one frame PNG).
- `hooks-gate.mp4` — **the asset to post** (1080×1350, 4:5, 30fps, ~12s). Post
  the MP4: LinkedIn autoplays native video and it's sharper than the GIF.
- `hooks-gate.gif` — fallback (720×900) for surfaces that won't play video.
- `caption.md` — the LinkedIn post copy.

## Regenerate
```
python3 examples/linkedin-hooks/render_hooks_animation.py
```
Renders the full MP4 (H.264, CRF 18) + GIF. (The committed MP4 is transcoded to
a web-friendly CRF 22 / ~10 MB; LinkedIn re-encodes on upload regardless.)
