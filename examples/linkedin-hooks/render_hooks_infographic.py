#!/usr/bin/env python3
"""Render the premium 9-scene Shipmates "tool-boundary gate" ANIMATED INFOGRAPHIC.

A bespoke marketing-asset generator (NOT the `diagram` tool). It composes 600
frames in Pillow at 2x (2160x2700), LANCZOS-downscales to 1080x1350 (4:5
portrait), layers a parallax background (radial vignette + drifting particles +
faint dot-grid), a MID content layer (cards/terminal/pipeline), and an FG layer
(verdict badges / blooms / bursts / traveling pill), then applies deterministic
per-frame film grain and encodes an MP4 (primary) and a condensed GIF fallback.

Story (accurate to /ship-issue), the 6 phases `plan -> isolate -> build ->
verify -> review -> deliver`, and a per-harness PreToolUse hook that calls
`shipmates state gate` before EVERY tool call. Scene beats:
  1  hero title            0- 66
  2  setup / cast         66-150
  3  live terminal       150-216
  4  BEAT 1 allow push   216-288  -> exit 0
  5  BEAT 2 deny merge   288-372  -> exit 1
  6  phase advance       372-426  build -> verify -> review -> deliver
                                  (log-style gate lines, no prompt/command)
  7  BEAT 3 allow merge  426-499  -> exit 0 (payoff; extended hold)
  8  credibility         499-559  6 phases / 7 harnesses / exit 0-1
  9  punchline / outro   559-600  + soft loop dissolve toward scene 1

Deterministic by construction: grain, particles and bursts are seeded by frame
index, so a re-render is byte-identical.

Fonts are VENDORED OFL variable TTFs in ./fonts (Space Grotesk / Inter /
JetBrains Mono); falls back to DejaVu (guaranteed on host) if absent.

Usage:
  python3 render_hooks_infographic.py                 # full mp4 + gif
  python3 render_hooks_infographic.py --fast          # 1x, every 3rd frame, mp4 only
  python3 render_hooks_infographic.py --frame 250     # dump one PNG to /tmp/info_frames
  python3 render_hooks_infographic.py --scene 5       # render only scene 5's range
"""
import argparse
import math
import os
import shutil
import subprocess
import tempfile
import time
from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw, ImageFilter, ImageFont

HERE = Path(__file__).resolve().parent
FONTS = HERE / "fonts"
FFMPEG = "/usr/bin/ffmpeg"

# ---------------------------------------------------------------------------
# Canvas
# ---------------------------------------------------------------------------
W, H = 1080, 1350
FPS = 30
N_FRAMES = 600
CX = W / 2
MARGIN = 72
SS = 2


def P(v):
    return int(round(v * SS))


def Pf(v):
    return v * SS


# ---------------------------------------------------------------------------
# Palette (exact tokens from the art-director spec)
# ---------------------------------------------------------------------------
def hx(s):
    s = s.lstrip("#")
    return (int(s[0:2], 16), int(s[2:4], 16), int(s[4:6], 16))


BG = hx("#14110F")
BG_CORNER = hx("#0C0A09")
PANEL = hx("#1D1916")
TERMBG = hx("#211C18")
BORDER1 = hx("#332C26")
BORDER2 = hx("#4A4139")
TEXT = hx("#F2EDE8")
MUTED = hx("#B3A99F")
FAINT = hx("#6E645B")
ACCENT = hx("#D97757")
ACCENT_LT = hx("#E8916F")
GREEN = hx("#4ADE94")
GREEN_FLASH = hx("#9BFFC8")
RED = hx("#FB4A54")
RED_FLASH = hx("#FF8A80")
AMBER = hx("#E8B84A")


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
    return t * t * (3 - 2 * t)


def ease_out_back(t, c1=1.9):
    t = clamp01(t)
    c3 = c1 + 1
    return 1 + c3 * (t - 1) ** 3 + c1 * (t - 1) ** 2


def cubic_bezier(x1, y1, x2, y2):
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
# Fonts (vendored OFL variable TTFs; DejaVu fallback)
# ---------------------------------------------------------------------------
SG = FONTS / "SpaceGrotesk-var.ttf"
INTER = FONTS / "Inter-var.ttf"
JB = FONTS / "JetBrainsMono-var.ttf"
DEJ = "/usr/share/fonts/truetype/dejavu"
FONTS_USED = {}

_font_cache = {}


def _load(path, size, variation, fallback):
    key = (str(path), size, variation)
    if key in _font_cache:
        return _font_cache[key]
    try:
        f = ImageFont.truetype(str(path), size)
        if variation:
            f.set_variation_by_name(variation)
        used = Path(path).name
    except Exception:
        f = ImageFont.truetype(fallback, size)
        used = Path(fallback).name
    _font_cache[key] = f
    return f


def disp(size, weight="Bold"):
    FONTS_USED["display"] = "Space Grotesk" if SG.exists() else "DejaVu Sans"
    fb = f"{DEJ}/DejaVuSans-Bold.ttf"
    return _load(SG, P(size), weight, fb)


def label(size, weight="Medium"):
    FONTS_USED["label"] = "Inter" if INTER.exists() else "DejaVu Sans"
    fb = f"{DEJ}/DejaVuSans-Bold.ttf" if weight in ("SemiBold", "Bold") \
        else f"{DEJ}/DejaVuSans.ttf"
    return _load(INTER, P(size), weight, fb)


def mono(size, weight="Regular"):
    FONTS_USED["mono"] = "JetBrains Mono" if JB.exists() else "DejaVu Sans Mono"
    fb = f"{DEJ}/DejaVuSansMono-Bold.ttf" if weight in ("Bold", "Medium") \
        else f"{DEJ}/DejaVuSansMono.ttf"
    return _load(JB, P(size), weight, fb)


# ---------------------------------------------------------------------------
# Text helpers (FINAL coords; scale internally)
# ---------------------------------------------------------------------------
def text_w(d, text, fnt, tracking=0):
    w = d.textlength(text, font=fnt)
    if tracking and len(text) > 1:
        w += P(tracking) * (len(text) - 1)
    return w


def draw_center(d, cx, y, text, fnt, fill):
    d.text((P(cx), P(y)), text, font=fnt, fill=fill, anchor="mm")


def draw_left(d, x, y, text, fnt, fill):
    d.text((P(x), P(y)), text, font=fnt, fill=fill, anchor="lm")


def draw_tracked(d, cx, y, text, fnt, fill, tracking=0, anchor="mm"):
    total = text_w(d, text, fnt, tracking)
    x = P(cx) - (total / 2 if anchor == "mm" else 0)
    for ch in text:
        d.text((x, P(y)), ch, font=fnt, fill=fill, anchor="lm")
        x += d.textlength(ch, font=fnt) + P(tracking)


def draw_runs(d, cx, y, runs, fnt, tracking=0, anchor="mm"):
    total = sum(text_w(d, t, fnt, tracking) for t, _ in runs)
    x = P(cx) - (total / 2 if anchor == "mm" else 0)
    for t, fill in runs:
        for ch in t:
            d.text((x, P(y)), ch, font=fnt, fill=fill, anchor="lm")
            x += d.textlength(ch, font=fnt) + P(tracking)


# ---------------------------------------------------------------------------
# Glow / shadow (two-layer blur under crisp shape)
# ---------------------------------------------------------------------------
def glow_from_stamp(canvas, stamp, topleft, rgb, tight_a, wide_a,
                    tight_r=10, wide_r=32):
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
    ImageDraw.Draw(stamp).ellipse((pad - P(r), pad - P(r), pad + P(r),
                                   pad + P(r)), fill=255)
    glow_from_stamp(canvas, stamp, (P(cx) - pad, P(cy) - pad), rgb,
                    tight_a, wide_a, tight_r, wide_r)


def glow_rect(canvas, x0, y0, x1, y1, rad, rgb, tight_a, wide_a,
              tight_r=10, wide_r=32):
    pad = P(wide_r) * 2 + 4
    w = P(x1 - x0) + pad * 2
    h = P(y1 - y0) + pad * 2
    stamp = Image.new("L", (w, h), 0)
    ImageDraw.Draw(stamp).rounded_rectangle((pad, pad, w - pad, h - pad),
                                            radius=P(rad), fill=255)
    glow_from_stamp(canvas, stamp, (P(x0) - pad, P(y0) - pad), rgb,
                    tight_a, wide_a, tight_r, wide_r)


def drop_shadow(canvas, x0, y0, x1, y1, rad, a=0.55, blur=13, dy=8):
    pad = P(blur) * 2 + 6
    w = P(x1 - x0) + pad * 2
    h = P(y1 - y0) + pad * 2
    stamp = Image.new("L", (w, h), 0)
    ImageDraw.Draw(stamp).rounded_rectangle((pad, pad, w - pad, h - pad),
                                            radius=P(rad), fill=int(255 * a))
    stamp = stamp.filter(ImageFilter.GaussianBlur(Pf(blur)))
    layer = Image.new("RGBA", stamp.size, (0, 0, 0, 0))
    layer.putalpha(stamp)
    canvas.alpha_composite(layer, dest=(P(x0) - pad, P(y0) - pad + P(dy)))


def panel(d, canvas, x0, y0, x1, y1, rad=16, fill=PANEL, border=BORDER2,
          a=255, bw=2, shadow=True, hilite=True):
    if shadow:
        drop_shadow(canvas, x0, y0, x1, y1, rad, a=0.55 * a / 255)
    d.rounded_rectangle((P(x0), P(y0), P(x1), P(y1)), radius=P(rad),
                        fill=col(fill, a), outline=col(border, a), width=P(bw))
    if hilite:
        d.line((P(x0 + rad), P(y0 + 1), P(x1 - rad), P(y0 + 1)),
               fill=col(mix(fill, TEXT, 0.10), int(a * 0.7)), width=P(1))


# ---------------------------------------------------------------------------
# Background — layered parallax (radial vignette + dot-grid + particles)
# ---------------------------------------------------------------------------
_BASE_BG = None
_BASE_RGBA = None
_GRID = None


def build_base_bg():
    """Radial gradient BG center -> BG_CORNER at corners, FINAL res."""
    yy, xx = np.mgrid[0:H, 0:W].astype(np.float32)
    d = np.sqrt(((xx - CX) / CX) ** 2 + ((yy - H / 2) / (H / 2)) ** 2)
    d = np.clip(d / math.sqrt(2), 0, 1) ** 1.5
    d = d[:, :, None]
    c0 = np.array(BG, np.float32)
    c1 = np.array(BG_CORNER, np.float32)
    arr = c0 + (c1 - c0) * d
    return arr.astype(np.uint8)


def build_grid():
    """Faint dot-grid layer (RGBA), FINAL res, tiled to allow drift."""
    img = Image.new("RGBA", (W + 80, H + 80), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    step = 54
    for gy in range(0, H + 80, step):
        for gx in range(0, W + 80, step):
            d.ellipse((gx - 1, gy - 1, gx + 1, gy + 1), fill=col(FAINT, 42))
    return img


_PARTICLES = None


def _particles():
    global _PARTICLES
    if _PARTICLES is None:
        rng = np.random.default_rng(9182)
        n = 44
        _PARTICLES = [
            dict(x=rng.uniform(0, W), y=rng.uniform(0, H),
                 vx=rng.uniform(-0.10, 0.10), vy=rng.uniform(-0.16, -0.03),
                 r=rng.uniform(0.8, 2.2), ph=rng.uniform(0, 6.28),
                 sp=rng.uniform(0.02, 0.05))
            for _ in range(n)]
    return _PARTICLES


def build_bg(f, zoom, dark=0.0):
    global _BASE_BG, _GRID, _BASE_RGBA
    if _BASE_BG is None:
        _BASE_BG = build_base_bg()
        _BASE_RGBA = Image.fromarray(_BASE_BG, "RGB").convert("RGBA")
        _GRID = build_grid()
    img = _BASE_RGBA.copy()
    # dot-grid parallax drift (~8px)
    dx = int(8 * math.sin(f * 0.010)) - 40
    dy = int(-((f * 0.18) % 54)) - 40 + int(6 * math.cos(f * 0.008))
    img.alpha_composite(_GRID, dest=(dx, dy))
    # particle field
    d = ImageDraw.Draw(img)
    for p in _particles():
        px = (p["x"] + p["vx"] * f) % W
        py = (p["y"] + p["vy"] * f) % H
        tw = 0.35 + 0.65 * (0.5 + 0.5 * math.sin(p["ph"] + p["sp"] * f))
        r = p["r"]
        d.ellipse((px - r, py - r, px + r, py + r),
                  fill=col(mix(FAINT, ACCENT, 0.25), int(70 * tw)))
    img = img.convert("RGB")
    if dark > 0.001:
        arr = np.asarray(img, np.float32) * (1 - dark)
        img = Image.fromarray(arr.astype(np.uint8), "RGB")
    if zoom > 1.0001:
        img = crop_zoom(img, zoom)
    return img


def crop_zoom(img, z):
    w, h = img.size
    cw, ch = w / z, h / z
    box = ((w - cw) / 2, (h - ch) / 2, (w + cw) / 2, (h + ch) / 2)
    return img.resize((w, h), Image.LANCZOS, box=box)


def crop_zoom_rgba(img, z):
    if z <= 1.0001:
        return img
    return crop_zoom(img, z)


# ---------------------------------------------------------------------------
# Cast primitives (drawn as PATHS)
# ---------------------------------------------------------------------------
def draw_agent(canvas, cx, cy, s, a, rgb=TEXT):
    """Agent chip: rounded square + two dots + baseline."""
    d = ImageDraw.Draw(canvas)
    x0, y0, x1, y1 = cx - s, cy - s, cx + s, cy + s
    d.rounded_rectangle((P(x0), P(y0), P(x1), P(y1)), radius=P(s * 0.32),
                        outline=col(rgb, a), width=P(6))
    r = s * 0.16
    for sx in (-0.36, 0.36):
        ex = cx + s * sx
        d.ellipse((P(ex - r), P(cy - s * 0.18 - r), P(ex + r),
                   P(cy - s * 0.18 + r)), fill=col(rgb, a))
    d.line((P(cx - s * 0.42), P(cy + s * 0.36), P(cx + s * 0.42),
            P(cy + s * 0.36)), fill=col(rgb, a), width=P(6))


def pill_size(d, textstr, size=22):
    fnt = mono(size, "Medium")
    tw = d.textlength(textstr, font=fnt) / SS
    return tw + 46, 48, fnt


def draw_pill(canvas, cx, cy, textstr, a, scale=1.0, lead=ACCENT,
              glow=0.0, size=22, text_rgb=TEXT):
    """Tool-pill: mono pill w/ literal command text; leading terracotta edge."""
    d = ImageDraw.Draw(canvas)
    pw, ph, fnt = pill_size(d, textstr, size)
    hw, hh = pw / 2 * scale, ph / 2 * scale
    x0, y0, x1, y1 = cx - hw, cy - hh, cx + hw, cy + hh
    if glow > 0.01:
        glow_rect(canvas, x0, y0, x1, y1, 12, ACCENT, 0.4 * glow, 0.3 * glow,
                  tight_r=8, wide_r=26)
    aa = int(a)
    d.rounded_rectangle((P(x0), P(y0), P(x1), P(y1)), radius=P(ph / 2 * scale),
                        fill=col(mix(PANEL, (44, 36, 30), 0.7), aa),
                        outline=col(BORDER2, aa), width=P(2))
    d.rounded_rectangle((P(x1 - 8), P(y0 + 6), P(x1 - 3), P(y1 - 6)),
                        radius=P(2), fill=col(lead, aa))
    d.text((P(cx - 3), P(cy)), textstr, font=fnt, fill=col(text_rgb, aa),
           anchor="mm")


def draw_hook(canvas, cx, cy, s, a, glow=0.4, verdict=None, vprog=0.0):
    """Hook/interceptor diamond straddling the wire, inward chevrons, glow.

    verdict: None / 'allow' / 'deny' draws a stroke-revealed check / X inside.
    """
    if glow > 0.01:
        gr = GREEN if verdict == "allow" else (RED if verdict == "deny" else ACCENT)
        glow_circle(canvas, cx, cy, s * 1.15, gr, 0.5 * glow, 0.4 * glow,
                    tight_r=12, wide_r=40)
    d = ImageDraw.Draw(canvas)
    pts = [(cx, cy - s), (cx + s, cy), (cx, cy + s), (cx - s, cy)]
    edge = GREEN if verdict == "allow" else (RED if verdict == "deny" else
                                             mix(BORDER2, ACCENT, 0.55))
    d.polygon([(P(x), P(y)) for x, y in pts], fill=col(TERMBG, a),
              outline=col(edge, a))
    d.line([(P(x), P(y)) for x, y in pts] + [(P(pts[0][0]), P(pts[0][1]))],
           fill=col(edge, a), width=P(6), joint="curve")
    # inward chevrons (left + right)
    cv = s * 0.34
    for sgn in (-1, 1):
        bx = cx + sgn * s * 0.62
        d.line((P(bx), P(cy - cv), P(bx - sgn * cv * 0.7), P(cy),
                P(bx), P(cy + cv)), fill=col(mix(edge, TEXT, 0.2), a),
               width=P(5), joint="curve")
    if verdict == "allow":
        stroke_check(d, cx, cy, s * 0.42, GREEN_FLASH, a, vprog, P(7))
    elif verdict == "deny":
        stroke_x(d, cx, cy, s * 0.40, RED_FLASH, a, vprog, P(7))


def stroke_check(d, cx, cy, s, rgb, a, prog, width):
    p1 = (cx - s, cy + s * 0.1)
    p2 = (cx - s * 0.22, cy + s * 0.72)
    p3 = (cx + s, cy - s * 0.72)
    seg1 = 0.4
    if prog <= seg1:
        t = prog / seg1
        pts = [p1, (p1[0] + (p2[0] - p1[0]) * t, p1[1] + (p2[1] - p1[1]) * t)]
    else:
        t = (prog - seg1) / (1 - seg1)
        pts = [p1, p2, (p2[0] + (p3[0] - p2[0]) * t, p2[1] + (p3[1] - p2[1]) * t)]
    if len(pts) >= 2:
        d.line([(P(x), P(y)) for x, y in pts], fill=col(rgb, a), width=width,
               joint="curve")


def stroke_x(d, cx, cy, s, rgb, a, prog, width):
    a1 = clamp01(prog / 0.5)
    a2 = clamp01((prog - 0.5) / 0.5)
    if a1 > 0:
        d.line((P(cx - s), P(cy - s), P(cx - s + 2 * s * a1),
                P(cy - s + 2 * s * a1)), fill=col(rgb, a), width=width)
    if a2 > 0:
        d.line((P(cx + s), P(cy - s), P(cx + s - 2 * s * a2),
                P(cy - s + 2 * s * a2)), fill=col(rgb, a), width=width)


# ---------------------------------------------------------------------------
# Phase icons (monoline, same weight/box)
# ---------------------------------------------------------------------------
PHASES = ["plan", "isolate", "build", "verify", "review", "deliver"]


def phase_icon(d, name, cx, cy, s, rgb, a, w=None):
    w = w or P(5)
    R = col(rgb, a)
    if name == "plan":                       # map pin + dotted path
        d.ellipse((P(cx - s * 0.5), P(cy - s * 0.75), P(cx + s * 0.5),
                   P(cy + s * 0.05)), outline=R, width=w)
        d.line((P(cx), P(cy + 0.02 * s), P(cx), P(cy + s * 0.6)), fill=R, width=w)
        d.ellipse((P(cx - s * 0.14), P(cy - s * 0.45), P(cx + s * 0.14),
                   P(cy - s * 0.17)), fill=R)
        for i in range(3):
            dx = cx + s * (0.28 + i * 0.26)
            d.ellipse((P(dx - 2.2), P(cy + s * 0.55), P(dx + 2.2),
                       P(cy + s * 0.55 + 4.4)), fill=R)
    elif name == "isolate":                  # branch fork
        d.line((P(cx - s * 0.5), P(cy - s * 0.6), P(cx - s * 0.5),
                P(cy + s * 0.6)), fill=R, width=w)
        d.ellipse((P(cx - s * 0.5 - 7), P(cy - s * 0.6 - 7), P(cx - s * 0.5 + 7),
                   P(cy - s * 0.6 + 7)), outline=R, width=w)
        d.line((P(cx - s * 0.5), P(cy), P(cx + s * 0.45), P(cy - s * 0.5)),
               fill=R, width=w)
        d.ellipse((P(cx + s * 0.45 - 7), P(cy - s * 0.5 - 7), P(cx + s * 0.45 + 7),
                   P(cy - s * 0.5 + 7)), outline=R, width=w)
        d.ellipse((P(cx - s * 0.5 - 7), P(cy + s * 0.6 - 7), P(cx - s * 0.5 + 7),
                   P(cy + s * 0.6 + 7)), outline=R, width=w)
    elif name == "build":                    # wrench cross
        for ang in (45, -45):
            a2 = math.radians(ang)
            dx, dy = math.cos(a2) * s * 0.62, math.sin(a2) * s * 0.62
            d.line((P(cx - dx), P(cy - dy), P(cx + dx), P(cy + dy)),
                   fill=R, width=w)
            hx_ = cx + dx
            hy_ = cy + dy
            d.ellipse((P(hx_ - s * 0.2), P(hy_ - s * 0.2), P(hx_ + s * 0.2),
                       P(hy_ + s * 0.2)), outline=R, width=w)
    elif name == "verify":                   # checklist + tick
        d.rounded_rectangle((P(cx - s * 0.5), P(cy - s * 0.6), P(cx + s * 0.5),
                             P(cy + s * 0.6)), radius=P(6), outline=R, width=w)
        for i in range(3):
            ly = cy - s * 0.3 + i * s * 0.3
            d.line((P(cx - s * 0.28), P(ly), P(cx + s * 0.28), P(ly)),
                   fill=R, width=max(2, w - P(1)))
        stroke_check(d, cx + s * 0.1, cy + s * 0.0, s * 0.5,
                     ACCENT_LT if rgb != ACCENT_LT else TEXT, a, 1.0, w)
    elif name == "review":                   # scales
        d.line((P(cx), P(cy - s * 0.6), P(cx), P(cy + s * 0.55)), fill=R, width=w)
        d.line((P(cx - s * 0.55), P(cy - s * 0.45), P(cx + s * 0.55),
                P(cy - s * 0.45)), fill=R, width=w)
        for sx in (-0.55, 0.55):
            bx = cx + s * sx
            d.line((P(bx), P(cy - s * 0.45), P(bx - s * 0.18), P(cy + s * 0.05)),
                   fill=R, width=max(2, w - P(2)))
            d.line((P(bx), P(cy - s * 0.45), P(bx + s * 0.18), P(cy + s * 0.05)),
                   fill=R, width=max(2, w - P(2)))
            d.arc((P(bx - s * 0.2), P(cy - s * 0.1), P(bx + s * 0.2),
                   P(cy + s * 0.2)), 0, 180, fill=R, width=w)
        d.line((P(cx - s * 0.2), P(cy + s * 0.55), P(cx + s * 0.2),
                P(cy + s * 0.55)), fill=R, width=w)
    elif name == "deliver":                  # checkered flag
        d.line((P(cx - s * 0.5), P(cy - s * 0.65), P(cx - s * 0.5),
                P(cy + s * 0.65)), fill=R, width=w)
        fx0, fy0 = cx - s * 0.5, cy - s * 0.6
        cell = s * 0.28
        for r in range(3):
            for c in range(3):
                if (r + c) % 2 == 0:
                    d.rectangle((P(fx0 + c * cell), P(fy0 + r * cell),
                                 P(fx0 + (c + 1) * cell), P(fy0 + (r + 1) * cell)),
                                fill=R)
        d.rectangle((P(fx0), P(fy0), P(fx0 + 3 * cell), P(fy0 + 3 * cell)),
                    outline=R, width=max(2, w - P(2)))


def draw_phase_node(canvas, cx, cy, name, s, filled, a, ring=0.0,
                    icon_reveal=1.0, label_on=True):
    d = ImageDraw.Draw(canvas)
    box = s
    fill = mix(PANEL, ACCENT, 0.14) if filled else PANEL
    border = ACCENT if filled else BORDER2
    panel(d, canvas, cx - box, cy - box, cx + box, cy + box, rad=14,
          fill=fill, border=border, a=a, bw=2, shadow=True, hilite=True)
    if ring > 0.01:
        rr = box + 6 + 10 * ring
        d.ellipse((P(cx - rr), P(cy - rr), P(cx + rr), P(cy + rr)),
                  outline=col(ACCENT_LT, int(a * (1 - ring) * 0.9)), width=P(3))
    ir = int(a * icon_reveal)
    if ir > 4:
        phase_icon(d, name, cx, cy - box * 0.06, box * 0.62,
                   ACCENT_LT if filled else MUTED, ir)
    if label_on:
        draw_center(d, cx, cy + box + 20, name, mono(16, "Medium"),
                    col(TEXT if filled else MUTED, a))


# ---------------------------------------------------------------------------
# Terminal window
# ---------------------------------------------------------------------------
FACT_CMD = 'shipmates state gate --run 216 --tool "%s"'
J_ALLOW_PUSH = ('{"command":"ship-issue","issue":216,"phase":"build",'
                '"tool":"git push","allow":true,"reason":null}')
J_DENY = ('{"command":"ship-issue","issue":216,"phase":"build",'
          '"tool":"gh pr merge --squash","allow":false,"reason":"gate: '
          'gh pr merge requires phase>=deliver, run is at build"}')
J_ALLOW_MERGE = ('{"command":"ship-issue","issue":216,"phase":"deliver",'
                 '"tool":"gh pr merge --squash","allow":true,"reason":null}')


def draw_term_frame(canvas, x0, y0, x1, y1, title_runs, a):
    d = ImageDraw.Draw(canvas)
    panel(d, canvas, x0, y0, x1, y1, rad=16, fill=TERMBG, border=BORDER2, a=a)
    bar_h = 44
    d.rounded_rectangle((P(x0), P(y0), P(x1), P(y0 + bar_h)), radius=P(16),
                        fill=col(mix(TERMBG, BG, 0.5), a))
    d.rectangle((P(x0), P(y0 + bar_h - 16), P(x1), P(y0 + bar_h)),
                fill=col(mix(TERMBG, BG, 0.5), a))
    d.line((P(x0), P(y0 + bar_h), P(x1), P(y0 + bar_h)),
           fill=col(BORDER1, a), width=P(1))
    for i, c in enumerate((RED, AMBER, GREEN)):
        dx = x0 + 24 + i * 22
        d.ellipse((P(dx - 6), P(y0 + bar_h / 2 - 6), P(dx + 6),
                   P(y0 + bar_h / 2 + 6)), fill=col(c, a))
    if title_runs:
        draw_runs(d, (x0 + x1) / 2, y0 + bar_h / 2, title_runs,
                  mono(17, "Medium"))
    return y0 + bar_h


def wrap_code(d, text, fnt, maxw):
    """Word-aware wrap keeping exact order; returns list of (start,end).

    Breaks at spaces (each line is a contiguous slice, trailing space kept on
    the line it ends); only a single token longer than one line is hard-broken.
    """
    lines = []
    i = 0
    n = len(text)
    while i < n:
        j = i
        last_space_end = None       # index just past a space that still fits
        while j < n:
            if j > i and d.textlength(text[i:j + 1], font=fnt) / SS > maxw:
                break
            j += 1
            if j <= n and text[j - 1] == " ":
                last_space_end = j
        if j >= n:
            lines.append((i, n))
            break
        if last_space_end is not None and last_space_end > i:
            lines.append((i, last_space_end))
            i = last_space_end
        else:                        # single token longer than a line
            lines.append((i, j))
            i = j
    return lines


def draw_code_wrapped(d, x0, y0, maxw, text, fnt, line_h, base, a,
                      spans=(), cursor=False):
    """Char-accurate wrapped mono with per-substring color spans."""
    lines = wrap_code(d, text, fnt, maxw)
    y = y0
    end_x = x0
    for (s, e) in lines:
        x = x0
        for gi in range(s, e):
            ch = text[gi]
            c = base
            for (a0, a1, rgb) in spans:
                if a0 <= gi < a1:
                    c = rgb
                    break
            d.text((P(x), P(y)), ch, font=fnt, fill=col(c, a), anchor="lm")
            x += d.textlength(ch, font=fnt) / SS
        end_x = x
        y += line_h
    if cursor:
        cy = y - line_h
        d.rectangle((P(end_x + 3), P(cy - 12), P(end_x + 12), P(cy + 12)),
                    fill=col(TEXT, a))
    return y


def span(text, sub, rgb):
    i = text.find(sub)
    return (i, i + len(sub), rgb) if i >= 0 else (0, 0, rgb)


# ---------------------------------------------------------------------------
# Particle burst / shockwave (deterministic)
# ---------------------------------------------------------------------------
def draw_burst(canvas, cx, cy, prog, n, rgb, seed, spread=190, gravity=140):
    if prog <= 0 or prog >= 1:
        return
    rng = np.random.default_rng(seed)
    d = ImageDraw.Draw(canvas)
    e = ease_out_cubic(prog)
    for _ in range(n):
        ang = rng.uniform(0, 2 * math.pi)
        spd = rng.uniform(0.5, 1.0) * spread
        px = cx + math.cos(ang) * spd * e
        py = cy + math.sin(ang) * spd * e + gravity * prog * prog
        r = (2.4 + rng.uniform(0, 1.6)) * (1 - prog)
        aa = int(230 * (1 - prog))
        if r > 0.3 and aa > 4:
            d.ellipse((P(px - r), P(py - r), P(px + r), P(py + r)),
                      fill=col(rgb, aa))


def draw_shockwave(canvas, cx, cy, prog, rgb, r0=20, r1=170):
    if prog <= 0 or prog >= 1:
        return
    r = r0 + (r1 - r0) * ease_out_cubic(prog)
    aa = int(230 * (0.9 - 0.9 * prog))
    d = ImageDraw.Draw(canvas)
    d.ellipse((P(cx - r), P(cy - r), P(cx + r), P(cy + r)),
              outline=col(rgb, aa), width=P(3))


def shake_x(prog, amp=3.0, osc=3):
    if prog <= 0 or prog >= 1:
        return 0.0
    return math.sin(prog * math.pi * osc) * amp * (1 - prog)


# ===========================================================================
# SCENES  (each draws onto a fresh 2x transparent RGBA and returns it)
# ===========================================================================
# Payoff (retry-allow climax) holds 13f longer than before so the emotional
# payoff is not the briefest beat; those 13f are borrowed from the outro hold
# so TOTAL stays 600f/20s and the 599->0 loop seam is preserved.
SCENES = [
    ("hero", 0, 66), ("cast", 66, 150), ("term", 150, 216),
    ("allow1", 216, 288), ("deny", 288, 372), ("advance", 372, 426),
    ("payoff", 426, 499), ("cred", 499, 559), ("outro", 559, 600),
]
NAME2 = {n: (s, e) for (n, s, e) in SCENES}


def new_layer():
    return Image.new("RGBA", (W * SS, H * SS), (0, 0, 0, 0))


def eyebrow(d, text, y=118, rgb=ACCENT_LT):
    draw_tracked(d, CX, y, text, label(15, "SemiBold"), col(rgb, 240),
                 tracking=3)


# --- Scene 1: hero -----------------------------------------------------------
HERO_L1 = "Your AI can't merge"
HERO_L2 = "out of turn."


def sc_hero(lf, dur, f):
    cv = new_layer()
    d = ImageDraw.Draw(cv)
    intro = ease_out_cubic(seg(lf, 0, 16))
    eyebrow(d, "SHIPMATES · TOOL-BOUNDARY ENFORCEMENT", y=250,
            rgb=mix(BG, ACCENT_LT, intro))
    # staggered words
    def draw_words(line, cy, base_delay):
        fnt = disp(74, "Bold")
        words = line.split(" ")
        widths = [d.textlength(w + " ", font=fnt) / SS for w in words]
        total = sum(widths) - (d.textlength(" ", font=fnt) / SS)
        x = CX - total / 2
        for i, w in enumerate(words):
            t = ease_out_cubic(seg(lf, base_delay + i * 5, base_delay + i * 5 + 18))
            yoff = 20 * (1 - t)
            a = int(255 * t)
            d.text((P(x), P(cy + yoff)), w, font=fnt, fill=col(TEXT, a),
                   anchor="lm")
            x += d.textlength(w + " ", font=fnt) / SS
    draw_words(HERO_L1, 470, 6)
    draw_words(HERO_L2, 560, 20)
    # underline wipe under line 2
    uw = ease_out_cubic(seg(lf, 40, 60))
    if uw > 0:
        fnt = disp(74, "Bold")
        w2 = d.textlength(HERO_L2, font=fnt) / SS
        ux0 = CX - w2 / 2
        d.line((P(ux0), P(605), P(ux0 + w2 * uw), P(605)),
               fill=col(ACCENT, 255), width=P(6))
    # foreshadow pill drifts in, parks center-low
    pt = ease_out_cubic(seg(lf, 26, 52))
    if pt > 0:
        py = 900 + 30 * (1 - pt)
        draw_pill(cv, CX, py, "git push", int(255 * pt), lead=ACCENT,
                  glow=0.25 * pt)
    return cv


# --- Scene 2: cast -----------------------------------------------------------
def sc_cast(lf, dur, f):
    cv = new_layer()
    d = ImageDraw.Draw(cv)
    a0 = ease_out_cubic(seg(lf, 0, 14))
    eyebrow(d, "EVERY TOOL CALL IS CHECKED FIRST", y=210)
    wy = 560
    ax, hx_, bx = 210, CX, 872
    # wire
    wr = ease_out_cubic(seg(lf, 6, 26))
    d.line((P(ax + 60), P(wy), P(ax + 60 + (bx - 60 - ax - 60) * wr), P(wy)),
           fill=col(BORDER2, int(220 * a0)), width=P(2))
    draw_agent(cv, ax, wy, 44, int(255 * a0))
    draw_center(d, ax, wy + 78, "agent", mono(17, "Medium"),
                col(MUTED, int(255 * a0)))
    # build phase pill (destination)
    bpa = int(255 * ease_out_cubic(seg(lf, 10, 24)))
    panel(d, cv, bx - 62, wy - 34, bx + 62, wy + 34, rad=16, fill=PANEL,
          border=BORDER2, a=bpa)
    draw_center(d, bx, wy, "build", mono(22, "Bold"), col(TEXT, bpa))
    draw_center(d, bx, wy + 60, "phase", mono(16, "Medium"),
                col(MUTED, bpa))
    # hook diamond scales in with bloom
    ht = ENTRANCE(seg(lf, 20, 40))
    hs = 58 * (0.4 + 0.6 * ht)
    bloom = math.sin(math.pi * clamp01(seg(lf, 22, 52))) * 0.9
    draw_hook(cv, hx_, wy, hs, int(255 * ht), glow=0.3 + bloom)
    # typed label under diamond
    lbl = "PreToolUse → shipmates state gate"
    reveal = int(len(lbl) * clamp01(seg(lf, 34, 62)))
    if reveal > 0:
        draw_runs(d, hx_, wy + 108,
                  [("PreToolUse → ", col(MUTED, 235)),
                   ("shipmates state gate"[:max(0, reveal - 13)], col(ACCENT_LT, 245))],
                  mono(19, "Medium"))
    # 3 faint ghost pills tapping the diamond first (micro-loop: "EVERY call")
    if lf < 46:
        ghosts = ["git commit", "gh pr view", "git push"]
        for i, gl in enumerate(ghosts):
            ph = clamp01((lf - i * 12) / 12.0)   # each ghost taps in sequence
            if 0 < ph < 1.0:
                gt = ease_out_cubic(ph)
                gx = ax + 60 + (hx_ - 72 - (ax + 60)) * gt
                ga = int(105 * math.sin(math.pi * ph))
                draw_pill(cv, gx, wy, gl, ga, scale=0.8, lead=FAINT, size=18)
    # then the hero git push pill travels agent -> diamond and lands
    if lf >= 46:
        tt = ease_out_cubic(seg(lf, 46, 72))
        px = ax + 60 + (hx_ - (ax + 60)) * tt
        draw_pill(cv, px, wy, "git push", 255, lead=ACCENT,
                  glow=0.2 + 0.6 * max(0, 1 - abs(px - hx_) / 60))
    return cv


# --- Terminal scenes share layout -------------------------------------------
TX0, TY0, TX1 = 96, 452, 984
TERM_PAD = 34
STRIP_Y = 356


def pipe_x(i, x0=MARGIN + 40, x1=W - MARGIN - 40):
    return x0 + (x1 - x0) * i / 5


def draw_mini_strip(canvas, phase_idx, a, pulse_f, y=STRIP_Y, s=30):
    """6-node mini phase strip; `phase_idx` filled, current pulses."""
    d = ImageDraw.Draw(canvas)
    xs = [pipe_x(i) for i in range(6)]
    d.line((P(xs[0]), P(y), P(xs[-1]), P(y)), fill=col(BORDER1, a), width=P(2))
    fx = xs[min(phase_idx, 5)]
    d.line((P(xs[0]), P(y), P(fx), P(y)), fill=col(ACCENT, a), width=P(3))
    for i, name in enumerate(PHASES):
        cur = i == phase_idx
        reached = i <= phase_idx
        if cur:
            pr = 0.5 + 0.5 * math.sin(pulse_f * 0.2)
            glow_circle(canvas, xs[i], y, s * 0.5, ACCENT_LT, 0.5 * a / 255,
                        0.3 * a / 255, tight_r=8, wide_r=22)
            rr = s * 0.5 + 4 + 5 * pr
            d.ellipse((P(xs[i] - rr), P(y - rr), P(xs[i] + rr), P(y + rr)),
                      outline=col(ACCENT_LT, int(a * 0.8)), width=P(2))
        r = s * 0.42
        d.ellipse((P(xs[i] - r), P(y - r), P(xs[i] + r), P(y + r)),
                  fill=col(mix(PANEL, ACCENT, 0.5) if reached else PANEL, a),
                  outline=col(ACCENT if reached else BORDER2, a), width=P(2))
        draw_center(d, xs[i], y + s * 0.9, name, mono(14, "Medium"),
                    col(TEXT if cur else MUTED, int(a * (1 if cur else 0.85))))


def draw_prompt(d, x, y, tool, reveal_chars, size=21):
    """$ shipmates state gate --run 216 --tool "<tool>"  with reveal + cursor."""
    full = "$ " + (FACT_CMD % tool)
    shown = full[:reveal_chars]
    fnt = mono(size, "Regular")
    # colorize: $ faint, tool value accent
    tool_full = '"%s"' % tool
    ti = full.find(tool_full)
    spans = [(0, 1, FAINT)]
    if ti >= 0:
        spans.append((ti, ti + len(tool_full), ACCENT_LT))
    x2 = x
    for gi, ch in enumerate(shown):
        c = TEXT
        for (a0, a1, rgb) in spans:
            if a0 <= gi < a1:
                c = rgb
                break
        d.text((P(x2), P(y)), ch, font=fnt, fill=col(c, 255), anchor="lm")
        x2 += d.textlength(ch, font=fnt) / SS
    return x2, len(full)


# --- Scene 3: live terminal --------------------------------------------------
def sc_term(lf, dur, f):
    cv = new_layer()
    d = ImageDraw.Draw(cv)
    eyebrow(d, "THE HOOK RUNS ONE COMMAND", y=118)
    draw_center(d, CX, 168, "Before every tool call", disp(34, "Medium"),
                col(TEXT, 255))
    draw_mini_strip(cv, 2, 255, f)
    ty1 = 1000
    top = draw_term_frame(cv, TX0, TY0, TX1, ty1,
                          [("run-216 · phase: ", col(MUTED, 255)),
                           ("build", col(ACCENT_LT, 255))], 255)
    reveal = int((28 / 30) * lf) + 1
    ex, total = draw_prompt(d, TX0 + TERM_PAD, top + 44, "git push", reveal)
    if lf % 20 < 12 and reveal <= total:
        d.rectangle((P(ex + 3), P(top + 44 - 13), P(ex + 12), P(top + 44 + 13)),
                    fill=col(TEXT, 255))
    if reveal >= total:
        aa = int(255 * seg(lf, 44, 56))
        draw_left(d, TX0 + TERM_PAD, top + 96, "› resolving gate…",
                  mono(18, "Regular"), col(FAINT, aa))
    return cv


# --- terminal verdict scenes (allow1 / deny / payoff) ------------------------
def draw_verdict_terminal(cv, tool, json_str, allow, phase_word, lf,
                          type_end, reveal_start, exit_code, caption,
                          bloom_scale, burst_n, seed):
    d = ImageDraw.Draw(cv)
    verdict_rgb = GREEN if allow else RED
    eyebrow(d, ("IN ORDER" if allow else "OUT OF ORDER"), y=118,
            rgb=ACCENT_LT)
    draw_center(d, CX, 168,
                ("The gate opens" if allow else "The gate refuses"),
                disp(34, "Medium"), col(TEXT, 255))
    phase_idx = PHASES.index(phase_word)
    draw_mini_strip(cv, phase_idx, 255, lf + 100)
    ty1 = 1030
    top = draw_term_frame(cv, TX0, TY0, TX1, ty1,
                          [("run-216 · phase: ", col(MUTED, 255)),
                           (phase_word, col(ACCENT_LT, 255))], 255)
    # prompt (typed then static)
    reveal = min(len("$ " + (FACT_CMD % tool)),
                 int((30 / 30) * max(0, lf) * (28 / 30)) + 3) \
        if lf < type_end else len("$ " + (FACT_CMD % tool))
    py = top + 44
    ex, total = draw_prompt(d, TX0 + TERM_PAD, py, tool, reveal)
    if lf < type_end and lf % 20 < 12:
        d.rectangle((P(ex + 3), P(py - 13), P(ex + 12), P(py + 13)),
                    fill=col(TEXT, 255))
    # output JSON reveal
    reveal_prog = seg(lf, reveal_start, reveal_start + 12)
    if reveal_prog > 0:
        jfnt = mono(30, "Regular")
        maxw = (TX1 - TX0) - 2 * TERM_PAD
        oy = py + 74
        spans = []
        if allow:
            spans.append(span(json_str, '"allow":true', GREEN))
        else:
            spans.append(span(json_str, '"allow":false', RED))
            ri = json_str.find('"reason":"')
            if ri >= 0:
                spans.append((ri + 10, len(json_str) - 2, ACCENT_LT))
        # progressive char reveal for the JSON
        n_show = int(len(json_str) * ease_out_cubic(reveal_prog))
        shown = json_str[:n_show]
        endy = draw_code_wrapped(d, TX0 + TERM_PAD, oy, maxw, shown, jfnt,
                                 44, MUTED, 255, spans=spans,
                                 cursor=(n_show < len(json_str)))
        # exit annotation
        if n_show >= len(json_str):
            ea = int(255 * seg(lf, reveal_start + 12, reveal_start + 20))
            draw_runs(d, CX, endy + 22,
                      [("→ exit ", col(MUTED, ea)),
                       (str(exit_code), col(verdict_rgb, ea))],
                      mono(24, "Bold"))
    # caption under terminal
    ca = int(255 * seg(lf, reveal_start + 16, reveal_start + 28))
    if ca > 4:
        draw_center(d, CX, 1130, caption, disp(30, "Medium"),
                    col(TEXT, ca))
    return top, ty1, phase_idx


# --- Scene 4: BEAT 1 allow ---------------------------------------------------
def sc_allow1(lf, dur, f):
    cv = new_layer()
    top, ty1, _ = draw_verdict_terminal(
        cv, "git push", J_ALLOW_PUSH, True, "build", lf,
        type_end=0, reveal_start=8, exit_code=0,
        caption="In order → allowed.", bloom_scale=1.0, burst_n=14, seed=4004)
    # verdict badge (green check) at right of terminal titlebar area
    bx, by = TX1 - 70, TY0 - 6
    vprog = seg(lf, 12, 24)
    bloom = math.sin(math.pi * clamp01(seg(lf, 12, 46))) * 0.75
    if bloom > 0.01:
        glow_circle(cv, bx, by, 46, GREEN, 0.6 * bloom, 0.45 * bloom,
                    tight_r=12, wide_r=44)
    d = ImageDraw.Draw(cv)
    d.ellipse((P(bx - 34), P(by - 34), P(bx + 34), P(by + 34)),
              fill=col(mix(PANEL, GREEN, 0.12), 255), outline=col(GREEN, 255),
              width=P(3))
    stroke_check(d, bx, by, 18, GREEN_FLASH, 255, vprog, P(7))
    # particle burst
    draw_burst(cv, bx, by, seg(lf, 16, 52), 14, GREEN, 4004)
    return cv


# --- Scene 5: BEAT 2 deny ----------------------------------------------------
def sc_deny(lf, dur, f):
    cv = new_layer()
    sh = shake_x(seg(lf, 44, 60), amp=3.0, osc=3)
    # apply terminal shake by drawing content shifted: we draw normally but nudge
    # the whole verdict block via a horizontal offset on a sub-layer.
    sub = new_layer()
    top, ty1, _ = draw_verdict_terminal(
        sub, "gh pr merge --squash", J_DENY, False, "build", lf,
        type_end=26, reveal_start=32, exit_code=1,
        caption="Out of order → hard stop.", bloom_scale=1.0, burst_n=0,
        seed=5005)
    if abs(sh) > 0.05:
        cv.alpha_composite(sub, dest=(int(P(sh)), 0))
    else:
        cv.alpha_composite(sub)
    # verdict badge (red X)
    bx, by = TX1 - 70, TY0 - 6
    vprog = seg(lf, 36, 48)
    bloom = math.sin(math.pi * clamp01(seg(lf, 36, 66))) * 0.8
    if bloom > 0.01:
        glow_circle(cv, bx, by, 46, RED, 0.6 * bloom, 0.45 * bloom,
                    tight_r=12, wide_r=44)
    d = ImageDraw.Draw(cv)
    d.ellipse((P(bx - 34), P(by - 34), P(bx + 34), P(by + 34)),
              fill=col(mix(PANEL, RED, 0.12), 255), outline=col(RED, 255),
              width=P(3))
    stroke_x(d, bx, by, 17, RED_FLASH, 255, vprog, P(7))
    # single hard shockwave ring centered on the terminal
    draw_shockwave(cv, CX, (TY0 + ty1) / 2, seg(lf, 40, 60), RED,
                   r0=40, r1=360)
    return cv


# --- Scene 6: phase advance --------------------------------------------------
def sc_advance(lf, dur, f):
    cv = new_layer()
    d = ImageDraw.Draw(cv)
    eyebrow(d, "ADVANCE THROUGH THE PHASES", y=118)
    draw_center(d, CX, 170, "plan → isolate → build → verify → review → deliver",
                mono(20, "Medium"), col(MUTED, 235))
    # full 6-node pipeline centered
    py = 560
    xs = [pipe_x(i, x0=MARGIN + 60, x1=W - MARGIN - 60) for i in range(6)]
    s = 58
    # liquid fill build(2)->deliver(5): 3 hops, 12f each starting lf=6
    lead = 2.0 + 3.0 * ease_in_out(seg(lf, 6, 42))     # 2..5
    lead = min(lead, 5.0)
    # connectors
    for i in range(5):
        x0c, x1c = xs[i] + s, xs[i + 1] - s
        d.line((P(x0c), P(py), P(x1c), P(py)), fill=col(BORDER1, 255), width=P(3))
        fillt = clamp01(lead - i)
        if fillt > 0 and i >= 2:
            fx = x0c + (x1c - x0c) * fillt
            d.line((P(x0c), P(py), P(fx), P(py)), fill=col(ACCENT, 255), width=P(5))
            glow_circle(cv, fx, py, 6, ACCENT_LT, 0.6, 0.4, tight_r=8, wide_r=24)
        elif i < 2:
            d.line((P(x0c), P(py), P(x1c), P(py)), fill=col(ACCENT, 255), width=P(5))
    for i, name in enumerate(PHASES):
        filled = lead >= i - 0.02
        ir = clamp01((lead - (i - 0.6)) / 0.6) if i >= 2 else 1.0
        ring = 0.0
        if i == 5:
            ring = math.sin(math.pi * clamp01(seg(lf, 40, 54)))
        draw_phase_node(cv, xs[i], py, name, s, filled, 255,
                        ring=ring, icon_reveal=ir)
    # titlebar phase morph shown on shrunken terminal card at bottom.
    # LOG-STYLE (no prompt, no command): plain gate log lines reveal in sync
    # with each pipeline hop as the liquid fill crosses each connector.
    cur_word = PHASES[min(5, int(round(lead)))]
    tx0, tyy0, tx1, tyy1 = 150, 998, 930, 1176
    top = draw_term_frame(cv, tx0, tyy0, tx1, tyy1,
                          [("run-216 · phase: ", col(MUTED, 255)),
                           (cur_word, col(ACCENT_LT, 255))], 255)
    lfnt = mono(18, "Regular")
    ly = top + 38
    for k, (frm, to) in enumerate((("build", "verify"),
                                   ("verify", "review"),
                                   ("review", "deliver"))):
        # connector (2 + k) fills as lead goes (2 + k) -> (3 + k); reveal in sync
        la = int(255 * clamp01((lead - (2 + k)) / 0.8))
        if la > 4:
            draw_runs(d, (tx0 + tx1) / 2, ly + k * 42,
                      [("phase: ", col(FAINT, la)),
                       (frm, col(TEXT, la)),
                       (" → ", col(MUTED, la)),
                       (to, col(ACCENT_LT, la))], lfnt)
    return cv


# --- Scene 7: BEAT 3 allow payoff --------------------------------------------
def sc_payoff(lf, dur, f):
    cv = new_layer()
    top, ty1, _ = draw_verdict_terminal(
        cv, "gh pr merge --squash", J_ALLOW_MERGE, True, "deliver", lf,
        type_end=12, reveal_start=18, exit_code=0,
        caption="Reach deliver → the same call is allowed.",
        bloom_scale=1.6, burst_n=22, seed=7007)
    bx, by = TX1 - 70, TY0 - 6
    vprog = seg(lf, 22, 34)
    bloom = math.sin(math.pi * clamp01(seg(lf, 22, 58))) * 1.1
    if bloom > 0.01:
        glow_circle(cv, bx, by, 60, GREEN, 0.7 * bloom, 0.55 * bloom,
                    tight_r=14, wide_r=54)
    d = ImageDraw.Draw(cv)
    d.ellipse((P(bx - 42), P(by - 42), P(bx + 42), P(by + 42)),
              fill=col(mix(PANEL, GREEN, 0.14), 255), outline=col(GREEN, 255),
              width=P(4))
    stroke_check(d, bx, by, 24, GREEN_FLASH, 255, vprog, P(8))
    draw_burst(cv, bx, by, seg(lf, 24, 60), 24, GREEN, 7007, spread=240)
    # pill sails through into a branch-merge glyph (top-right), lit terracotta
    mt = ease_in_cubic(seg(lf, 30, 48))
    gx, gy = 900, 300
    if mt > 0:
        px = bx - 40 + (gx - (bx - 40)) * mt
        draw_pill(cv, px, gy + (by - gy) * (1 - mt) * 0.0 + (TY0 - 6 - gy) * (1 - mt),
                  "gh pr merge", int(255 * (1 - seg(lf, 44, 50))), lead=ACCENT,
                  glow=0.4)
    lit = seg(lf, 40, 52)
    if lit > 0:
        glow_circle(cv, gx, gy, 30, ACCENT, 0.5 * lit, 0.4 * lit)
        # branch-merge glyph: two branches merging into one
        d.line((P(gx - 40), P(gy - 26), P(gx), P(gy)), fill=col(ACCENT_LT, 255),
               width=P(6), joint="curve")
        d.line((P(gx - 40), P(gy + 26), P(gx), P(gy)), fill=col(ACCENT_LT, 255),
               width=P(6), joint="curve")
        d.line((P(gx), P(gy), P(gx + 44), P(gy)), fill=col(ACCENT_LT, 255),
               width=P(6))
        for dx, dy in ((-40, -26), (-40, 26), (44, 0)):
            d.ellipse((P(gx + dx - 8), P(gy + dy - 8), P(gx + dx + 8),
                       P(gy + dy + 8)), fill=col(ACCENT, 255))
    return cv


# --- Scene 8: credibility ----------------------------------------------------
HARNESSES = ["Claude Code", "Cursor", "Windsurf", "Antigravity",
             "GitHub Copilot", "opencode", "Codex"]


def sc_cred(lf, dur, f):
    cv = new_layer()
    d = ImageDraw.Draw(cv)
    a0 = ease_out_cubic(seg(lf, 0, 12))
    eyebrow(d, "THE SAME GATE, EVERY HARNESS", y=140)
    # shared hook-diamond above with 7 wires fanning to chips
    hxx, hyy = CX, 300
    # chip layout
    rows = [HARNESSES[:4], HARNESSES[4:]]
    row_y = [620, 730]
    chip_h = 58
    positions = []
    for r, row in enumerate(rows):
        widths = []
        for name in row:
            tw = d.textlength(name, font=label(20, "SemiBold")) / SS
            widths.append(tw + 52)
        gap = 22
        total = sum(widths) + gap * (len(row) - 1)
        x = CX - total / 2
        for i, name in enumerate(row):
            positions.append((name, x + widths[i] / 2, row_y[r], widths[i]))
            x += widths[i] + gap
    # wires
    for idx, (name, px, py, pw) in enumerate(positions):
        wa = int(150 * ease_out_cubic(seg(lf, 6 + idx, 24 + idx)))
        d.line((P(hxx), P(hyy + 30), P(px), P(py - chip_h / 2)),
               fill=col(mix(BORDER2, ACCENT, 0.3), wa), width=P(2))
    draw_hook(cv, hxx, hyy, 44, int(255 * a0), glow=0.35)
    # counters row
    cy = 470
    counters = [(min(6, 6 * ease_out_cubic(seg(lf, 8, 30))), "phases", "6"),
                (min(7, 7 * ease_out_cubic(seg(lf, 10, 34))), "harnesses", "7"),
                (None, "exit code", "0/1")]
    cxs = [CX - 300, CX, CX + 300]
    for (val, lab, static), ccx in zip(counters, cxs):
        if val is None:
            big = static
        else:
            big = str(int(round(val)))
        draw_center(d, ccx, cy, big, disp(58, "Bold"), col(ACCENT, 255))
        draw_center(d, ccx, cy + 48, lab, label(18, "Medium"),
                    col(MUTED, 255))
    # chips scale/flip in staggered
    for idx, (name, px, py, pw) in enumerate(positions):
        t = ENTRANCE(seg(lf, 8 + idx * 2, 24 + idx * 2))
        if t <= 0.01:
            continue
        sx = max(0.05, t)                      # horizontal "flip" grow
        hw = pw / 2 * sx
        is_codex = name == "Codex"
        border = ACCENT if is_codex else BORDER2
        aa = int(255 * clamp01(t * 1.2))
        panel(d, cv, px - hw, py - chip_h / 2, px + hw, py + chip_h / 2,
              rad=14, fill=PANEL, border=border, a=aa, bw=2 if not is_codex else 3)
        if sx > 0.6:
            draw_center(d, px, py, name, label(20, "SemiBold"),
                        col(TEXT if is_codex else MUTED, aa))
        if is_codex and t > 0.7:
            glow_rect(cv, px - hw, py - chip_h / 2, px + hw, py + chip_h / 2,
                      14, ACCENT, 0.25, 0.2, tight_r=8, wide_r=24)
            draw_center(d, px, py + chip_h / 2 + 22, "worked example",
                        label(15, "Medium"), col(ACCENT_LT, aa))
    return cv


# --- Scene 9: punchline / outro ---------------------------------------------
PUNCH = "It enforces order — not a wall."


def sc_outro(lf, dur, f):
    cv = new_layer()
    d = ImageDraw.Draw(cv)
    # `dur` may be shortened (payoff borrowed hold frames from the outro), so
    # compress the reveal choreography proportionally: it still fully resolves
    # right at the loop point, keeping the dark->dark 599->0 seam identical.
    def s(a, b):
        k = dur / 54.0
        return seg(lf, a * k, b * k)
    # punchline resolves word-by-word
    fnt = disp(52, "Bold")
    words = PUNCH.split(" ")
    widths = [d.textlength(w + " ", font=fnt) / SS for w in words]
    total = sum(widths) - d.textlength(" ", font=fnt) / SS
    x = CX - total / 2
    order_i = words.index("order")
    for i, w in enumerate(words):
        t = ease_out_cubic(s(4 + i * 4, 4 + i * 4 + 16))
        a = int(255 * t)
        rgb = ACCENT if i == order_i else TEXT
        yoff = 12 * (1 - t)
        d.text((P(x), P(560 + yoff)), w, font=fnt, fill=col(rgb, a), anchor="lm")
        x += d.textlength(w + " ", font=fnt) / SS
    # underline wipe under whole line
    uw = ease_out_cubic(s(30, 46))
    if uw > 0:
        d.line((P(CX - total / 2), P(598), P(CX - total / 2 + total * uw),
                P(598)), fill=col(ACCENT, 255), width=P(5))
    # sub-line: 6 phase words illuminate L->R
    sy = 690
    pfnt = mono(22, "Medium")
    joiner = "  →  "
    seq = []
    for i, p in enumerate(PHASES):
        seq.append((p, i))
    fullw = sum(d.textlength(p + (joiner if i < 5 else ""), font=pfnt) / SS
                for i, p in enumerate(PHASES))
    x = CX - fullw / 2
    for i, p in enumerate(PHASES):
        lit = ease_out_cubic(s(20 + i * 4, 20 + i * 4 + 14))
        rgb = mix(FAINT, ACCENT, lit)
        d.text((P(x), P(sy)), p, font=pfnt, fill=col(rgb, 255), anchor="lm")
        x += d.textlength(p, font=pfnt) / SS
        if i < 5:
            d.text((P(x), P(sy)), joiner, font=pfnt, fill=col(FAINT, 200),
                   anchor="lm")
            x += d.textlength(joiner, font=pfnt) / SS
    # wordmark + tag
    wa = int(255 * ease_out_cubic(s(34, 48)))
    draw_center(d, CX, 940, "Shipmates", disp(44, "Bold"), col(TEXT, wa))
    draw_center(d, CX, 1000, "A hard stop at the tool boundary.",
                label(20, "Medium"), col(MUTED, wa))
    return cv


SCENE_FN = {
    "hero": sc_hero, "cast": sc_cast, "term": sc_term, "allow1": sc_allow1,
    "deny": sc_deny, "advance": sc_advance, "payoff": sc_payoff,
    "cred": sc_cred, "outro": sc_outro,
}

_scene_cache = {}


def render_scene(name, lf):
    dur = NAME2[name][1] - NAME2[name][0]
    return SCENE_FN[name](lf, dur, NAME2[name][0] + lf)


# ---------------------------------------------------------------------------
# Compositor
# ---------------------------------------------------------------------------
DISS = 6


def scene_layers(f):
    """Return list of (name, lf, alpha, dur)."""
    for i in range(1, len(SCENES)):
        b = SCENES[i][1]
        if b - DISS <= f < b + DISS:
            t = ease_in_out((f - (b - DISS)) / (2 * DISS))
            o, n = SCENES[i - 1], SCENES[i]
            return [(o[0], f - o[1], 1 - t, o[2] - o[1]),
                    (n[0], f - n[1], t, n[2] - n[1])]
    for (name, s, e) in SCENES:
        if s <= f < e:
            return [(name, f - s, 1.0, e - s)]
    return [("outro", f - 559, 1.0, 41)]


def zoom_mid(lf, dur):
    return 1.0 + 0.02 * ease_out_cubic(clamp01(lf / (0.6 * dur)))


def zoom_bg(lf, dur):
    return 1.0 + 0.035 * ease_out_cubic(clamp01(lf / (0.7 * dur)))


def render_frame(f, grain=True):
    layers = scene_layers(f)
    # loop dissolve toward scene-1 composition
    # Seamless soft-loop: the last ~15f cross-dissolve the outro punchline back
    # toward the scene-1 OPENING composition (hero at lf=0 — a calm dark
    # starfield, identical to frame 0), so the 599->0 wrap is a soft dark->dark
    # match rather than a bright-title->empty pop. (Reinterprets the brief's
    # "toward the scene-1 title" as "toward the scene-1 opening" to keep the loop
    # genuinely seamless; scene 1 then rebuilds the title from this state.)
    loop_a = 0.0
    if f >= 585:
        loop_a = ease_in_out(seg(f, 585, 600))
        layers = [(n, lf, a * (1 - loop_a), d) for (n, lf, a, d) in layers]
        layers.append(("hero", 0, loop_a, 66))

    primary = layers[0]
    # outro / loop darkens background
    dark = 0.35 * ease_in_out(seg(f, 559, 579)) if f >= 559 else 0.0
    if f >= 585:
        dark *= (1 - loop_a)
    bg = build_bg(f, zoom_bg(primary[1], primary[3]), dark=dark)
    bg = bg.convert("RGBA")

    for (name, lf, a, dur) in layers:
        if a <= 0.004:
            continue
        ov = render_scene(name, lf)
        if SS != 1:
            ov = ov.resize((W, H), Image.LANCZOS)
        ov = crop_zoom_rgba(ov, zoom_mid(lf, dur))
        if a < 0.999:
            r, g, b, al = ov.split()
            al = al.point(lambda p, a=a: int(p * a))
            ov = Image.merge("RGBA", (r, g, b, al))
        bg.alpha_composite(ov)

    final = bg.convert("RGB")
    arr = np.asarray(final, dtype=np.float32)
    if grain:
        rng = np.random.default_rng(f // 2)   # regenerate every 2nd frame
        noise = rng.normal(0.0, 3.4, (H, W, 1)).astype(np.float32)
        arr += noise
    np.clip(arr, 0, 255, out=arr)
    return Image.fromarray(arr.astype(np.uint8), "RGB")


# ---------------------------------------------------------------------------
# Encoding
# ---------------------------------------------------------------------------
def encode_mp4(frames_dir, out_path, fps, pattern="f%04d.png"):
    cmd = [FFMPEG, "-y", "-framerate", str(fps), "-i",
           str(frames_dir / pattern),
           "-c:v", "libx264", "-crf", "18", "-pix_fmt", "yuv420p",
           "-movflags", "+faststart", str(out_path)]
    subprocess.run(cmd, check=True, stdout=subprocess.DEVNULL,
                   stderr=subprocess.DEVNULL)
    return " ".join(cmd)


GIF_W = 810
GIF_H = 1013
GIF_FPS = 15
# condensed cut segments (avoid dissolve edges): title, allow, deny, advance,
# payoff, outro.  7-harness beat + grain dropped.
GIF_SEGMENTS = [(6, 60), (224, 286), (296, 370), (374, 424), (430, 484),
                (561, 599)]


def gif_frame_indices():
    idxs = []
    for (s, e) in GIF_SEGMENTS:
        idxs.extend(range(s, e, 2))          # 30fps -> 15fps
    return idxs


def encode_gif(frames_dir, out_path, fps):
    palette = out_path.parent / "_palette.png"
    src = str(frames_dir / "f%04d.png")
    scale = f"scale={GIF_W}:{GIF_H}:flags=lanczos"
    vf = f"{scale},palettegen=max_colors=128:stats_mode=diff"
    c1 = [FFMPEG, "-y", "-framerate", str(fps), "-i", src, "-vf", vf,
          str(palette)]
    subprocess.run(c1, check=True, stdout=subprocess.DEVNULL,
                   stderr=subprocess.DEVNULL)
    lavfi = "[x][1:v]paletteuse=dither=floyd_steinberg"
    c2 = [FFMPEG, "-y", "-framerate", str(fps), "-i", src, "-i", str(palette),
          "-lavfi", f"[0:v]{scale}[x];{lavfi}", "-loop", "0", str(out_path)]
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
    ap.add_argument("--fast", action="store_true")
    ap.add_argument("--frame", type=int, default=None)
    ap.add_argument("--scene", type=int, default=None,
                    help="render only scene K (1-9) as an mp4")
    ap.add_argument("--frames-only", action="store_true")
    ap.add_argument("--out-dir", default=str(HERE))
    args = ap.parse_args()

    out_dir = Path(args.out_dir)
    out_dir.mkdir(parents=True, exist_ok=True)
    if args.fast:
        SS = 1

    if args.frame is not None:
        dest = Path("/tmp/info_frames")
        dest.mkdir(parents=True, exist_ok=True)
        img = render_frame(args.frame)
        p = dest / f"f{args.frame:04d}.png"
        img.save(p)
        print(f"wrote {p}  fonts={FONTS_USED}")
        return

    if args.scene is not None:
        s, e = SCENES[args.scene - 1][1], SCENES[args.scene - 1][2]
        idxs = list(range(max(0, s - 6), min(N_FRAMES, e + 6)))
    else:
        step = 3 if args.fast else 1
        idxs = list(range(0, N_FRAMES, step))

    t0 = time.time()
    tmp = Path(tempfile.mkdtemp(prefix="info_frames_"))
    try:
        for out_i, f in enumerate(idxs):
            render_frame(f).save(tmp / f"f{out_i:04d}.png")
            if f % 30 == 0:
                print(f"  frame {f}/{N_FRAMES}", flush=True)
        print(f"rendered {len(idxs)} frames in {time.time() - t0:.1f}s "
              f"fonts={FONTS_USED}")
        if args.frames_only:
            print(f"frames in {tmp}")
            return
        eff_fps = FPS // 3 if args.fast else FPS
        suffix = f"-scene{args.scene}" if args.scene else ""
        mp4 = out_dir / f"hooks-infographic{suffix}.mp4"
        print("mp4:", encode_mp4(tmp, mp4, eff_fps))
        print(f"mp4 -> {mp4}  ({mp4.stat().st_size} bytes)")

        if not args.fast and args.scene is None:
            gif_tmp = Path(tempfile.mkdtemp(prefix="info_gif_"))
            try:
                set_scale(1)
                tg = time.time()
                for out_i, f in enumerate(gif_frame_indices()):
                    render_frame(f, grain=False).save(gif_tmp / f"f{out_i:04d}.png")
                print(f"rendered {out_i + 1} grain-free GIF frames in "
                      f"{time.time() - tg:.1f}s")
                gif = out_dir / "hooks-infographic.gif"
                print("gif:", encode_gif(gif_tmp, gif, GIF_FPS))
                print(f"gif -> {gif}  ({gif.stat().st_size} bytes)")
            finally:
                shutil.rmtree(gif_tmp, ignore_errors=True)
        print(f"total {time.time() - t0:.1f}s")
    finally:
        shutil.rmtree(tmp, ignore_errors=True)


def set_scale(ss):
    global SS, _BASE_BG, _GRID
    SS = ss


if __name__ == "__main__":
    main()
