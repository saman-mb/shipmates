import React from 'react';
import { AbsoluteFill } from 'remotion';
import {
  COLORS,
  FONT_MONO,
  FONT_SIZES,
  TERMINAL_CONTENT_TOP,
  TERMINAL_GUTTER,
  TERMINAL_TITLEBAR_HEIGHT,
  type ColorKey,
} from '../theme';

/**
 * Dumb terminal chrome: dark full-screen card + window bar with three dots.
 * Renders the `lines` prop as-is; any overlays (checklist, stamps, carets)
 * come in via children. No globals, no timing logic, no storyboard copy.
 */
export type TerminalLine = {
  text: string;
  colorKey?: ColorKey;
  bold?: boolean;
};

export const Terminal: React.FC<{
  lines: TerminalLine[];
  title?: string;
  fontSize?: number;
  children?: React.ReactNode;
}> = ({ lines, title = 'shipmates', fontSize = FONT_SIZES.terminal, children }) => {
  return (
    <AbsoluteFill style={{ backgroundColor: COLORS.bg, fontFamily: FONT_MONO }}>
      {/* Window bar */}
      <div
        style={{
          position: 'absolute',
          top: 0,
          left: 0,
          right: 0,
          height: TERMINAL_TITLEBAR_HEIGHT,
          display: 'flex',
          alignItems: 'center',
          paddingLeft: 32,
          gap: 14,
          borderBottom: `1px solid ${COLORS.dim}33`,
        }}
      >
        {[0, 1, 2].map((i) => (
          <div
            key={i}
            style={{
              width: 20,
              height: 20,
              borderRadius: 10,
              backgroundColor: COLORS.dim,
              opacity: 0.6,
            }}
          />
        ))}
        <div
          style={{
            marginLeft: 24,
            color: COLORS.dim,
            fontSize: 22,
            letterSpacing: 1,
          }}
        >
          {title}
        </div>
      </div>

      {/* Terminal body — starts at TERMINAL_CONTENT_TOP so the caption and
          checklist own the header band above it (see theme.ts band math). */}
      <div
        style={{
          position: 'absolute',
          top: TERMINAL_TITLEBAR_HEIGHT,
          left: 0,
          right: 0,
          bottom: 0,
          paddingTop: TERMINAL_CONTENT_TOP - TERMINAL_TITLEBAR_HEIGHT,
          paddingLeft: TERMINAL_GUTTER,
          paddingRight: TERMINAL_GUTTER,
          paddingBottom: 48,
        }}
      >
        {lines.map((line, i) => (
          <div
            key={i}
            style={{
              fontSize,
              lineHeight: 1.5,
              color: COLORS[line.colorKey ?? 'text'],
              fontWeight: line.bold ? 700 : 400,
              whiteSpace: 'pre',
            }}
          >
            {line.text}
          </div>
        ))}
      </div>

      {children}
    </AbsoluteFill>
  );
};
