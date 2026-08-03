---
name: svgflow
description: Render a box-and-arrow flow, pipeline, or state diagram as a committed SVG from a small JSON spec — no browser, no mermaid runtime, no headless renderer. Reach for this whenever a task calls for a diagram to explain a design, pipeline, request path, or state machine in a README or docs page rather than describing it in prose or leaving a mermaid block for something else to render. The output self-sizes and is styled for dark pages. Never a slash command; the crew reach for it implicitly when the intent calls for one.
---

# svgflow

A tool the crew uses on its own. When you're writing a README, an architecture
note, or a docs page and the natural artifact is *a small diagram of how the
pieces connect* — a build pipeline, a request path, a state machine — draw one
with this instead of pasting a mermaid block that needs a runtime to render, or
describing the boxes in a paragraph.

The SVG it emits is plain text and fully deterministic, so it drops straight
into the repo next to the doc it illustrates and renders everywhere Markdown
shows images — no build step, no JavaScript.

## Run it

The renderer `svgflow.py` sits next to this file. It depends only on the Python
standard library — nothing to install.

```
python3 svgflow.py --spec spec.json --out flow.svg
# or pipe the spec on stdin:
echo '{"nodes":[…],"edges":[…]}' | python3 svgflow.py --out flow.svg
```

## Spec

```json
{
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

`direction` is `"down"` (a column, the default) or `"right"` (a row). Nodes are
laid out in declaration order and each box is sized to its `label`. Edges between
adjacent nodes draw as a straight spine arrow; a skip, a back-edge, or a
self-loop (`from` equal to `to`) bows out to the side so it stays readable. Edge
`label` is optional, and omitting `edges` entirely chains the nodes in order. The
output self-sizes — correct `width`, `height`, and `viewBox` — and is styled for
the dark tool pages: a rounded panel, light node cards, and a teal accent.

## Honesty

Draw the real components and the real transitions, with plain labels — no
invented services, states, or steps that the thing being documented doesn't
actually have. A diagram that lies is worse than a paragraph that's vague. This
is a layout tool, not a general vector editor: it does one clean thing — a single
column or row of boxes with labelled arrows — and for a genuinely two-dimensional
graph you want a real graph layout tool instead.
