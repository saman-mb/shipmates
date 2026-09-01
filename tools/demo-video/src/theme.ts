/**
 * SINGLE SOURCE OF TRUTH for the demo video's visual language.
 * Governs palette, glyphs, fonts, type-on cadence and type sizes for all beats.
 * Every colour literal, ✓/✗ glyph and font-size decision lives HERE and only here.
 * Visual spec: docs/DEMO_VIDEO.md §4 (typography & palette).
 */

/** GitHub-native palette (docs/DEMO_VIDEO.md §4). Never hardcode these hex values elsewhere. */
export const COLORS = {
  bg: '#0D1117',
  text: '#E6EDF3',
  dim: '#8B949E',
  green: '#3FB950', // accent: success / shipped / CTA only (docs/DEMO_VIDEO.md §4)
  blue: '#58A6FF', // commands
  red: '#F85149', // failure — used once (beat 1 strike, beat 3 test fail)
} as const;

export type ColorKey = keyof typeof COLORS;

/** Success/checkmark glyph. The one ✓ — components must import it, never inline it. */
export const CHECK = '✓';
/** Failure glyph — the beat-1 strike and the beat-3 test fail. */
export const FAIL = '✗';
/** Command prompt / result arrow. */
export const PROMPT = '❯';
export const ARROW = '→';

/**
 * Type-on rate: 30 chars/s (docs/DEMO_VIDEO.md §2 "types fast").
 * Injected into every typing surface; tests/stills may override.
 */
export const TYPE_ON_CHARS_PER_SECOND = 30;

/**
 * Font stack. Primary: JetBrains Mono (SIL OFL 1.1 — license permits bundling).
 * Loaded via @remotion/google-fonts/JetBrainsMono in Root.tsx.
 * Berkeley Mono (the storyboard's ideal) is license-restricted — it may be used
 * as a LOCAL-ONLY fallback on machines that have it licensed, but it must never
 * be committed to this repository. See README.md ("Fonts & licensing").
 */
export const FONT_MONO = '"JetBrains Mono", ui-monospace, monospace';
/** Captions per §4: IBM Plex Sans Bold (OFL, same loading path). */
export const FONT_SANS = '"IBM Plex Sans", ui-sans-serif, sans-serif';

/**
 * Type sizes at 1920-wide render. Constraint (docs/DEMO_VIDEO.md §6 TODO):
 * effective glyph height must be ≥ 20px when the frame is downscaled to a
 * phone feed width (1080px → scale 0.5625). Hence terminal text is ≥ 36px
 * here (36 × 0.5625 = 20.25px). scripts/legibility-check.ts enforces this
 * by construction.
 */
export const FEED_SCALE = 1080 / 1920; // 0.5625
export const MIN_FEED_GLYPH_PX = 20;

export const FONT_SIZES = {
  /** Terminal body text — the legibility floor. */
  terminal: 36,
  /** Phase checklist pinned top-right — same floor as terminal text. */
  checklist: 36,
  /** Top-centre captions on the 40%-black plate. */
  caption: 48,
  /** Oversized gate results (beat 4). */
  gate: 72,
  /** Beat-1 hook line (typed prose + strike-through). */
  hook: 48,
  /** Beat-1 command stamp (`/ship-issue 42`). */
  stamp: 56,
} as const;

export type ContentFontSizeKey = keyof typeof FONT_SIZES;

/** Effective glyph height at LinkedIn-feed scale for a given 1920-wide font size. */
export const effectiveGlyphHeight = (fontSizePx: number): number =>
  fontSizePx * FEED_SCALE;

/** Caption plate backdrop: 40% black (docs/DEMO_VIDEO.md §4). */
export const CAPTION_PLATE_BG = 'rgba(0, 0, 0, 0.4)';
