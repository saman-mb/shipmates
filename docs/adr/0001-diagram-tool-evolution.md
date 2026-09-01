# ADR 0001 — Evolving `svgflow` into a general `diagram` tool

**Status:** Proposed
**Date:** 2026-08-06
**Deciders:** architect (judge), devops-engineer (delivery-axis judge)
**Bundles:** #221 (PNG export) + #222 (rename → general `diagram` tool: animation + intent-routed multi-backend)
**Decided by:** `/shipmates-spike` — three disposable prototypes, judged against the toolbox doctrine.

---

## Context

The `svgflow` tool renders a JSON spec into a theme-exact SVG diagram. Two asks are open:

- **#221** — export **PNG** (today PNGs are produced by an external `resvg` pipe, not by the tool).
- **#222** — rename to a general **`diagram`** tool that also supports **animation** and **routes to the right renderer by the user's intent** across diagram types.

The governing constraint is the toolbox doctrine (`AGENTS.md:67,69`): a tool directory is **exactly `tool.md` + one runnable `<name>.py`**, and a tool **must work out of the box after `shipmates install` — self-provisioning its deps, running offline and deterministically**. Any option that needs a system binary, a network fetch at install/run, or a second shipped file is fighting that constraint.

**Decision criteria (weighted):** self-containment & determinism (HARD — a failure here is fatal) · diagram-type breadth · exact site-theme control (svgflow's signature) · PNG rasterization path & dep weight · animation feasibility · intent-routing complexity · **reversibility** (spend certainty on one-way doors; move fast on two-way doors).

## Options considered

Three approaches were prototyped as throwaway spikes and measured against real rendered artifacts.

### A — Extend the single hand-rolled engine (`svgflow++`)

Keep svgflow's one self-contained engine; add PNG, animation, and more diagram types inside it.

- **Key insight:** PNG is **not** done by SVG→PNG conversion. A **second Pillow painter** repaints the *same* primitive list the SVG painter uses. So PNG + GIF depend on **Pillow only** (already provisioned by the `termgif`/`pixelart` pattern) — no cairo, no resvg, no pip-bootstrap. Verified **byte-identical** (deterministic) and offline.
- **Animation:** frame-based **GIF via Pillow** (proven — an eased accent-pulse), reusing termgif's save path. Optional SMIL in the SVG is browser-only and does not survive rasterization (cosmetic, not a deliverable).
- **Breadth:** a whole new **sequence** diagram cost **~32 LOC** reusing shared primitives, rendering in both SVG and PNG. Ceiling: a **curated menu** of hand-laid types (flow / sequence / state / timeline) — **no auto-layout**, not arbitrary 2-D graphs.
- **Intent routing:** `kind` field, else a keyword→builder dict. Deterministic, no LLM.
- **Theme:** preserved exactly (both painters read the same palette hex).
- **Sharp edges:** deterministic PNG text needs a **known font**; two painters are only *semantically* equal, not pixel-identical.

### B — Multi-backend router (svgflow + Graphviz / Mermaid)

Rename to `diagram`; keep svgflow for themed diagrams; add mature engines for standard types behind an intent router.

- **The router is nearly free** (dict + regex; routed 5/5 intents) — and it is the one genuinely good idea in this option.
- **Both mature backends are disqualified on self-containment, and not marginally.** **Graphviz** needs the `dot` binary **+ ~18 system shared-library packages**, no `apt`/`sudo`, not pip-vendorable (`pip install graphviz` is bindings only → `ExecutableNotFound`). **Mermaid `mmdc`** triggers puppeteer **downloading 150 MB+ Chromium over the network at install** (and needs Node ≥ 22; the env had 18). Neither backend animates.
- In a doctrine-compliant install the router **falls back to svgflow for every kind** → it buys nothing while adding an engine-management burden. Only a **pure-Python** backend could ever fit.

### C — Adopt one mature library wholesale (`blockdiag` / `seqdiag` / `actdiag` / `nwdiag`)

Replace the hand-rolled engine with a pure-Python library covering many types with native PNG.

- **Good:** pure-Python, pip-installable, offline, **byte-identical**; real breadth (native sequence/activity/network that svgflow structurally cannot do); thin 1:1 intent→package routing.
- **Theme fidelity lost:** no page/panel concept (svgflow's signature dark card is unreproducible); node-border and edge color both ride `default_linecolor` (accent bleeds); typography downgraded (single TTF, no CSS stack); a Unicode `→` in a label **throws and drops the label**.
- **Maintenance rot:** breaks against current Pillow (forces `Pillow==9.5.0`), depends on the **removed `pkg_resources`** (`setuptools<81`, already past its removal date) — effectively unmaintained.
- **Cross-tool corruption:** that `Pillow==9.5.0` pin **collides with `termgif`/`pixelart`** (which resolve to Pillow 12) in the **shared flat cache** `~/.cache/shipmates/pylib` — one flat `PIL/` import root, whoever provisions last wins. C does not just constrain itself; it destabilises sibling tools.
- Animation entirely unsolved.

## Decision

**Adopt A — extend the single hand-rolled engine into `diagram`. Reject B and C as backends.** Ranking: **A ≫ C > B**. A is the only option with no failing cell in a load-bearing criterion.

Concretely, the **#221 + #222 bundle builds now**:

1. **Rename `svgflow` → `diagram`**, keeping a thin **`svgflow` alias / deprecation shim** for one release (flow becomes a *kind* under `diagram`; the shim forwards and emits a one-line deprecation note). This converts the one irreversible decision in the bundle into a staged, reversible migration.
2. **PNG via a second Pillow painter** over the shared primitive list — lightest possible path, reusing the deps termgif already provisions.
3. **GIF animation** = exactly one proven effect (accent-pulse), reusing termgif's GIF save path. No general animation DSL.
4. **Intent router** = the `kind`-else-keyword dict, grafted from B (engine-agnostic, so it can later dispatch to a curated builder *or* an isolated add-on with no rewrite).
5. **One new curated `kind`: sequence** (~32 LOC), the most common thing svgflow can't do — proof the menu extends.
6. **Deterministic PNG text via an embedded font:** ship a **subsetted, redistributable TTF** (DejaVu family) as a `zlib`+`base64` blob **inside the one `.py`**, loaded from memory. This is the only path that is both deterministic *and* doctrine-compliant (a sidecar `.ttf` violates the one-file rule; a system-font fallback floats per host).

**Defer:** further `kind`s (state/timeline — additive, two-way); SMIL-in-SVG (document as browser-only); any auto-layout / arbitrary-graph ambition; and `blockdiag` — kept **on the shelf** as a documented, opt-in escape hatch **behind its own separately-pinned cache** (never the shared `pylib`) *only if* real sequence/activity/network breadth is later demanded beyond A's ceiling.

### Why (reversibility-weighted)

The choice is dominated by one-way vs two-way doors. **A adds capability without adding a door** — PNG and GIF are new *outputs* of the existing primitives; a new type is ~32 LOC; the dependency footprint is already paid for by termgif. **B and C buy breadth by walking through a one-way door:** B bundles an unvendorable native runtime (and silently degrades to svgflow anyway); C pins an *unmaintained* library to an EOL `Pillow`/`setuptools` and **corrupts sibling tools** through the shared cache. In this environment (no pip/ensurepip, Node 18, no sudo) the mature engines are not merely heavier — they are **unreachable**, so their breadth is imaginary while their dependency cost is real. We accept a **low, documented breadth ceiling** to keep the tool self-contained, deterministic, and behind only two-way doors.

## Consequences

**What this commits us to**
- `diagram` is a **curated menu of hand-laid diagram types** with exact site-theme control and deterministic SVG + PNG + GIF — **not** a general graph renderer, with **no auto-layout**. Each type is deliberately laid out (~32 LOC).
- PNG is a **faithful re-render of the same spec, deterministic per painter, not pixel-identical to the SVG**. `tool.md` must state this. Do **not** chase pixel-equality between painters — that is accidental complexity.
- The `svgflow` name lives on as a deprecation shim for one release, then is removed.

**What becomes harder / must be fixed for the promise to hold** (delivery constraints the implementation must satisfy — from the devops-engineer judgment):
1. **Per-tool cache version isolation.** The flat `~/.cache/shipmates/pylib` cannot hold two Pillow versions; pip `--target` clobbers `PIL/` in place. Namespace it (`pylib/<tool>/<version>/` or a per-tool venv). **Until this lands, no tool may pin a Pillow different from any other tool's** (this alone bars C). *This is a latent corruption bug in the pool **today**, independent of this ADR — filed separately.*
2. **Pin every runtime dep and make the cache authoritative.** `_ensure_pillow` checks `import PIL` *before* adding the cache to `sys.path` (`termgif.py:46-55`), so the resolved Pillow is **whatever the host has** (9.4.0 here, not the cached 12) — which falsifies "byte-identical across hosts". Fix the resolution order and replace bare `"Pillow"` with a pin.
3. **A no-network, no-pip path.** On a host with neither pip nor system Pillow, `_ensure_pillow` exits — violating "runs after `shipmates install`, full stop." Vendor the pinned wheel or download-once **with a recorded hash**; `--provision` must place bytes, not `import PIL` and print "ready".
4. **Deterministic text = embedded font, no `load_default()` fallback** in a deterministic-output tool.
5. **No timestamps / host paths in artifacts** (PNG without `tIME`, GIF without host metadata); assert byte-identical output across two clean runs **and** two hosts in CI.
6. **Single-file layout is non-negotiable** (`AGENTS.md:67`) — rules out B and forces the font in-file.

**Deferred to `security-engineer` at implementation time (not blockers for this proposal):** the specific embedded font's redistribution license; and — should the shelved `blockdiag` add-on ever be built — the acceptability of its stale `Pillow 9.5.0` pin. (Mermaid's install-time Chromium download is noted as a supply-chain concern, but B is already rejected on reproducibility grounds.)

**Reversibility summary:** every capability added by A is two-way (deletable). The only one-way step, the rename, is mitigated by the alias shim. The rejected options were rejected substantially *because* they were one-way (unvendorable native dep / unmaintained pinned dep with cross-tool blast radius).

## Follow-ups

- Implement the bundle (#221 + #222) per the Decision — a `/ship-issue` unit once this ADR is accepted.
- **Separate bug** (pre-existing, higher-leverage than this ADR): shared-cache version isolation + Pillow pinning + no-pip provisioning path for the toolbox self-provision pattern (`termgif.py`/`pixelart.py`). Fixing it is a prerequisite for any multi-tool-with-different-deps future and removes a latent reproducibility defect today.
