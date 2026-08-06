#!/usr/bin/env python3
"""Render the premium Shipmates "state gate" motion-graphics asset for LinkedIn.

This is a bespoke marketing-asset generator (NOT the `diagram` tool). It composes
360 frames in Pillow at 2x, LANCZOS-downscales to a 1080x1350 (4:5 portrait) canvas,
applies a film-grain + vignette pass, and encodes an MP4 (primary) and a GIF fallback
via ffmpeg.

Story (accurate to /ship-issue): a per-harness PreToolUse hook calls
`shipmates state gate` before every tool call. Three beats:
  1. `git push`      in phase build   -> ALLOW  (gate opens, packet continues right)
  2. `gh pr merge`   in phase build   -> DENY   (gate shut, packet springs back left)
  3. phase advances build->deliver, `gh pr merge` retried -> ALLOW (bigger payoff)
Punchline: it enforces ORDER, not a blanket block. Codex is the worked example.

Outputs (next to this file):
  hooks-gate.mp4   H.264 yuv420p CRF 18 +faststart, 1080x1350, 30fps, 12.0s
  hooks-gate.gif   two-pass palette fallback

Usage:
  python3 render_hooks_animation.py                 # full render (mp4 + gif)
  python3 render_hooks_animation.py --fast          # 1x, every 3rd frame, mp4 only
  python3 render_hooks_animation.py --frame 96      # dump a single frame PNG for review
  python3 render_hooks_animation.py --frames-only   # render PNG frames, skip ffmpeg

Deterministic by construction: the per-frame grain is seeded by frame index, so a
re-render is byte-identical. Fonts fall back to DejaVu Sans / DejaVu Sans Mono
(guaranteed on the host); Inter / JetBrains Mono are used if a TTF is found.
"""
import argparse
import math
import os
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw, ImageFilter, ImageFont

HERE = Path(__file__).resolve().parent
FFMPEG = "/usr/bin/ffmpeg"

# ---------------------------------------------------------------------------
# Canvas
# ---------------------------------------------------------------------------
W, H = 1080, 1350          # final (4:5 portrait)
FPS = 30
N_FRAMES = 360             # 12.0s; F360 state == F0 (seamless loop)
MARGIN = 72

SS = 2                     # supersample factor (set to 1 in --fast)


def P(v):
    """Scale a FINAL-space coordinate/length into the 2x render space (int)."""
    return int(round(v * SS))


def Pf(v):
    return v * SS


# ---------------------------------------------------------------------------
# Palette (exact tokens from the art-director spec)
# ---------------------------------------------------------------------------
def hx(s):
    s = s.lstrip("#")
    return (int(s[0:2], 16), int(s[2:4], 16), int(s[4:6], 16))


BG_TOP = hx("#14110F")
BG_MID = hx("#1A1512")
BG_BOT = hx("#14110F")
PANEL = hx("#1D1916")
TEXT = hx("#F2EDE8")
MUTED = hx("#B3A99F")
BORDER1 = hx("#332C26")
BORDER2 = hx("#4A4139")
ACCENT = hx("#D97757")
ACCENT_LT = hx("#E8916F")

GREEN = hx("#4ADE94")
GREEN_RIM = hx("#1E7A52")
GREEN_FLASH = hx("#7BFFC0")

RED = hx("#F25C54")
RED_DEEP = hx("#C6382F")
RED_FLASH = hx("#FF7A6B")


def col(rgb, a=255):
    return (rgb[0], rgb[1], rgb[2], int(max(0, min(255, a))))


def mix(a, b, t):
    t = max(0.0, min(1.0, t))
    return (round(a[0] + (b[0] - a[0]) * t),
            round(a[1] + (b[1] - a[1]) * t),
            round(a[2] + (b[2] - a[2]) * t))


# ---------------------------------------------------------------------------
# Easing
# ---------------------------------------------------------------------------
def clamp01(t):
    return 0.0 if t < 0 else (1.0 if t > 1 else t)


def seg(f, a, b):
    """Normalised 0..1 progress of frame f across [a, b] (clamped)."""
    if b <= a:
        return 1.0 if f >= b else 0.0
    return clamp01((f - a) / (b - a))


def ease_out_cubic(t):
    t = clamp01(t)
    return 1 - (1 - t) ** 3


def ease_in_cubic(t):
    t = clamp01(t)
    return t ** 3


def ease_in_out(t):
    t = clamp01(t)
    return t * t * (3 - 2 * t)          # smoothstep


def ease_out_back(t, c1=1.9):
    t = clamp01(t)
    c3 = c1 + 1
    return 1 + c3 * (t - 1) ** 3 + c1 * (t - 1) ** 2


def cubic_bezier(x1, y1, x2, y2):
    """Return an easing fn y(t) for a CSS-style cubic-bezier (Newton solve)."""
    def bx(t):
        return 3 * (1 - t) ** 2 * t * x1 + 3 * (1 - t) * t * t * x2 + t ** 3

    def by(t):
        return 3 * (1 - t) ** 2 * t * y1 + 3 * (1 - t) * t * t * y2 + t ** 3

    def solve(x):
        x = clamp01(x)
        t = x
        for _ in range(8):
            err = bx(t) - x
            if abs(err) < 1e-5:
                break
            d = (bx(t + 1e-4) - bx(t - 1e-4)) / 2e-4
            if abs(d) < 1e-6:
                break
            t -= err / d
        return by(clamp01(t))

    return solve


ENTRANCE = cubic_bezier(0.2, 0.8, 0.2, 1)     # spec entrance easing


# ---------------------------------------------------------------------------
# Fonts
# ---------------------------------------------------------------------------
_FONT_DIRS = ["/usr/share/fonts", str(HERE)]


def _find_font(names):
    for root in _FONT_DIRS:
        for base, _dirs, files in os.walk(root):
            for fn in files:
                if fn in names:
                    return os.path.join(base, fn)
    return None


SANS_REG = _find_font(["Inter-Regular.ttf", "Inter.ttf", "DejaVuSans.ttf"]) \
    or _find_font(["DejaVuSans.ttf"])
SANS_BOLD = _find_font(["Inter-Bold.ttf", "DejaVuSans-Bold.ttf"])
MONO_REG = _find_font(["JetBrainsMono-Regular.ttf", "IBMPlexMono-Regular.ttf",
                       "DejaVuSansMono.ttf"])
MONO_BOLD = _find_font(["JetBrainsMono-Bold.ttf", "IBMPlexMono-Bold.ttf",
                        "DejaVuSansMono-Bold.ttf"])

_font_cache = {}


def font(path, size):
    key = (path, size)
    if key not in _font_cache:
        _font_cache[key] = ImageFont.truetype(path, size)
    return _font_cache[key]


def sans(size, bold=False):
    return font(SANS_BOLD if bold else SANS_REG, P(size))


def mono(size, bold=False):
    return font(MONO_BOLD if bold else MONO_REG, P(size))


# ---------------------------------------------------------------------------
# Text helpers (work in FINAL coords; scale internally)
# ---------------------------------------------------------------------------
def text_width(draw, text, fnt, tracking=0):
    w = draw.textlength(text, font=fnt)
    if tracking and len(text) > 1:
        w += P(tracking) * (len(text) - 1)
    return w


def draw_tracked(draw, cx, y, text, fnt, fill, tracking=0, anchor="mm"):
    """Draw letter-spaced text. cx,y in FINAL coords; anchor 'mm' or 'lm'."""
    total = text_width(draw, text, fnt, tracking)
    x = P(cx) - (total / 2 if anchor == "mm" else 0)
    for ch in text:
        draw.text((x, P(y)), ch, font=fnt, fill=fill, anchor="lm")
        x += draw.textlength(ch, font=fnt) + P(tracking)


def draw_center(draw, cx, y, text, fnt, fill):
    draw.text((P(cx), P(y)), text, font=fnt, fill=fill, anchor="mm")


def draw_runs(draw, cx, y, runs, fnt, tracking=0):
    """Centered multi-colour text. runs = [(text, fill), ...]."""
    total = sum(text_width(draw, t, fnt, tracking) for t, _ in runs)
    x = P(cx) - total / 2
    for t, fill in runs:
        for ch in t:
            draw.text((x, P(y)), ch, font=fnt, fill=fill, anchor="lm")
            x += draw.textlength(ch, font=fnt) + P(tracking)


# ---------------------------------------------------------------------------
# Glow (two-layer blur under a crisp shape; animate INTENSITY not radius)
# ---------------------------------------------------------------------------
def glow_from_stamp(canvas, stamp, topleft, rgb, tight_a, wide_a,
                    tight_r=10, wide_r=32):
    """stamp: 'L' alpha image. topleft in 2x-canvas px."""
    for rad, a in ((wide_r, wide_a), (tight_r, tight_a)):
        if a <= 0.003:
            continue
        blurred = stamp.filter(ImageFilter.GaussianBlur(Pf(rad)))
        layer = Image.new("RGBA", blurred.size, rgb + (0,))
        layer.putalpha(blurred.point(lambda p, a=a: int(p * a)))
        canvas.alpha_composite(layer, dest=topleft)


def glow_circle(canvas, cx, cy, r, rgb, tight_a, wide_a, tight_r=10, wide_r=32):
    pad = P(wide_r) * 2 + P(r) + 4
    size = pad * 2
    stamp = Image.new("L", (size, size), 0)
    d = ImageDraw.Draw(stamp)
    d.ellipse((pad - P(r), pad - P(r), pad + P(r), pad + P(r)), fill=255)
    glow_from_stamp(canvas, stamp, (P(cx) - pad, P(cy) - pad), rgb,
                    tight_a, wide_a, tight_r, wide_r)


def glow_rect(canvas, x0, y0, x1, y1, rad, rgb, tight_a, wide_a,
              tight_r=10, wide_r=32):
    pad = P(wide_r) * 2 + 4
    w = P(x1 - x0) + pad * 2
    h = P(y1 - y0) + pad * 2
    stamp = Image.new("L", (w, h), 0)
    d = ImageDraw.Draw(stamp)
    d.rounded_rectangle((pad, pad, w - pad, h - pad), radius=P(rad), fill=255)
    glow_from_stamp(canvas, stamp, (P(x0) - pad, P(y0) - pad), rgb,
                    tight_a, wide_a, tight_r, wide_r)


# ---------------------------------------------------------------------------
# Background (gradient + vignette prebuilt once at final res)
# ---------------------------------------------------------------------------
def build_background():
    """Vertical 3-stop gradient at 2x render res."""
    h = H * SS
    grad = np.zeros((h, W * SS, 3), dtype=np.float32)
    top = np.array(BG_TOP, np.float32)
    midc = np.array(BG_MID, np.float32)
    bot = np.array(BG_BOT, np.float32)
    for y in range(h):
        u = y / (h - 1)
        if u < 0.5:
            c = top + (midc - top) * ease_in_out(u / 0.5)
        else:
            c = midc + (bot - midc) * ease_in_out((u - 0.5) / 0.5)
        grad[y, :, :] = c
    img = Image.fromarray(grad.astype(np.uint8), "RGB").convert("RGBA")
    return img


def build_vignette_mask():
    """Radial darkening multiplier at FINAL res (~12% at corners)."""
    yy, xx = np.mgrid[0:H, 0:W].astype(np.float32)
    cx, cy = W / 2, H / 2
    d = np.sqrt(((xx - cx) / cx) ** 2 + ((yy - cy) / cy) ** 2) / math.sqrt(2)
    m = 1.0 - 0.12 * np.clip(d, 0, 1) ** 2.2
    return m[:, :, None]


# ---------------------------------------------------------------------------
# Scene geometry (FINAL coords)
# ---------------------------------------------------------------------------
WIRE_Y = 725
AGENT_X = 175
HOOK_X = 360
GATE_X = 540
DEST_X = 905
NODE_W, NODE_H = 140, 110
WIRE_L = AGENT_X + NODE_W / 2          # 245
WIRE_R = DEST_X - NODE_W / 2           # 835
GATE_STOP = 430                        # packet's verdict position (slot stays visible)
REPEL_X = 300                          # where a denied packet springs back to

# Phase rail
RAIL_Y = 336
PHASES = ["plan", "isolate", "build", "verify", "review", "deliver"]
RAIL_X0 = MARGIN + 6
RAIL_X1 = W - MARGIN - 6


def pip_x(i):
    return RAIL_X0 + (RAIL_X1 - RAIL_X0) * (i / (len(PHASES) - 1))


# ---------------------------------------------------------------------------
# Global timeline drivers
# ---------------------------------------------------------------------------
def appear(_f):
    """The static composition is PERSISTENT (title, nodes, gate, rail scaffold,
    punchline, credit never blank) so the loop is invisible — only the dynamic
    beat layer resets. Kept as a hook returning 1.0 so element renderers can
    still modulate by it if a future variant wants a fade-in."""
    return 1.0


def punch_emphasis(f):
    """Punchline brightness pulse: base -> full near F348 -> base by F360.
    Periodic (== base at both F0 and F360) so the loop seam matches exactly."""
    up = ease_out_cubic(seg(f, 330, 342))
    down = ease_in_cubic(seg(f, 348, 360))
    boost = 0.72 + 0.28 * (up * (1 - down))
    lift = 6.0 * (1 - up * (1 - down))
    return boost, lift


def phase_value(f):
    """Continuous phase index. build(2) most of the run; advances to deliver(5)
    in the transition; returns to 2 by F360 (masked by the fade)."""
    kf = [(0, 2.0), (206, 2.0), (216, 3.0), (220, 3.0), (230, 4.0),
          (234, 4.0), (244, 5.0), (348, 5.0), (360, 2.0)]
    for (fa, va), (fb, vb) in zip(kf, kf[1:]):
        if f <= fb:
            if vb == va:
                return va
            return va + (vb - va) * ease_in_out(seg(f, fa, fb))
    return kf[-1][1]


# ---------------------------------------------------------------------------
# Packet model — pure functions so trailing ghosts sample past sub-frames
# ---------------------------------------------------------------------------
# beat windows (frames @30fps)
B1 = dict(s0=36, s1=48, t0=48, t1=90, v0=90, v1=102, r0=102, r1=118,
          label="git push")
B2 = dict(s0=122, s1=134, t0=134, t1=168, v0=168, v1=180, x0=180, x1=200,
          label="gh pr merge")
B3 = dict(s0=264, s1=276, t0=276, t1=308, v0=308, v1=322, r0=320, r1=334,
          label="gh pr merge")


def packet_state(f):
    """Return dict describing the single packet on the wire, or None.
    keys: label, x, scale, alpha, stretch, lead_rgb, mode."""
    # Beat 1 — allow git push
    if B1["s0"] <= f < B2["s0"]:
        b = B1
        if f < b["s1"]:
            sc = 0.8 + 0.2 * ENTRANCE(seg(f, b["s0"], b["s1"]))
            return dict(label=b["label"], x=AGENT_X, scale=sc, alpha=seg(f, b["s0"], b["s1"]),
                        stretch=1.0, lead=ACCENT, mode="spawn")
        if f < b["t1"]:
            t = ease_out_cubic(seg(f, b["t0"], b["t1"]))
            return dict(label=b["label"], x=AGENT_X + (GATE_STOP - AGENT_X) * t,
                        scale=1.0, alpha=1.0, stretch=1.0, lead=ACCENT, mode="travel_in")
        if f < b["v1"]:
            return dict(label=b["label"], x=GATE_STOP, scale=1.0, alpha=1.0,
                        stretch=1.0, lead=ACCENT, mode="hold")
        if f <= b["r1"] + 8:
            t = ease_in_cubic(seg(f, b["r0"], b["r1"]))
            x = GATE_STOP + (DEST_X - GATE_STOP) * t
            fade = 1.0 - seg(f, b["r1"], b["r1"] + 8)
            stretch = 1.0 + 0.15 * (1 - abs(2 * seg(f, b["r0"], b["r1"]) - 1))
            return dict(label=b["label"], x=x, scale=1.0, alpha=fade,
                        stretch=stretch, lead=ACCENT, mode="release")
        return None
    # Beat 2 — deny gh pr merge
    if B2["s0"] <= f < 210:
        b = B2
        if f < b["s1"]:
            sc = 0.8 + 0.2 * ENTRANCE(seg(f, b["s0"], b["s1"]))
            return dict(label=b["label"], x=AGENT_X, scale=sc, alpha=seg(f, b["s0"], b["s1"]),
                        stretch=1.0, lead=ACCENT, mode="spawn")
        if f < b["t1"]:
            t = ease_out_cubic(seg(f, b["t0"], b["t1"]))
            return dict(label=b["label"], x=AGENT_X + (GATE_STOP - AGENT_X) * t,
                        scale=1.0, alpha=1.0, stretch=1.0, lead=ACCENT, mode="travel_in")
        if f < b["v1"]:
            return dict(label=b["label"], x=GATE_STOP, scale=1.0, alpha=1.0,
                        stretch=1.0, lead=RED, mode="hold_deny")
        if f <= b["x1"]:
            # Fix 1 — physical deny recoil (not a slide): spring back PAST the
            # codex rest x, overshoot ~12px, then settle back over the final ~4f.
            over = 12.0
            peakf = b["x1"] - 4                     # F196: overshoot peak
            if f <= peakf:
                # snappy reversed spring-back to ~over px past the rest x. (A plain
                # ease_out_cubic keeps the overshoot at a controlled 12px; layering
                # an easeOutBack here would stack its own overshoot on top.)
                t = ease_out_cubic(seg(f, b["x0"], peakf))
                x = GATE_STOP + ((REPEL_X - over) - GATE_STOP) * t
            else:
                # final settle: +12 -> 0, easing back onto the codex rest x
                x = (REPEL_X - over) + over * ease_out_back(
                    seg(f, peakf, b["x1"]), c1=2.2)
            fade = 1.0 - seg(f, b["x1"] - 6, b["x1"])
            return dict(label=b["label"], x=x, scale=1.0, alpha=fade,
                        stretch=1.0, lead=RED, mode="repel")
        return None
    # Beat 3 — allow gh pr merge (retry, later phase)
    if B3["s0"] <= f < 344:
        b = B3
        if f < b["s1"]:
            sc = 0.8 + 0.2 * ENTRANCE(seg(f, b["s0"], b["s1"]))
            return dict(label=b["label"], x=AGENT_X, scale=sc, alpha=seg(f, b["s0"], b["s1"]),
                        stretch=1.0, lead=ACCENT, mode="spawn")
        if f < b["t1"]:
            t = ease_out_cubic(seg(f, b["t0"], b["t1"]))
            return dict(label=b["label"], x=AGENT_X + (GATE_STOP - AGENT_X) * t,
                        scale=1.0, alpha=1.0, stretch=1.0, lead=ACCENT, mode="travel_in")
        if f < b["v1"]:
            return dict(label=b["label"], x=GATE_STOP, scale=1.0, alpha=1.0,
                        stretch=1.0, lead=ACCENT, mode="hold")
        if f <= b["r1"] + 10:
            t = ease_in_cubic(seg(f, b["r0"], b["r1"]))
            x = GATE_STOP + (DEST_X - GATE_STOP) * t
            fade = 1.0 - seg(f, b["r1"], b["r1"] + 10)
            stretch = 1.0 + 0.15 * (1 - abs(2 * seg(f, b["r0"], b["r1"]) - 1))
            return dict(label=b["label"], x=x, scale=1.0, alpha=fade,
                        stretch=stretch, lead=ACCENT, mode="release")
        return None
    return None


# ---------------------------------------------------------------------------
# Element renderers
# ---------------------------------------------------------------------------
def draw_shadow(canvas, cx, cy, rx, ry, a=0.30):
    pad = P(28)
    w = P(rx) * 2 + pad * 2
    h = P(ry) * 2 + pad * 2
    stamp = Image.new("L", (w, h), 0)
    d = ImageDraw.Draw(stamp)
    d.ellipse((pad, pad, w - pad, h - pad), fill=int(255 * a))
    stamp = stamp.filter(ImageFilter.GaussianBlur(Pf(14)))
    layer = Image.new("RGBA", stamp.size, (0, 0, 0, 0))
    layer.putalpha(stamp)
    canvas.alpha_composite(layer, dest=(P(cx) - w // 2, P(cy) - h // 2))


def draw_node(canvas, cx, label, caption, ap, scale, label_rgb=TEXT,
              accent_edge=False, status=None, status_rgb=GREEN):
    a = int(255 * ap)
    if a <= 2:
        return
    hw = NODE_W / 2 * scale
    hh = NODE_H / 2 * scale
    x0, y0, x1, y1 = cx - hw, WIRE_Y - hh, cx + hw, WIRE_Y + hh
    draw_shadow(canvas, cx, WIRE_Y + 6, hw + 4, hh, a=0.30 * ap)
    d = ImageDraw.Draw(canvas)
    d.rounded_rectangle((P(x0), P(y0), P(x1), P(y1)), radius=P(16),
                        fill=col(PANEL, a),
                        outline=col(ACCENT if accent_edge else BORDER2, a),
                        width=P(2))
    draw_center(d, cx, WIRE_Y, label, mono(24, bold=True), col(label_rgb, a))
    if caption:
        draw_center(d, cx, y0 - 22, caption, mono(17),
                    col(MUTED, int(a * 0.9)))
    if status:
        draw_center(d, cx, y1 + 24, status, mono(19, bold=True),
                    col(status_rgb, a))


def draw_hook(canvas, ap, blip):
    a = int(255 * ap)
    if a <= 2:
        return
    d = ImageDraw.Draw(canvas)
    top, bot = WIRE_Y - 30, WIRE_Y + 30
    if blip > 0.01:
        glow_rect(canvas, HOOK_X - 3, top, HOOK_X + 3, bot, 3, ACCENT_LT,
                  0.5 * blip * ap, 0.28 * blip * ap, tight_r=8, wide_r=22)
    edge = mix(BORDER2, ACCENT_LT, 0.4 + 0.6 * blip)
    d.line((P(HOOK_X), P(top), P(HOOK_X), P(bot)),
           fill=col(edge, a), width=P(3))
    d.line((P(HOOK_X - 7), P(top), P(HOOK_X + 7), P(top)),
           fill=col(edge, a), width=P(2))
    d.line((P(HOOK_X - 7), P(bot), P(HOOK_X + 7), P(bot)),
           fill=col(edge, a), width=P(2))
    draw_center(d, HOOK_X, top - 40, "PreToolUse", mono(15),
                col(MUTED, int(a * 0.95)))
    draw_center(d, HOOK_X, top - 22, "→ state gate", mono(15),
                col(ACCENT_LT, int(a * 0.95)))


def _stroke_check(d, cx, cy, s, rgb, a, prog, width):
    """Animated checkmark stroke (0..1)."""
    p1 = (cx - s, cy + s * 0.1)
    p2 = (cx - s * 0.25, cy + s * 0.75)
    p3 = (cx + s, cy - s * 0.7)
    seg1 = 0.4
    pts = [p1]
    if prog <= seg1:
        t = prog / seg1
        pts = [p1, (p1[0] + (p2[0] - p1[0]) * t, p1[1] + (p2[1] - p1[1]) * t)]
    else:
        t = (prog - seg1) / (1 - seg1)
        pts = [p1, p2, (p2[0] + (p3[0] - p2[0]) * t, p2[1] + (p3[1] - p2[1]) * t)]
    xy = [(P(x), P(y)) for x, y in pts]
    if len(xy) >= 2:
        d.line(xy, fill=col(rgb, a), width=width, joint="curve")


def _stroke_x(d, cx, cy, s, rgb, a, prog, width):
    a1 = clamp01(prog / 0.5)
    a2 = clamp01((prog - 0.5) / 0.5)
    if a1 > 0:
        d.line((P(cx - s), P(cy - s),
                P(cx - s + 2 * s * a1), P(cy - s + 2 * s * a1)),
               fill=col(rgb, a), width=width)
    if a2 > 0:
        d.line((P(cx + s), P(cy - s),
                P(cx + s - 2 * s * a2), P(cy - s + 2 * s * a2)),
               fill=col(rgb, a), width=width)


def gate_shake(f):
    """Damped ±3px x jitter on the gate leaves right after a DENY verdict."""
    if B2["v0"] <= f < B2["v0"] + 6:            # F168–174 (peak inside F171–173)
        sh = seg(f, B2["v0"], B2["v0"] + 6)
        return math.sin(sh * math.pi * 3) * 3 * (1 - sh)
    return 0.0


def draw_gate(canvas, f, ap):
    """The hero: two vertical leaves meeting at a slot, 4% larger + accent halo."""
    a = int(255 * ap)
    if a <= 2:
        return
    scale = 1.04
    leaf_w = 40 * scale
    leaf_h = 230 * scale
    top = WIRE_Y - leaf_h / 2
    bot = WIRE_Y + leaf_h / 2
    slot_half = 7

    # verdict state
    verdict = None       # 'allow' / 'deny'
    vprog = 0.0
    open_px = 0.0
    rim_rgb = BORDER2
    bloom = 0.0
    shake = 0.0
    if B1["v0"] <= f < B1["r1"]:
        verdict, vprog = "allow", seg(f, B1["v0"], B1["v0"] + 9)
        open_px = 28 * ease_out_cubic(seg(f, B1["v0"], B1["v1"]))
        if f > B1["v1"]:
            open_px *= 1 - 0.7 * seg(f, B1["v1"], B1["r1"])
        bloom = math.sin(math.pi * clamp01(seg(f, B1["v0"], B1["r1"]))) * 0.7
    elif B2["v0"] <= f < B2["x0"] + 8:
        verdict, vprog = "deny", seg(f, B2["v0"], B2["v0"] + 9)
        bloom = math.sin(math.pi * clamp01(seg(f, B2["v0"], B2["v0"] + 12))) * 0.8
        shake = gate_shake(f)
    elif B3["v0"] <= f < B3["r1"]:
        verdict, vprog = "allow", seg(f, B3["v0"], B3["v0"] + 9)
        open_px = 34 * ease_out_cubic(seg(f, B3["v0"], B3["v1"]))
        if f > B3["v1"]:
            open_px *= 1 - 0.7 * seg(f, B3["v1"], B3["r1"])
        bloom = math.sin(math.pi * clamp01(seg(f, B3["v0"], B3["r1"]))) * 1.0

    armed = ease_in_out(clamp01((phase_value(f) - 4.3) / 0.7))

    # halo behind the gate
    if verdict == "allow":
        glow_rect(canvas, GATE_X - 70, top - 20, GATE_X + 70, bot + 20, 20,
                  GREEN, 0.55 * bloom * ap, 0.4 * bloom * ap, tight_r=12, wide_r=40)
        rim_rgb = mix(GREEN_RIM, GREEN_FLASH, vprog)
    elif verdict == "deny":
        glow_rect(canvas, GATE_X - 70, top - 20, GATE_X + 70, bot + 20, 20,
                  RED, 0.55 * bloom * ap, 0.42 * bloom * ap, tight_r=12, wide_r=40)
        rim_rgb = mix(RED_DEEP, RED_FLASH, vprog)
    else:
        halo = 0.10 + 0.18 * armed + 0.05 * (0.5 + 0.5 * math.sin(f * 0.18))
        glow_rect(canvas, GATE_X - 66, top - 14, GATE_X + 66, bot + 14, 20,
                  ACCENT, 0.22 * halo * ap, 0.30 * halo * ap, tight_r=12, wide_r=44)
        rim_rgb = mix(BORDER2, ACCENT, armed * 0.6)

    dx = shake
    shadow_ry = leaf_h / 2
    draw_shadow(canvas, GATE_X + dx, bot + 6, leaf_w + slot_half + 8,
                18, a=0.34 * ap)

    d = ImageDraw.Draw(canvas)
    for sgn in (-1, 1):
        inner = GATE_X + sgn * (slot_half + open_px) + dx
        outer = inner + sgn * leaf_w
        lx0, lx1 = min(inner, outer), max(inner, outer)
        d.rounded_rectangle((P(lx0), P(top), P(lx1), P(bot)), radius=P(8),
                            fill=col(mix(PANEL, (30, 26, 22), 0.0), a),
                            outline=col(rim_rgb, a), width=P(3))
        # inner bevel line
        d.line((P(inner), P(top + 10), P(inner), P(bot - 10)),
               fill=col(mix(rim_rgb, TEXT, 0.15), int(a * 0.5)), width=P(1))

    # slot verdict glyph — the DENY ✗ is drawn later, ON TOP of the packet
    # (see draw_deny_x), so it reads bold and un-occluded.
    if verdict == "allow":
        _stroke_check(d, GATE_X + dx, WIRE_Y, 22, GREEN_FLASH, a, vprog, P(6))

    # gate tag: require: build -> require: deliver
    # Fix 4 — during the phase advance the merge verdict is still in the air, so
    # dim the stale `require: build` to ~40%, then crossfade to `require: deliver`
    # as the rail marker seats on deliver (~F248–252).
    if 198 <= f < 252:
        dip = 1.0 - 0.60 * seg(f, 198, 210)     # full -> ~40% over F198–210
        rise = seg(f, 248, 252)                 # crossfade build -> deliver
        build_a = int(a * dip * (1 - rise))
        deliver_a = int(a * rise)
        if build_a > 2:
            draw_runs(d, GATE_X + dx, bot + 30,
                      [("require: ", col(MUTED, build_a)),
                       ("build", col(ACCENT, build_a))], mono(18, bold=True))
        if deliver_a > 2:
            draw_runs(d, GATE_X + dx, bot + 30,
                      [("require: ", col(MUTED, deliver_a)),
                       ("deliver", col(GREEN, deliver_a))], mono(18, bold=True))
    else:
        word = "deliver" if armed > 0.5 else "build"
        fade = abs(armed - 0.5) * 2  # 1 at either stable end, dips mid-crossfade
        tag_a = int(a * (0.55 + 0.45 * fade))
        draw_runs(d, GATE_X + dx, bot + 30,
                  [("require: ", col(MUTED, tag_a)),
                   (word, col(ACCENT if word == "build" else GREEN, tag_a))],
                  mono(18, bold=True))


def draw_rail(canvas, f, ap):
    a = int(255 * ap)
    if a <= 2:
        return
    d = ImageDraw.Draw(canvas)
    ph = phase_value(f)
    # base track
    d.line((P(RAIL_X0), P(RAIL_Y), P(RAIL_X1), P(RAIL_Y)),
           fill=col(BORDER1, a), width=P(3))
    # fill up to current phase (revealed by establish)
    fill_to = pip_x(min(ph, len(PHASES) - 1))
    reveal_x = RAIL_X0 + (RAIL_X1 - RAIL_X0) * clamp01(ap * 1.0)
    fx = min(fill_to, reveal_x)
    if fx > RAIL_X0 + 1:
        d.line((P(RAIL_X0), P(RAIL_Y), P(fx), P(RAIL_Y)),
               fill=col(ACCENT, a), width=P(4))
    # rail light sweep during transition
    if 200 <= f < 216:
        sw = seg(f, 200, 216)
        sx = RAIL_X0 + (RAIL_X1 - RAIL_X0) * sw
        glow_circle(canvas, sx, RAIL_Y, 5, ACCENT_LT, 0.6 * ap, 0.4 * ap,
                    tight_r=8, wide_r=26)
    # pips + labels
    for i, name in enumerate(PHASES):
        x = pip_x(i)
        reached = ph >= i - 0.05
        pr = col(ACCENT if reached else BORDER2, a)
        d.ellipse((P(x - 5), P(RAIL_Y - 5), P(x + 5), P(RAIL_Y + 5)),
                  fill=col(PANEL, a), outline=pr, width=P(2))
        if reached:
            d.ellipse((P(x - 2.5), P(RAIL_Y - 2.5), P(x + 2.5), P(RAIL_Y + 2.5)),
                      fill=pr)
        cur = abs(ph - i) < 0.5
        lab_rgb = TEXT if cur else MUTED
        draw_center(d, x, RAIL_Y + 26, name, mono(16, bold=cur),
                    col(lab_rgb, int(a * (1.0 if cur else 0.85))))
    # glowing current-phase marker with pulse
    mx = pip_x(min(ph, len(PHASES) - 1))
    pulse = 0.5 + 0.5 * math.sin(f * 0.20)
    intens = 0.55 + 0.35 * pulse
    seated = 1 - abs((ph % 1.0) - 0.0)  # brighter when seated on a pip
    glow_circle(canvas, mx, RAIL_Y, 9, ACCENT_LT,
                intens * ap, (0.35 + 0.25 * pulse) * ap, tight_r=9, wide_r=30)
    d.ellipse((P(mx - 7), P(RAIL_Y - 7), P(mx + 7), P(RAIL_Y + 7)),
              fill=col(mix(ACCENT, ACCENT_LT, 0.4 + 0.4 * pulse), a),
              outline=col(TEXT, int(a * 0.6)), width=P(1))
    # require tag (right-aligned above the rail)
    armed = ease_in_out(clamp01((ph - 4.3) / 0.7))
    word = "deliver" if armed > 0.5 else "build"
    runs = [("require: ", col(MUTED, a)),
            (word, col(ACCENT if word == "build" else GREEN, a))]
    total = sum(text_width(d, t, mono(18, bold=True)) for t, _ in runs)
    draw_runs(d, RAIL_X1 - total / (2 * SS) / 1, RAIL_Y - 34, runs,
              mono(18, bold=True))


def draw_wire(canvas, ap):
    a = int(255 * ap)
    if a <= 2:
        return
    d = ImageDraw.Draw(canvas)
    reveal = clamp01(ap * 1.15)
    xr = WIRE_L + (WIRE_R - WIRE_L) * reveal
    d.line((P(WIRE_L), P(WIRE_Y), P(xr), P(WIRE_Y)),
           fill=col(BORDER2, int(a * 0.9)), width=P(2))


def draw_packet(canvas, ps, ap):
    if ps is None:
        return
    a_master = ap * ps["alpha"]
    if a_master <= 0.02:
        return
    label = ps["label"]
    d = ImageDraw.Draw(canvas)
    fnt = mono(21, bold=True)
    tw = d.textlength(label, font=fnt) / SS
    pw = tw + 44
    ph = 44
    sc = ps["scale"]
    stretch = ps.get("stretch", 1.0)
    lead = ps["lead"]

    def chip(cx, alpha, scl, stretchx, lead_alpha=1.0, text=True):
        hw = pw / 2 * scl * stretchx
        hh = ph / 2 * scl
        x0, y0, x1, y1 = cx - hw, WIRE_Y - hh, cx + hw, WIRE_Y + hh
        aa = int(255 * alpha)
        if aa <= 2:
            return
        dd = ImageDraw.Draw(canvas)
        dd.rounded_rectangle((P(x0), P(y0), P(x1), P(y1)), radius=P(12),
                             fill=col(mix(PANEL, (40, 33, 28), 0.6), aa),
                             outline=col(BORDER2, aa), width=P(2))
        # leading edge accent bar
        dd.rounded_rectangle((P(x1 - 6), P(y0 + 4), P(x1 - 2), P(y1 - 4)),
                             radius=P(2), fill=col(lead, int(aa * lead_alpha)))
        if text:
            dd.text((P(cx), P(WIRE_Y)), label, font=fnt,
                    fill=col(TEXT, aa), anchor="mm")

    # comet tail: sample past sub-frames of the SAME packet
    fnow = ps["_f"]
    base_gs = stretch * (1.15 if stretch > 1.01 else 1.0)
    # Fix 1 — fastest deny-return segment gets a longer, stretched comet trail
    fast_repel = ps["mode"] == "repel" and 182 <= fnow <= 186
    ghosts = []
    for k in range(1, 8):
        pg = packet_state(fnow - k)
        if pg is None or pg["label"] != label or pg["mode"] in ("spawn",):
            break
        gs = 1.15 if (fast_repel and k == 1) else base_gs   # leading ghost stretched
        ghosts.append((pg["x"], 0.45 * (1 - (k - 1) / 7.0), gs))
    if fast_repel:
        for j, k in enumerate((8, 9)):                       # 2 extra ghosts
            pg = packet_state(fnow - k)
            if pg is not None and pg["label"] == label:
                ghosts.append((pg["x"], 0.30 * (1 - j * 0.4), base_gs))
    for gx, ga, gs in reversed(ghosts):
        chip(gx, ga * a_master, sc * 0.98, gs, lead_alpha=0.0, text=False)
    # crisp chip
    chip(ps["x"], a_master, sc, stretch, lead_alpha=1.0, text=True)


def draw_toast(canvas, f, ap):
    if not (B2["x0"] <= f < B2["x1"] + 6):
        return
    prog = seg(f, B2["x0"], B2["x1"] + 6)
    rise = 24 * ease_out_cubic(prog)
    alpha = math.sin(math.pi * clamp01(prog)) * ap
    y = WIRE_Y - 70 - rise
    d = ImageDraw.Draw(canvas)
    draw_center(d, GATE_X - 40, y, "denied — needs deliver",
                mono(20, bold=True), col(RED_FLASH, int(255 * alpha)))


def draw_deny_x(canvas, f, ap):
    """Fix 2 — bold deny ✗ composited ON TOP of the packet.

    Spans ~70% of the gate-slot height, 7px stroke, flash-red, with a 1-frame
    white-hot core at F171 that decays to red by ~F175.
    """
    if not (B2["v0"] <= f < B2["x0"] + 4):          # F168–184
        return
    fade = 1.0 - seg(f, B2["x0"], B2["x0"] + 4)     # hold to F180, gone by F184
    a = int(255 * ap * fade)
    if a <= 2:
        return
    leaf_h = 230 * 1.04
    s = leaf_h * 0.35                                # 2s ≈ 70% of the slot height
    vprog = seg(f, B2["v0"], B2["v0"] + 9)          # ✗ strokes in over F168–177
    core = 1.0 - seg(f, B2["v0"] + 3, B2["v0"] + 7)  # white-hot F171 -> red F175
    rgb = mix(RED_FLASH, (255, 255, 255), core)
    dx = gate_shake(f)
    d = ImageDraw.Draw(canvas)
    _stroke_x(d, GATE_X + dx, WIRE_Y, s, rgb, a, vprog, P(7))


def draw_ring(canvas, f, ap):
    """Expanding green ring when beat-3 packet lands on destination."""
    if not (B3["r1"] - 2 <= f < B3["r1"] + 12):
        return
    prog = seg(f, B3["r1"] - 2, B3["r1"] + 12)
    r = 20 + 70 * ease_out_cubic(prog)
    alpha = (1 - prog) * ap
    d = ImageDraw.Draw(canvas)
    d.ellipse((P(DEST_X - r), P(WIRE_Y - r), P(DEST_X + r), P(WIRE_Y + r)),
              outline=col(GREEN, int(230 * alpha)), width=P(3))


# ---------------------------------------------------------------------------
# Text blocks
# ---------------------------------------------------------------------------
def draw_texts(canvas, f, ap):
    a = int(255 * ap)
    if a <= 2:
        return
    d = ImageDraw.Draw(canvas)
    # eyebrow
    draw_tracked(d, W / 2, 118, "SHIPMATES  /  STATE GATE", mono(19),
                 col(MUTED, int(a * 0.95)), tracking=4)
    # title (2 lines)
    draw_center(d, W / 2, 178, "Every tool call clears", sans(58, bold=True),
                col(TEXT, a))
    draw_center(d, W / 2, 240, "the gate first.", sans(58, bold=True),
                col(TEXT, a))
    # punchline
    pl_boost, lift = punch_emphasis(f)
    pa = int(a * pl_boost)
    draw_runs(d, W / 2, 1108 + lift,
              [("It enforces ", col(TEXT, pa)),
               ("order", col(ACCENT, pa)),
               (" —", col(TEXT, pa))],
              sans(48, bold=True))
    draw_center(d, W / 2, 1164 + lift, "not a blanket block.",
                sans(48, bold=True), col(TEXT, pa))
    # credit (two lines to fit within margins)
    draw_center(d, W / 2, 1244,
                "Same gate on every hook-supporting harness —",
                mono(19), col(MUTED, int(a * 0.9)))
    draw_center(d, W / 2, 1272,
                "Claude Code · Cursor · Windsurf · Antigravity "
                "· GitHub Copilot · opencode · Codex",
                mono(17), col(MUTED, int(a * 0.9)))


# ---------------------------------------------------------------------------
# Frame renderer
# ---------------------------------------------------------------------------
_BG = None
_VIGNETTE = None


def set_scale(ss):
    """Switch supersample factor and invalidate the SS-dependent bg cache."""
    global SS, _BG
    SS = ss
    _BG = None


def render_frame(f, grain=True):
    global _BG, _VIGNETTE
    if _BG is None:
        _BG = build_background()
    canvas = _BG.copy()
    ap = appear(f)

    # phase rail
    draw_rail(canvas, f, ap)
    # wire under nodes
    draw_wire(canvas, ap)

    # hook blip driven by packet proximity
    ps = packet_state(f)
    blip = 0.0
    if ps is not None and ps["mode"] in ("travel_in",):
        blip = max(0.0, 1 - abs(ps["x"] - HOOK_X) / 55.0)
    draw_hook(canvas, ap, blip)

    # gate (hero) sits behind packet
    draw_gate(canvas, f, ap)

    # nodes
    sc = 0.94 + 0.06 * ap
    draw_node(canvas, AGENT_X, "codex", "agent", ap, sc)
    # destination state
    armed = ease_in_out(clamp01((phase_value(f) - 4.3) / 0.7))
    dest_label = "PR #" if armed > 0.5 else "remote"
    status = None
    srgb = GREEN
    if 116 <= f < 206:
        status = "✓ pushed"
        st_a = ap * (seg(f, 116, 124) if f < 124 else (1 - seg(f, 200, 206)))
        status_alpha = st_a
    elif 332 <= f < 358:
        status = "✓ merged"
        status_alpha = ap * (seg(f, 332, 340) - seg(f, 352, 358))
    else:
        status_alpha = ap
    draw_node(canvas, DEST_X, dest_label, "destination", ap, sc,
              accent_edge=(armed > 0.5))
    if status:
        d = ImageDraw.Draw(canvas)
        pop = 1.0
        if 108 <= f < 122:
            pop = 1.0 + 0.12 * math.sin(math.pi * seg(f, 108, 122))
        if 326 <= f < 342:
            pop = 1.0 + 0.12 * math.sin(math.pi * seg(f, 326, 342))
        draw_center(d, DEST_X, WIRE_Y + NODE_H / 2 + 24, status,
                    mono(19, bold=True), col(srgb, int(255 * status_alpha)))

    # packet + trail
    if ps is not None:
        ps["_f"] = f
        draw_packet(canvas, ps, ap)

    # deny ✗ on top of the packet (Fix 2)
    draw_deny_x(canvas, f, ap)

    draw_toast(canvas, f, ap)
    draw_ring(canvas, f, ap)
    draw_texts(canvas, f, ap)

    # --- downscale to final, then grain + vignette ---
    final = canvas.convert("RGB")
    if SS != 1:
        final = final.resize((W, H), Image.LANCZOS)
    arr = np.asarray(final, dtype=np.float32)

    if _VIGNETTE is None:
        _VIGNETTE = build_vignette_mask()
    arr *= _VIGNETTE

    if grain:
        rng = np.random.default_rng(f)        # deterministic per-frame grain
        noise = rng.normal(0.0, 3.6, (H, W, 1)).astype(np.float32)
        arr += noise

    np.clip(arr, 0, 255, out=arr)
    return Image.fromarray(arr.astype(np.uint8), "RGB")


# ---------------------------------------------------------------------------
# Encoding
# ---------------------------------------------------------------------------
def encode_mp4(frames_dir, out_path, fps):
    cmd = [FFMPEG, "-y", "-framerate", str(fps), "-i",
           str(frames_dir / "f%04d.png"),
           "-c:v", "libx264", "-crf", "18", "-pix_fmt", "yuv420p",
           "-movflags", "+faststart", str(out_path)]
    subprocess.run(cmd, check=True, stdout=subprocess.DEVNULL,
                   stderr=subprocess.DEVNULL)
    return " ".join(cmd)


# GIF fallback: rendered from its OWN grain-free frame pass. Animated grain is
# kept for the MP4 (it dithers the 8-bit gradients) but is dropped for the GIF —
# per-frame noise makes every pixel change and explodes GIF size to ~300MB, while
# paletteuse's sierra2_4a dithering already suppresses banding in the palette.
GIF_W = 720
GIF_FPS = 30          # divides 30fps evenly -> 360 frames, exact 12.0s loop


def encode_gif(frames_dir, out_path, fps):
    palette = out_path.parent / "_palette.png"
    src = str(frames_dir / "f%04d.png")
    vf = (f"scale={GIF_W}:-1:flags=lanczos,palettegen=stats_mode=diff")
    c1 = [FFMPEG, "-y", "-framerate", str(fps), "-i", src, "-vf", vf,
          str(palette)]
    subprocess.run(c1, check=True, stdout=subprocess.DEVNULL,
                   stderr=subprocess.DEVNULL)
    lavfi = (f"scale={GIF_W}:-1:flags=lanczos [x];"
             f"[x][1:v]paletteuse=dither=sierra2_4a")
    c2 = [FFMPEG, "-y", "-framerate", str(fps), "-i", src, "-i", str(palette),
          "-lavfi", lavfi, str(out_path)]
    subprocess.run(c2, check=True, stdout=subprocess.DEVNULL,
                   stderr=subprocess.DEVNULL)
    palette.unlink(missing_ok=True)
    return " ".join(c1) + "  &&  " + " ".join(c2)


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------
def main():
    global SS
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--fast", action="store_true",
                    help="1x supersample, every 3rd frame, mp4 only")
    ap.add_argument("--frame", type=int, default=None,
                    help="dump a single frame PNG and exit")
    ap.add_argument("--frames-only", action="store_true",
                    help="render PNG frames, skip ffmpeg")
    ap.add_argument("--out-dir", default=str(HERE))
    args = ap.parse_args()

    out_dir = Path(args.out_dir)
    out_dir.mkdir(parents=True, exist_ok=True)

    if args.fast:
        SS = 1

    if args.frame is not None:
        img = render_frame(args.frame)
        dest = Path("/tmp/anim_frames")
        dest.mkdir(parents=True, exist_ok=True)
        p = dest / f"f{args.frame:04d}.png"
        img.save(p)
        print(f"wrote {p}")
        return

    step = 3 if args.fast else 1
    frame_idxs = list(range(0, N_FRAMES, step))

    t0 = time.time()
    tmp = Path(tempfile.mkdtemp(prefix="hooks_frames_"))
    try:
        for out_i, f in enumerate(frame_idxs):
            img = render_frame(f)
            img.save(tmp / f"f{out_i:04d}.png")
            if f % 30 == 0:
                print(f"  frame {f}/{N_FRAMES}", flush=True)
        render_dt = time.time() - t0
        print(f"rendered {len(frame_idxs)} frames in {render_dt:.1f}s")

        if args.frames_only:
            print(f"frames in {tmp}")
            return

        eff_fps = FPS // step if args.fast else FPS
        mp4 = out_dir / "hooks-gate.mp4"
        cmd_mp4 = encode_mp4(tmp, mp4, eff_fps)
        print(f"mp4 -> {mp4}")
        print(f"  ffmpeg: {cmd_mp4}")

        if not args.fast:
            # dedicated grain-free GIF pass at 1x (downscaled to GIF_W), GIF_FPS
            gif_tmp = Path(tempfile.mkdtemp(prefix="hooks_gif_"))
            try:
                set_scale(1)
                step_gif = max(1, FPS // GIF_FPS)
                gif_idxs = list(range(0, N_FRAMES, step_gif))
                tg = time.time()
                # Fix 3 — bake ONE fixed static dither texture (~6 luma amplitude)
                # into every GIF frame BEFORE quantize. Identical across frames, so
                # it breaks the bloom/vignette contour rings the palette exposes
                # without exploding the GIF (per-frame noise made it ~300MB).
                dither_rng = np.random.default_rng(20260806)
                static_dither = dither_rng.uniform(-6.0, 6.0,
                                                   (H, W, 1)).astype(np.float32)
                for out_i, f in enumerate(gif_idxs):
                    arr = np.asarray(render_frame(f, grain=False),
                                     dtype=np.float32) + static_dither
                    np.clip(arr, 0, 255, out=arr)
                    Image.fromarray(arr.astype(np.uint8), "RGB").save(
                        gif_tmp / f"f{out_i:04d}.png")
                print(f"rendered {len(gif_idxs)} grain-free GIF frames "
                      f"in {time.time() - tg:.1f}s")
                gif = out_dir / "hooks-gate.gif"
                cmd_gif = encode_gif(gif_tmp, gif, GIF_FPS)
                print(f"gif -> {gif}")
                print(f"  ffmpeg: {cmd_gif}")
            finally:
                shutil.rmtree(gif_tmp, ignore_errors=True)
        print(f"total time {time.time() - t0:.1f}s")
    finally:
        shutil.rmtree(tmp, ignore_errors=True)


if __name__ == "__main__":
    main()
