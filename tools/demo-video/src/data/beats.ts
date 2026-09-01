/**
 * ALL beat copy and intra-beat frame timings, transcribed verbatim from
 * docs/DEMO_VIDEO.md §2 (beats 1–7). Components hold no storyboard strings —
 * they consume these constants.
 *
 * Frame timings are at 30 fps (see FPS). Beat windows:
 *   Beat 1 Hook    0:00–0:03 → 90 frames
 *   Beat 2 Crew    0:03–0:09 → 180 frames
 *   Beat 3 Phases  0:09–0:22 → 390 frames (hero beat)
 *   Beat 4 Gates   0:22–0:29 → 210 frames
 *   Beat 5 Epic    0:29–0:38 → 270 frames
 *   Beat 6 Close   0:38–0:41 → 90 frames
 *   Beat 7 End     0:41–0:45 → 120 frames
 */
import { interpolate } from 'remotion';
import {
  ARROW,
  CHECK,
  FAIL,
  PROMPT,
} from '../theme';

export const FPS = 30;

export type BeatLine = {
  text: string;
  colorKey?: 'text' | 'dim' | 'green' | 'blue' | 'red';
  bold?: boolean;
};

/* ------------------------------------------------------------------ */
/* Beat 1 — Hook · 0:00–0:03                                           */
/* ------------------------------------------------------------------ */
export const BEAT1 = {
  hookLine: `${PROMPT} You are the control loop. Prompt. Read. Prompt again. Sigh.`,
  commandLine: `${PROMPT} /ship-issue 42`,
  captionA: 'PROMPT. READ. REPEAT. STOP.',
  captionB: 'GIVE IT A CREW.',
  typeStartFrame: 8,
  captionFlipFrame: 55,
  strikeStartFrame: 76,
  strikeDurationFrames: 8,
  stampFrame: 86,
} as const;

/* ------------------------------------------------------------------ */
/* Beat 2 — What it is · 0:03–0:09                                     */
/* ------------------------------------------------------------------ */
export const BEAT2 = {
  commandLine: `${PROMPT} /ship-issue 42`,
  rosterLine: `⚓ crew: planner · senior-engineer · sdet · architect · product-manager · security`,
  caption: '12 specialist sub-agents · one CLI',
  rosterTypeStartFrame: 30,
} as const;

/* ------------------------------------------------------------------ */
/* Beat 3 — Phases run · 0:09–0:22 (hero beat)                         */
/* ------------------------------------------------------------------ */

/** The 6-slot phase checklist — the spine of the whole video (§4 motif). */
export const CHECKLIST_LABELS = [
  'worktree',
  'planner',
  'builder',
  'tests',
  'board',
  'PR',
] as const;

export type ChecklistLabel = (typeof CHECKLIST_LABELS)[number];

export const BEAT3 = {
  caption: `ISOLATES ${ARROW} PLANS ${ARROW} BUILDS ${ARROW} TESTS`,
  lines: {
    worktree: `${ARROW} git worktree add ../shipmates-42`,
    worktreeDone: `${CHECK} worktree isolated`,
    planner: `${ARROW} task(sub-agent: planner) … "plan: 4 steps, 2 files touched"`,
    builder: `${ARROW} task(sub-agent: senior-engineer) … Edit src/auth/session.ts (+38 −6)`,
    testRun: `${ARROW} tool: Bash — npm test`,
    testFail: `${FAIL} 1 test failed — retrying`,
    testPass: `${CHECK} 214 passed`,
  },
  timeline: {
    checklistAppearFrame: 6,
    worktreeStart: 10,
    worktreePass: 45,
    plannerStart: 55,
    plannerPass: 117,
    builderStart: 127,
    builderPass: 199,
    testStart: 209,
    failFrame: 240,
    /** The deliberate red fail holds ~0.6s (18f @30fps) — imperfection = credibility. */
    failHoldFrames: 18,
  },
} as const;
export const BEAT3_PASS_FRAME =
  BEAT3.timeline.failFrame + BEAT3.timeline.failHoldFrames; // 258

/* ------------------------------------------------------------------ */
/* Beat 4 — Gates · 0:22–0:29                                          */
/* ------------------------------------------------------------------ */
export const BEAT4 = {
  caption: `REVIEW BOARD · CI GATE · GREEN OR IT DOESN'T SHIP`,
  ciLine: `${CHECK} CI green`,
  boardLine: `${CHECK} review board 4/4 sign-off`,
  prLine: `${ARROW} gh pr create … "https://github.com/you/app/pull/87"`,
  ciFrame: 10,
  boardFrame: 40,
  prFrame: 80,
  boardChecklistFrame: 40,
  prChecklistFrame: 100,
} as const;

/* ------------------------------------------------------------------ */
/* Beat 5 — Epic timelapse · 0:29–0:38                                 */
/* ------------------------------------------------------------------ */
export const BEAT5 = {
  caption: `1 EPIC ${ARROW} N CHILD PRs · /ship-issue ALL THE WAY DOWN`,
  epicTitle: '#88 Ship auth overhaul',
  /** Child-PR rows, verbatim from DEMO_VIDEO.md §2 beat 5 — never invent titles. */
  childPRs: [
    { number: '#89', title: 'Add session refresh' },
    { number: '#90', title: 'Rotate token store' },
    { number: '#91', title: 'Rate-limit refresh' },
    { number: '#92', title: 'Audit logging' },
  ],
  /** Row state column (§2 beat 5: `merged` / `…`). */
  mergedLabel: 'merged',
  pendingLabel: '…',
  spawnCommand: '/ship-issue',
  /** Board sign-off stamps (§2 beat 5: `architect ✓ sdet ✓ pm ✓ security ✓`). */
  stampLabels: ['architect', 'sdet', 'pm', 'security'],
  /**
   * Speed ramp: ~1 tick/s for the first 4s, ~3/s after — 19 ticks land by
   * f240 (§2 beat 5 "ticking ~1/s … speed ramps 1 tick/s → ~3 ticks/s").
   * Piecewise-linear frames→ticks; the ONE clock for checklist, stamps and
   * corner inset (no drift).
   */
  ramp: { frames: [0, 120, 240], ticks: [0, 4, 19] },
  /** Stamps stagger in on the final ticks, as each sign-off lands. */
  stampStartTick: 16,
} as const;

/**
 * Pure tick clock for the beat-5 speed ramp: frames → completed-tick index
 * (float; callers floor it). Deterministic, no hooks — shared by the checklist,
 * the stamps and the TerminalInset.
 */
export const beat5TickIndex = (frame: number): number =>
  interpolate(frame, [...BEAT5.ramp.frames], [...BEAT5.ramp.ticks], {
    extrapolateLeft: 'clamp',
    extrapolateRight: 'clamp',
  });

/**
 * Inverse of beat5TickIndex: the frame at which a given tick lands. Used for
 * per-tick type-on starts and SFX sequences. Piecewise-linear over the same
 * ramp constants, so the two can never drift.
 */
export const beat5TickStartFrame = (tick: number): number => {
  const { frames, ticks } = BEAT5.ramp;
  for (let i = 0; i < frames.length - 1; i++) {
    if (tick <= ticks[i + 1]) {
      const t0 = ticks[i];
      const t1 = ticks[i + 1];
      return frames[i] + ((tick - t0) / (t1 - t0)) * (frames[i + 1] - frames[i]);
    }
  }
  return frames[frames.length - 1];
};

/* ------------------------------------------------------------------ */
/* Beat 6 — Close · 0:38–0:41                                          */
/* ------------------------------------------------------------------ */
export const BEAT6 = {
  caption: `${CHECK} You stayed the captain.`,
  captionAppearFrame: 30,
  /** Simplified terminal-mock tile grid (built from theme constants). */
  grid: { cols: 4, rows: 3 },
  /**
   * Transform-based pull-back: the tile grid scales 2.8→1 with a decaying
   * offset over f0–f60, settled (static) for the last 30f. No frame sampling
   * of other compositions — the tiles are live mocks, so text stays crisp.
   */
  pull: { frames: [0, 60], scale: [2.8, 1], offset: { x: 160, y: 100 } },
} as const;

/* ------------------------------------------------------------------ */
/* Beat 7 — End card · 0:41–0:45 (hold 4s, loop-friendly)              */
/* ------------------------------------------------------------------ */
export const BEAT7 = {
  repoUrl: 'github.com/saman-mb/shipmates',
  siteUrl: 'saman-mb.github.io/shipmates',
  /** Dim install line (§2 beat 7) — the repo README's Homebrew command. */
  installLine: 'brew install saman-mb/tap/shipmates',
  /** Entry fade/slide settles here; every later frame is pixel-identical. */
  settleFrame: 60,
  /** Pixel-art sailboat logo from the landing site (copied to public/). */
  logoFile: 'logo.png',
} as const;
