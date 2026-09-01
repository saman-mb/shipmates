import React from 'react';
import { interpolate, useCurrentFrame } from 'remotion';
import {
  CAPTION_PLATE_BG,
  COLORS,
  FONT_SANS,
  FONT_SIZES,
} from '../theme';

/**
 * Top-centre caption: 2–4 words, bold sans, white-on-40%-black plate
 * (docs/DEMO_VIDEO.md §4). Copy comes in via props — never inline storyboard
 * strings here.
 */
export const Caption: React.FC<{ text: string; appearFrame?: number }> = ({
  text,
  appearFrame = 0,
}) => {
  const frame = useCurrentFrame();
  const opacity = interpolate(frame - appearFrame, [0, 8], [0, 1], {
    extrapolateLeft: 'clamp',
    extrapolateRight: 'clamp',
  });
  return (
    <div
      style={{
        position: 'absolute',
        top: 150,
        left: '50%',
        transform: 'translateX(-50%)',
        opacity,
        background: CAPTION_PLATE_BG,
        borderRadius: 12,
        padding: '18px 40px',
        fontFamily: FONT_SANS,
        fontWeight: 700,
        fontSize: FONT_SIZES.caption,
        color: COLORS.text,
        whiteSpace: 'nowrap',
      }}
    >
      {text}
    </div>
  );
};
