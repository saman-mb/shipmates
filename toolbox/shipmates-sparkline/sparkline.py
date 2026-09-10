#!/usr/bin/env python3
"""sparkline — turn a short number series into a tiny inline trend chart (SVG).

Self-contained: the only dependency is the Python standard library. The chart is
written as plain SVG text — a scalable, self-sizing vector, legible on the dark
tool pages (a light teal stroke on a subtle dark panel by default). The output is
verified as valid XML with `xml.etree` before it is written.

This is the runnable payload of the shipmates `sparkline` tool. An agent reaches
for it, per tool.md, when a task hands over a short series of numbers — a
benchmark trend, a metrics readout, a latency/throughput/error-rate history — and
a single tiny chart tells the story better than a wall of digits. It is never a
slash command.

Usage:
    python3 sparkline.py --data "12,18,9,22,15,27" --out spark.svg
    python3 sparkline.py --data "120 90 74 61 58" --label "p95 ms" --color coral --out lat.svg

Options:
    --data     comma- or whitespace-separated numbers (required)
    --out      output .svg path (required)
    --label    small caption drawn at the top-left (optional)
    --width    canvas width in px   (default 240)
    --height   canvas height in px  (default 60)
    --color    stroke colour — a hex (#5fd2dc / #abc) or a named swatch
               (teal cyan green blue purple orange coral gold sage white grey)
    --no-baseline   drop the muted min/max reference lines
    --bare          transparent — no dark panel background

Exit codes: 0 ok; 2 bad data/usage (empty series, unparseable number, bad colour).
"""
import argparse
import re
import sys
import xml.etree.ElementTree as ET

# --- Palette (tuned to read on the dark #011627 tool panels) ----------------
NAMED = {
    "teal":   "#43d9c2",
    "cyan":   "#5fd2dc",
    "green":  "#7ee787",
    "blue":   "#82aaff",
    "purple": "#aa96ff",
    "orange": "#ffb45a",
    "coral":  "#ff9182",
    "gold":   "#f5c864",
    "sage":   "#96c8a0",
    "white":  "#dee8f0",
    "grey":   "#788c9e",
}
DEFAULT_COLOR = "cyan"

PANEL   = "#011627"   # subtle dark panel, matched to the termgif/social-card frame
BORDER  = "#20364a"
BASELN  = "#33475b"   # muted min/max reference lines
LABEL   = "#8497a9"   # muted caption text

HEX_RE = re.compile(r"^#(?:[0-9a-fA-F]{3}|[0-9a-fA-F]{6})$")
NAME_RE = re.compile(r"^[a-zA-Z]+$")


def parse_data(raw):
    """Comma/whitespace-separated -> list of floats. Raises ValueError."""
    tokens = [t for t in re.split(r"[,\s]+", raw.strip()) if t]
    if not tokens:
        raise ValueError("no numbers in --data (the series is empty)")
    out = []
    for t in tokens:
        try:
            out.append(float(t))
        except ValueError:
            raise ValueError(f"not a number: {t!r}")
    return out


def resolve_color(spec):
    """A hex string, a named swatch, or a bare CSS colour name -> a colour str."""
    s = spec.strip()
    if HEX_RE.match(s):
        return s
    low = s.lower()
    if low in NAMED:
        return NAMED[low]
    if NAME_RE.match(s):          # a plain CSS colour name (e.g. "tomato")
        return low
    raise ValueError(f"bad --color: {spec!r} (use a hex like #5fd2dc or a name)")


def _fmt(v):
    """Compact fixed-precision number for SVG coordinates."""
    return f"{v:.2f}".rstrip("0").rstrip(".")


def smooth_path(pts):
    """A Catmull-Rom spline through pts, emitted as cubic-bezier path data."""
    if len(pts) == 1:
        x, y = pts[0]
        return f"M {_fmt(x)} {_fmt(y)}"
    d = [f"M {_fmt(pts[0][0])} {_fmt(pts[0][1])}"]
    n = len(pts)
    for i in range(n - 1):
        p0 = pts[i - 1] if i > 0 else pts[0]
        p1 = pts[i]
        p2 = pts[i + 1]
        p3 = pts[i + 2] if i + 2 < n else pts[-1]
        c1x = p1[0] + (p2[0] - p0[0]) / 6.0
        c1y = p1[1] + (p2[1] - p0[1]) / 6.0
        c2x = p2[0] - (p3[0] - p1[0]) / 6.0
        c2y = p2[1] - (p3[1] - p1[1]) / 6.0
        d.append(f"C {_fmt(c1x)} {_fmt(c1y)}, {_fmt(c2x)} {_fmt(c2y)}, "
                 f"{_fmt(p2[0])} {_fmt(p2[1])}")
    return " ".join(d)


def build_svg(values, width, height, color, label, baseline, bare):
    """Compose the sparkline as an SVG string (verified valid XML by the caller)."""
    W, H = float(width), float(height)
    pad_l, pad_r, pad_b = 9.0, 11.0, 10.0
    pad_t = 20.0 if label else 9.0
    left, right = pad_l, W - pad_r
    top, bottom = pad_t, H - pad_b
    if right <= left:
        right = left + 1.0
    if bottom <= top:
        bottom = top + 1.0

    vmin, vmax = min(values), max(values)
    span = vmax - vmin
    n = len(values)

    def x_at(i):
        return (left + right) / 2.0 if n == 1 else left + i * (right - left) / (n - 1)

    def y_at(v):
        t = 0.5 if span == 0 else (v - vmin) / span
        return bottom - t * (bottom - top)

    pts = [(x_at(i), y_at(v)) for i, v in enumerate(values)]
    line_d = smooth_path(pts)
    fill_id = "sparkFill"

    parts = [
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{_fmt(W)}" '
        f'height="{_fmt(H)}" viewBox="0 0 {_fmt(W)} {_fmt(H)}" '
        f'role="img" aria-label="{_esc(label or "sparkline")} trend chart">',
        '<defs>',
        f'<linearGradient id="{fill_id}" x1="0" y1="0" x2="0" y2="1">',
        f'<stop offset="0" stop-color="{color}" stop-opacity="0.28"/>',
        f'<stop offset="1" stop-color="{color}" stop-opacity="0"/>',
        '</linearGradient>',
        '</defs>',
    ]

    if not bare:
        parts.append(
            f'<rect x="0.5" y="0.5" width="{_fmt(W - 1)}" height="{_fmt(H - 1)}" '
            f'rx="10" ry="10" fill="{PANEL}" stroke="{BORDER}" stroke-width="1"/>'
        )

    if baseline and span > 0:
        for v in (vmax, vmin):
            y = y_at(v)
            parts.append(
                f'<line x1="{_fmt(left)}" y1="{_fmt(y)}" x2="{_fmt(right)}" '
                f'y2="{_fmt(y)}" stroke="{BASELN}" stroke-width="1" '
                f'stroke-dasharray="2 3"/>'
            )

    if n > 1:
        fill_d = (f'{line_d} L {_fmt(pts[-1][0])} {_fmt(bottom)} '
                  f'L {_fmt(pts[0][0])} {_fmt(bottom)} Z')
        parts.append(f'<path d="{fill_d}" fill="url(#{fill_id})"/>')
        parts.append(
            f'<path d="{line_d}" fill="none" stroke="{color}" stroke-width="2" '
            f'stroke-linecap="round" stroke-linejoin="round"/>'
        )

    # Mark the last point with a haloed dot.
    lx, ly = pts[-1]
    parts.append(f'<circle cx="{_fmt(lx)}" cy="{_fmt(ly)}" r="5" fill="{color}" '
                 f'fill-opacity="0.22"/>')
    parts.append(f'<circle cx="{_fmt(lx)}" cy="{_fmt(ly)}" r="2.8" fill="{color}"/>')

    if label:
        parts.append(
            f'<text x="{_fmt(left)}" y="14" fill="{LABEL}" font-size="11" '
            f'font-family="ui-sans-serif, system-ui, -apple-system, Segoe UI, '
            f'Roboto, Helvetica, Arial, sans-serif">{_esc(label)}</text>'
        )

    parts.append('</svg>')
    return "\n".join(parts)


def _esc(text):
    return (str(text).replace("&", "&amp;").replace("<", "&lt;")
            .replace(">", "&gt;").replace('"', "&quot;"))


def main(argv=None):
    ap = argparse.ArgumentParser(
        prog="sparkline",
        description="Turn a short number series into a tiny inline SVG trend chart.")
    ap.add_argument("--data", required=True,
                    help="comma- or whitespace-separated numbers")
    ap.add_argument("--out", required=True, help="output .svg path")
    ap.add_argument("--label", default="", help="small caption drawn top-left")
    ap.add_argument("--width", type=int, default=240, help="canvas width (px)")
    ap.add_argument("--height", type=int, default=60, help="canvas height (px)")
    ap.add_argument("--color", default=DEFAULT_COLOR,
                    help="stroke colour: hex (#5fd2dc) or a name (teal/coral/…)")
    ap.add_argument("--no-baseline", dest="baseline", action="store_false",
                    help="drop the muted min/max reference lines")
    ap.add_argument("--bare", action="store_true",
                    help="transparent — no dark panel background")
    args = ap.parse_args(argv)

    try:
        values = parse_data(args.data)
        color = resolve_color(args.color)
    except ValueError as e:
        print(f"sparkline: {e}", file=sys.stderr)
        return 2

    if args.width < 20 or args.height < 16:
        print("sparkline: --width/--height too small (min 20x16)", file=sys.stderr)
        return 2

    svg = build_svg(values, args.width, args.height, color,
                    args.label, args.baseline, args.bare)

    try:
        ET.fromstring(svg)          # guarantee we only ever write valid XML
    except ET.ParseError as e:
        print(f"sparkline: internal error — produced invalid SVG: {e}",
              file=sys.stderr)
        return 2

    try:
        with open(args.out, "w", encoding="utf-8") as fh:
            fh.write(svg + "\n")
    except OSError as e:
        print(f"sparkline: could not write {args.out}: {e}", file=sys.stderr)
        return 2

    print(f"sparkline: wrote {args.out} ({len(values)} points)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
