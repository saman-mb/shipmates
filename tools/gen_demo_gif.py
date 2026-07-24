#!/usr/bin/env python3
"""Generate assets/demo.gif — an illustrative animated terminal of a `/ship-issue` run.

Honest by construction: it depicts the *actual stage sequence* the workflow performs
(Plan → Isolate → Build → Self-check → CI gate → Review → Remediate → Deliver) with
generic labels — no fabricated test counts or invented file names. Deterministic and
committed, matching the repo's other generators. Regenerate with:  python3 tools/gen_demo_gif.py
"""
from PIL import Image, ImageDraw, ImageFont

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
    ("REVIEW",     "board reviews the PR head …",        "product-manager · sdet · security — accept"),
    ("REMEDIATE",  "apply fixes, re-review …",           "0 blockers · nits filed as follow-ups"),
    ("DELIVER",    "",                                        "PR #143 — reviewed, CI-green, yours to merge"),
]

W, H = 940, 604
PADX, TOPBAR = 34, 40
LINE_H = 30
FONT_DIR = "/usr/share/fonts/truetype/dejavu/"
f  = ImageFont.truetype(FONT_DIR + "DejaVuSansMono.ttf", 19)
fb = ImageFont.truetype(FONT_DIR + "DejaVuSansMono-Bold.ttf", 19)
CH = fb.getlength("M")  # mono advance

SPIN = ["⠂", "⡆", "⣤", "⣰", "⢸", "⠹", "⠛", "⠏"]
# fall back to ascii spinner if braille not covered
if fb.getbbox(SPIN[0]) is None:
    SPIN = ["|", "/", "-", "\\"]


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
        x += f.getlength(text) if not bold else fb.getlength(text)
    return x


def render(lines, cursor_xy=None):
    """lines: list of segment-lists. cursor_xy: (line_index, at_end) to draw a block cursor."""
    img, d = base_frame()
    y0 = 16 + TOPBAR + 20
    for i, segs in enumerate(lines):
        y = y0 + i * LINE_H
        endx = draw_segments(d, PADX, y, segs)
        if cursor_xy is not None and cursor_xy == i:
            d.rectangle([endx + 2, y + 2, endx + 2 + CH, y + 21], fill=CURSOR)
    return img


def stage_line(label, symbol, sym_color, detail, detail_color):
    lab = label.ljust(11)
    return [("  ", GREY, False), (symbol + " ", sym_color, True),
            (lab, STAGE_COLORS[label], True), (detail, detail_color, False)]


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

for label, running, done in STAGES:
    color = STAGE_COLORS[label]
    # running: spinner cycles
    for s in range(4):
        line = stage_line(label, SPIN[s % len(SPIN)], color, running or "working …", FAINT)
        frame_lines = log + [line]
        frames.append(render(frame_lines)); durations.append(95)
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

# quantize for small size
pal = frames[0].convert("P", palette=Image.ADAPTIVE, colors=64)
qframes = [fr.convert("RGB").quantize(palette=pal, dither=Image.NONE) for fr in frames]

out = "assets/demo.gif"
qframes[0].save(out, save_all=True, append_images=qframes[1:], duration=durations,
                loop=0, optimize=True, disposal=2)
print(f"wrote {out}  ({len(qframes)} frames)")
