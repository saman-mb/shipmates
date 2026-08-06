#!/usr/bin/env python3
"""pixelart — render pixel art (static PNG or animated GIF) from a JSON spec.

Self-contained: its only dependency is Pillow, which it installs for itself on
first run (into a private cache) if missing — see `_ensure_pillow` below — so a
plain `python3 pixelart.py` works with nothing to set up. This is the runnable payload of
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


# The Pillow version this tool pins itself to. Pinning is what makes output
# byte-reproducible: a floating version renders differently host to host. Pillow
# 12.3.0 is a current stable release that supports every API this tool calls
# (Image.new/frombytes/putpalette/resize with Image.NEAREST, and PNG + animated
# transparent GIF save) and does NOT rely on anything Pillow 10 removed
# (ANTIALIAS / textsize / getsize). Bump deliberately and only after re-verifying
# those calls.
_PILLOW_VERSION = "12.3.0"


def _ensure_pillow():
    """Make the *pinned* Pillow importable with no separate install by the user.

    The pinned version lives in a version-namespaced cache dir,
    `~/.cache/shipmates/pylib/Pillow-<version>/`, which is placed at the FRONT of
    `sys.path` so it is authoritative — a differently-versioned system Pillow can
    no longer shadow it (that shadowing is what falsified byte-identical claims,
    and the per-version dir means two tools needing different Pillows never
    clobber one flat `PIL/`). If the pinned cache is absent it is provisioned once
    with pip; later runs reuse it. Needs the network the first time only.

    If the pinned version cannot be provisioned (no pip, or offline) but a system
    Pillow is importable, it falls back to that and warns on stderr that output
    may not be byte-reproducible. It hard-fails only when no Pillow exists at all.
    """
    import os
    root = os.environ.get("XDG_CACHE_HOME") or os.path.join(os.path.expanduser("~"), ".cache")
    libdir = os.path.join(root, "shipmates", "pylib", "Pillow-" + _PILLOW_VERSION)

    def _import_from_pinned():
        """Put the versioned cache first, drop any stale PIL, import; return the
        module iff it actually resolves from the pinned dir, else None."""
        import importlib
        while libdir in sys.path:
            sys.path.remove(libdir)
        sys.path.insert(0, libdir)
        for name in [m for m in sys.modules if m == "PIL" or m.startswith("PIL.")]:
            del sys.modules[name]
        importlib.invalidate_caches()
        try:
            import PIL
        except ImportError:
            return None
        if os.path.abspath(getattr(PIL, "__file__", "")).startswith(os.path.abspath(libdir) + os.sep):
            return PIL
        return None

    if _import_from_pinned() is not None:
        return
    import subprocess
    os.makedirs(libdir, 0o700, exist_ok=True)  # user-private: the cache is front of sys.path
    pinned = "Pillow==" + _PILLOW_VERSION
    installed = False
    for pip_cmd in ([sys.executable, "-m", "pip"], ["pip3"], ["pip"]):
        try:
            # --only-binary=:all: forbids sdists, so no install-time setup.py runs.
            r = subprocess.run(pip_cmd + ["install", "--target", libdir, "--quiet",
                                          "--only-binary=:all:",
                                          "--disable-pip-version-check", pinned],
                               stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        except (FileNotFoundError, OSError):
            continue
        if r.returncode == 0:
            installed = True
            break
    if installed and _import_from_pinned() is not None:
        return

    # Honest fallback: the pinned cache could not be provisioned. Prefer a system
    # Pillow so the tool still works out of the box, but say so on stderr.
    import importlib
    while libdir in sys.path:
        sys.path.remove(libdir)
    for name in [m for m in sys.modules if m == "PIL" or m.startswith("PIL.")]:
        del sys.modules[name]
    importlib.invalidate_caches()
    try:
        import PIL
    except ImportError:
        PIL = None
    if PIL is not None:
        sys.stderr.write(
            "pixelart: warning: could not provision the pinned Pillow {} "
            "(no pip, or offline); using system Pillow {} instead — output may "
            "not be byte-reproducible without the pinned version.\n".format(
                _PILLOW_VERSION, getattr(PIL, "__version__", "?")))
        return
    sys.exit("pixelart: needs Pillow and could not install it automatically "
             "(no pip found, or offline) and no system Pillow is importable. "
             "Run: python3 -m pip install 'Pillow=={}'".format(_PILLOW_VERSION))


_ensure_pillow()
from PIL import Image, PngImagePlugin


def _save_png(img, path):
    """Write a PNG with no ancillary metadata so repeated runs are byte-identical.

    An explicit empty PngInfo blocks any inherited text chunks, and Pillow writes
    no tIME (verified: the pinned build emits only IHDR/IDAT/IEND), so the same
    grid always hashes the same. Pairs with the pinned Pillow version above.
    """
    img.save(path, format="PNG", pnginfo=PngImagePlugin.PngInfo())


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
        _save_png(img, out)
        note = f"pixelart: wrote {out} ({img.width}x{img.height})"
        if poster:                    # a static poster is just the same image
            _save_png(img, poster)
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
    # No comment/time metadata is passed, and the pinned Pillow writes none of
    # its own, so the same frames always produce a byte-identical GIF.
    gif_frames[0].save(
        out, format="GIF", save_all=True, append_images=gif_frames[1:],
        duration=durations, loop=0, transparency=0, disposal=2, optimize=False)
    w, h = dims[0] * scale, dims[1] * scale
    note = f"pixelart: wrote {out} ({len(gif_frames)} frames, {w}x{h})"
    if poster:                        # final-frame poster for prefers-reduced-motion
        _save_png(render_rgba(frames[-1], palette, scale), poster)
        note += f" + poster {poster}"
    return note


def main(argv=None):
    ap = argparse.ArgumentParser(
        description="Render pixel art (static PNG or animated GIF) from a JSON spec.")
    ap.add_argument("--spec", help="path to a JSON spec file (default: stdin)")
    ap.add_argument("--out", help="output .png (grid) or .gif (frames)")
    ap.add_argument("--poster", help="also write a static PNG poster of the final frame")
    ap.add_argument("--provision", action="store_true",
                    help="ensure runtime dependencies are installed, then exit (used at install time)")
    args = ap.parse_args(argv)

    if args.provision:
        # _ensure_pillow() ran on import and placed the pinned Pillow bytes into
        # the version-namespaced cache (or fell back with a stderr warning).
        from PIL import __version__ as _pil_version
        print("pixelart: ready (Pillow {}, pinned {})".format(_pil_version, _PILLOW_VERSION))
        return 0
    if not args.out:
        print("pixelart: --out is required", file=sys.stderr)
        return 2

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
