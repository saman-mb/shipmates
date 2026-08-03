#!/usr/bin/env python3
"""social-card — render a 1280x640 social/OG preview PNG from a small JSON spec.

Self-contained: the only dependency is Pillow. DejaVu Sans / DejaVu Sans Bold
are used when present (they ship with most Linux distros and are installable
everywhere); if they are missing, Pillow's built-in bitmap font is used as a
fallback so the tool still runs — just less crisply.

This is the runnable payload of the shipmates `social-card` tool. An agent
reaches for it, per tool.md, when a task implies producing a share/preview
image — an Open Graph card for a launch, a release, or a docs page — instead of
hand-composing one or asking the user to open a design app. It is never a slash
command.

Usage:
    python3 social_card.py --spec spec.json --out card.png
    echo '{"title":"…"}' | python3 social_card.py --out card.png

Spec (JSON):
    {
      "eyebrow":  "LAUNCH",                       # small kicker, uppercased
      "title":    "Shipmates — a crew for your agent",  # large, wraps
      "subtitle": "Reviewed, CI-green pull requests…",  # muted, wraps
      "accent":   "#58a6ff",                      # hex; drives pill + footer dot
      "wordmark": "shipmates.dev",                # footer text
      "bg":       "#0d1117",                      # optional background hex
      "fg":       "#e6edf3"                        # optional foreground hex
    }

Only `title` is required. The card is a fixed 1280x640 frame; long text wraps
and the type auto-fits so nothing ever overflows.
Exit codes: 0 ok; 2 bad spec/usage.
"""
import argparse
import json
import sys

try:
    from PIL import Image, ImageDraw, ImageFont
except ImportError:
    sys.exit("social-card: Pillow is required — install it with: pip install Pillow")

# --- Frame -----------------------------------------------------------------
W, H = 1280, 640
MARGIN_X = 92
CONTENT_W = W - 2 * MARGIN_X
TOP = 84                     # top of the content region
FOOTER_ZONE = 116           # reserved height at the bottom for the wordmark row
BLOCK_TOP = TOP
BLOCK_BOTTOM = H - FOOTER_ZONE

# --- Defaults --------------------------------------------------------------
DEFAULT_BG = (13, 17, 23)       # #0d1117
DEFAULT_FG = (230, 237, 243)    # #e6edf3
DEFAULT_ACCENT = (88, 166, 255)  # #58a6ff

FONT_REG = [
    "DejaVuSans.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    "/Library/Fonts/DejaVuSans.ttf",
]
FONT_BOLD = [
    "DejaVuSans-Bold.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
    "/Library/Fonts/DejaVuSans-Bold.ttf",
]


def _font(cands, size):
    for c in cands:
        try:
            return ImageFont.truetype(c, size)
        except OSError:
            continue
    return ImageFont.load_default()


def _len(font, text):
    try:
        return font.getlength(text)
    except AttributeError:
        return len(text) * font.size * 0.6


def _line_h(font, leading):
    try:
        asc, desc = font.getmetrics()
        return int((asc + desc) * leading)
    except AttributeError:
        return int(getattr(font, "size", 16) * leading)


def _mix(a, b, t):
    """Blend colour a toward b by fraction t (0..1)."""
    return tuple(int(round(a[i] * (1 - t) + b[i] * t)) for i in range(3))


def _hex(value, fallback):
    """Parse '#rrggbb', 'rrggbb', or '#rgb' into an (r,g,b) tuple."""
    if value is None:
        return fallback
    if not isinstance(value, str):
        raise ValueError(f"colour must be a hex string, got {value!r}")
    s = value.strip().lstrip("#")
    if len(s) == 3:
        s = "".join(ch * 2 for ch in s)
    if len(s) != 6:
        raise ValueError(f"bad hex colour: {value!r}")
    try:
        return tuple(int(s[i:i + 2], 16) for i in (0, 2, 4))
    except ValueError:
        raise ValueError(f"bad hex colour: {value!r}")


def _wrap(font, text, max_w):
    """Greedy word-wrap; hard-breaks any single word wider than max_w."""
    words = str(text).split()
    if not words:
        return []
    lines, cur = [], words[0]
    for w in words[1:]:
        trial = cur + " " + w
        if _len(font, trial) <= max_w:
            cur = trial
        else:
            lines.append(cur)
            cur = w
    lines.append(cur)
    out = []
    for ln in lines:
        if _len(font, ln) <= max_w:
            out.append(ln)
            continue
        buf = ""
        for ch in ln:
            if _len(font, buf + ch) <= max_w or not buf:
                buf += ch
            else:
                out.append(buf)
                buf = ch
        if buf:
            out.append(buf)
    return out


def _draw_tracked(d, pos, text, font, fill, tracking):
    """Draw text with extra per-character spacing (kicker feel)."""
    x, y = pos
    for ch in text:
        d.text((x, y), ch, font=font, fill=fill)
        x += _len(font, ch) + tracking


# Candidate (title_size, subtitle_size) pairs, largest first. The first pair
# whose laid-out block fits the content region is used, so long copy shrinks
# gracefully instead of overflowing the frame.
_FIT_STEPS = [(80, 34), (72, 32), (64, 30), (56, 28), (50, 26), (46, 24), (42, 22)]

EYEBROW_SIZE = 23
WORDMARK_SIZE = 24
GAP_EYEBROW = 30            # pill -> title
GAP_TITLE = 26             # title -> subtitle
PILL_PADX, PILL_PADY = 16, 9


def _layout(eyebrow, title, subtitle):
    """Choose fonts and wrapped lines that fit the content region."""
    max_w = CONTENT_W
    fit = None
    for tsize, ssize in _FIT_STEPS:
        tfont = _font(FONT_BOLD, tsize)
        sfont = _font(FONT_REG, ssize)
        tlines = _wrap(tfont, title, max_w) if title else []
        slines = _wrap(sfont, subtitle, max_w) if subtitle else []
        tlh = _line_h(tfont, 1.14)
        slh = _line_h(sfont, 1.34)
        efont = _font(FONT_BOLD, EYEBROW_SIZE)
        pill_h = (_line_h(efont, 1.0) + 2 * PILL_PADY) if eyebrow else 0

        height = 0
        if eyebrow:
            height += pill_h + GAP_EYEBROW
        height += len(tlines) * tlh
        if slines:
            height += GAP_TITLE + len(slines) * slh

        fit = (tfont, sfont, efont, tlines, slines, tlh, slh, pill_h, height)
        if height <= (BLOCK_BOTTOM - BLOCK_TOP) and len(tlines) <= 4 and len(slines) <= 4:
            break
    # If even the smallest step overflows vertically, trim trailing lines so the
    # frame is never breached (better a clipped subtitle than a broken card).
    tfont, sfont, efont, tlines, slines, tlh, slh, pill_h, height = fit
    avail = BLOCK_BOTTOM - BLOCK_TOP
    while height > avail and slines:
        slines.pop()
        height -= slh
        if slines and _len(sfont, slines[-1]) <= CONTENT_W:
            slines[-1] = slines[-1].rstrip(" .") + "…"
    while height > avail and len(tlines) > 1:
        tlines.pop()
        height -= tlh
    return tfont, sfont, efont, tlines, slines, tlh, slh, pill_h, height


def render(spec):
    if not isinstance(spec, dict):
        raise ValueError("spec must be a JSON object")
    title = spec.get("title")
    if not title or not str(title).strip():
        raise ValueError("spec.title is required")
    eyebrow = str(spec.get("eyebrow", "")).strip()
    subtitle = str(spec.get("subtitle", "")).strip()
    wordmark = str(spec.get("wordmark", "")).strip()

    bg = _hex(spec.get("bg"), DEFAULT_BG)
    fg = _hex(spec.get("fg"), DEFAULT_FG)
    accent = _hex(spec.get("accent"), DEFAULT_ACCENT)
    muted = _mix(fg, bg, 0.42)
    hairline = _mix(fg, bg, 0.86)
    pill_bg = _mix(accent, bg, 0.84)
    pill_border = _mix(accent, bg, 0.62)
    pill_text = _mix(accent, (255, 255, 255), 0.12)

    img = Image.new("RGB", (W, H), bg)
    d = ImageDraw.Draw(img)

    (tfont, sfont, efont, tlines, slines,
     tlh, slh, pill_h, block_h) = _layout(eyebrow, str(title).strip(), subtitle)

    # Vertically centre the block within the content region for balance.
    y = BLOCK_TOP + max(0, (BLOCK_BOTTOM - BLOCK_TOP - block_h) // 2)

    if eyebrow:
        label = eyebrow.upper()
        tracking = 2
        text_w = sum(_len(efont, ch) + tracking for ch in label) - tracking
        pill_w = int(text_w) + 2 * PILL_PADX
        d.rounded_rectangle(
            [MARGIN_X, y, MARGIN_X + pill_w, y + pill_h],
            radius=pill_h // 2, fill=pill_bg, outline=pill_border, width=1,
        )
        _draw_tracked(d, (MARGIN_X + PILL_PADX, y + PILL_PADY - 1), label, efont, pill_text, tracking)
        y += pill_h + GAP_EYEBROW

    for ln in tlines:
        d.text((MARGIN_X, y), ln, font=tfont, fill=fg)
        y += tlh

    if slines:
        y += GAP_TITLE
        for ln in slines:
            d.text((MARGIN_X, y), ln, font=sfont, fill=muted)
            y += slh

    # Footer: hairline divider, accent dot, wordmark.
    fy = H - 66
    d.line([MARGIN_X, H - 92, W - MARGIN_X, H - 92], fill=hairline, width=1)
    wfont = _font(FONT_BOLD, WORDMARK_SIZE)
    dot_r = 6
    cx = MARGIN_X + dot_r
    cy = fy + _line_h(wfont, 1.0) // 2 - 1
    d.ellipse([cx - dot_r, cy - dot_r, cx + dot_r, cy + dot_r], fill=accent)
    if wordmark:
        d.text((cx + dot_r + 14, fy), wordmark, font=wfont, fill=_mix(fg, bg, 0.16))

    return img


def main(argv=None):
    ap = argparse.ArgumentParser(
        description="Render a 1280x640 social/OG preview PNG from a JSON spec.")
    ap.add_argument("--spec", help="path to a JSON spec file (default: stdin)")
    ap.add_argument("--out", required=True, help="output .png path")
    args = ap.parse_args(argv)

    try:
        raw = open(args.spec, encoding="utf-8").read() if args.spec else sys.stdin.read()
        spec = json.loads(raw)
    except (OSError, json.JSONDecodeError) as e:
        print(f"social-card: could not read spec: {e}", file=sys.stderr)
        return 2

    try:
        img = render(spec)
    except (ValueError, KeyError) as e:
        print(f"social-card: bad spec: {e}", file=sys.stderr)
        return 2

    try:
        img.save(args.out, format="PNG", optimize=True)
    except OSError as e:
        print(f"social-card: could not write {args.out}: {e}", file=sys.stderr)
        return 2
    print(f"social-card: wrote {args.out} ({img.width}x{img.height})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
