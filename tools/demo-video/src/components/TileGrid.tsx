import React from 'react';
import {
  AbsoluteFill,
  Easing,
  interpolate,
  useCurrentFrame,
} from 'remotion';
import { BEAT6 } from '../data/beats';
import { COLORS } from '../theme';

/* Tile chrome sizing — simplified window mocks, no text inside. */
const TILE_GAP = 24;
const GRID_PAD = 48;
const TILE_TITLEBAR_HEIGHT = 28;
const TILE_DOT = 8;
const DIFF_LINE_HEIGHT = 10;
const DIFF_LINES_PER_TILE = 4;
const TILE_PAD = 20;
const DIFF_LINE_GAP = 12;

/** Deterministic dim diff-line width per tile/line (no randomness). */
const diffWidthPct = (tile: number, line: number): number =>
  35 + ((tile * 7 + line * 13) % 45);

/**
 * Beat-6 tile grid: ~4×3 simplified terminal-window mocks (titlebar + dim
 * diff lines, built from theme constants) pulled back via transform —
 * scale 2.8→1 and offset→0 over BEAT6.pull.frames, easing out cubic.
 * Settled (static) after the pull completes. No frame sampling of other
 * compositions; the tiles are live mocks.
 */
export const TileGrid: React.FC = () => {
  const frame = useCurrentFrame();
  const { frames, scale, offset } = BEAT6.pull;
  const ease = Easing.out(Easing.cubic);
  const clamp = {
    extrapolateLeft: 'clamp',
    extrapolateRight: 'clamp',
  } as const;
  const s = interpolate(frame, [...frames], [...scale], { easing: ease, ...clamp });
  const offsetX = interpolate(frame, [...frames], [offset.x, 0], {
    easing: ease,
    ...clamp,
  });
  const offsetY = interpolate(frame, [...frames], [offset.y, 0], {
    easing: ease,
    ...clamp,
  });

  const tiles = Array.from(
    { length: BEAT6.grid.cols * BEAT6.grid.rows },
    (_, tile) => (
      <div
        key={tile}
        style={{
          backgroundColor: COLORS.bg,
          border: `1px solid ${COLORS.dim}33`,
          borderRadius: 8,
          overflow: 'hidden',
          display: 'flex',
          flexDirection: 'column',
        }}
      >
        {/* Simplified titlebar — traffic dots only, no label text */}
        <div
          style={{
            height: TILE_TITLEBAR_HEIGHT,
            flexShrink: 0,
            display: 'flex',
            alignItems: 'center',
            gap: 6,
            paddingLeft: 10,
            borderBottom: `1px solid ${COLORS.dim}22`,
          }}
        >
          {[0, 1, 2].map((dot) => (
            <div
              key={dot}
              style={{
                width: TILE_DOT,
                height: TILE_DOT,
                borderRadius: TILE_DOT / 2,
                backgroundColor: COLORS.dim,
                opacity: 0.6,
              }}
            />
          ))}
        </div>
        {/* Dim diff lines */}
        <div
          style={{
            padding: TILE_PAD,
            display: 'flex',
            flexDirection: 'column',
            gap: DIFF_LINE_GAP,
          }}
        >
          {Array.from({ length: DIFF_LINES_PER_TILE }, (_, line) => (
            <div
              key={line}
              style={{
                height: DIFF_LINE_HEIGHT,
                width: `${diffWidthPct(tile, line)}%`,
                backgroundColor: COLORS.dim,
                opacity: 0.3,
                borderRadius: 2,
              }}
            />
          ))}
        </div>
      </div>
    ),
  );

  return (
    <AbsoluteFill style={{ backgroundColor: COLORS.bg }}>
      <div
        style={{
          position: 'absolute',
          inset: 0,
          display: 'grid',
          gridTemplateColumns: `repeat(${BEAT6.grid.cols}, 1fr)`,
          gridTemplateRows: `repeat(${BEAT6.grid.rows}, 1fr)`,
          gap: TILE_GAP,
          padding: GRID_PAD,
          transform: `translate(${offsetX}px, ${offsetY}px) scale(${s})`,
          transformOrigin: 'center center',
        }}
      >
        {tiles}
      </div>
    </AbsoluteFill>
  );
};
