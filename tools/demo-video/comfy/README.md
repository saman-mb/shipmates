# MiniMax H3 b-roll pipeline (ComfyUI, headless)

Cinematic, **text-free** b-roll for the 45s demo clip. Storyboard and the
research behind the no-text rule: [`docs/DEMO_VIDEO.md`](../../../docs/DEMO_VIDEO.md)
§6 and §6b.

Everything the crew renders comes from `shots.json` plus a seed, driven over
ComfyUI's HTTP API by `render.py` — no GUI, no ComfyUI import, Python stdlib
only. The workflows are committed in ComfyUI's **API format**, which the
ComfyUI frontend also loads (`Workflow → Open`), so one file serves both the
headless driver and hand-tweaking in the GUI.

## The one hard rule

**AI video never renders text.** Glyphs below roughly 3% of frame height drift
and garble frame to frame, and terminals are the worst case: small, dense,
monospace, every character exact. B-roll paints mood; every readable character
in the video is stamped by Remotion. A shot containing legible text is rejected
by definition, not judged on merit.

## Prerequisites

A ComfyUI install with the MiniMax H3 weights (~42 GB) in place. Download them
from the upstream repos — **never redistribute the weights**:

| Folder | File | Size |
|---|---|---|
| `models/diffusion_models/` | [`minimax_h3_fl2va_pruned_int8_convrot.safetensors`](https://huggingface.co/Comfy-Org/MiniMax-H3/resolve/main/diffusion_models/minimax_h3_fl2va_pruned_int8_convrot.safetensors) | 21 GB |
| `models/text_encoders/` | [`qwen3vl_32b_minimax_h3_nvfp4_awq.safetensors`](https://huggingface.co/Comfy-Org/MiniMax-H3/resolve/main/text_encoders/qwen3vl_32b_minimax_h3_nvfp4_awq.safetensors) | 16 GB |
| `models/vae/` | [`minimax_h3_video_vae_fp16.safetensors`](https://huggingface.co/Comfy-Org/MiniMax-H3/resolve/main/vae/minimax_h3_video_vae_fp16.safetensors) | 5 GB |
| `models/loras/` *(optional)* | [`minimax_h3_fl2v_turbo_8step_v1.0_comfyui_bf16.safetensors`](https://huggingface.co/lightx2v/Minimax-h3-Turbo) | — |

The audio VAE the stock template loads is **not** needed here (see
[Design decisions](#design-decisions)).

Hardware: a big-VRAM or unified-memory box. Developed on a Strix Halo (Radeon
8060S, ROCm). Expect minutes per clip at 768p.

Start the server, then drive it from this directory:

```bash
cd /path/to/ComfyUI && python main.py --listen 127.0.0.1 --port 8188
```

## Quickstart

```bash
python3 render.py --list                              # what shots exist
python3 render.py --shot hook-atmosphere --dry-run    # resolve, print, queue nothing
python3 render.py --shot hook-atmosphere              # render it
```

Point at a non-default server with `--server http://host:port` or
`COMFY_SERVER`. Iterate cheaply with `--turbo --preset 480p-16x9`, then drop
both flags for the take you keep.

## Shots

`shots.json` is the registry: one entry per shot, each pinning its own prompt,
pipeline, plate and seed.

| Shot | Pipeline | Used by |
|---|---|---|
| `hook-atmosphere` | i2v | Beat 1 hook underlay |
| `hook-atmosphere-explore` | t2v | Exploration only |
| `epic-transition` | i2v | Beat 4 → 5 hard cut |
| `epic-transition-explore` | t2v | Exploration only |

**I2V is the primary path.** The plate pins the composition and H3 animates
camera and lighting only — which is what you want next to crisp UI. The `-explore`
T2V twins let the model invent the frame; useful for finding a look, but the
composition is uncontrolled, so a T2V take never gets cut against UI. They also
run with no plate, so they are the quickest way to check the pipeline works.

### Plates

I2V shots read `plates/<name>.png`, uploaded to the server automatically at
queue time. Author the plate however you like — a Flux or SDXL still in ComfyUI
is the intended path — subject to one rule: **zero legible text**, including
signage, watermarks and UI chrome. Render it at the shot's preset resolution so
H3 does not crop it.

## Presets and duration

| Preset | Size | Use |
|---|---|---|
| `768p-16x9` | 1344×768 | Default. H3's official 768p, for the 1920×1080 export. |
| `768p-1x1` | 768×768 | For the 1080×1080 square feed export. |
| `480p-16x9` | 864×480 | Iteration only. Never final. |

H3's canvas is a 768px short edge capped at 768×1344, rounded to a multiple of
32; the presets are pre-computed points on that grid, so nothing gets cropped
on the way into Remotion.

Duration snaps **up** to H3's 17k+5 frame grid at 24 fps — `render.py` mirrors
`align_frame_count` from ComfyUI's `nodes_minimax_h3.py`, so `--dry-run` shows
the frame count you will actually get. The default 5.0s resolves to 124 frames
(5.167s), which is the bottom of the model's trained range (~124–362 frames).
**Generate at or above 124 frames and trim in the edit** rather than asking for
a 2s clip the model was never trained to make.

## Output contract

```
out/<shot-id>/<seed>-<prompt-hash>.mp4     the clip, ready for Remotion
out/<shot-id>/<seed>-<prompt-hash>.json    every resolved parameter
```

The prompt hash is the first 8 hex of the SHA-256 of the final prompt
(shot prompt + shared guardrail), so a re-render after a prompt edit lands
beside the old take instead of silently overwriting it. `out/` is gitignored:
the seed and the sidecar are the reproducible artifact, not the bytes.

Determinism was verified on the development host — re-queueing a seed after a
different seed had evicted ComfyUI's execution cache produced a byte-identical
MP4. Determinism *across hosts* is not claimed.

## Design decisions

- **No negative prompt exists, so the guardrail lives in the positive prompt.**
  `MiniMaxH3ImageToVideo` emits positive conditioning only, and sampling runs
  through `BasicGuider` — there is no CFG branch and no negative input to
  attach "no text, no watermark" to. Rather than invent a key the node does not
  have, the no-text guardrail is appended to every prompt from a single
  `guardrail` string in `shots.json`. Change it once, every shot changes.
- **The clips are silent on purpose.** H3 generates native stereo audio jointly
  with video, but `docs/DEMO_VIDEO.md` §5 authors its own bed — narration,
  ~85 BPM pulse, checklist SFX. So `CreateVideo.audio` is left unconnected and
  the audio VAE and `VAEDecodeAudio` are dropped from the graph entirely,
  saving a decode and a model load per run.
- **Frame-count maths lives in Python, not in the graph.** The stock template
  computes `length` with a `ComfyMathExpression` node. Doing it in the driver
  makes the resolved frame count visible to `--dry-run` and recordable in the
  sidecar.
- **The turbo LoRA is injected by the driver, not switched in the graph.** The
  stock template toggles it with `ComfySwitchNode`, which is GUI-shaped. Under
  `--turbo` the driver adds the `LoraLoaderModelOnly` node and repoints the
  guider and scheduler at it, so the graph that gets queued contains only what
  actually runs.
- **Node ids match the upstream template** where the node is the same one, which
  keeps diffing this pipeline against a refreshed stock template cheap.

## Related

- Shot generation and approval: [#358](https://github.com/saman-mb/shipmates/issues/358)
- Publishing this pipeline for community use: [#363](https://github.com/saman-mb/shipmates/issues/363)
