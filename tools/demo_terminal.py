#!/usr/bin/env python3
"""Shared primitives for the animated terminal demo GIFs.

One terminal look — the Night-Owl-ish chrome, palette and DejaVu Sans Mono face
first established by ``gen_demo_gif.py`` — factored out so every demo (the
install reel and the per-command reels) renders identically and is drift-checked
the same way.

Honest by construction, like its sibling: a reel depicts real commands and the
*actual* stage sequence a workflow performs, with generic labels — no fabricated
counts or invented file names. Deterministic and committed.

Determinism note: the committed artifacts are compared as *pixels* (see
``content_signature``), which come from a pinned Pillow's bundled freetype and a
pinned DejaVu Sans Mono. The CI job installs both; a local regenerate must use
the same pins or it will re-render every glyph.
"""
import hashlib
import io
import os
import sys
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont, ImageSequence

# ---- palette (shared with gen_demo_gif.py — keep in lockstep) ----
PAGE   = (13, 17, 23)      # github dark page
PANEL  = (1, 22, 39)       # terminal body
BAR    = (1, 30, 52)       # title bar
BORDER = (32, 54, 74)
DOT_R  = (255, 95, 86)
DOT_Y  = (255, 189, 46)
DOT_G  = (39, 201, 63)
GREY   = (120, 140, 158)   # dim detail
FAINT  = (78, 96, 112)     # running/dim
WHITE  = (222, 232, 240)
GREEN  = (126, 231, 135)   # ✓ / success / cargo status word
PROMPT = (86, 214, 122)    # $ prompt
CURSOR = (200, 220, 235)

# Stage accent colours, shared with gen_demo_gif.py's STAGE_COLORS so a command
# reel that reuses a stage name paints it the same hue the /ship-issue reel does.
BLUE   = (130, 170, 255)
PURPLE = (170, 150, 255)
ORANGE = (255, 180, 90)
CYAN   = (95, 210, 220)
CORAL  = (255, 145, 130)
GOLD   = (245, 200, 100)
SAGE   = (150, 200, 160)

FONT_REGULAR = "DejaVuSansMono.ttf"
FONT_BOLD = "DejaVuSansMono-Bold.ttf"
FONT_SIZE = 19

PADX, TOPBAR = 34, 40
LINE_H = 30
Y0 = 16 + TOPBAR + 20

# Never a real glyph in any font — the explicit .notdef probe the spinner guard
# compares against.
NOTDEF_PROBE = "￿"
SPIN_BRAILLE = ["⠂", "⡆", "⣤", "⣰", "⢸", "⠹", "⠛", "⠏"]
SPIN_ASCII = ["|", "/", "-", "\\"]

PALETTE_COLORS = 128


def load_font(filename):
    """Load FILENAME at FONT_SIZE, or exit with an actionable per-distro message.

    DejaVu Sans Mono is the reference face — the committed GIFs are rendered with
    it, so a different face silently re-renders every glyph. ImageFont.truetype
    already recursively walks the standard font dirs, so there is nothing left to
    search here.
    """
    try:
        return ImageFont.truetype(filename, FONT_SIZE)
    except OSError:
        sys.exit(
            f"demo_terminal: {filename} not found.\n"
            f"  Debian/Ubuntu:  sudo apt install fonts-dejavu-core\n"
            f"  Fedora/RHEL:    sudo dnf install dejavu-sans-mono-fonts\n"
            f"  Arch:           sudo pacman -S ttf-dejavu\n"
            f"  macOS:          brew install --cask font-dejavu"
        )


def spinner(fb):
    """Braille spinner glyphs if the bold face has them, else ASCII.

    getbbox() returns a box for the .notdef glyph too, so a None guard can't
    detect a missing glyph. Comparing a candidate's mask bbox against an
    explicit .notdef probe's bbox does.
    """
    notdef_bbox = fb.getmask(NOTDEF_PROBE).getbbox()
    if fb.getmask(SPIN_BRAILLE[0]).getbbox() == notdef_bbox:
        return SPIN_ASCII
    return SPIN_BRAILLE


class Terminal:
    """Chrome + one-frame renderer for a fixed-size terminal window."""

    def __init__(self, width, height, title):
        self.W = width
        self.H = height
        self.title = title
        self.f = load_font(FONT_REGULAR)
        self.fb = load_font(FONT_BOLD)
        self.ch = self.fb.getlength("M")  # mono advance
        self.spin = spinner(self.fb)

    def base_frame(self):
        img = Image.new("RGB", (self.W, self.H), PAGE)
        d = ImageDraw.Draw(img)
        d.rounded_rectangle([16, 16, self.W - 16, self.H - 16], radius=14,
                            fill=PANEL, outline=BORDER, width=1)
        d.rounded_rectangle([16, 16, self.W - 16, 16 + TOPBAR], radius=14, fill=BAR)
        d.rectangle([16, 30 + TOPBAR - 14, self.W - 16, 16 + TOPBAR], fill=PANEL)
        for i, c in enumerate((DOT_R, DOT_Y, DOT_G)):
            cx = 40 + i * 26
            d.ellipse([cx, 30, cx + 13, 43], fill=c)
        tw = self.f.getlength(self.title)
        d.text(((self.W - tw) / 2, 27), self.title, font=self.f, fill=GREY)
        return img, d

    def _draw_segments(self, d, x, y, segments):
        for text, color, bold in segments:
            d.text((x, y), text, font=(self.fb if bold else self.f), fill=color)
            x += self.fb.getlength(text) if bold else self.f.getlength(text)
        return x

    def render(self, lines, cursor_line=None):
        """lines: list of segment-lists ((text, color, bold), …).
        cursor_line: index of the line to draw a block cursor after."""
        img, d = self.base_frame()
        for i, segs in enumerate(lines):
            y = Y0 + i * LINE_H
            endx = self._draw_segments(d, PADX, y, segs)
            if cursor_line is not None and cursor_line == i:
                d.rectangle([endx + 2, y + 2, endx + 2 + self.ch, y + 21], fill=CURSOR)
        return img


class Reel:
    """Accumulates frames + per-frame durations, tracking a persistent log of
    committed lines and a small set of palette-seed frames.

    A beat appends frames; the log is the lines that stay on screen. Palette
    seeds are the first frame, one mid frame, and (after the caller finishes) the
    last frame — between them they carry every colour any frame uses.
    """

    def __init__(self, term):
        self.term = term
        self.frames = []
        self.durations = []
        self.log = []
        self._mid_sample = None

    def _emit(self, lines, dur, cursor_line=None):
        frame = self.term.render(lines, cursor_line)
        self.frames.append(frame)
        self.durations.append(dur)
        if self._mid_sample is None and len(self.frames) > 1:
            self._mid_sample = frame
        return frame

    def type_command(self, segments_prefix, text, char_ms=55, hold_blinks=2):
        """Type TEXT one char at a time after SEGMENTS_PREFIX (e.g. the prompt),
        on a new line below the current log, then hold with a blinking cursor."""
        line_idx = len(self.log)
        for k in range(len(text) + 1):
            line = list(segments_prefix) + [(text[:k], WHITE, True)]
            self._emit(self.log + [line], char_ms, cursor_line=line_idx)
        full = list(segments_prefix) + [(text, WHITE, True)]
        for _ in range(hold_blinks):
            self._emit(self.log + [full], 110, cursor_line=line_idx)
            self._emit(self.log + [full], 110)
        self.log = self.log + [full]

    def reveal(self, segments, dur=240):
        """Commit one output line to the log and hold it briefly."""
        self.log = self.log + [segments]
        self._emit(self.log, 60)
        self._emit(self.log, dur)

    def blank(self):
        self.log = self.log + [[("", GREY, False)]]
        self._emit(self.log, 60)

    def stage(self, label, color, running, done, done_detail_color=GREY, cycles=4):
        """Spinner-running then a green ✓ done line — the /ship-issue reel's
        stage idiom, reusable per command. CYCLES tunes the spinner frame count
        (fewer = smaller GIF)."""
        lab = label.ljust(13)
        for s in range(cycles):
            sp = self.term.spin[s % len(self.term.spin)]
            line = [("  ", GREY, False), (sp + " ", color, True),
                    (lab, color, True), (running or "working …", FAINT, False)]
            self._emit(self.log + [line], 95)
        done_line = [("  ", GREY, False), ("✓ ", GREEN, True),
                     (lab, color, True), (done, done_detail_color, False)]
        self.log = self.log + [done_line]
        self._emit(self.log, 70)
        self._emit(self.log, 240)

    def hold(self, dur, times=1):
        for _ in range(times):
            self._emit(self.log, dur)

    def palette_seeds(self):
        seeds = [self.frames[0], self.frames[-1]]
        if self._mid_sample is not None:
            seeds.insert(1, self._mid_sample)
        return tuple(seeds)


def quantize_palette(palette_frames, W, H, colors=PALETTE_COLORS):
    composite = Image.new("RGB", (W, H * len(palette_frames)))
    for i, frame in enumerate(palette_frames):
        composite.paste(frame, (0, i * H))
    return composite.convert("P", palette=Image.ADAPTIVE, colors=colors)


def encode(reel, W, H, colors=PALETTE_COLORS):
    """Return (gif_bytes, poster_png_bytes, encoded_frame_count) for a reel."""
    pal = quantize_palette(reel.palette_seeds(), W, H, colors)
    qframes = [fr.convert("RGB").quantize(palette=pal, dither=Image.NONE)
               for fr in reel.frames]
    gif_buf = io.BytesIO()
    qframes[0].save(
        gif_buf, format="GIF", save_all=True, append_images=qframes[1:],
        duration=reel.durations, loop=0, optimize=True, disposal=2,
    )
    gif_bytes = gif_buf.getvalue()
    frame_count = Image.open(io.BytesIO(gif_bytes)).n_frames

    poster_buf = io.BytesIO()
    reel.frames[-1].convert("RGB").save(poster_buf, format="PNG", compress_level=6)
    return gif_bytes, poster_buf.getvalue(), frame_count


# ---------------------------------------------------------------------------
# Writing / checking — identical semantics to gen_demo_gif.py so the drift gate
# behaves the same for every demo artifact.
# ---------------------------------------------------------------------------


def write_all(files: dict, root: Path) -> list:
    written = []
    for rel in sorted(files):
        target = root / rel
        body = files[rel]
        if target.is_file() and target.read_bytes() == body:
            continue
        target.parent.mkdir(parents=True, exist_ok=True)
        tmp = target.with_name(f"{target.name}.tmp-{os.getpid()}")
        with open(tmp, "wb") as handle:
            handle.write(body)
        os.replace(tmp, target)
        written.append(rel)
    return written


def content_signature(data: bytes) -> tuple:
    """What an artifact *is* — format, size, frame count, durations and a hash of
    every frame's decoded RGB pixels. Encoded bytes are not a function of the
    pixels (zlib/zlib-ng differ across Pillow builds), so we diff pixels."""
    with Image.open(io.BytesIO(data)) as img:
        fmt, size = img.format, img.size
        digests, durations = [], []
        for frame in ImageSequence.Iterator(img):
            durations.append(frame.info.get("duration"))
            digests.append(hashlib.sha256(frame.convert("RGB").tobytes()).hexdigest())
    return (fmt, size, len(digests), tuple(durations), tuple(digests))


def check_all(files: dict, root: Path) -> list:
    report = []
    for rel in sorted(files):
        target = root / rel
        if not target.is_file():
            report.append(f"missing: {rel}")
            continue
        actual = target.read_bytes()
        if actual == files[rel]:
            continue
        try:
            committed = content_signature(actual)
        except Exception as exc:
            report.append(f"unreadable: {rel} ({exc})")
            continue
        generated = content_signature(files[rel])
        if committed != generated:
            report.append(
                f"drift: {rel} (committed {committed[0]} {committed[1]} "
                f"{committed[2]} frame(s), generated {generated[0]} {generated[1]} "
                f"{generated[2]} frame(s); pixel or timing content differs)"
            )
    return report
