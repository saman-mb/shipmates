---
name: diagram
description: Render a curated diagram — a box-and-arrow flow, pipeline, or state machine, or a sequence/interaction with actors and message arrows — from a small JSON spec. Outputs a committed SVG for docs, a deterministic PNG (with `--scale N` for crisp hi-res exports for slides, social, or LinkedIn), or an animated GIF (a subtle accent `pulse`, or a step-by-step `reveal` that builds the diagram up to explain it). Pick the diagram type with `kind` (flow or sequence) or let it route from a `prompt`; no browser, no mermaid runtime, no headless renderer — the SVG is text and the PNG/GIF are repainted from the same primitives, byte-for-byte deterministic. Reach for this whenever a task calls for a diagram to explain a design, pipeline, request path, state machine, or interaction — in a README or docs page (SVG), a slide or social post (hi-res PNG/GIF), or a step-by-step explainer (reveal GIF) — rather than describing it in prose or leaving a mermaid block for something else to render. Never a slash command; the crew reach for it implicitly when the intent calls for one.
requires: pillow
---

# diagram

A tool the crew uses on its own. When you're writing a README, an architecture
note, or a docs page and the natural artifact is *a small diagram of how the
pieces connect or talk* — a build pipeline, a request path, a state machine, an
interaction between services — draw one with this instead of pasting a mermaid
block that needs a runtime to render, or describing the boxes in a paragraph.

`diagram` is the evolution of the older `svgflow` tool (`svgflow` is now a thin
deprecated alias that forwards here). It keeps svgflow's signature — a theme-exact,
byte-for-byte deterministic SVG assembled as text — and adds two more outputs and
a second diagram type.

## Run it

The renderer `diagram.py` sits next to this file.

```
python3 diagram.py --spec spec.json --out flow.svg           # SVG (standard library only)
python3 diagram.py --spec spec.json --out flow.png           # deterministic PNG
python3 diagram.py --spec spec.json --out flow.png --scale 3  # hi-res PNG (3×) for slides / social
python3 diagram.py --spec spec.json --out flow.gif           # animated GIF (accent pulse)
python3 diagram.py --spec spec.json --out flow.gif --animate reveal  # build-up reveal GIF
python3 diagram.py --spec spec.json --out flow.gif --animate reveal --scale 3  # hi-res reveal for a post
# or pipe the spec on stdin, and pass --format when the extension is ambiguous:
echo '{"nodes":[…],"edges":[…]}' | python3 diagram.py --out flow.svg
```

The output format is inferred from the `--out` extension (`.svg` / `.png` /
`.gif`), or set it explicitly with `--format`. `--animate` is shorthand for a GIF.

**Choosing an output.** Reach for **SVG** when the diagram is committed next to a
doc (crisp at any size, tiny, pure text). Reach for a **PNG** — with `--scale N`
(2–3×) — when it needs to look sharp in a slide, a social/LinkedIn post, or
anywhere a raster is expected. Reach for a **GIF** to show motion: `pulse` for a
subtle living-diagram loop, or `reveal` to walk a reader through the steps. The
raster outputs (`--scale`) are the way to export a presentation- or
social-quality image straight from the same spec.

### Hi-res export — `--scale`

`--scale N` (an integer ≥ 1, default 1) multiplies the raster (PNG/GIF)
resolution: the tool paints at N× and downsamples, so a `--scale 3` PNG or GIF is
a crisp, presentation-grade asset (e.g. a 274×384 flow becomes 822×1152) with no
change to the layout — the same spec, just sharper. It has no effect on SVG (which
is already resolution-independent). Use `--scale 2` or `--scale 3` for anything
that will be viewed large; leave it at 1 for an inline thumbnail.

### Animation modes

A GIF animates in one of two modes:

- **`pulse`** (default) — a gentle accent pulse loops along the arrows. This is
  what a bare `.gif` / `--format gif` / `--animate` produces, unchanged.
- **`reveal`** — the diagram *builds up* element by element over the frames, then
  the complete picture holds a longer beat before the loop. The canvas stays a
  constant, full-diagram size across every frame, so nothing jumps — each frame
  draws only the revealed subset on the full-size canvas. This is **sequence-first**:
  a sequence reveals its always-on scaffold (title, actor cards, lifelines) plus
  its messages one at a time, in order; a flow reveals its nodes in declaration
  order, each edge appearing once both its endpoints are on screen.

Pick the mode with `--animate reveal` / `--animate pulse` (a bare `--animate` is
`pulse`), or set it in the spec with a top-level `"animation": "reveal"` /
`"pulse"` field; either implies a GIF, and the CLI value overrides the spec.
Reveal frames are ordered with fixed hold times — no clock, no randomness — so the
same spec+mode renders byte-identically run to run, exactly like the pulse GIF and
the PNG. `reveal` is a GIF-only concern; the SVG output is unaffected.

- **SVG** needs nothing installed — it is pure Python standard library, assembled
  as text, and drops straight into the repo next to the doc it illustrates.
- **PNG and GIF** need Pillow, which the tool provisions for itself the first time
  it renders raster output — you never install anything. It pins one Pillow
  version and caches it per-version under `~/.cache/shipmates/pylib/Pillow-<version>/`,
  placed first on the import path so that pinned build is authoritative (a
  differently-versioned system Pillow can't shadow it) — this is what keeps raster
  output byte-reproducible run to run. Because that cache dir sits first on the
  import path it is created user-private (`0700`) and must stay trusted. If the
  pinned version can't be provisioned (no pip, or offline) it falls back to a
  system Pillow and warns on stderr that output may not be byte-reproducible. Run
  `python3 diagram.py --provision` to place the raster dependency ahead of time.

Raster text is rendered with an **embedded** DejaVu Sans Mono face, loaded from
memory, so PNG/GIF text is identical on every host — there is no per-host font
fallback.

## Intent routing

The builder is chosen by an explicit `kind` field. Omit it and the tool routes on
a `prompt` (or `intent`) string through a small keyword map — `"sequence"`,
`"interaction"`, `"actor"`, `"message"` route to a sequence diagram; `"flow"`,
`"pipeline"`, `"state"` route to a flow. With neither a `kind` nor a matching
prompt it defaults to `flow`, so every existing svgflow spec renders unchanged.
The routing is a plain dictionary — deterministic, no model call.

## Kinds

### flow (default)

A box-and-arrow flow, pipeline, or state diagram.

```json
{
  "kind": "flow",
  "direction": "down",
  "title": "CI pipeline",
  "nodes": [
    {"id": "build",  "label": "Build"},
    {"id": "test",   "label": "Test"},
    {"id": "deploy", "label": "Deploy"}
  ],
  "edges": [
    {"from": "build", "to": "test",   "label": "on push"},
    {"from": "test",  "to": "deploy", "label": "green"},
    {"from": "test",  "to": "build",  "label": "red → retry"}
  ]
}
```

`direction` is `"down"` (a column, the default) or `"right"` (a row). Nodes lay
out in declaration order, each box sized to its `label`. Adjacent nodes join with
a straight spine arrow; a skip, a back-edge, or a self-loop (`from` equal to `to`)
bows out to the side so it stays readable. Edge `label` is optional, and omitting
`edges` chains the nodes in order.

### sequence

Actors across the top, a dashed lifeline dropping from each, and directional
message arrows between them in order. Set `"return": true` on a message for a
dashed reply arrow.

```json
{
  "kind": "sequence",
  "title": "Checkout — request path",
  "actors": ["Client", "API", "Payments", "DB"],
  "messages": [
    {"from": "Client",   "to": "API",      "label": "POST /checkout"},
    {"from": "API",      "to": "Payments", "label": "charge"},
    {"from": "Payments", "to": "API",      "label": "ok", "return": true},
    {"from": "API",      "to": "DB",       "label": "save order"},
    {"from": "API",      "to": "Client",   "label": "201 Created", "return": true}
  ]
}
```

Both kinds render in SVG and PNG, and are styled for the dark tool pages: a
rounded warm panel, elevated cards, and a terracotta accent.

## Honesty

Draw the real components and the real transitions or messages, with plain labels —
no invented services, states, or steps that the thing being documented doesn't
actually have. A diagram that lies is worse than a paragraph that's vague.

This is a **curated menu of hand-laid diagram types**, not a general graph
renderer: there is **no auto-layout**, and it deliberately does a small set of
clean things (a column/row of boxes with labelled arrows; actors with message
arrows) rather than arbitrary two-dimensional graphs. For a genuinely
two-dimensional graph you want a real graph layout tool instead. The **PNG is a
faithful, deterministic re-render of the same spec** — the same spec twice is
byte-identical — but it is repainted from the primitives, **not pixel-identical to
the SVG** (the two painters are only semantically equal). The optional SVG could
carry SMIL animation, but SMIL is browser-only and does not survive
rasterization, so animation ships as the GIF.
