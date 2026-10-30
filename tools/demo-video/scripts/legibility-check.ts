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
import { existsSync, readFileSync } from 'node:fs';
import {
  effectiveGlyphHeight,
  FEED_SCALE,
  FONT_SIZES,
  MIN_FEED_GLYPH_PX,
} from '../src/theme';
import { assertTypeOnWindow } from '../src/components/TypeOn';
import { validateBeatTypeOnWindows } from '../src/data/beats';

// TypeOn over-length guard (#369): validate all beat type-on windows
validateBeatTypeOnWindows();
console.log('ok   TypeOn windows: all beat copy fits inside allocated frame windows.');

// Negative self-check (#369): assertTypeOnWindow must fail loudly on over-length copy
let threw = false;
try {
  assertTypeOnWindow(
    'This storyboard string is way too long to fit in five frames',
    5,
    30,
    30,
    'NegativeCheck',
  );
} catch {
  threw = true;
}
if (!threw) {
  console.error(
    'FAIL: assertTypeOnWindow did not throw on over-length string.',
  );
  process.exit(1);
}
console.log(
  'ok   assertTypeOnWindow: negative self-check caught over-length copy as expected.',
);

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
// on first run — see README.md). The scale must equal theme FEED_SCALE; the
// package.json literal is asserted so the two can never drift.
const pkg = JSON.parse(readFileSync('package.json', 'utf8')) as {
  scripts: Record<string, string>;
};
const stillsScript = pkg.scripts.stills ?? '';
const scaleLiteral = `--scale=${FEED_SCALE}`;
if (!stillsScript.includes(scaleLiteral)) {
  console.error(
    `FAIL: package.json "stills" script must carry ${scaleLiteral} (drift from theme FEED_SCALE).`,
  );
  process.exit(1);
}
const OUT = 'out/beat3-feed.png';
execSync(
  `npx remotion still src/index.ts Beat3Phases out/beat3-feed.png ${scaleLiteral}`,
  { stdio: 'inherit' },
);
if (!existsSync(OUT)) {
  console.error(`FAIL: expected still at ${OUT} but none was produced.`);
  process.exit(1);
}
console.log(`Legibility check passed — still at ${OUT}.`);
