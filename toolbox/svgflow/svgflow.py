#!/usr/bin/env python3
"""svgflow — render a box-and-arrow flow diagram as a committed SVG from a JSON spec.

Self-contained: the only dependency is the Python standard library. There is no
browser, no mermaid runtime, and no headless renderer — the SVG is assembled as
text and is byte-for-byte deterministic, so the same spec always produces the
same file. That makes the output safe to commit next to the docs it illustrates.

This is the runnable payload of the shipmates `svgflow` tool. An agent reaches
for it, per tool.md, when a task implies drawing a small pipeline, request flow,
or state machine into a README or docs page instead of describing it in prose.
It is never a slash command.

Usage:
    python3 svgflow.py --spec spec.json --out flow.svg
    echo '{"nodes":[…],"edges":[…]}' | python3 svgflow.py --out flow.svg

Spec (JSON):
    {
      "direction": "down",                     # "down" (default) or "right"
      "title": "CI pipeline",                  # optional caption above the flow
      "nodes": [
        {"id": "build",  "label": "Build"},
        {"id": "test",   "label": "Test"},
        {"id": "deploy", "label": "Deploy"}
      ],
      "edges": [
        {"from": "build", "to": "test",   "label": "on push"},
        {"from": "test",  "to": "deploy", "label": "green"}
      ]
    }

Nodes are laid out in a single column ("down") or row ("right") in declaration
order; each box is sized to its label. Edges between adjacent nodes draw as a
straight spine arrow; skips, back-edges, and self-loops bow out to the side so
they stay legible. Omit `edges` entirely and the nodes are chained in order.

The output self-sizes (correct width/height and viewBox) and is styled for the
dark shipmates tool pages: a rounded panel, light node cards, and a teal accent.

Exit codes: 0 ok; 2 bad spec/usage.
"""
import argparse
import json
import math
import sys
from xml.sax.saxutils import escape

# --- palette (legible on the dark tool pages: page ~#0d1117) ----------------
PAGE = "#0d1117"          # fills the SVG corners; blends into the tool page
PANEL = "#0b1a2b"         # the rounded panel the flow sits on
PANEL_STROKE = "#1d3a57"  # panel outline
NODE_FILL = "#15314c"     # node cards — a shade lighter than the panel
NODE_STROKE = "#34638c"   # node outline
NODE_TEXT = "#dee8f0"     # node label text
ACCENT = "#2fd4b6"        # arrows + arrowheads — teal/green
EDGE_TEXT = "#8fe3d4"     # edge labels
TITLE_TEXT = "#7f9bb3"    # the optional caption

# --- geometry ---------------------------------------------------------------
FONT_PX = 15
CHAR_W = FONT_PX * 0.6          # approximate proportional-font advance
NODE_PADX = 20
NODE_MIN_W = 96
BH = 46                         # node box height
GAP_DOWN = 78                   # vertical gap between stacked boxes
GAP_RIGHT = 108                 # horizontal gap between boxes in a row
LABEL_PX = 12
LABEL_CHAR_W = LABEL_PX * 0.6
LABEL_PADX = 9
LABEL_H = 22
BOW = 54                        # how far side/back/self edges bulge out
AH_LEN = 11                     # arrowhead length
AH_W = 5.5                      # arrowhead half-width
PAD = 28                        # panel-edge → content margin
TITLE_PX = 14
TITLE_H = 34                    # reserved band above the flow when titled
RX = 16                         # panel corner radius
STROKE = 2.0
FONT_STACK = ("ui-sans-serif, -apple-system, 'Segoe UI', Roboto, "
              "Helvetica, Arial, sans-serif")


def _n(v):
    """Format a coordinate: fixed 2dp, trailing zeros trimmed, deterministic."""
    s = f"{v:.2f}".rstrip("0").rstrip(".")
    return "0" if s in ("", "-0") else s


class Bounds:
    """Axis-aligned bounding box of everything drawn, in content coordinates."""

    def __init__(self):
        self.minx = self.miny = math.inf
        self.maxx = self.maxy = -math.inf

    def pt(self, x, y):
        self.minx = min(self.minx, x)
        self.miny = min(self.miny, y)
        self.maxx = max(self.maxx, x)
        self.maxy = max(self.maxy, y)

    def rect(self, x, y, w, h):
        self.pt(x, y)
        self.pt(x + w, y + h)


def _label_w(text):
    return len(text) * LABEL_CHAR_W + 2 * LABEL_PADX


def validate(spec):
    """Return (direction, title, nodes, edges) or raise ValueError on a bad spec."""
    if not isinstance(spec, dict):
        raise ValueError("spec must be a JSON object")
    direction = spec.get("direction", "down")
    if direction not in ("down", "right"):
        raise ValueError("direction must be 'down' or 'right'")
    title = spec.get("title")
    if title is not None and not isinstance(title, str):
        raise ValueError("title must be a string")

    raw_nodes = spec.get("nodes")
    if not isinstance(raw_nodes, list) or not raw_nodes:
        raise ValueError("spec.nodes must be a non-empty list")
    nodes, index = [], {}
    for i, nd in enumerate(raw_nodes):
        if not isinstance(nd, dict):
            raise ValueError(f"nodes[{i}] must be an object")
        nid = nd.get("id")
        label = nd.get("label", nid)
        if not isinstance(nid, str) or not nid:
            raise ValueError(f"nodes[{i}] needs a non-empty string id")
        if nid in index:
            raise ValueError(f"duplicate node id: {nid!r}")
        if not isinstance(label, str):
            raise ValueError(f"nodes[{i}].label must be a string")
        index[nid] = i
        nodes.append({"id": nid, "label": label})

    raw_edges = spec.get("edges", [])
    if not isinstance(raw_edges, list):
        raise ValueError("spec.edges must be a list")
    edges = []
    for j, ed in enumerate(raw_edges):
        if not isinstance(ed, dict):
            raise ValueError(f"edges[{j}] must be an object")
        src, dst = ed.get("from"), ed.get("to")
        if src not in index:
            raise ValueError(f"edges[{j}].from references unknown node {src!r}")
        if dst not in index:
            raise ValueError(f"edges[{j}].to references unknown node {dst!r}")
        elabel = ed.get("label", "")
        if not isinstance(elabel, str):
            raise ValueError(f"edges[{j}].label must be a string")
        edges.append({"fi": index[src], "ti": index[dst], "label": elabel})

    # No edges given → chain the nodes in declaration order.
    if not edges:
        edges = [{"fi": i, "ti": i + 1, "label": ""} for i in range(len(nodes) - 1)]
    return direction, title, nodes, edges


def layout(direction, nodes):
    """Place each node box; return a list of geometry dicts in declaration order."""
    boxes = []
    widths = [max(NODE_MIN_W, len(nd["label"]) * CHAR_W + 2 * NODE_PADX) for nd in nodes]
    if direction == "down":
        cx, y = 0.0, 0.0
        for w in widths:
            boxes.append({"left": cx - w / 2, "right": cx + w / 2, "top": y,
                          "bottom": y + BH, "cx": cx, "cy": y + BH / 2, "w": w})
            y += BH + GAP_DOWN
    else:
        cy, x = 0.0, 0.0
        for w in widths:
            boxes.append({"left": x, "right": x + w, "top": cy - BH / 2,
                          "bottom": cy + BH / 2, "cx": x + w / 2, "cy": cy, "w": w})
            x += w + GAP_RIGHT
    return boxes


def _arrowhead(px, py, dx, dy, b):
    """A filled triangle whose tip is (px,py), pointing along unit vector (dx,dy)."""
    m = math.hypot(dx, dy) or 1.0
    dx, dy = dx / m, dy / m
    bx, by = px - dx * AH_LEN, py - dy * AH_LEN
    perpx, perpy = -dy, dx
    p1 = (bx + perpx * AH_W, by + perpy * AH_W)
    p2 = (bx - perpx * AH_W, by - perpy * AH_W)
    for x, y in ((px, py), p1, p2):
        b.pt(x, y)
    pts = " ".join(f"{_n(x)},{_n(y)}" for x, y in ((px, py), p1, p2))
    return f'<polygon points="{pts}" fill="{ACCENT}"/>'


def _line(x1, y1, x2, y2, b):
    b.pt(x1, y1)
    b.pt(x2, y2)
    return (f'<line x1="{_n(x1)}" y1="{_n(y1)}" x2="{_n(x2)}" y2="{_n(y2)}" '
            f'stroke="{ACCENT}" stroke-width="{STROKE}" stroke-linecap="round"/>')


def _curve(s, c1, c2, e, b):
    for x, y in (s, c1, c2, e):
        b.pt(x, y)
    return (f'<path d="M {_n(s[0])} {_n(s[1])} C {_n(c1[0])} {_n(c1[1])} '
            f'{_n(c2[0])} {_n(c2[1])} {_n(e[0])} {_n(e[1])}" fill="none" '
            f'stroke="{ACCENT}" stroke-width="{STROKE}" stroke-linecap="round"/>')


def _label(text, cx, cy, b):
    """A masking pill + centered edge label; returns (pill_frag, text_frag)."""
    if not text:
        return None, None
    w, h = _label_w(text), LABEL_H
    b.rect(cx - w / 2, cy - h / 2, w, h)
    pill = (f'<rect x="{_n(cx - w / 2)}" y="{_n(cy - h / 2)}" width="{_n(w)}" '
            f'height="{_n(h)}" rx="6" fill="{PANEL}"/>')
    txt = (f'<text x="{_n(cx)}" y="{_n(cy + LABEL_PX * 0.35)}" text-anchor="middle" '
           f'font-size="{LABEL_PX}" fill="{EDGE_TEXT}">{escape(text)}</text>')
    return pill, txt


def _edge(direction, boxes, edge, b):
    """Return (stroke_frags, label_frags) for one edge."""
    fi, ti, text = edge["fi"], edge["ti"], edge["label"]
    s, t = boxes[fi], boxes[ti]
    strokes, labels = [], []

    if fi == ti:  # self-loop
        if direction == "down":
            S = (s["right"], s["cy"] - 10); E = (s["right"], s["cy"] + 10)
            ax = s["right"] + BOW * 0.85
            c1, c2 = (ax, s["cy"] - 22), (ax, s["cy"] + 22)
            lx, ly = ax + _label_w(text) / 2 + 6, s["cy"]
        else:
            S = (s["cx"] - 10, s["bottom"]); E = (s["cx"] + 10, s["bottom"])
            ay = s["bottom"] + BOW * 0.85
            c1, c2 = (s["cx"] - 22, ay), (s["cx"] + 22, ay)
            lx, ly = s["cx"], ay + LABEL_H / 2 + 6
        strokes.append(_curve(S, c1, c2, E, b))
        strokes.append(_arrowhead(E[0], E[1], E[0] - c2[0], E[1] - c2[1], b))
    elif ti == fi + 1:  # adjacent forward → straight spine
        if direction == "down":
            S = (s["cx"], s["bottom"]); E = (t["cx"], t["top"])
            strokes.append(_line(S[0], S[1], E[0], E[1], b))
            strokes.append(_arrowhead(E[0], E[1], 0, 1, b))
            # Spine labels sit to the LEFT; side/back-edge arcs bow to the RIGHT,
            # so the two never collide in the gap between two boxes.
            lx, ly = s["cx"] - 12 - _label_w(text) / 2, (S[1] + E[1]) / 2
        else:
            S = (s["right"], s["cy"]); E = (t["left"], t["cy"])
            strokes.append(_line(S[0], S[1], E[0], E[1], b))
            strokes.append(_arrowhead(E[0], E[1], 1, 0, b))
            lx, ly = (S[0] + E[0]) / 2, s["cy"] - 14 - LABEL_H / 2
    else:  # skip or back-edge → bow out to the side
        if direction == "down":
            S = (s["right"], s["cy"]); E = (t["right"], t["cy"])
            ax = max(s["right"], t["right"]) + BOW
            c1, c2 = (ax, s["cy"]), (ax, t["cy"])
            lx = (S[0] + E[0] + 6 * ax) / 8; ly = (s["cy"] + t["cy"]) / 2
        else:
            S = (s["cx"], s["bottom"]); E = (t["cx"], t["bottom"])
            ay = max(s["bottom"], t["bottom"]) + BOW
            c1, c2 = (s["cx"], ay), (t["cx"], ay)
            lx = (s["cx"] + t["cx"]) / 2; ly = (S[1] + E[1] + 6 * ay) / 8
        strokes.append(_curve(S, c1, c2, E, b))
        strokes.append(_arrowhead(E[0], E[1], E[0] - c2[0], E[1] - c2[1], b))

    pill, txt = _label(text, lx, ly, b)
    if pill:
        labels.extend((pill, txt))
    return strokes, labels


def _node(box, label, b):
    b.rect(box["left"], box["top"], box["w"], BH)
    rect = (f'<rect x="{_n(box["left"])}" y="{_n(box["top"])}" width="{_n(box["w"])}" '
            f'height="{_n(BH)}" rx="10" fill="{NODE_FILL}" stroke="{NODE_STROKE}" '
            f'stroke-width="1.5"/>')
    txt = (f'<text x="{_n(box["cx"])}" y="{_n(box["cy"] + FONT_PX * 0.35)}" '
           f'text-anchor="middle" font-size="{FONT_PX}" fill="{NODE_TEXT}">'
           f'{escape(label)}</text>')
    return rect, txt


def build_svg(spec):
    """Render the spec to (svg_text, width, height). Raises ValueError on bad spec."""
    direction, title, nodes, edges = validate(spec)
    boxes = layout(direction, nodes)
    b = Bounds()

    edge_frags, label_frags, node_frags = [], [], []
    for edge in edges:
        strokes, labels = _edge(direction, boxes, edge, b)
        edge_frags.extend(strokes)
        label_frags.extend(labels)
    for box, nd in zip(boxes, nodes):
        rect, txt = _node(box, nd["label"], b)
        node_frags.append(rect)
        node_frags.append(txt)

    title_h = TITLE_H if title else 0
    content_w = b.maxx - b.minx
    content_h = b.maxy - b.miny
    W = math.ceil(content_w + 2 * PAD)
    H = math.ceil(content_h + 2 * PAD + title_h)
    ox = PAD - b.minx
    oy = PAD + title_h - b.miny

    out = [
        '<?xml version="1.0" encoding="UTF-8"?>',
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{W}" height="{H}" '
        f'viewBox="0 0 {W} {H}" font-family="{FONT_STACK}">',
        f'<rect x="0" y="0" width="{W}" height="{H}" fill="{PAGE}"/>',
        f'<rect x="0.5" y="0.5" width="{W - 1}" height="{H - 1}" rx="{RX}" '
        f'fill="{PANEL}" stroke="{PANEL_STROKE}" stroke-width="1"/>',
    ]
    if title:
        out.append(
            f'<text x="{_n(W / 2)}" y="{_n(title_h * 0.62)}" text-anchor="middle" '
            f'font-size="{TITLE_PX}" fill="{TITLE_TEXT}">{escape(title)}</text>')
    out.append(f'<g transform="translate({_n(ox)},{_n(oy)})">')
    out.extend(edge_frags)      # arrows first…
    out.extend(node_frags)      # …then node cards on top…
    out.extend(label_frags)     # …then edge labels mask any line they cross
    out.append('</g>')
    out.append('</svg>')
    return "\n".join(out) + "\n", W, H


def main(argv=None):
    ap = argparse.ArgumentParser(
        description="Render a box-and-arrow flow diagram as an SVG from a JSON spec.")
    ap.add_argument("--spec", help="path to a JSON spec file (default: stdin)")
    ap.add_argument("--out", required=True, help="output .svg path")
    args = ap.parse_args(argv)

    try:
        raw = open(args.spec, encoding="utf-8").read() if args.spec else sys.stdin.read()
        spec = json.loads(raw)
    except (OSError, json.JSONDecodeError) as e:
        print(f"svgflow: could not read spec: {e}", file=sys.stderr)
        return 2

    try:
        svg, W, H = build_svg(spec)
    except (ValueError, KeyError) as e:
        print(f"svgflow: bad spec: {e}", file=sys.stderr)
        return 2

    try:
        with open(args.out, "w", encoding="utf-8") as fh:
            fh.write(svg)
    except OSError as e:
        print(f"svgflow: could not write output: {e}", file=sys.stderr)
        return 2

    n = len(spec.get("nodes", []))
    print(f"svgflow: wrote {args.out} ({W}x{H}, {n} nodes)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
