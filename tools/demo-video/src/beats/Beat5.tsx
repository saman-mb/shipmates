import React from 'react';
import { Sequence } from 'remotion';
import { Caption } from '../components/Caption';
import { PRTimelapse } from '../components/PRTimelapse';
import { Tick } from '../components/Tick';
import { BEAT5, beat5TickStartFrame } from '../data/beats';

/**
 * Beat 5 — Epic timelapse · 0:29–0:38 (docs/DEMO_VIDEO.md §2).
 * The PRTimelapse mock plus the beat caption; one soft tick SFX per child-PR
 * checklist check (same placeholder as beats 3–4).
 */
export const Beat5: React.FC = () => {
  return (
    <>
      <PRTimelapse />
      <Caption text={BEAT5.caption} />
      {BEAT5.childPRs.map((pr, i) => (
        <Sequence
          key={pr.number}
          from={beat5TickStartFrame(i + 1)}
          durationInFrames={2}
        >
          <Tick />
        </Sequence>
      ))}
    </>
  );
};
