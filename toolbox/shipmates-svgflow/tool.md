---
name: shipmates-svgflow
description: Shipmates: Deprecated alias for the `diagram` tool. svgflow's flow diagram is now the default `kind` of `diagram`, which also renders PNG and animated GIF and adds a sequence kind. This shim forwards to diagram.py so nothing that already reaches for svgflow breaks; reach for `diagram` instead. Never a slash command.
---

# svgflow (deprecated → `diagram`)

**svgflow is now [`diagram`](../diagram/tool.md).** Flow became a *kind* of a more
general diagram tool (ADR 0001) that keeps svgflow's theme-exact, deterministic
SVG and adds:

- **PNG** and **animated GIF** output, repainted from the same primitives;
- a **sequence** kind (actors, dashed lifelines, message arrows);
- **intent routing** — pick the `kind`, or let a `prompt`/`intent` string route.

The default kind is `flow`, so an existing svgflow spec renders exactly the same
through `diagram`.

## Run it

`svgflow.py` is kept for one release as a thin deprecation shim: it prints a
one-line deprecation notice to stderr and forwards every argument, unchanged, to
`diagram.py`. The shim only forwards — it needs the `diagram` tool installed
beside it; installing `svgflow` alone will tell you to install `diagram`.
Existing calls keep working:

```
python3 svgflow.py --spec spec.json --out flow.svg   # forwards to diagram.py
```

Prefer `diagram` directly — see its [tool.md](../diagram/tool.md):

```
python3 diagram.py --spec spec.json --out flow.svg
python3 diagram.py --spec spec.json --out flow.png
python3 diagram.py --spec spec.json --out flow.gif
```
