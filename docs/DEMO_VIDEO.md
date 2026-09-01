# Shipmates 45s LinkedIn Demo — Video Plan

> Status: planning. Target: 45-second AI-assisted demo clip for LinkedIn showing the
> `/` commands in action — terminal phases, dynamic crew leverage, tool calls, and an
> epic PR timelapse with board sign-off. Narrated in the captain's own (cloned) voice.

## 1. Production pipeline (decided)

Hybrid: **programmatic animation for all UI/text + AI video for cinematic shots only**.

| Beat | Engine | Why |
|------|--------|-----|
| Terminal phases (beats 1–4) | **Remotion 4.x** (React → MP4) | Pixel-perfect monospace text; deterministic re-renders |
| GitHub PR timelapse (beat 5) | Remotion UI mock (DOM-style layers) | AI video cannot render legible UI text (see §6) |
| Pull-back / transitions (beat 6) | **MiniMax H3** (ComfyUI, local ROCm) — optional | Cinematic polish only; never renders text |
| Narration | Voice clone: ElevenLabs IVC (~$6 one month) or local Chatterbox Multilingual v3 | British male, sampled from captain's voice |
| Assembly | Remotion (audio + transitions in-composition) | Single deterministic render |

Cost: ≈ $6 (cloud voice) or ≈ $0 (fully local). AI-video b-roll is optional and capped at ~10s.

## 2. Beat-by-beat storyboard

### Beat 1 — Hook · 0:00–0:03
- **Visual:** full-screen dark terminal (`#0D1117`). Cursor blinks, text types fast:
  ```
  ❯ You are the control loop. Prompt. Read. Prompt again. Sigh.
  ```
  Line gets struck-through in red; stamps beneath in bold green:
  ```
  ❯ /ship-issue 42
  ```
- **Narration:** "Stop being your AI's for-loop."
- **Caption:** `PROMPT. READ. REPEAT. STOP.` → flips to `GIVE IT A CREW.`

### Beat 2 — What it is · 0:03–0:09
- **Visual:** crew roster prints beneath the command:
  ```
  ⚓ crew: planner · senior-engineer · sdet · architect · product-manager · security
  ```
- **Narration:** "Shipmates gives Claude Code a crew of specialists — planners, builders, testers, reviewers."
- **Caption:** `12 specialist sub-agents · one CLI`

### Beat 3 — Phases run · 0:09–0:22 (hero beat)
- **Visual:** realistic git/gh cadence; persistent phase checklist pinned top-right:
  ```
  ✓ worktree   ✓ planner   ✓ builder   ○ tests   ○ board   ○ PR
  ```
  Synced terminal text:
  ```
  → git worktree add ../shipmates-42
  ✓ worktree isolated
  → task(sub-agent: planner) … "plan: 4 steps, 2 files touched"
  → task(sub-agent: senior-engineer) … Edit src/auth/session.ts (+38 −6)
  → tool: Bash — npm test … "✓ 214 passed"
  ```
  One deliberate red fail: `✗ 1 test failed — retrying` → `✓ 214 passed` (imperfection = credibility).
- **Narration:** "One command — slash, ship-issue, forty-two. It isolates a worktree, plans the work, builds it, and runs the tests."
- **Caption:** `ISOLATES → PLANS → BUILDS → TESTS`

### Beat 4 — Gates · 0:22–0:29
- **Visual:** oversized gate results:
  ```
  ✓ CI green        ✓ review board 4/4 sign-off
  → gh pr create …  "https://github.com/you/app/pull/87"
  ```
- **Narration:** "Then a review board signs it off — and CI goes green."
- **Caption:** `REVIEW BOARD · CI GATE · GREEN OR IT DOESN'T SHIP`

### Beat 5 — Epic timelapse · 0:29–0:38
- **Visual:** hard cut to stylised GitHub PR mock: epic PR "#88 Ship auth overhaul" with child-PR checklist ticking ~1/s, corner inset showing `/ship-issue` spawning per child:
  ```
  ☑ #89  Add session refresh      merged
  ☑ #90  Rotate token store       merged
  ☑ #91  Rate-limit refresh       merged
  ☐ #92  Audit logging            …
  ```
  Speed ramps 1 tick/s → ~3 ticks/s; board sign-off stamps end: `architect ✓ sdet ✓ pm ✓ security ✓`
- **Narration:** "Bigger work? Ship an epic: ship-issue runs as a subprocess — child pull requests landing, one by one."
- **Caption:** `1 EPIC → N CHILD PRs · /ship-issue ALL THE WAY DOWN`

### Beat 6 — Close · 0:38–0:41
- **Visual:** both screens tile side by side, dimmed, cursor still blinking. Final line types:
  ```
  ✓ You stayed the captain.
  ```
- **Narration:** "You stay the captain."

### Beat 7 — End card · 0:41–0:45 (hold 4s, loop-friendly)
- **Visual:** dark card, sailboat logo top-centre:
  ```
  github.com/saman-mb/shipmates
  saman-mb.github.io/shipmates
  ```
  dim install line beneath.

## 3. Narration script (~72 words, ≈33s at 130 wpm)

> Stop being your AI's for-loop.
> Shipmates gives Claude Code a crew of specialists — planners, builders, testers, reviewers.
> One command — slash, ship-issue, forty-two. It isolates a worktree, plans the work, builds it, and runs the tests.
> Then a review board signs it off — and CI goes green.
> Bigger work? Ship an epic: ship-issue runs as a subprocess — child pull requests landing, one by one.
> You stay the captain.

Optional 95-word variant for beat 3 adds self-remediation: "…spawns builders as it goes. Tool calls fire — tests run, one fails, it fixes itself, and runs again."

## 4. Typography & palette

- **Font:** Berkeley Mono (ideal) or JetBrains Mono. Render at 2×, downscale for feed sharpness. Never warp/scale/blur text.
- **Palette (GitHub-native):** bg `#0D1117` · text `#E6EDF3` · dim `#8B949E` · **accent `#3FB950`** (green = shipped, only for success/CTA) · commands `#58A6FF` · failure `#F85149` (used once).
- **Motif:** the 6-slot phase checklist is the spine — terminal (beats 1–4) → child-PR checklist (beat 5) → install line (end card). Same `✓`, same green.
- **Captions:** IBM Plex Sans Bold, 2–4 words, top-centre, white on 40% black plate, max 8 words on screen.

## 5. Audio

- Music: minimal low-pulse electronic, ~85 BPM, −20 LUFS, enters 0:03, +2 dB at beat 5, resolves on a low note at 0:41. No vocals/risers.
- SFX: soft mechanical tick per checklist check (same every time); quiet keystroke clatter; low thud on beat-1 strike-through; warm confirm chirp on `✓ board`. All ≥ −18 LUFS under VO.
- EU AI Act Art. 50 disclosure: add "narration is AI-generated" in the LinkedIn caption.

## 6. Key research findings (why this pipeline)

1. **AI video must not render terminal/UI text.** Sub-3%-frame-height glyphs drift/garble per frame (Veo 3.1/Kling 3.0 manage only 1–3 large static words; <40px text "almost always garbles"). Terminals are the worst case: small, dense, monospace, every character must be exact. Rule: *generate the plate empty, stamp the text in the timeline.*
2. **Strix Halo/ROCm fits MiniMax H3** (~41 GB total across text-encoder/diffusion/VAE — inside unified memory). LTX-2 also viable (10–13 min per 10s clip). Wan 2.2 is 27–36 min per 5s — not for iteration.
3. **Voice:** ElevenLabs IVC (Starter $6/mo) = best British-male fidelity; Chatterbox Multilingual v3 (MIT, 4–8 GB VRAM, built-in PerTh watermark) = $0 local path. Needs a 1–5 min clean mono sample from the captain.

## 7. TODO

- [ ] Record captain's voice sample (1–5 min, quiet room) → clone + approve narration take
- [ ] Scaffold Remotion project (beats 1–5 + end card)
- [ ] Render beat 3 first; verify legibility at phone-feed scale (≥20 px effective glyph height) before committing to full build
- [ ] Generate MiniMax H3 transition shot (once ComfyUI model downloads finish)
- [ ] Assemble, mix audio, export 1080×1080 + 1920×1080
- [ ] LinkedIn post copy + AI-disclosure line
