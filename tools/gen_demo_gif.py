#!/usr/bin/env python3
"""Generate site/assets/demo.gif — an illustrative animated terminal of a `/ship-issue` run.

Honest by construction: it depicts the *actual stage sequence* the workflow performs
(Plan → Isolate → Build → Self-check → CI gate → Review → Remediate → Deliver) with
generic labels — no fabricated test counts or invented file names. Deterministic and
committed, matching the repo's other generators.

Writes two artifacts, both derived from the same frames:
  site/assets/demo.gif        — canonical animation (README and site)
  site/assets/demo-poster.png — final frame, served under prefers-reduced-motion

Regenerate:            python3 tools/gen_demo_gif.py
Check for drift (CI):  python3 tools/gen_demo_gif.py --check
"""
import argparse
import hashlib
import io
import os
import sys
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont, ImageSequence

ROOT = Path(__file__).resolve().parents[1]

# ---- palette (Night-Owl-ish terminal) ----
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
GREEN  = (126, 231, 135)   # ✓ / success
PROMPT = (86, 214, 122)    # $ prompt
CURSOR = (200, 220, 235)

STAGE_COLORS = {
    "PLAN":       (130, 170, 255),
    "ISOLATE":    (170, 150, 255),
    "BUILD":      (255, 180, 90),
    "SELF-CHECK": (95, 210, 220),
    "CI GATE":    (255, 145, 130),
    "REVIEW":     (245, 200, 100),
    "REMEDIATE":  (150, 200, 160),
    "DELIVER":    (126, 231, 135),
}

# (label, running detail, done detail)
STAGES = [
    ("PLAN",       "reading the issue + your docs …",   "work units · acceptance criteria · validation plan"),
    ("ISOLATE",    "creating a throwaway git worktree …","feat/issue-142  (sandbox — base stays clean)"),
    ("BUILD",      "senior-engineer ×3, in parallel …","built to the plan"),
    ("SELF-CHECK", "sdet runs the real test/build …",   "tests pass"),
    ("CI GATE",    "waiting for CI to go green …",       "CI green on the pushed PR"),
    ("REVIEW",     "board reviews the PR head …",        "product-manager · sdet · flagged specialists — accept"),
    ("REMEDIATE",  "apply fixes, re-review …",           "0 blockers · nits filed as follow-ups"),
    ("DELIVER",    "",                                        "PR #143 — reviewed, CI-green, yours to merge"),
]

W, H = 940, 604
PADX, TOPBAR = 34, 40
LINE_H = 30

FONT_REGULAR = "DejaVuSansMono.ttf"
FONT_BOLD = "DejaVuSansMono-Bold.ttf"
FONT_SIZE = 19

# Never a real glyph in any font — the explicit .notdef probe the spinner
# guard below compares against.
NOTDEF_PROBE = "￿"
SPIN_BRAILLE = ["⠂", "⡆", "⣤", "⣰", "⢸", "⠹", "⠛", "⠏"]
SPIN_ASCII = ["|", "/", "-", "\\"]


def _load_font(filename):
    """Load FILENAME at FONT_SIZE, or exit with an actionable per-distro install message.

    DejaVu Sans Mono is the reference face — the committed GIF is rendered with
    it, so a different face silently re-renders every glyph. No manual search
    needed: ImageFont.truetype(name) already recursively walks
    $XDG_DATA_DIRS/fonts, ~/.local/share/fonts, /usr/local/share/fonts,
    /usr/share/fonts, and on darwin /Library/Fonts, /System/Library/Fonts and
    ~/Library/Fonts (where the Homebrew cask installs it) — a strict superset
    of any per-distro directory list this module could hand-maintain, so
    there is nothing left for this function to search itself.
    """
    try:
        return ImageFont.truetype(filename, FONT_SIZE)
    except OSError:
        sys.exit(
            f"gen_demo_gif: {filename} not found.\n"
            f"  Debian/Ubuntu:  sudo apt install fonts-dejavu-core\n"
            f"  Fedora/RHEL:    sudo dnf install dejavu-sans-mono-fonts\n"
            f"  Arch:           sudo pacman -S ttf-dejavu\n"
            f"  macOS:          brew install --cask font-dejavu"
        )


def _spinner(fb):
    """Braille spinner glyphs if the bold face actually has them, else ASCII.

    getbbox() returns a box for the .notdef glyph too — it is never None, for
    any codepoint — so a `getbbox(ch) is None` guard can't detect a missing
    glyph. Comparing getmask(ch)'s bbox against an explicit .notdef probe's
    bbox does: DejaVu Sans Mono has no Braille Patterns glyphs, so all eight
    render as .notdef without this check.
    """
    notdef_bbox = fb.getmask(NOTDEF_PROBE).getbbox()
    if fb.getmask(SPIN_BRAILLE[0]).getbbox() == notdef_bbox:
        return SPIN_ASCII
    return SPIN_BRAILLE


def stage_line(label, symbol, sym_color, detail, detail_color):
    lab = label.ljust(11)
    return [("  ", GREY, False), (symbol + " ", sym_color, True),
            (lab, STAGE_COLORS[label], True), (detail, detail_color, False)]


def render_frames():
    """Build every frame + duration, plus the small set of frames used to seed
    the GIF palette. The only I/O is the font load, which fails loudly via
    _load_font rather than silently mis-rendering.
    """
    f = _load_font(FONT_REGULAR)
    fb = _load_font(FONT_BOLD)
    ch = fb.getlength("M")  # mono advance
    spin = _spinner(fb)

    def base_frame():
        img = Image.new("RGB", (W, H), PAGE)
        d = ImageDraw.Draw(img)
        d.rounded_rectangle([16, 16, W - 16, H - 16], radius=14, fill=PANEL, outline=BORDER, width=1)
        d.rounded_rectangle([16, 16, W - 16, 16 + TOPBAR], radius=14, fill=BAR)
        d.rectangle([16, 30 + TOPBAR - 14, W - 16, 16 + TOPBAR], fill=PANEL)  # square off bottom of bar
        for i, c in enumerate((DOT_R, DOT_Y, DOT_G)):
            cx = 40 + i * 26
            d.ellipse([cx, 30, cx + 13, 43], fill=c)
        title = "shipmates — /ship-issue"
        tw = f.getlength(title)
        d.text(((W - tw) / 2, 27), title, font=f, fill=GREY)
        return img, d

    def draw_segments(d, x, y, segments):
        for text, color, bold in segments:
            d.text((x, y), text, font=(fb if bold else f), fill=color)
            x += fb.getlength(text) if bold else f.getlength(text)
        return x

    def render(lines, cursor_xy=None):
        """lines: list of segment-lists. cursor_xy: line index to draw a block cursor at."""
        img, d = base_frame()
        y0 = 16 + TOPBAR + 20
        for i, segs in enumerate(lines):
            y = y0 + i * LINE_H
            endx = draw_segments(d, PADX, y, segs)
            if cursor_xy is not None and cursor_xy == i:
                d.rectangle([endx + 2, y + 2, endx + 2 + ch, y + 21], fill=CURSOR)
        return img

    frames, durations = [], []
    CMD = "/ship-issue 142"
    prompt_segs = [("$ ", PROMPT, True)]

    # 1) type the command
    for k in range(len(CMD) + 1):
        segs = [prompt_segs[0], (CMD[:k], WHITE, True)]
        frames.append(render([segs], cursor_xy=0)); durations.append(70)
    # small hold with blinking cursor
    for _ in range(3):
        frames.append(render([[prompt_segs[0], (CMD, WHITE, True)]], cursor_xy=0)); durations.append(120)
        frames.append(render([[prompt_segs[0], (CMD, WHITE, True)]])); durations.append(120)

    log = [[prompt_segs[0], (CMD, WHITE, True)], [("", GREY, False)]]  # cmd + blank spacer

    # A running frame, kept for the palette: it's the only place FAINT (the
    # dim in-progress detail colour) appears — done lines never use it.
    sample_running_frame = None
    for label, running, done in STAGES:
        color = STAGE_COLORS[label]
        # running: spinner cycles
        for s in range(4):
            line = stage_line(label, spin[s % len(spin)], color, running or "working …", FAINT)
            frame = render(log + [line])
            if sample_running_frame is None:
                sample_running_frame = frame
            frames.append(frame); durations.append(95)
        # done
        done_line = stage_line(label, "✓", GREEN, done, GREY)
        log = log + [done_line]
        frames.append(render(log)); durations.append(70)
        frames.append(render(log)); durations.append(240)  # brief hold per stage

    # footer
    log = log + [[("", GREY, False)],
                 [("  ✓ ", GREEN, True), ("Done — a reviewed PR, handed to you. You stay the captain. ⚓", GREEN, False)]]
    for _ in range(6):
        frames.append(render(log)); durations.append(300)

    # Every stage colour, once its done-line is appended, stays in `log`
    # permanently — so frames[0] (chrome + prompt only), one running frame
    # (FAINT + a spinner colour) and frames[-1] (every completed stage's
    # colour, all at once) between them carry every distinct colour the run
    # uses. Kept as real rendered frames, not a synthetic swatch, so the
    # palette is still built from actual pixel frequency, not one-pixel-per-
    # colour noise.
    palette_frames = (frames[0], sample_running_frame, frames[-1])
    return frames, durations, palette_frames


PALETTE_COLORS = 128  # 64 left several stage colours a visible few units off


def _quantize_palette(palette_frames):
    """Build the adaptive palette from frames that between them carry every
    colour the run uses — not frame 0 alone. Frame 0 is the typed-command
    frame: it has no stage colour in it at all, so quantizing against it left
    PLAN, ISOLATE and SELF-CHECK (whichever stage colours frame 0 doesn't
    happen to share) rounded to a nearby unrelated colour instead of their
    designed value.

    Antialiasing on the text glyphs fills most of a small colour budget with
    edge blends, so 64 slots (the original budget, chosen for file size)
    still left a few stage colours a handful of RGB units off their design —
    close, but not what shipped. 128 buys back every stage colour that
    matters here (still well under the GIF format's 256-colour ceiling) for
    ~7% more bytes.
    """
    composite = Image.new("RGB", (W, H * len(palette_frames)))
    for i, frame in enumerate(palette_frames):
        composite.paste(frame, (0, i * H))
    return composite.convert("P", palette=Image.ADAPTIVE, colors=PALETTE_COLORS)


def build_artifacts():
    """Return ({relative posix path: bytes}, real encoded GIF frame count).

    Pure aside from the font load inside render_frames, which can only fail
    loudly (sys.exit with the install message), never silently mis-render.
    """
    frames, durations, palette_frames = render_frames()
    pal = _quantize_palette(palette_frames)
    qframes = [fr.convert("RGB").quantize(palette=pal, dither=Image.NONE) for fr in frames]

    gif_buf = io.BytesIO()
    qframes[0].save(
        gif_buf, format="GIF", save_all=True, append_images=qframes[1:],
        duration=durations, loop=0, optimize=True, disposal=2,
    )
    gif_bytes = gif_buf.getvalue()
    # Pillow merges consecutive identical frames (and sums their durations) on
    # save under optimize=True — len(qframes) is what went in, not what's in
    # the file.
    frame_count = Image.open(io.BytesIO(gif_bytes)).n_frames

    # prefers-reduced-motion poster — the finished-run frame, shown in place
    # of the animation. It must be the *last* frame, or it depicts a run that
    # never ended.
    poster_buf = io.BytesIO()
    # compress_level pinned so the committed file is at least stable for a
    # given Pillow. It is NOT stable across Pillow builds — see
    # _content_signature — which is why the drift check compares pixels.
    frames[-1].convert("RGB").save(poster_buf, format="PNG", compress_level=6)

    return (
        {
            "site/assets/demo.gif": gif_bytes,
            "site/assets/demo-poster.png": poster_buf.getvalue(),
        },
        frame_count,
    )


# ---------------------------------------------------------------------------
# Writing / checking — the only place this module touches the filesystem
# outside of loading a font.
# ---------------------------------------------------------------------------


def write_all(files: dict, root: Path) -> list:
    """Write every artifact that differs, atomically, creating parent
    directories as needed. The only writer in this module: a crash mid-run
    leaves at most the artifacts already renamed into place, never a
    half-written file a reader could see, and never one artifact updated
    while its siblings are left stale.
    """
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


def _content_signature(data: bytes) -> tuple:
    """What an artifact *is* — format, size, frame count, per-frame durations
    and a hash of every frame's decoded RGB pixels.

    Encoded bytes are not a function of the pixels, so they are the wrong
    thing to diff. Pillow's PNG deflate comes from whatever zlib the wheel was
    linked against — classic zlib and the zlib-ng that Pillow 11.3+ bundles
    emit different streams for identical input — so one poster encodes to
    47861 / 48818 / 49101 bytes on three machines whose pixels are
    bit-identical. It also moves with compress_level and with any future
    change to Pillow's PNG defaults. The GIF happens to be stable today only
    because Pillow's LZW is its own C encoder and never touches zlib; that is
    a Pillow implementation detail, not a contract, so it gets the same
    treatment.
    """
    with Image.open(io.BytesIO(data)) as img:
        fmt, size = img.format, img.size
        digests, durations = [], []
        for frame in ImageSequence.Iterator(img):
            durations.append(frame.info.get("duration"))
            digests.append(hashlib.sha256(frame.convert("RGB").tobytes()).hexdigest())
    return (fmt, size, len(digests), tuple(durations), tuple(digests))


def check_all(files: dict, root: Path) -> list:
    """Drift report lines. Writes NOTHING — this function never opens a path
    for writing."""
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
            committed = _content_signature(actual)
        except Exception as exc:  # unreadable/corrupt committed artifact
            report.append(f"unreadable: {rel} ({exc})")
            continue
        generated = _content_signature(files[rel])
        if committed != generated:
            report.append(
                f"drift: {rel} (committed {committed[0]} {committed[1]} "
                f"{committed[2]} frame(s), generated {generated[0]} {generated[1]} "
                f"{generated[2]} frame(s); pixel or timing content differs)"
            )

    return report


# ---------------------------------------------------------------------------
# CLI — the only layer that prints or exits. Root comes from __file__, never
# from cwd, and paths are always root-relative so this runs the same from
# anywhere.
# ---------------------------------------------------------------------------

REGENERATE_HINT = (
    "run: python3 tools/gen_demo_gif.py && "
    "git add site/assets/demo.gif site/assets/demo-poster.png"
)


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(
        description=(
            "Generate site/assets/demo.gif and "
            "site/assets/demo-poster.png from one set of rendered frames."
        )
    )
    parser.add_argument(
        "--check",
        action="store_true",
        help="report drift against the committed artifacts and exit 1; write nothing",
    )
    parser.add_argument(
        "--root",
        default=str(ROOT),
        metavar="PATH",
        help="repository root (default: the repo this script lives in)",
    )
    args = parser.parse_args(argv)

    root = Path(args.root).resolve()
    files, frame_count = build_artifacts()

    if args.check:
        report = check_all(files, root)
        if report:
            for line in report:
                print(line)
            print(REGENERATE_HINT)
            return 1
        print(f"up to date: 2 artifacts, {frame_count} encoded GIF frames")
        return 0

    written = write_all(files, root)
    notes = {
        "site/assets/demo.gif": f"({frame_count} frames)",
        "site/assets/demo-poster.png": "(final frame)",
    }
    for rel in sorted(files):
        if rel in written:
            print(f"wrote {rel}  {notes[rel]}")
    print(
        f"{len(written)} of {len(files)} artifacts updated "
        f"({frame_count} encoded GIF frames)"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
