import React from 'react';
import { Composition } from 'remotion';
import { loadFont as loadJetBrainsMono } from '@remotion/google-fonts/JetBrainsMono';
import { loadFont as loadIBMPlexSans } from '@remotion/google-fonts/IBMPlexSans';
import { Beat1 } from './beats/Beat1';
import { Beat2 } from './beats/Beat2';
import { Beat3 } from './beats/Beat3';
import { Beat4 } from './beats/Beat4';
import { Beat5 } from './beats/Beat5';
import { Beat6 } from './beats/Beat6';
import { Beat7 } from './beats/Beat7';
import { FPS } from './data/beats';

// Fonts (OFL-licensed) load at bundle/render time; Remotion waits for
// document.fonts.ready before the first frame. Fire-and-forget is the
// documented pattern — failures surface at render, not as unhandled rejections.
loadJetBrainsMono()
  .waitUntilDone()
  .catch((err: unknown) => {
    console.error('Failed to load JetBrains Mono:', err);
  });
loadIBMPlexSans()
  .waitUntilDone()
  .catch((err: unknown) => {
    console.error('Failed to load IBM Plex Sans:', err);
  });

export const VIDEO_WIDTH = 1920;
export const VIDEO_HEIGHT = 1080;

export const RemotionRoot: React.FC = () => {
  return (
    <>
      {/* Beat 1 — Hook · 0:00–0:03 (docs/DEMO_VIDEO.md §2) */}
      <Composition
        id="Beat1Hook"
        component={Beat1}
        durationInFrames={90}
        fps={FPS}
        width={VIDEO_WIDTH}
        height={VIDEO_HEIGHT}
      />
      {/* Beat 2 — What it is · 0:03–0:09 */}
      <Composition
        id="Beat2Crew"
        component={Beat2}
        durationInFrames={180}
        fps={FPS}
        width={VIDEO_WIDTH}
        height={VIDEO_HEIGHT}
      />
      {/* Beat 3 — Phases run · 0:09–0:22 (hero beat) */}
      <Composition
        id="Beat3Phases"
        component={Beat3}
        durationInFrames={390}
        fps={FPS}
        width={VIDEO_WIDTH}
        height={VIDEO_HEIGHT}
      />
      {/* Beat 4 — Gates · 0:22–0:29 */}
      <Composition
        id="Beat4Gates"
        component={Beat4}
        durationInFrames={210}
        fps={FPS}
        width={VIDEO_WIDTH}
        height={VIDEO_HEIGHT}
      />
      {/* Beat 5 — Epic timelapse · 0:29–0:38 */}
      <Composition
        id="Beat5PR"
        component={Beat5}
        durationInFrames={270}
        fps={FPS}
        width={VIDEO_WIDTH}
        height={VIDEO_HEIGHT}
      />
      {/* Beat 6 — Close · 0:38–0:41 */}
      <Composition
        id="Beat6Close"
        component={Beat6}
        durationInFrames={90}
        fps={FPS}
        width={VIDEO_WIDTH}
        height={VIDEO_HEIGHT}
      />
      {/* Beat 7 — End card · 0:41–0:45 (hold 4s, loop-friendly) */}
      <Composition
        id="Beat7EndCard"
        component={Beat7}
        durationInFrames={120}
        fps={FPS}
        width={VIDEO_WIDTH}
        height={VIDEO_HEIGHT}
      />
    </>
  );
};
