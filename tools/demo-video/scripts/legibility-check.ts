/**
 * Legibility gate — by-construction assertion, per docs/DEMO_VIDEO.md §6 TODO:
 * "verify legibility at phone-feed scale (≥20 px effective glyph height)".
 *
 * 1. Computes each content font size's effective glyph height at LinkedIn-feed
 *    scale (1920 → 1080, factor 0.5625) from theme.ts constants and fails if
 *    any drops below MIN_FEED_GLYPH_PX.
 * 2. Renders a Beat3 still at feed scale to prove the frame actually produces.
 *
 * Exits nonzero on any violation.
 */
import { execSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import {
  effectiveGlyphHeight,
  FONT_SIZES,
  MIN_FEED_GLYPH_PX,
} from '../src/theme';

let failed = false;

for (const [name, sizePx] of Object.entries(FONT_SIZES)) {
  const effective = effectiveGlyphHeight(sizePx);
  if (effective < MIN_FEED_GLYPH_PX) {
    console.error(
      `FAIL ${name}: ${sizePx}px → ${effective.toFixed(2)}px at feed scale (< ${MIN_FEED_GLYPH_PX}px)`,
    );
    failed = true;
  } else {
    console.log(
      `ok   ${name}: ${sizePx}px → ${effective.toFixed(2)}px at feed scale (≥ ${MIN_FEED_GLYPH_PX}px)`,
    );
  }
}

if (failed) {
  console.error(
    'Legibility violation: bump the size(s) in src/theme.ts FONT_SIZES.',
  );
  process.exit(1);
}

// Prove the feed-scale still actually renders (downloads Chrome Headless Shell
// on first run — see README.md).
const OUT = 'out/beat3-feed.png';
execSync(
  'npx remotion still src/index.ts Beat3Phases out/beat3-feed.png --scale=0.5625',
  { stdio: 'inherit' },
);
if (!existsSync(OUT)) {
  console.error(`FAIL: expected still at ${OUT} but none was produced.`);
  process.exit(1);
}
console.log(`Legibility check passed — still at ${OUT}.`);
