import React from 'react';
import { useCurrentFrame, useVideoConfig } from 'remotion';
import { Caption } from '../components/Caption';
import { Terminal, type TerminalLine } from '../components/Terminal';
import { PhaseChecklist } from '../components/PhaseChecklist';
import { typedCharCount } from '../components/TypeOn';
import { BEAT2, CHECKLIST_LABELS } from '../data/beats';
import { TYPE_ON_CHARS_PER_SECOND } from '../theme';

/**
 * Beat 2 — What it is · 0:03–0:09 (docs/DEMO_VIDEO.md §2).
 * The command persists; the crew roster types beneath it.
 */
export const Beat2: React.FC = () => {
  const frame = useCurrentFrame();
  const { fps } = useVideoConfig();

  const typedRoster = BEAT2.rosterLine.slice(
    0,
    typedCharCount(
      frame - BEAT2.rosterTypeStartFrame,
      fps,
      BEAT2.rosterLine.length,
      TYPE_ON_CHARS_PER_SECOND,
    ),
  );

  const lines: TerminalLine[] = [
    { text: BEAT2.commandLine, colorKey: 'blue', bold: true },
  ];
  if (frame >= BEAT2.rosterTypeStartFrame) {
    lines.push({ text: typedRoster, colorKey: 'text' });
  }

  return (
    <Terminal lines={lines}>
      <PhaseChecklist
        items={CHECKLIST_LABELS.map((label) => ({ label, state: 'pending' }))}
      />
      <Caption text={BEAT2.caption} />
    </Terminal>
  );
};
