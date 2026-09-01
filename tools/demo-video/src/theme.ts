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
  /**
   * Beat-4 PR-command line. The gate size (72px) pushes the
   * `gh pr create … "…/pull/87"` line past the 1920px right edge; 52px keeps
   * it inside the frame (53 mono chars × 0.6em × 52 ≈ 1654px + margins).
   */
  gateUrl: 52,
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

/* ------------------------------------------------------------------ */
/* Layout bands (1920-wide render).                                     */
/*                                                                      */
/* The band below the titlebar is reserved for the two overlays so      */
/* neither ever strikes a terminal line. Every overlay renders at an    */
/* explicit lineHeight of 1.2 so these extents are deterministic:       */
/*                                                                      */
/*   titlebar     0      – 64                                           */
/*   checklist    80     – 123.2   (CHECKLIST_TOP + 36 × 1.2)           */
/*   caption      132    – 225.6   (CAPTION_TOP + 48 × 1.2 + 36 pad)    */
/*   body content 236    – …       (TERMINAL_CONTENT_TOP)               */
/* ------------------------------------------------------------------ */

/** Window titlebar (traffic lights + label) height. */
export const TERMINAL_TITLEBAR_HEIGHT = 64;
/** Phase checklist top — first row of the reserved header band, top-right. */
export const CHECKLIST_TOP = 80;
/** Caption plate top — second row of the reserved header band, top-centre. */
export const CAPTION_TOP = 132;
/** Y where terminal body content begins, below the reserved header band. */
export const TERMINAL_CONTENT_TOP = 236;
