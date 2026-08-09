---
name: pixelart
description: Render pixel art — a static PNG or an animated GIF — from a small JSON spec of a character grid and a limited palette, upscaled by a whole-number factor with nearest-neighbour and no smoothing. This is the technique behind the shipmates logo, made reusable. Reach for it whenever a task calls for a small hard-edged asset in that style — an icon, sprite, favicon, badge, or twinkling animation — rather than a photographic image or a vector. Never a slash command; the crew reach for it implicitly when the intent calls for one.
requires: pillow
---

# pixelart

A tool the crew uses on its own. When you're producing an asset and the natural
artifact is a small piece of *pixel art* — a hard-edged icon, sprite, favicon,
or badge, or a short looping animation of one — render it with this instead of
reaching for a photo, a vector, or a smooth-scaled bitmap.

It reproduces the technique behind the shipmates mark. `site/assets/logo.png` is
a 48×48 logical grid on a deliberately limited palette (~39 colours), upscaled
×14 to 672×672 with nearest-neighbour and no smoothing. BRAND.md states the rule
this tool enforces: **"Never resample with smoothing. Scale by whole-number
factors and use nearest-neighbour."** Every upscale here is `Image.NEAREST`, so
each logical pixel lands as a solid `scale`×`scale` block — chunky and aliased on
purpose, never a gradient.

## Run it

The renderer `pixelart.py` sits next to this file. It needs Pillow, which it
installs for itself the first time it runs — you never have to install anything.
It pins one Pillow version and caches it per-version under
`~/.cache/shipmates/pylib/Pillow-<version>/`, placed first on the import path so
that pinned build is authoritative (a differently-versioned system Pillow can't
shadow it) — this is what keeps the same spec byte-reproducible run to run.
Because that cache dir sits first on the import path it is created user-private
(`0700`) and must stay trusted — anything planted there would be loaded. If
the pinned version can't be provisioned (no pip, or offline) it falls back to a
system Pillow and warns on stderr that output may not be byte-reproducible.

```
# static PNG — the spec has a "grid"
python3 pixelart.py --spec spec.json --out art.png

# animated GIF — the spec has "frames"; --poster also writes a static
# final-frame PNG for prefers-reduced-motion
python3 pixelart.py --spec spec.json --out art.gif --poster art_poster.png

# the spec can also arrive on stdin
cat spec.json | python3 pixelart.py --out art.png
```

## Spec

A palette maps **single characters** to colours; a **grid** (or a list of
**frames**) draws with them. Rows must all be the same length, and every
character in the art must be defined in the palette.

```json
{
  "scale": 16,
  "palette": {
    ".": "#00000000",
    "#": "#1A1F36",
    "o": "#FFC24A"
  },
  "grid": [
    "..####..",
    ".#oooo#.",
    "#oooooo#",
    "#oooooo#",
    ".#oooo#.",
    "..####.."
  ]
}
```

Colours are hex: `#RGB`, `#RGBA`, `#RRGGBB`, or 8-digit `#RRGGBBAA` for
per-pixel alpha. The words `none` / `transparent` (and a lone `.`) also mean
fully transparent — a convention worth keeping for the "empty" character, as
above. Output is RGBA and honours transparency.

Animate by giving `frames` (a list of grids, all the same size) instead of a
single `grid`:

```json
{
  "scale": 16,
  "palette": { ".": "none", "#": "#1A1F36", "o": "#FFC24A" },
  "frames": [
    ["..#..", ".###.", "#####", ".###.", "..#.."],
    [".....", "..#..", ".###.", "..#..", "....."]
  ],
  "durations": [220, 160]
}
```

`durations` are milliseconds — one number for every frame, or a per-frame list.
Omit it for ~200ms. The GIF loops, uses frame disposal so transparency shows
between frames, and carries a transparent index.

## Treatment

The same discipline the shipmates logo is held to — this is what makes pixel art
read as pixel art rather than a blurry thumbnail.

- **Nearest-neighbour, no smoothing.** The tool upscales with `Image.NEAREST`
  only; there is no path that resamples with interpolation. A blurred pixel-art
  asset is a broken one.
- **Whole-number scale.** `scale` is an integer. A 24-px grid at `scale` 16 is a
  384-px asset with every logical pixel a crisp 16-px block. Fractional scaling
  is rejected.
- **Limited palette.** Keep it small and deliberate — the logo lives on ~39
  colours. A handful of well-chosen hues with hand-placed shading beats a wide
  gradient every time.

## Honesty

Draw the asset the task actually needs, at a size that suits its use — a favicon
is tiny, a hero sprite is larger. For anything that carries meaning, hand the
caller alt text; and where motion is decorative, pair a GIF with the static
`--poster` PNG so a reduced-motion reader still gets the image.
