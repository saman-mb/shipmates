import React from 'react';
import {
  CHECK,
  COLORS,
  FAIL,
  FONT_MONO,
  FONT_SIZES,
} from '../theme';

/**
 * The 6-slot phase checklist — the spine of the video (docs/DEMO_VIDEO.md §4).
 * Pinned top-right. Glyphs and colours come ONLY from theme.ts; this component
 * never inlines a ✓, ✗ or hex value.
 */
export type ChecklistItemState =
  | 'pending'
  | 'active'
  | 'pass'
  | 'fail'
  | 'retry';

export type ChecklistItem = {
  label: string;
  state: ChecklistItemState;
};

const STATE_GLYPH: Record<ChecklistItemState, string> = {
  pending: '○',
  active: '●',
  pass: CHECK,
  fail: FAIL,
  retry: FAIL,
};

const STATE_COLOR: Record<ChecklistItemState, string> = {
  pending: COLORS.dim,
  active: COLORS.blue,
  pass: COLORS.green,
  fail: COLORS.red,
  retry: COLORS.red,
};

export const PhaseChecklist: React.FC<{
  items: ChecklistItem[];
  visible?: boolean;
}> = ({ items, visible = true }) => {
  if (!visible) {
    return null;
  }
  return (
    <div
      style={{
        position: 'absolute',
        top: 96,
        right: 64,
        display: 'flex',
        gap: 28,
        fontFamily: FONT_MONO,
        fontSize: FONT_SIZES.checklist,
        lineHeight: 1.5,
      }}
    >
      {items.map((item) => (
        <span key={item.label} style={{ whiteSpace: 'pre' }}>
          <span style={{ color: STATE_COLOR[item.state] }}>
            {STATE_GLYPH[item.state]}
          </span>{' '}
          <span
            style={{
              color: item.state === 'pending' ? COLORS.dim : COLORS.text,
            }}
          >
            {item.label}
          </span>
        </span>
      ))}
    </div>
  );
};
