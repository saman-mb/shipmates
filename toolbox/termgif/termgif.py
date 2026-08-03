#!/usr/bin/env python3
"""termgif — render an animated terminal demo GIF from a small JSON spec.

Self-contained: the only dependency is Pillow. DejaVu Sans Mono is used when
present (it ships with most Linux distros and is installable everywhere); if it
is missing, Pillow's built-in bitmap font is used as a fallback so the tool
still runs — just less crisply.

This is the runnable payload of the shipmates `termgif` tool. An agent reaches
for it, per tool.md, when a task implies producing a terminal demo GIF (a README
recording, a docs hero, a release note). It is never a slash command.

Usage:
    python3 termgif.py --spec spec.json --out demo.gif
    echo '{"title":"…","beats":[…]}' | python3 termgif.py --out demo.gif

Spec (JSON):
    {
      "title": "shipmates — /ship-issue",   # title-bar text
      "width": 900,                          # optional, default 860
      "beats": [
        {"type": "command", "text": "/ship-issue 142"},   # typed after a $ prompt
        {"type": "blank"},
        {"type": "stage", "label": "PLAN", "detail": "work units, acceptance criteria"},
        {"type": "line",  "text": "Installed harness: claude-code", "color": "white"},
        {"type": "done",  "text": "Reviewed PR, handed to you."}     # green ✓ line
      ]
    }

Colors: prompt/green/white/grey/blue/purple/orange/cyan/coral/gold/sage/faint.
Exit codes: 0 ok; 2 bad spec/usage.
"""
import argparse
import json
import sys

try:
    from PIL import Image, ImageDraw, ImageFont
except ImportError:
    sys.exit("termgif: Pillow is required — install it with: pip install Pillow")

PAGE   = (13, 17, 23)
PANEL  = (1, 22, 39)
BAR    = (1, 30, 52)
BORDER = (32, 54, 74)
DOTS   = ((255, 95, 86), (255, 189, 46), (39, 201, 63))
CURSOR = (200, 220, 235)

COLORS = {
    "prompt": (86, 214, 122),
    "green":  (126, 231, 135),
    "white":  (222, 232, 240),
    "grey":   (120, 140, 158),
    "faint":  (78, 96, 112),
    "blue":   (130, 170, 255),
    "purple": (170, 150, 255),
    "orange": (255, 180, 90),
    "cyan":   (95, 210, 220),
    "coral":  (255, 145, 130),
    "gold":   (245, 200, 100),
    "sage":   (150, 200, 160),
}
ACCENTS = ["blue", "purple", "orange", "cyan", "coral", "gold", "sage"]

FONT_SIZE = 19
PADX, TOPBAR, LINE_H = 34, 40, 30
Y0 = 16 + TOPBAR + 20
SPIN = ["|", "/", "-", "\\"]

FONT_CANDIDATES = [
    "DejaVuSansMono.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
    "/Library/Fonts/DejaVuSansMono.ttf",
]
FONT_BOLD_CANDIDATES = [
    "DejaVuSansMono-Bold.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSansMono-Bold.ttf",
]


def _load(cands):
    for c in cands:
        try:
            return ImageFont.truetype(c, FONT_SIZE)
        except OSError:
            continue
    return ImageFont.load_default()


class Term:
    def __init__(self, width, title):
        self.W = width
        self.title = title
        self.f = _load(FONT_CANDIDATES)
        self.fb = _load(FONT_BOLD_CANDIDATES)
        try:
            self.ch = self.fb.getlength("M")
        except AttributeError:
            self.ch = FONT_SIZE * 0.6

    def _len(self, text, bold):
        font = self.fb if bold else self.f
        try:
            return font.getlength(text)
        except AttributeError:
            return len(text) * self.ch

    def frame(self, H, lines, cursor_line=None):
        img = Image.new("RGB", (self.W, H), PAGE)
        d = ImageDraw.Draw(img)
        d.rounded_rectangle([16, 16, self.W - 16, H - 16], radius=14, fill=PANEL, outline=BORDER, width=1)
        d.rounded_rectangle([16, 16, self.W - 16, 16 + TOPBAR], radius=14, fill=BAR)
        d.rectangle([16, 30 + TOPBAR - 14, self.W - 16, 16 + TOPBAR], fill=PANEL)
        for i, c in enumerate(DOTS):
            cx = 40 + i * 26
            d.ellipse([cx, 30, cx + 13, 43], fill=c)
        tw = self._len(self.title, False)
        d.text(((self.W - tw) / 2, 27), self.title, font=self.f, fill=COLORS["grey"])
        for i, segs in enumerate(lines):
            x, y = PADX, Y0 + i * LINE_H
            for text, color, bold in segs:
                d.text((x, y), text, font=(self.fb if bold else self.f), fill=COLORS.get(color, COLORS["white"]))
                x += self._len(text, bold)
            if cursor_line == i:
                d.rectangle([x + 2, y + 2, x + 2 + self.ch, y + 21], fill=CURSOR)
        return img


def build(spec):
    beats = spec.get("beats", [])
    if not isinstance(beats, list) or not beats:
        raise ValueError("spec.beats must be a non-empty list")
    width = int(spec.get("width", 860))
    term = Term(width, spec.get("title", "terminal"))

    # Final line count → height (a command line + committed output lines).
    n_lines = sum(1 for b in beats if b.get("type") != "command") + \
        sum(1 for b in beats if b.get("type") == "command")
    H = Y0 + (n_lines + 1) * LINE_H + 24
    H += H % 2

    frames, durations, log = [], [], []
    accent_i = 0

    def emit(lines, dur, cursor_line=None):
        frames.append(term.frame(H, lines, cursor_line))
        durations.append(dur)

    for beat in beats:
        kind = beat.get("type")
        if kind == "command":
            text = beat.get("text", "")
            idx = len(log)
            prefix = [("$ ", "prompt", True)]
            for k in range(len(text) + 1):
                emit(log + [prefix + [(text[:k], "white", True)]], 55, cursor_line=idx)
            full = prefix + [(text, "white", True)]
            emit(log + [full], 110, cursor_line=idx)
            log = log + [full]
        elif kind == "blank":
            log = log + [[("", "grey", False)]]
            emit(log, 60)
        elif kind == "stage":
            color = beat.get("color") or ACCENTS[accent_i % len(ACCENTS)]
            accent_i += 1
            label = str(beat.get("label", "")).ljust(13)
            detail = beat.get("detail", "")
            for s in range(2):
                line = [("  ", "grey", False), (SPIN[s % len(SPIN)] + " ", color, True),
                        (label, color, True), (detail, "faint", False)]
                emit(log + [line], 95)
            done = [("  ", "grey", False), ("✓ ", "green", True),
                    (label, color, True), (detail, "grey", False)]
            log = log + [done]
            emit(log, 240)
        elif kind == "line":
            if "segments" in beat:
                segs = [(s.get("text", ""), s.get("color", "white"), bool(s.get("bold"))) for s in beat["segments"]]
            else:
                segs = [(beat.get("text", ""), beat.get("color", "white"), bool(beat.get("bold")))]
            log = log + [segs]
            emit(log, 240)
        elif kind == "done":
            log = log + [[("✓ ", "green", True), (beat.get("text", ""), "green", False)]]
            emit(log, 300)
        else:
            raise ValueError(f"unknown beat type: {kind!r}")

    for _ in range(4):
        emit(log, 300)
    return frames, durations


def main(argv=None):
    ap = argparse.ArgumentParser(description="Render an animated terminal demo GIF from a JSON spec.")
    ap.add_argument("--spec", help="path to a JSON spec file (default: stdin)")
    ap.add_argument("--out", required=True, help="output .gif path")
    args = ap.parse_args(argv)

    try:
        raw = open(args.spec, encoding="utf-8").read() if args.spec else sys.stdin.read()
        spec = json.loads(raw)
    except (OSError, json.JSONDecodeError) as e:
        print(f"termgif: could not read spec: {e}", file=sys.stderr)
        return 2

    try:
        frames, durations = build(spec)
    except (ValueError, KeyError) as e:
        print(f"termgif: bad spec: {e}", file=sys.stderr)
        return 2

    frames[0].save(args.out, format="GIF", save_all=True, append_images=frames[1:],
                   duration=durations, loop=0, optimize=True, disposal=2)
    print(f"termgif: wrote {args.out} ({len(frames)} frames)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
