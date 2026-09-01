---
name: shipmates-sparkline
description: Shipmates: Turn a short series of numbers — a benchmark trend, a metrics readout, a latency/throughput/error-rate history — into a tiny inline trend chart as a self-contained SVG. Reach for this whenever a task hands over a run of numbers and the honest artifact is a single small chart showing the shape of the trend rather than a wall of digits (a benchmark note, a metrics summary, a README stat, a status line). Never a slash command; use it implicitly when the intent calls for one.
---

# sparkline

A tool the crew uses on its own. When you're writing up a benchmark, a metrics
report, or a status note and the data is *a short series of numbers*, render its
trend as one tiny chart with this instead of pasting a row of digits and asking
the reader to picture the curve.

## Run it

The renderer `sparkline.py` sits next to this file. It has **no dependencies** —
just `python3` and the standard library. The chart is written as plain SVG text
(scalable, self-sizing) and verified as valid XML before it is written.

```
python3 sparkline.py --data "12,18,9,22,15,27" --out spark.svg
python3 sparkline.py --data "182 168 150 121 96 88" --label "p95 ms" --color coral --out lat.svg
```

## Options

`--data` (required) is a comma- or whitespace-separated run of numbers; `--out`
(required) is the SVG path. Optional: `--label` draws a small caption top-left;
`--width` / `--height` size the canvas (default `240x60`); `--color` sets the
stroke — a hex (`#5fd2dc`, `#abc`) or a named swatch
(`teal cyan green blue purple orange coral gold sage white grey`). `--no-baseline`
drops the muted min/max reference lines; `--bare` renders on a transparent
background instead of the dark panel.

The series is scaled to its own min/max, drawn as a smooth polyline over a faint
gradient fill, with the last point marked by a dot — so the chart shows the
*shape* of the trend, not absolute magnitudes. An empty series exits `2` with a
message; a single point renders as a lone dot.

## Honesty

Plot the real numbers a task hands you — never invent, smooth away, or reorder
points to flatter the trend. A sparkline is a compression of the data, not a
substitute for it: when exact values matter, keep them alongside the chart.
