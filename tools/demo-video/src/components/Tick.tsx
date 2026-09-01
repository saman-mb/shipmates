import React from 'react';
import { Audio, staticFile } from 'remotion';

/**
 * SFX PLACEHOLDER — soft mechanical tick per checklist check.
 *
 * public/tick.mp3 is a generated 30ms near-silence standing in for the real
 * SFX (docs/DEMO_VIDEO.md §5: soft mechanical tick, ≥ −18 LUFS under VO).
 * Real SFX are pending; swap the file in public/ and the volume here when
 * they land. Render inside a <Sequence> so it fires on the check frame.
 */
export const Tick: React.FC<{ volume?: number }> = ({ volume = 0.15 }) => {
  return <Audio src={staticFile('tick.mp3')} volume={volume} />;
};
