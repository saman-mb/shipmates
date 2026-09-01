import React from 'react';
import { useVideoConfig } from 'remotion';
import { Terminal, type TerminalLine } from './Terminal';
import { COLORS, TERMINAL_INSET_SCALE } from '../theme';

/**
 * Beat-5 corner inset: the existing Terminal chrome at TERMINAL_INSET_SCALE,
 * pinned bottom-right. No fork of the renderer and no new layout math — the
 * full-size component renders inside a scaled box sized from useVideoConfig.
 */
export const TerminalInset: React.FC<{
  lines: TerminalLine[];
  title?: string;
}> = ({ lines, title }) => {
  const { width, height } = useVideoConfig();
  return (
    <div
      style={{
        position: 'absolute',
        right: 48,
        bottom: 48,
        width: width * TERMINAL_INSET_SCALE,
        height: height * TERMINAL_INSET_SCALE,
        overflow: 'hidden',
        border: `2px solid ${COLORS.dim}55`,
        borderRadius: 8,
        boxShadow: '0 8px 32px rgba(0, 0, 0, 0.5)',
      }}
    >
      <div
        style={{
          position: 'absolute',
          top: 0,
          left: 0,
          width,
          height,
          transform: `scale(${TERMINAL_INSET_SCALE})`,
          transformOrigin: 'top left',
        }}
      >
        <Terminal lines={lines} title={title} />
      </div>
    </div>
  );
};
