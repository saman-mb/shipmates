import React from 'react';
import { Sequence, useCurrentFrame, useVideoConfig } from 'remotion';
import { Caption } from '../components/Caption';
import { PhaseChecklist } from '../components/PhaseChecklist';
import { Terminal, type TerminalLine } from '../components/Terminal';
import { Tick } from '../components/Tick';
import { typedCharCount } from '../components/TypeOn';
import {
  BEAT3,
  BEAT3_PASS_FRAME,
  CHECKLIST_LABELS,
  type ChecklistLabel,
} from '../data/beats';
import {
  TYPE_ON_CHARS_PER_SECOND,
} from '../theme';
import type { ChecklistItem, ChecklistItemState } from '../components/PhaseChecklist';

/**
 * Beat 3 — Phases run · 0:09–0:22, the hero beat (docs/DEMO_VIDEO.md §2).
 * worktree → planner → builder → test run, with the persistent checklist
 * ticking top-right and ONE deliberate red fail before the green pass line
 * (imperfection = credibility). Copy lives in data/beats.ts BEAT3.
 */
const checklistState = (
  label: ChecklistLabel,
  frame: number,
): ChecklistItemState => {
  const t = BEAT3.timeline;
  switch (label) {
    case 'worktree':
      return frame >= t.worktreePass
        ? 'pass'
        : frame >= t.worktreeStart
          ? 'active'
          : 'pending';
    case 'planner':
      return frame >= t.plannerPass
        ? 'pass'
        : frame >= t.plannerStart
          ? 'active'
          : 'pending';
    case 'builder':
      return frame >= t.builderPass
        ? 'pass'
        : frame >= t.builderStart
          ? 'active'
          : 'pending';
    case 'tests':
      return frame >= BEAT3_PASS_FRAME
        ? 'pass'
        : frame >= t.failFrame
          ? 'fail'
          : frame >= t.testStart
            ? 'active'
            : 'pending';
    default:
      return 'pending'; // board + PR complete in Beat 4
  }
};

export const Beat3: React.FC = () => {
  const frame = useCurrentFrame();
  const { fps } = useVideoConfig();
  const t = BEAT3.timeline;

  const typed = (text: string, start: number): string =>
    text.slice(
      0,
      typedCharCount(frame - start, fps, text.length, TYPE_ON_CHARS_PER_SECOND),
    );

  const lines: TerminalLine[] = [];
  if (frame >= t.worktreeStart) {
    lines.push({ text: typed(BEAT3.lines.worktree, t.worktreeStart) });
  }
  if (frame >= t.worktreePass) {
    lines.push({ text: BEAT3.lines.worktreeDone, colorKey: 'green' });
  }
  if (frame >= t.plannerStart) {
    lines.push({ text: typed(BEAT3.lines.planner, t.plannerStart) });
  }
  if (frame >= t.builderStart) {
    lines.push({ text: typed(BEAT3.lines.builder, t.builderStart) });
  }
  if (frame >= t.testStart) {
    lines.push({ text: typed(BEAT3.lines.testRun, t.testStart) });
  }
  if (frame >= t.failFrame) {
    lines.push({
      text:
        frame >= BEAT3_PASS_FRAME ? BEAT3.lines.testPass : BEAT3.lines.testFail,
      colorKey: frame >= BEAT3_PASS_FRAME ? 'green' : 'red',
      bold: true,
    });
  }

  const items: ChecklistItem[] = CHECKLIST_LABELS.map((label) => ({
    label,
    state: checklistState(label, frame),
  }));

  return (
    <Terminal lines={lines}>
      <PhaseChecklist items={items} visible={frame >= t.checklistAppearFrame} />
      <Caption text={BEAT3.caption} />
      {/* Soft mechanical tick on each checklist check (placeholder SFX). */}
      <Sequence from={t.worktreePass} durationInFrames={2}>
        <Tick />
      </Sequence>
      <Sequence from={t.plannerPass} durationInFrames={2}>
        <Tick />
      </Sequence>
      <Sequence from={t.builderPass} durationInFrames={2}>
        <Tick />
      </Sequence>
      <Sequence from={BEAT3_PASS_FRAME} durationInFrames={2}>
        <Tick />
      </Sequence>
    </Terminal>
  );
};
