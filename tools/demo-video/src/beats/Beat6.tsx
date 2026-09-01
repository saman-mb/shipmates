import React from 'react';
import { Caption } from '../components/Caption';
import { TileGrid } from '../components/TileGrid';
import { BEAT6 } from '../data/beats';

/**
 * Beat 6 — Close · 0:38–0:41 (docs/DEMO_VIDEO.md §2).
 * Tile-grid pull-back; the final line "✓ You stayed the captain." fades in
 * on the caption band from BEAT6.captionAppearFrame.
 */
export const Beat6: React.FC = () => {
  return (
    <>
      <TileGrid />
      <Caption text={BEAT6.caption} appearFrame={BEAT6.captionAppearFrame} />
    </>
  );
};
