# Shipmates demo video — Remotion scaffold (beats 1–7)

Programmatic animation for the 45s LinkedIn demo clip — all seven beats.
Visual spec and storyboard: [`docs/DEMO_VIDEO.md`](../../docs/DEMO_VIDEO.md)
(§2 beats 1–7, §4 typography/palette).

## Compositions (1920×1080 @ 30 fps)

| id             | Beat                 | Window     | Frames |
|----------------|----------------------|------------|--------|
| `Beat1Hook`    | Hook                 | 0:00–0:03  | 90     |
| `Beat2Crew`    | What it is (crew)    | 0:03–0:09  | 180    |
| `Beat3Phases`  | Phases run (hero)    | 0:09–0:22  | 390    |
| `Beat4Gates`   | Gates (CI + board)   | 0:22–0:29  | 210    |
| `Beat5PR`      | Epic timelapse       | 0:29–0:38  | 270    |
| `Beat6Close`   | Close (pull-back)    | 0:38–0:41  | 90     |
| `Beat7EndCard` | End card (hold 4s)   | 0:41–0:45  | 120    |

All beat copy and intra-beat frame timings live in `src/data/beats.ts`
(transcribed from DEMO_VIDEO.md §2). Palette, glyphs, fonts, type-on rate and
font sizes live in `src/theme.ts` — the single source of truth. Components are
dumb: `Terminal` (window chrome + lines), `TypeOn` (pure frame→charCount
timing), `PhaseChecklist` (pinned top-right, ✓/green only from theme),
`Tick` (SFX), `Caption` (top-centre plate), `TerminalInset` (beat-5 corner
chrome at 0.25 scale — the full renderer, just smaller), `PRTimelapse`
(beat-5 epic-PR mock driven by one pure tick-clock `beat5TickIndex`),
`TileGrid` (beat-6 pull-back grid), `EndCard` (beat-7 card).

Design decisions (ADR-style):

- **Beat 6 tile technique:** the pull-back animates a live 4×3 grid of
  simplified terminal-mock tiles via CSS transform (scale 2.8→1, offset→0,
  eased cubic) — no frame sampling of other compositions, so every rendered
  frame stays crisp text-free vector geometry and the render stays
  deterministic.
- **Beat 7 logo:** the end card reuses the landing site's pixel-art sailboat
  (`site/assets/logo.png`, copied to `public/logo.png`, rendered with
  `image-rendering: pixelated`) instead of drawing a second mark — one brand
  asset, two surfaces.

## Commands

```bash
pnpm install        # first install only
pnpm typecheck      # tsc --noEmit, strict
pnpm render         # renders beat1–beat4 MP4s into out/
pnpm stills         # renders the Beat3 feed-scale still into out/
pnpm legibility     # asserts ≥20px effective glyph height at feed scale,
                    # then renders the Beat3 still as proof; exits nonzero
                    # on violation
```

Beats 5–7 render the same way, per composition:

```bash
npx remotion render src/index.ts Beat5PR out/beat5.mp4
npx remotion render src/index.ts Beat6Close out/beat6.mp4
npx remotion render src/index.ts Beat7EndCard out/beat7.mp4
```

Single-composition commands (what the scripts wrap):

```bash
npx remotion render src/index.ts Beat3Phases out/beat3.mp4
npx remotion still  src/index.ts Beat3Phases out/beat3-feed.png --scale=0.5625
```

## First render: Chrome Headless Shell download

Remotion downloads **Chrome Headless Shell (~150 MB)** on the very first
render/still on a machine. That is a one-time cost; subsequent renders are
offline. Don't be alarmed by the download progress on first run.

## Fonts & licensing

- **JetBrains Mono** — primary terminal font. Licensed under the **SIL Open
  Font License 1.1**, which permits bundling/redistribution. Loaded via
  `@remotion/google-fonts/JetBrainsMono` in `src/Root.tsx`.
- **IBM Plex Sans** — caption font (DEMO_VIDEO.md §4). Also OFL, same loading
  path.
- **Berkeley Mono** — the storyboard's *ideal* terminal font, but it is
  **license-restricted and must never be committed to this repository**. It is
  supported as a local-only fallback: if you have a licensed copy, install it
  locally and put it first in `FONT_MONO` in `src/theme.ts`; the stack falls
  back to JetBrains Mono for everyone else.

## Legibility rule (why 36px terminal text)

The clip is judged at LinkedIn feed scale: 1920-wide frames downscale to
1080 (factor **0.5625**). DEMO_VIDEO.md §6 requires ≥ **20 px effective glyph
height** there, so terminal text is rendered at **36 px** (36 × 0.5625 =
20.25 px). `scripts/legibility-check.ts` asserts this by construction from the
`FONT_SIZES` constants in `src/theme.ts` — bump a size down without updating
the check and CI-style verification fails.

## SFX placeholder

`public/tick.mp3` is a **PLACEHOLDER**: a generated 30 ms near-silence (1 kHz
tone at −34 dBFS) standing in for the real "soft mechanical tick" SFX from
DEMO_VIDEO.md §5. Real SFX are pending. When they land, replace the file in
`public/` and adjust the volume in `src/components/Tick.tsx`.

## Remotion license

Remotion is free for individuals and teams of **3 or fewer people**; larger
organizations need a company license (https://remotion.dev/license). This
scaffold uses Remotion 4.0.x pinned exactly across `remotion`,
`@remotion/cli` and `@remotion/google-fonts`.
