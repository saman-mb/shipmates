#!/usr/bin/env python3
"""pixelart — render pixel art (static PNG or animated GIF) from a JSON spec.

Self-contained: the only dependency is Pillow. This is the runnable payload of
the shipmates `pixelart` tool. An agent reaches for it, per tool.md, when a task
implies producing a small pixel-art asset — an icon, sprite, favicon, or badge —
in the same hard-edged style as the shipmates logo. It is never a slash command.

The technique is the logo's, made reusable: a small logical grid of palette
characters, a deliberately limited palette, upscaled by a whole-number factor
with nearest-neighbour and no smoothing. `site/assets/logo.png` is a 48x48 grid
scaled x14 to 672x672 the same way. BRAND.md's rule — "Never resample with
smoothing. Scale by whole-number factors and use nearest-neighbour." — is
enforced here: every upscale is `Image.NEAREST`, so each logical pixel becomes a
solid `scale`x`scale` block with no gradient.

Usage:
    # static PNG (spec has "grid")
    python3 pixelart.py --spec spec.json --out art.png

    # animated GIF (spec has "frames"); optional reduced-motion poster PNG
    python3 pixelart.py --spec spec.json --out art.gif --poster art_poster.png

Spec (JSON):
    {
      "scale": 14,
      "palette": {                     # single-char keys -> colour
        ".": "#00000000",              #   8-digit #RRGGBBAA, or the words
        "#": "#1A1F36",                #   "none"/"transparent"/"." = transparent
        "o": "#FFC24A"
      },
      "grid": ["..###..", ".#ooo#."]   # rows of palette chars, all equal length
    }

    Animate by giving "frames" (a list of grids) instead of "grid":
      "frames": [grid, grid, ...],
      "durations": 200                 # ms; an int for all, or a per-frame list

Exit codes: 0 ok; 2 bad spec / usage.
"""
import argparse
import json
import sys

try:
    from PIL import Image
except ImportError:
    sys.exit("pixelart: Pillow is required — install it with: pip install Pillow")

MAX_SCALE = 512          # a sane upper bound on the whole-number upscale factor
TRANSPARENT = (0, 0, 0, 0)
_TRANSPARENT_WORDS = {".", "none", "transparent", "clear"}


class SpecError(Exception):
    """A spec that cannot be rendered. Reported to the user, exits 2."""


# ---------------------------------------------------------------------------
# Parsing / validation
# ---------------------------------------------------------------------------

def parse_color(value):
    """A palette value -> an (r, g, b, a) tuple.

    Accepts #RGB, #RGBA, #RRGGBB and #RRGGBBAA hex, plus the words
    none/transparent/clear (and the ".") for fully transparent.
    """
    if not isinstance(value, str):
        raise SpecError(f"colour must be a string, got {value!r}")
    s = value.strip()
    if s.lower() in _TRANSPARENT_WORDS:
        return TRANSPARENT
    if not s.startswith("#"):
        raise SpecError(f"colour {value!r} must start with '#' (or be transparent)")
    h = s[1:]
    if not all(c in "0123456789abcdefABCDEF" for c in h):
        raise SpecError(f"colour {value!r} has non-hex characters")
    if len(h) in (3, 4):                      # shorthand: expand each nibble
        h = "".join(c * 2 for c in h)
    if len(h) == 6:
        h += "ff"
    if len(h) != 8:
        raise SpecError(
            f"colour {value!r} must be #RGB, #RGBA, #RRGGBB or #RRGGBBAA")
    r, g, b, a = (int(h[i:i + 2], 16) for i in (0, 2, 4, 6))
    return (r, g, b, a)


def build_palette(raw):
    if not isinstance(raw, dict) or not raw:
        raise SpecError("spec.palette must be a non-empty object of char -> colour")
    palette = {}
    for key, value in raw.items():
        if not isinstance(key, str) or len(key) != 1:
            raise SpecError(
                f"palette keys must be single characters — {key!r} is not")
        palette[key] = parse_color(value)
    return palette


def validate_grid(grid, palette, where):
    if not isinstance(grid, list) or not grid:
        raise SpecError(f"{where} must be a non-empty list of rows")
    width = None
    for i, row in enumerate(grid):
        if not isinstance(row, str):
            raise SpecError(f"{where} row {i} must be a string")
        if width is None:
            width = len(row)
            if width == 0:
                raise SpecError(f"{where} rows must not be empty")
        elif len(row) != width:
            raise SpecError(
                f"{where} rows must be equal length: row {i} is {len(row)}, "
                f"expected {width}")
        for ch in row:
            if ch not in palette:
                raise SpecError(
                    f"{where} row {i} uses {ch!r}, which is not in the palette")
    return width, len(grid)


def read_scale(spec):
    if "scale" not in spec:
        raise SpecError("spec.scale is required (a whole-number upscale factor)")
    scale = spec["scale"]
    if isinstance(scale, float):
        if not scale.is_integer():
            raise SpecError(f"scale must be a whole number, got {scale}")
        scale = int(scale)
    if not isinstance(scale, int) or isinstance(scale, bool):
        raise SpecError(f"scale must be an integer, got {scale!r}")
    if not 1 <= scale <= MAX_SCALE:
        raise SpecError(f"scale must be between 1 and {MAX_SCALE}, got {scale}")
    return scale


# ---------------------------------------------------------------------------
# Rendering — every upscale is Image.NEAREST (no smoothing).
# ---------------------------------------------------------------------------

def render_rgba(grid, palette, scale):
    """A grid -> a full-size RGBA image, upscaled with nearest-neighbour."""
    w, h = len(grid[0]), len(grid)
    small = Image.new("RGBA", (w, h))
    small.putdata([palette[ch] for row in grid for ch in row])
    return small.resize((w * scale, h * scale), Image.NEAREST)


def _gif_index_tables(palette):
    """Map palette chars -> GIF colour indices (index 0 = transparent).

    GIF alpha is 1-bit: a palette entry with alpha < 128 is transparent.
    Returns (char_to_index, flat_palette_bytes).
    """
    opaque = []                      # list of (r, g, b), in first-seen order
    rgb_to_index = {}
    char_to_index = {}
    for ch, (r, g, b, a) in palette.items():
        if a < 128:
            char_to_index[ch] = 0    # transparent slot
            continue
        rgb = (r, g, b)
        if rgb not in rgb_to_index:
            rgb_to_index[rgb] = len(opaque) + 1     # +1: reserve 0 for transparent
            opaque.append(rgb)
        char_to_index[ch] = rgb_to_index[rgb]
    if len(opaque) > 255:
        raise SpecError(
            f"too many opaque colours for a GIF ({len(opaque)}); the limit is 255")
    flat = [0, 0, 0]                 # index 0 (transparent) — colour is unused
    for rgb in opaque:
        flat += list(rgb)
    flat += [0, 0, 0] * (256 - len(opaque) - 1)
    return char_to_index, flat


def render_gif_frame(grid, char_to_index, flat, scale):
    """A grid -> a full-size P-mode frame carrying a transparent index."""
    w, h = len(grid[0]), len(grid)
    data = bytes(char_to_index[ch] for row in grid for ch in row)
    small = Image.frombytes("P", (w, h), data)
    big = small.resize((w * scale, h * scale), Image.NEAREST)
    big.putpalette(flat)
    big.info["transparency"] = 0
    return big


def read_durations(spec, n):
    raw = spec.get("durations", 200)
    if isinstance(raw, bool):
        raise SpecError("durations must be a number or list of numbers")
    if isinstance(raw, (int, float)):
        return [int(raw)] * n
    if isinstance(raw, list):
        if len(raw) != n:
            raise SpecError(
                f"durations has {len(raw)} entries but there are {n} frames")
        out = []
        for d in raw:
            if isinstance(d, bool) or not isinstance(d, (int, float)):
                raise SpecError(f"each duration must be a number, got {d!r}")
            out.append(int(d))
        return out
    raise SpecError("durations must be a number or a list of numbers")


# ---------------------------------------------------------------------------
# Drive
# ---------------------------------------------------------------------------

def render(spec, out, poster=None):
    if not isinstance(spec, dict):
        raise SpecError("spec must be a JSON object")
    scale = read_scale(spec)
    palette = build_palette(spec.get("palette"))
    has_frames = "frames" in spec
    has_grid = "grid" in spec
    if has_frames and has_grid:
        raise SpecError("spec has both 'grid' and 'frames' — use one or the other")
    if not has_frames and not has_grid:
        raise SpecError("spec needs a 'grid' (static) or 'frames' (animated)")

    if has_grid:
        validate_grid(spec["grid"], palette, "grid")
        img = render_rgba(spec["grid"], palette, scale)
        img.save(out, format="PNG")
        note = f"pixelart: wrote {out} ({img.width}x{img.height})"
        if poster:                    # a static poster is just the same image
            img.save(poster, format="PNG")
            note += f" + poster {poster}"
        return note

    frames = spec["frames"]
    if not isinstance(frames, list) or len(frames) < 1:
        raise SpecError("frames must be a non-empty list of grids")
    dims = None
    for i, grid in enumerate(frames):
        wh = validate_grid(grid, palette, f"frames[{i}]")
        if dims is None:
            dims = wh
        elif wh != dims:
            raise SpecError(
                f"every frame must share one size: frames[{i}] is {wh[0]}x{wh[1]}, "
                f"expected {dims[0]}x{dims[1]}")
    durations = read_durations(spec, len(frames))
    char_to_index, flat = _gif_index_tables(palette)
    gif_frames = [render_gif_frame(g, char_to_index, flat, scale) for g in frames]
    gif_frames[0].save(
        out, format="GIF", save_all=True, append_images=gif_frames[1:],
        duration=durations, loop=0, transparency=0, disposal=2, optimize=False)
    w, h = dims[0] * scale, dims[1] * scale
    note = f"pixelart: wrote {out} ({len(gif_frames)} frames, {w}x{h})"
    if poster:                        # final-frame poster for prefers-reduced-motion
        render_rgba(frames[-1], palette, scale).save(poster, format="PNG")
        note += f" + poster {poster}"
    return note


def main(argv=None):
    ap = argparse.ArgumentParser(
        description="Render pixel art (static PNG or animated GIF) from a JSON spec.")
    ap.add_argument("--spec", help="path to a JSON spec file (default: stdin)")
    ap.add_argument("--out", required=True, help="output .png (grid) or .gif (frames)")
    ap.add_argument("--poster", help="also write a static PNG poster of the final frame")
    args = ap.parse_args(argv)

    try:
        raw = open(args.spec, encoding="utf-8").read() if args.spec else sys.stdin.read()
        spec = json.loads(raw)
    except (OSError, json.JSONDecodeError) as e:
        print(f"pixelart: could not read spec: {e}", file=sys.stderr)
        return 2

    try:
        note = render(spec, args.out, args.poster)
    except SpecError as e:
        print(f"pixelart: bad spec: {e}", file=sys.stderr)
        return 2

    print(note)
    return 0


if __name__ == "__main__":
    sys.exit(main())
