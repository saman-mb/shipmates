import React from 'react';
import { spring, useCurrentFrame, useVideoConfig } from 'remotion';
import { Terminal } from './Terminal';
import { TerminalInset } from './TerminalInset';
import {
  BEAT5,
  beat5TickIndex,
  beat5TickStartFrame,
} from '../data/beats';
import {
  CHECK,
  CHECKBOX_CHECKED,
  CHECKBOX_EMPTY,
  COLORS,
  FONT_SIZES,
  PROMPT,
  TERMINAL_CONTENT_TOP,
  TERMINAL_GUTTER,
} from '../theme';

/** Column width for aligning the child-PR titles before the state label. */
const TITLE_COLUMN_CHARS = 22;

/**
 * Beat-5 full-frame epic-PR mock (docs/DEMO_VIDEO.md §2 beat 5). One clock —
 * beat5TickIndex — drives everything: the child-PR checklist ticks, the board
 * sign-off stamps staggering in, and the corner TerminalInset cycling per-child
 * /ship-issue spawn lines. Copy lives in data/beats.ts BEAT5.
 */
export const PRTimelapse: React.FC = () => {
  const frame = useCurrentFrame();
  const { fps } = useVideoConfig();

  const tick = beat5TickIndex(frame);
  const landed = Math.floor(tick);

  const rows = BEAT5.childPRs.map((pr, i) => {
    const ticked = landed >= i + 1;
    return {
      number: pr.number,
      text: `${ticked ? CHECKBOX_CHECKED : CHECKBOX_EMPTY} ${pr.number}  ${pr.title.padEnd(TITLE_COLUMN_CHARS)}${ticked ? BEAT5.mergedLabel : BEAT5.pendingLabel}`,
      ticked,
    };
  });

  // Same clock, no drift: each landed tick spawns the next child in the inset.
  const spawnTick = Math.max(landed, 1);
  const spawnChild = BEAT5.childPRs[(spawnTick - 1) % BEAT5.childPRs.length];
  // Pure function of the tick clock: before the shown child's tick lands the
  // inset reads dim "…" (spawn state); once it lands, green "merged".
  const spawnChildMerged = tick >= spawnTick;

  return (
    <Terminal lines={[]}>
      <div
        style={{
          position: 'absolute',
          top: TERMINAL_CONTENT_TOP,
          left: TERMINAL_GUTTER,
          right: TERMINAL_GUTTER,
        }}
      >
        {/* Epic PR header */}
        <div
          style={{
            fontSize: FONT_SIZES.gate,
            fontWeight: 700,
            color: COLORS.text,
            lineHeight: 1.2,
            whiteSpace: 'pre',
          }}
        >
          {BEAT5.epicTitle}
        </div>

        {/* Child-PR checklist — ticks at the ramp cadence */}
        <div style={{ marginTop: 48 }}>
          {rows.map((row) => (
            <div
              key={row.number}
              style={{
                fontSize: FONT_SIZES.terminal,
                lineHeight: 1.5,
                color: row.ticked ? COLORS.green : COLORS.dim,
                fontWeight: row.ticked ? 700 : 400,
                whiteSpace: 'pre',
              }}
            >
              {row.text}
            </div>
          ))}
        </div>

        {/* Board sign-off stamps — stagger in on the final ticks */}
        <div style={{ marginTop: 72, display: 'flex', gap: 48 }}>
          {BEAT5.stampLabels.map((label, i) => {
            const stampTick = BEAT5.stampStartTick + i;
            if (tick < stampTick) {
              return null;
            }
            const pop = spring({
              frame: frame - beat5TickStartFrame(stampTick),
              fps,
              config: { damping: 14 },
            });
            return (
              <div
                key={label}
                style={{
                  fontSize: FONT_SIZES.stamp,
                  fontWeight: 700,
                  color: COLORS.green,
                  whiteSpace: 'pre',
                  transform: `scale(${0.9 + pop * 0.1})`,
                  transformOrigin: 'left center',
                }}
              >
                {`${label} ${CHECK}`}
              </div>
            );
          })}
        </div>
      </div>

      {/* Corner inset — /ship-issue spawning per child, one clock */}
      <TerminalInset
        lines={[
          {
            text: `${PROMPT} ${BEAT5.spawnCommand} ${spawnChild.number}`,
            colorKey: 'blue',
          },
          {
            text: spawnChildMerged ? BEAT5.mergedLabel : BEAT5.pendingLabel,
            colorKey: spawnChildMerged ? 'green' : 'dim',
          },
        ]}
      />
    </Terminal>
  );
};
