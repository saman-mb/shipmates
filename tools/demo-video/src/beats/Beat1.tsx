import React from 'react';
import { interpolate, spring, useCurrentFrame, useVideoConfig } from 'remotion';
import { Caption } from '../components/Caption';
import { Terminal } from '../components/Terminal';
import { TypeOn } from '../components/TypeOn';
import { BEAT1 } from '../data/beats';
import { COLORS, FONT_SIZES, TERMINAL_CONTENT_TOP, TERMINAL_GUTTER } from '../theme';

/**
 * Beat 1 — Hook · 0:00–0:03 (docs/DEMO_VIDEO.md §2).
 * The hook line types → red strike-through → the green command stamp.
 * Copy lives in data/beats.ts BEAT1.
 */
export const Beat1: React.FC = () => {
  const frame = useCurrentFrame();
  const { fps } = useVideoConfig();
  const { typeStartFrame, strikeStartFrame, strikeDurationFrames, stampFrame } =
    BEAT1;

  // Animated red strike sweeps across the hook line.
  const strikeWidth = interpolate(
    frame,
    [strikeStartFrame, strikeStartFrame + strikeDurationFrames],
    [0, 100],
    { extrapolateLeft: 'clamp', extrapolateRight: 'clamp' },
  );
  const strikeOpacity = frame >= strikeStartFrame ? 1 : 0;

  // Stamp: bold green command slams in with a spring.
  const stampScale = spring({
    frame: frame - stampFrame,
    fps,
    config: { damping: 12, stiffness: 160, mass: 0.6 },
  });
  const stampVisible = frame >= stampFrame;

  return (
    <Terminal lines={[]}>
      {/* Below the reserved header band — the hook must never enter the
          titlebar/caption zone (theme.ts band math). Aligned to the same
          gutter as every other beat's terminal lines. */}
      <div
        style={{
          paddingTop: TERMINAL_CONTENT_TOP,
          paddingLeft: TERMINAL_GUTTER,
          paddingRight: TERMINAL_GUTTER,
        }}
      >
        <div
          style={{ position: 'relative', fontSize: FONT_SIZES.hook, lineHeight: 1.6 }}
        >
          <div style={{ whiteSpace: 'pre' }}>
            <TypeOn
              text={BEAT1.hookLine}
              startFrame={typeStartFrame}
              fontSize={FONT_SIZES.hook}
              showCaret={!stampVisible}
            />
          </div>
          {/* Strike-through overlay */}
          <div
            style={{
              position: 'absolute',
              top: '50%',
              left: 0,
              width: `${strikeWidth}%`,
              height: 5,
              background: COLORS.red,
              opacity: strikeOpacity,
            }}
          />
        </div>
        {stampVisible ? (
          <div
            style={{
              marginTop: 32,
              fontSize: FONT_SIZES.stamp,
              fontWeight: 700,
              color: COLORS.green,
              whiteSpace: 'pre',
              transform: `scale(${1 + (1 - stampScale) * 0.4})`,
              transformOrigin: 'left center',
            }}
          >
            {BEAT1.commandLine}
          </div>
        ) : null}
      </div>
      <Caption
        text={frame < BEAT1.captionFlipFrame ? BEAT1.captionA : BEAT1.captionB}
      />
    </Terminal>
  );
};
