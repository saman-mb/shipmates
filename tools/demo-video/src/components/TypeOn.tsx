import React from 'react';
import { useCurrentFrame, useVideoConfig } from 'remotion';
import {
  COLORS,
  FONT_MONO,
  TYPE_ON_CHARS_PER_SECOND,
} from '../theme';

/**
 * Pure frame→charCount timing. No hooks, no state — the typed prefix for any
 * frame, from an injected chars/s rate (default from theme.ts).
 */
export const typedCharCount = (
  framesSinceStart: number,
  fps: number,
  textLength: number,
  charsPerSecond: number,
): number =>
  Math.min(
    textLength,
    Math.max(0, Math.floor((framesSinceStart / fps) * charsPerSecond)),
  );

/** Fully-typed state, for stamp/strike effects that key off completion. */
export const isFullyTyped = (
  framesSinceStart: number,
  fps: number,
  textLength: number,
  charsPerSecond: number,
): boolean =>
  typedCharCount(framesSinceStart, fps, textLength, charsPerSecond) >=
  textLength;

const CARET_BLINK_FRAMES = 15;
const CARET = '▍';

/**
 * Type-on text component: renders the typed prefix of `text` plus a blinking
 * caret until fully typed. Pure timing — chars/s injected, default 30 from
 * theme.ts. Exposes `typedCharCount`/`isFullyTyped` above for stamp effects.
 */
export const TypeOn: React.FC<{
  text: string;
  startFrame?: number;
  charsPerSecond?: number;
  color?: string;
  fontSize?: number;
  showCaret?: boolean;
  style?: React.CSSProperties;
}> = ({
  text,
  startFrame = 0,
  charsPerSecond = TYPE_ON_CHARS_PER_SECOND,
  color = COLORS.text,
  fontSize,
  showCaret = true,
  style,
}) => {
  const frame = useCurrentFrame();
  const { fps } = useVideoConfig();
  const framesSinceStart = frame - startFrame;
  const count = typedCharCount(
    framesSinceStart,
    fps,
    text.length,
    charsPerSecond,
  );
  const complete = isFullyTyped(
    framesSinceStart,
    fps,
    text.length,
    charsPerSecond,
  );
  const caretVisible =
    showCaret && (!complete || Math.floor(frame / CARET_BLINK_FRAMES) % 2 === 0);

  return (
    <span
      style={{
        fontFamily: FONT_MONO,
        color,
        whiteSpace: 'pre',
        fontSize,
        ...style,
      }}
    >
      {text.slice(0, count)}
      {caretVisible ? CARET : ''}
    </span>
  );
};
