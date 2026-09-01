import React from 'react';
import { Sequence, spring, useCurrentFrame, useVideoConfig } from 'remotion';
import { Caption } from '../components/Caption';
import { PhaseChecklist } from '../components/PhaseChecklist';
import { Terminal } from '../components/Terminal';
import { Tick } from '../components/Tick';
import { BEAT4, CHECKLIST_LABELS } from '../data/beats';
import {
  COLORS,
  FONT_SIZES,
  TERMINAL_CONTENT_TOP,
} from '../theme';
import type { ChecklistItem, ChecklistItemState } from '../components/PhaseChecklist';

/**
 * Beat 4 — Gates · 0:22–0:29 (docs/DEMO_VIDEO.md §2).
 * Oversized gate results (copy in data/beats.ts BEAT4); the checklist
 * completes board + PR.
 */
const checklistState = (label: string, frame: number): ChecklistItemState => {
  const done: string[] = ['worktree', 'planner', 'builder', 'tests'];
  if (label === 'board') {
    return frame >= BEAT4.boardChecklistFrame ? 'pass' : 'pending';
  }
  if (label === 'PR') {
    return frame >= BEAT4.prChecklistFrame ? 'pass' : 'pending';
  }
  return done.includes(label) ? 'pass' : 'pending';
};

export const Beat4: React.FC = () => {
  const frame = useCurrentFrame();
  const { fps } = useVideoConfig();

  const gateLine = (
    text: string,
    from: number,
    colorKey: 'green' | 'blue',
    fontSize: number,
  ): {
    text: string;
    visible: boolean;
    scale: number;
    colorKey: 'green' | 'blue';
    fontSize: number;
  } => ({
    text,
    visible: frame >= from,
    scale: spring({ frame: frame - from, fps, config: { damping: 14 } }),
    colorKey,
    fontSize,
  });

  const gates = [
    gateLine(BEAT4.ciLine, BEAT4.ciFrame, 'green', FONT_SIZES.gate),
    gateLine(BEAT4.boardLine, BEAT4.boardFrame, 'green', FONT_SIZES.gate),
    // The URL line would overflow 1920px at gate size — use the smaller
    // FONT_SIZES.gateUrl (theme.ts) so it fits inside the frame.
    gateLine(BEAT4.prLine, BEAT4.prFrame, 'blue', FONT_SIZES.gateUrl),
  ];

  const items: ChecklistItem[] = CHECKLIST_LABELS.map((label) => ({
    label,
    state: checklistState(label, frame),
  }));

  return (
    <Terminal lines={[]} fontSize={FONT_SIZES.gate}>
      {/* Below the reserved header band (theme.ts band math). */}
      <div
        style={{
          paddingTop: TERMINAL_CONTENT_TOP,
          paddingLeft: 96,
          paddingRight: 96,
        }}
      >
        {gates.map((g, i) =>
          g.visible ? (
            <div
              key={i}
              style={{
                fontSize: g.fontSize,
                fontWeight: 700,
                lineHeight: 1.6,
                color: g.colorKey === 'green' ? COLORS.green : COLORS.blue,
                whiteSpace: 'pre',
                transform: `scale(${0.9 + g.scale * 0.1})`,
                transformOrigin: 'left center',
              }}
            >
              {g.text}
            </div>
          ) : null,
        )}
      </div>
      <PhaseChecklist items={items} />
      <Caption text={BEAT4.caption} />
      <Sequence from={BEAT4.boardChecklistFrame} durationInFrames={2}>
        <Tick />
      </Sequence>
      <Sequence from={BEAT4.prChecklistFrame} durationInFrames={2}>
        <Tick />
      </Sequence>
    </Terminal>
  );
};
