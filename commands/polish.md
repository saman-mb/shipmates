---
description: Iterate a visual / UI / output artifact to a specialist's sign-off — produce → critique → fix, looping until the artist, ux-ui-designer, or product-manager is genuinely happy (or a round cap).
argument-hint: <what to polish — a screen, an asset, a rendered surface> [reviewer: artist|ux-ui-designer|product-manager]
allowed-tools: Bash, Read, Write, Edit, Agent, Grep, Glob, WebSearch, WebFetch
---

# /polish — iterate to a specialist's sign-off

Take a visual / UI / output artifact and refine it in a **loop**: produce it, have the right
specialist critique what they actually SEE (not the source that made it), apply the concrete fixes,
re-produce, re-review — until that specialist **genuinely signs off** or a round cap is hit. This is
the render → critique → refine loop, formalised.

Input (**$ARGUMENTS**): what to polish (a screen/panel, a generated art asset, a rendered game view,
a chart, a piece of output…) and optionally which reviewer. If it's empty, ask what to polish.

---

## Config

- `REVIEWER` — chosen by the artifact's domain (or named in `$ARGUMENTS`):
  - rendered visual **art** (game world, sprites, shaders, generative imagery, brand/motion) → `artist`
  - on-screen application **UI** (screens, HUD, panels, components) → `ux-ui-designer`
  - general **output quality** / does-it-meet-the-goal (copy, a data view, a non-visual deliverable) → `product-manager`

  Infer from the repo's README/CLAUDE.md domain; when genuinely ambiguous, ask the user which reviewer.
- `MAX_ROUNDS` = 5 — loop cap before escalating to the user with the current state and the reviewer's
  outstanding notes. Never loop forever; never declare a sign-off the reviewer didn't give.
- `BUILDER` = `senior-engineer` — applies the reviewer's fixes each round.
- **Quality bar** = the project's stated visual/UX bar (README/CLAUDE.md), passed to the reviewer.

---

## Stage 0 — Secure a way to SEE the output  (the loop can't converge without it)

The reviewer must judge the ACTUAL produced artifact, not the code. Find the project's way to produce
it headlessly — a render / screenshot / build / export / preview harness (check its tools/scripts/
docs). If none exists and one is feasible, have a `senior-engineer` build a **minimal** one (e.g. a
headless script that writes a PNG, or a preview export). If it is genuinely impossible to produce or
view the artifact, say so plainly, fall back to a single static review flagged **"needs a human visual
pass,"** and tell the user the loop cannot truly converge blind — don't fake iterations.

## Stage 1 — Baseline

Produce the artifact once (run the harness) and capture it (PNG / output file). Call this round 0 and
keep it, so the final report can show a before → after.

## Stage 2 — The loop  (repeat up to `MAX_ROUNDS`)

Each round:
1. **Critique** — spawn the `REVIEWER` against the CURRENT produced artifact (the actual image/output
   file, which it opens and inspects — not the source). It returns a decisive verdict —
   `ACCEPT` (sign-off) / `ACCEPT-WITH-NITS` / `REJECT` — with SPECIFIC, plug-in fixes (values, not
   "make it nicer"), blockers separated from nits. Instruct it explicitly not to rubber-stamp to end
   the loop.
2. **Signed off?** `ACCEPT` → leave the loop. `ACCEPT-WITH-NITS` → leave the loop too (the nits become
   follow-ups) unless the caller asked to resolve nits as well. `REJECT` → continue.
3. **Fix** — spawn a `senior-engineer` with the reviewer's exact blocker list; apply the changes in
   place (this is refinement, not a PR unless the caller asked for one). Keep the change scoped to the
   notes — no unrelated drift.
4. **Re-produce** — re-run the harness and capture the new artifact.
5. Keep a one-line changelog per round so the trajectory is visible.

If `MAX_ROUNDS` is reached without a sign-off, **STOP** and hand the user the current artifact plus the
reviewer's remaining notes. Escalate; don't spin.

## Stage 3 — Report

Show the user the final artifact (path / screenshot), the reviewer's verdict in its own words, the
number of rounds, and a short before → after of what changed. Optionally file any allowed nits as
follow-up issues.

---

### Guardrails
- The reviewer judges the PRODUCED OUTPUT every round — never the source. Code that looks right can
  render wrong; that's the whole point of the loop.
- A reviewer that `ACCEPT`s round 0 with zero changes gets a sanity check — make sure it actually
  inspected the artifact and isn't rubber-stamping.
- Bounded by `MAX_ROUNDS` — escalate rather than loop forever.
- Scope each fix round to the reviewer's notes; the `senior-engineer` doesn't refactor or wander.
- The sign-off is the REVIEWER's to give, and the final report states what the reviewer actually said
  — not an optimistic paraphrase. A "needs a human visual pass" fallback is a real outcome, not a fail.
- Reviewer choice follows the project's domain: `artist` for art, `ux-ui-designer` for UI,
  `product-manager` for general output. When ambiguous, ask.
- Runs standalone, or as the visual pass inside/after `/ship-issue` on a UI/visual story.
- If a role doesn't resolve to a `.claude/agents/*.md`, fall back to `general-purpose` with the role's
  brief inlined, and note the fallback.
