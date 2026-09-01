/**
 * ALL beat copy and intra-beat frame timings, transcribed verbatim from
 * docs/DEMO_VIDEO.md §2 (beats 1–4). Components hold no storyboard strings —
 * they consume these constants.
 *
 * Frame timings are at 30 fps (see FPS). Beat windows:
 *   Beat 1 Hook    0:00–0:03 → 90 frames
 *   Beat 2 Crew    0:03–0:09 → 180 frames
 *   Beat 3 Phases  0:09–0:22 → 390 frames (hero beat)
 *   Beat 4 Gates   0:22–0:29 → 210 frames
 */
import { ARROW, CHECK, FAIL, PROMPT } from '../theme';

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
