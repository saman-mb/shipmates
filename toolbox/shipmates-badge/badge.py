#!/usr/bin/env python3
"""badge — render a shields-style status badge as an offline, committed SVG.

Self-contained: the standard library only. No network — the badge is written to
disk as plain SVG text, so it renders forever without a round-trip to shields.io
and diffs cleanly in git. Output is deterministic: the same arguments always
produce byte-for-byte the same file.

This is the runnable payload of the shipmates `badge` tool. An agent reaches for
it, per tool.md, when a task implies a small status/version/coverage badge for a
README, a docs page, or a release note — and the badge should live in the repo
rather than hot-linking an external service. It is never a slash command.

Usage:
    python3 badge.py --label build --message passing --color green --out badge.svg
    python3 badge.py --label coverage --message 98% --color brightgreen  # -> stdout

Colors: a named palette (brightgreen, green, yellowgreen, yellow, orange, red,
blue, purple, lightgrey, grey, plus the semantic aliases success, important,
critical, informational, inactive) or any `#rgb` / `#rrggbb` hex value.
Exit codes: 0 ok; 2 bad color / usage / write error.

The classic two-segment flat badge: a grey left segment carrying the label, a
coloured right segment carrying the message, rounded outer corners and a square
inner join. Segment widths are sized from a baked DejaVu Sans advance-width table
(the same metrics shields uses) and locked with SVG `textLength`, so the text is
never clipped and never loose regardless of the viewer's installed font.
"""
import argparse
import re
import sys
from xml.sax.saxutils import escape

# Per-character advance widths for DejaVu Sans, measured at font-size 110 (the
# x10 coordinate space the text is drawn in). Keyed by codepoint; printable
# ASCII 32..126. Unknown codepoints fall back to DEFAULT_W. These are the same
# metrics shields.io renders with, so segment sizing matches a live badge.
CHAR_W = {
    32: 35, 33: 44, 34: 51, 35: 92, 36: 70, 37: 105, 38: 86, 39: 30, 40: 43,
    41: 43, 42: 55, 43: 92, 44: 35, 45: 40, 46: 35, 47: 37, 48: 70, 49: 70,
    50: 70, 51: 70, 52: 70, 53: 70, 54: 70, 55: 70, 56: 70, 57: 70, 58: 37,
    59: 37, 60: 92, 61: 92, 62: 92, 63: 58, 64: 110, 65: 75, 66: 75, 67: 77,
    68: 85, 69: 70, 70: 63, 71: 85, 72: 83, 73: 32, 74: 32, 75: 72, 76: 61,
    77: 95, 78: 82, 79: 87, 80: 66, 81: 87, 82: 76, 83: 70, 84: 67, 85: 81,
    86: 75, 87: 109, 88: 75, 89: 67, 90: 75, 91: 43, 92: 37, 93: 43, 94: 92,
    95: 55, 96: 55, 97: 67, 98: 70, 99: 60, 100: 70, 101: 68, 102: 39, 103: 70,
    104: 70, 105: 31, 106: 31, 107: 64, 108: 31, 109: 107, 110: 70, 111: 67,
    112: 70, 113: 70, 114: 45, 115: 57, 116: 43, 117: 70, 118: 65, 119: 90,
    120: 65, 121: 65, 122: 58, 123: 70, 124: 37, 125: 70, 126: 92,
}
DEFAULT_W = 70  # advance for glyphs outside the table (safe mid-width)

HEIGHT = 20
PAD = 5          # horizontal padding, px, on each side of a segment's text
LABEL_BG = "#555"  # fixed grey for the left (label) segment

# Named colours -> hex. The shields palette plus its semantic aliases.
NAMED_COLORS = {
    "brightgreen": "#4c1",
    "green": "#97ca00",
    "yellowgreen": "#a4a61d",
    "yellow": "#dfb317",
    "orange": "#fe7d37",
    "red": "#e05d44",
    "blue": "#007ec6",
    "purple": "#9f45b0",
    "lightgrey": "#9f9f9f",
    "lightgray": "#9f9f9f",
    "grey": "#555",
    "gray": "#555",
    # semantic aliases
    "success": "#4c1",
    "important": "#fe7d37",
    "critical": "#e05d44",
    "informational": "#007ec6",
    "inactive": "#9f9f9f",
}

HEX_RE = re.compile(r"^#(?:[0-9a-fA-F]{3}|[0-9a-fA-F]{6})$")


def resolve_color(value):
    """Return a valid CSS hex colour for `value`, or raise ValueError."""
    key = value.strip().lower()
    if key in NAMED_COLORS:
        return NAMED_COLORS[key]
    if HEX_RE.match(value.strip()):
        return value.strip().lower()
    raise ValueError(
        f"unknown color {value!r} — use a named colour "
        f"({', '.join(sorted(NAMED_COLORS))}) or a #rgb / #rrggbb hex value"
    )


def text_width(s):
    """Advance width of `s` in the x10 (font-size-110) coordinate space."""
    return sum(CHAR_W.get(ord(c), DEFAULT_W) for c in s)


def _seg_width(units):
    """Pixel width of a segment holding text of `units` (x10) advance."""
    return int(units / 10 + 0.5) + 2 * PAD


def _num(x):
    """Format a coordinate: drop a trailing .0 so integers stay clean."""
    return f"{x:g}"


def render(label, message, color):
    """Return the badge SVG as a string. `color` must already be a hex value."""
    lw = text_width(label)      # x10 advance of the label text
    mw = text_width(message)    # x10 advance of the message text
    label_seg = _seg_width(lw) if label else 0
    msg_seg = _seg_width(mw)
    total = label_seg + msg_seg

    label_cx = label_seg / 2                 # px centre of the label segment
    msg_cx = label_seg + msg_seg / 2         # px centre of the message segment

    aria = escape(f"{label}: {message}" if label else message)

    def text_block(cx, units, s):
        # Shadow (dark, offset down) then the white face, both length-locked.
        x = _num(cx * 10)
        tl = _num(units)
        body = escape(s)
        return (
            f'    <text aria-hidden="true" x="{x}" y="150" fill="#010101" '
            f'fill-opacity=".3" transform="scale(.1)" textLength="{tl}">{body}</text>\n'
            f'    <text x="{x}" y="140" transform="scale(.1)" '
            f'textLength="{tl}">{body}</text>\n'
        )

    rects = ""
    if label:
        rects += f'    <rect width="{label_seg}" height="20" fill="{LABEL_BG}"/>\n'
        rects += f'    <rect x="{label_seg}" width="{msg_seg}" height="20" fill="{color}"/>\n'
    else:
        rects += f'    <rect width="{msg_seg}" height="20" fill="{color}"/>\n'

    texts = ""
    if label:
        texts += text_block(label_cx, lw, label)
    texts += text_block(msg_cx, mw, message)

    return (
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{total}" height="20" '
        f'role="img" aria-label="{aria}">\n'
        f'  <title>{aria}</title>\n'
        f'  <linearGradient id="s" x2="0" y2="100%">\n'
        f'    <stop offset="0" stop-color="#bbb" stop-opacity=".1"/>\n'
        f'    <stop offset="1" stop-opacity=".1"/>\n'
        f'  </linearGradient>\n'
        f'  <clipPath id="r">\n'
        f'    <rect width="{total}" height="20" rx="3" fill="#fff"/>\n'
        f'  </clipPath>\n'
        f'  <g clip-path="url(#r)">\n'
        f'{rects}'
        f'    <rect width="{total}" height="20" fill="url(#s)"/>\n'
        f'  </g>\n'
        f'  <g fill="#fff" text-anchor="middle" '
        f'font-family="DejaVu Sans,Verdana,Geneva,sans-serif" font-size="110">\n'
        f'{texts}'
        f'  </g>\n'
        f'</svg>\n'
    )


def main(argv=None):
    ap = argparse.ArgumentParser(
        description="Render a shields-style status badge as an offline SVG.")
    ap.add_argument("--label", required=True,
                    help="left (grey) segment text, e.g. build")
    ap.add_argument("--message", required=True,
                    help="right (coloured) segment text, e.g. passing")
    ap.add_argument("--color", "--colour", default="blue",
                    help="named colour (green, blue, red, brightgreen, ...) or "
                         "a #rgb / #rrggbb hex value (default: blue)")
    ap.add_argument("--out", help="output .svg path (default: stdout)")
    args = ap.parse_args(argv)

    try:
        color = resolve_color(args.color)
    except ValueError as e:
        print(f"badge: {e}", file=sys.stderr)
        return 2

    svg = render(args.label, args.message, color)

    if args.out:
        try:
            with open(args.out, "w", encoding="utf-8") as fh:
                fh.write(svg)
        except OSError as e:
            print(f"badge: could not write {args.out}: {e}", file=sys.stderr)
            return 2
        print(f"badge: wrote {args.out}")
    else:
        sys.stdout.write(svg)
    return 0


if __name__ == "__main__":
    sys.exit(main())
