import React from 'react';
import {
  AbsoluteFill,
  Easing,
  Img,
  interpolate,
  staticFile,
  useCurrentFrame,
} from 'remotion';
import { BEAT7 } from '../data/beats';
import { COLORS, FONT_MONO, FONT_SIZES } from '../theme';

/** End-card logo size (px). The asset is a 672×672 pixel-art sailboat. */
const LOGO_SIZE = 220;

/**
 * Beat-7 end card (docs/DEMO_VIDEO.md §2 beat 7): pixel-art sailboat logo,
 * repo + site URLs at the gateUrl size, dim install line beneath.
 * Entry fade/slide completes by BEAT7.settleFrame (f60); every frame after is
 * pixel-identical so the 4s hold is loop-friendly.
 */
export const EndCard: React.FC = () => {
  const frame = useCurrentFrame();
  const enter = interpolate(frame, [0, BEAT7.settleFrame], [0, 1], {
    easing: Easing.out(Easing.cubic),
    extrapolateLeft: 'clamp',
    extrapolateRight: 'clamp',
  });

  return (
    <AbsoluteFill
      style={{
        backgroundColor: COLORS.bg,
        fontFamily: FONT_MONO,
        display: 'flex',
        flexDirection: 'column',
        alignItems: 'center',
        justifyContent: 'center',
        gap: 36,
        opacity: enter,
        transform: `translateY(${(1 - enter) * 48}px)`,
      }}
    >
      <Img
        src={staticFile(BEAT7.logoFile)}
        width={LOGO_SIZE}
        height={LOGO_SIZE}
        style={{ imageRendering: 'pixelated' }}
      />
      <div
        style={{
          fontSize: FONT_SIZES.gateUrl,
          color: COLORS.text,
          lineHeight: 1.4,
          textAlign: 'center',
          whiteSpace: 'pre',
        }}
      >
        {`${BEAT7.repoUrl}\n${BEAT7.siteUrl}`}
      </div>
      <div style={{ fontSize: FONT_SIZES.terminal, color: COLORS.dim }}>
        {BEAT7.installLine}
      </div>
    </AbsoluteFill>
  );
};
