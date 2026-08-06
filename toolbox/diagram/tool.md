---
name: diagram
description: Render a curated diagram — a box-and-arrow flow, pipeline, or state machine, or a sequence/interaction with actors and message arrows — from a small JSON spec, as a committed SVG, a deterministic PNG, or an animated GIF. No browser, no mermaid runtime, no headless renderer; the SVG is assembled as text and the PNG/GIF are repainted from the same primitives. Reach for this whenever a task calls for a diagram to explain a design, pipeline, request path, state machine, or interaction in a README or docs page rather than describing it in prose or leaving a mermaid block for something else to render. Never a slash command; the crew reach for it implicitly when the intent calls for one.
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
python3 diagram.py --spec spec.json --out flow.svg   # SVG (standard library only)
python3 diagram.py --spec spec.json --out flow.png   # deterministic PNG
python3 diagram.py --spec spec.json --out flow.gif   # animated GIF (accent pulse)
# or pipe the spec on stdin, and pass --format when the extension is ambiguous:
echo '{"nodes":[…],"edges":[…]}' | python3 diagram.py --out flow.svg
```

The output format is inferred from the `--out` extension (`.svg` / `.png` /
`.gif`), or set it explicitly with `--format`. `--animate` is shorthand for a GIF.

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
