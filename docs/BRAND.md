# Brand

The single source of truth for Shipmates' identity, voice, and naming. When a rule here
conflicts with copy already in the repo, the copy is wrong — fix the copy.

Who this is for: anyone (human or agent) writing README copy, site copy, skill and agent
descriptions, release notes, issue titles, or social text for this project.

`AGENTS.md` carries a condensed version of the [naming register](#naming-register) rule
and links here for the full statement.

---

## What Shipmates is

Shipmates is an open-source (MIT) crew of specialist AI agents for
[Claude Code](https://claude.com/claude-code): **11 domain-neutral subagents** and
**9 reusable commands**. Its flagship, `/ship-issue`, takes a GitHub issue all the way to a
reviewed, CI-green pull request on its own — it plans the work, builds it in an isolated git
worktree, waits for CI to go green, convenes an adversarial review board, loops on fixes within
bounds, and hands you a PR to merge. You stay the captain. The shipmates do the twenty steps in
between.

---

## The metaphor

One nautical metaphor, used consistently. It exists to make an unfamiliar idea (multi-agent
delegation) land in one read.

| Term | Maps to | Where it appears today |
|---|---|---|
| **captain** | the user — you decide, you approve, you merge | README: "You stay the captain." |
| **crew** / **shipmates** | the 11 subagents in `agents/*.md` | README "Meet the crew"; site `#crew`; `crew-card` components |
| **shipmate** | a single subagent role | README crew table column header |
| **orders** / **commands** | the 9 skills in `skills/*/SKILL.md` | README "The orders"; site `#orders`; `order-card` components |
| **voyage** | one end-to-end run of a command | README + site "How the voyage works" |
| **come aboard** | installing Shipmates | README + site "Come aboard" / "Get the crew aboard" |
| **weigh anchor** | running your first command | README "Weigh anchor (use it)" |
| **on the horizon** | roadmap, not-yet-shipped work | README "On the horizon" |

### Where the metaphor stops

The metaphor is a wrapper, never a substitute. It must never be the only way a reader learns
something they need in order to act. Drop it — immediately and without apology — for:

- **Install paths and file layout.** `~/.claude/skills/<name>/SKILL.md`, not "where the orders
  are stowed."
- **Precedence and scoping rules.** "A project-level definition wins over a global one of the
  same name." No harbour metaphors.
- **Error states, failure modes, and gates.** "CI must go green before the acceptance board
  runs." Not "the tide must turn."
- **Requirements and prerequisites.** `git`, an authenticated `gh` CLI, a repo with CI.
- **Scope limits and honesty statements.** See ["Scope & honesty" in `AGENTS.md`](../AGENTS.md#scope--honesty).
  Those bullets say what is and isn't shipped in plain words, on purpose.
- **Anything a screen reader must convey.** Emoji are decorative; see
  [Emoji](#emoji).

Rule of thumb: **headings and hooks can sail; instructions dock.** If a sentence tells the
reader to do something or warns them about something, write it plainly.

Do not extend the metaphor with new coinages. The eight terms above are the vocabulary. No
"port", "cargo", "manifest", "first mate", "logbook", "swab", "ahoy", "arr", "matey".

---

## Naming register

**The load-bearing section.** Shipmates ships two artifact types with two correct names, and the
right one depends on what the passage is doing.

### The artifact

Shipmates ships reusable **skills** — the [Agent Skills](https://agentskills.io) open-standard
artifact, one directory per skill with a `SKILL.md` inside:

```
skills/ship-issue/SKILL.md
skills/fix-bug/SKILL.md
…
```

`install.sh` copies them to `~/.claude/skills/<name>/SKILL.md` (global) or
`<repo>/.claude/skills/<name>/SKILL.md` (project-scoped). In the Shipmates product domain,
these same things are **commands you give the crew** — you invoke one by typing `/ship-issue 42`.

Both names are correct. They are not synonyms you may swap at will; they belong to different
registers.

### Which register

| Register | Use | Because |
|---|---|---|
| **Brand-leading** — "command", "orders" | Site hero and section headings, card labels, README intro and tagline, example-usage prose, social copy, release announcements | The reader is being sold an idea. "Give the crew an order" is the product. |
| **Tech-leading** — "skill" | Install paths, repo layout and reference docs, frontmatter documentation, contributor instructions, validation tooling, portability / cross-harness material | The reader is being told a fact they must act on. The fact is the open standard. |

The two overlap in one place and that is fine: a doc may say "the `/ship-issue` **command**,
defined by the `ship-issue` **skill** at `skills/ship-issue/SKILL.md`." Naming the bridge once,
where the reader first meets it, is better than picking a side.

### Retired terms

| Retired | Use instead | Why |
|---|---|---|
| **slash command**, **slash-command workflow** | "command" (brand-leading) or "skill" (tech-leading) | Anthropic's old label. Custom commands were merged into skills. It is also Claude-Code-only, and Shipmates' artifact is a portable standard. |
| `.claude/commands/<name>.md` as the current layout | `.claude/skills/<name>/SKILL.md` | The flat layout is legacy. `install.sh` sweeps it aside on upgrade; document it only in migration notes. |
| **sub-agent**, **sub agent** (hyphenated or spaced) | **subagent**, one word | Matches the Claude Code, Cursor, and OpenCode docs. One spelling, everywhere. |

Plain **"command"** as the product noun is correct and wanted. The retirement is of the
*compound* "slash command", not of the word "command".

`subagent` is the technical noun. "The crew" / "shipmates" / "a shipmate" is the brand
equivalent — use those in brand-leading copy, `subagent` in tech-leading copy.

### The hard rule

> **Never bulk-substitute one noun for the other.** No `sed -i 's/command/skill/g'`. Decide per
> passage: is this sentence selling the idea, or telling the reader a fact they must act on?

A find-and-replace across the repo will produce "install the orders" and "the /ship-issue skill
takes a GitHub issue" in the same document. Both are wrong.

### Do / don't

Real before-and-after, drawn from copy in the repo. The "before" column is the copy as it stood
at commit `9a78bc7`; rewrites land in each surface's own change, not here.

| Surface | Don't (before) | Do (after) | Register |
|---|---|---|---|
| Site `#orders` heading | `Slash command workflows` | `The orders you give the crew` | Brand-leading |
| README subtitle | `Custom sub-agents & slash-command workflows for Claude Code.` | `Custom subagents & commands for Claude Code.` | Brand-leading |
| Site `<title>` | `Shipmates — Claude Code sub-agents & slash-command workflows` | `Shipmates — Claude Code subagents & commands` | Brand-leading |
| README FAQ answer | `slash commands are reusable workflows in .claude/commands/*.md` | `skills are reusable workflows in .claude/skills/<name>/SKILL.md, invoked as commands like /ship-issue` | Tech-leading |
| `CONTRIBUTING.md` heading | `## Adding a command` | `## Adding a skill` | Tech-leading |
| Site FAQ | `builds on Claude Code's public sub-agent and slash-command features` | `builds on Claude Code's public subagent and skill features` | Tech-leading |
| README section heading | `## 📜 The orders (commands)` | *(unchanged — already correct)* | Brand-leading |
| `AGENTS.md` layout bullet | `commands/*.md are slash-command workflows` | `skills/<name>/SKILL.md files are the commands the crew runs` | Tech-leading |

---

## Voice and tone

Derived from the copy that already works. Four rules:

1. **Direct.** Second person, imperative, active. The reader is doing something; say what.
   Short sentences. Em dashes over semicolons.
2. **Wry, not zany.** One dry joke per section at most, and it earns its place by being true
   ("Then go get coffee ☕"). Never a pun for its own sake.
3. **Nautical but never twee.** The metaphor lives in headings, taglines, and section names. It
   does not live in instructions. See [Where the metaphor stops](#where-the-metaphor-stops).
4. **Confident without overclaiming.** State what the thing does. Then state, in the same
   breath, where it doesn't. ["Scope & honesty" in `AGENTS.md`](../AGENTS.md#scope--honesty) is
   the precedent: three blunt bullets about Claude-Code-only support, prompt-driven rather than
   hook-enforced gates, and no Anthropic affiliation. Every surface owes the reader that
   treatment.

### Say / don't say

| Say | Don't say | Why |
|---|---|---|
| "Stop being your AI's for-loop. Give it a crew." | "Supercharge your AI-powered development workflow." | Names the reader's actual pain. No marketing verbs. |
| "Running the crew on opencode / Cursor / Copilot / Codex is on the roadmap, not shipped." | "Works with any coding agent." | Honest about the boundary. Overclaiming is the one unrecoverable brand error. |
| "The gates are driven by the workflow prompt; a code-enforced state machine is planned, not in the current release." | "Enforced, guaranteed gates." | Precise about the mechanism, so nobody is surprised by it. |
| "You stay the captain. The shipmates do the twenty steps in between." | "Fully autonomous, zero-touch AI engineering." | The metaphor doing real work — it says who decides. |
| "Visual specialists flat-out flag *'needs human visual pass'* when they can't render." | "AI-reviewed pixel-perfect UI." | Says what the tool can't do, unprompted. |

### Mechanics

| Item | Convention |
|---|---|
| Person | Second person ("you"). Avoid "we" — the project speaks as the product, not as a company. |
| Headings | Sentence case. `Get the crew aboard`, not `Get The Crew Aboard`. |
| Dashes | Em dash `—` for asides, spaced as in existing copy. |
| Command references | Always with the leading slash and in code: `` `/ship-issue` ``. |
| Role references | Always lowercase-hyphenated and in code: `` `senior-engineer` ``. |
| Product name | `Shipmates`, capital S, always. Never `shipmates` as the product, never `ShipMates`. Lowercase `shipmates` is only the metaphor noun ("the shipmates do the twenty steps"). |

---

## Visual identity

### Palette

Defined as CSS custom properties in [`site/styles.css`](../site/styles.css) (section 1, "Design
tokens"). Components never hardcode a colour; everything references `var(--…)`. These are the
values — light mode is the default, dark mode overrides tokens only.

| Token | Light | Dark | Used for |
|---|---|---|---|
| `--surface` | `#FBFAF9` | `#14110F` | Page background |
| `--surface-2` | `#F4F1EE` | `#1D1916` | Raised panels, cards, alternating sections |
| `--text` | `#1A1714` | `#F2EDE8` | Body and heading text |
| `--text-muted` | `#5C544E` | `#B3A99F` | Secondary text, captions, leads |
| `--accent` | `#D97757` | `#D97757` | UI accents and large text only (≥24px, or ≥18.66px bold) |
| `--accent-strong` | `#AD4526` | `#E8916F` | The accent at small text size — links, inline emphasis |
| `--btn-primary-bg` | `#BF4D2E` | `#E8916F` | Primary button fill |
| `--btn-primary-bg-hover` | `#A8401F` | `#F0A283` | Primary button hover fill |
| `--btn-primary-fg` | `#FFFFFF` | `#1A140F` | Primary button label |
| `--border` | `#E3DED8` | `#332C26` | Default hairlines, card edges |
| `--border-strong` | `#C9C1B8` | `#4A4139` | Emphasised dividers |
| `--success` | `#1E7A52` | `#4ADE94` | Green-CI / pass states |
| `--code-bg` | `#F3EFEA` | `#211C18` | Code block and inline-code background |
| `--code-text` | `#2B2620` | `#EDE7E0` | Code text |
| `--focus-ring` | `#AD4526` | `#E8916F` | Visible keyboard focus |

**`--accent` is not a text colour.** `#D97757` is constant across both themes and is reserved
for UI and large text. For anything small — links, labels, inline emphasis — use
`--accent-strong`, which is the contrast-safe variant and *does* flip between themes. Getting
this wrong is the most common accessibility regression on the site.

Shadows are tokenised too (`--shadow-1`, `--shadow-2`) and are theme-specific: warm ink-tinted
in light mode, plain black at higher opacity in dark.

### Typography

**No webfonts.** The site uses the platform stack — instant first paint, native rendering, and
one less thing to license.

```css
--font-sans: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, Oxygen, Ubuntu, Cantarell,
             "Helvetica Neue", Arial, "Noto Sans", sans-serif,
             "Apple Color Emoji", "Segoe UI Emoji", "Segoe UI Symbol", "Noto Color Emoji";
--font-mono: ui-monospace, SFMono-Regular, "SF Mono", Menlo, Consolas,
             "Liberation Mono", "Courier New", monospace;
```

The emoji families are part of `--font-sans` on purpose — the brand's emoji must render
consistently, even though they never carry meaning.

Scale (`--fs-*`, fluid steps use `clamp()`):

| Token | Value | Used for |
|---|---|---|
| `--fs-900` | `clamp(2rem, 1.15rem + 4vw, 3.25rem)` | Hero `h1`, line-height 1.05 |
| `--fs-800` | `clamp(1.5rem, 1.1rem + 2vw, 2rem)` | Section `h2` |
| `--fs-700` | `1.25rem` | Card `h3` |
| `--fs-600` | `clamp(1.125rem, 1rem + 0.6vw, 1.375rem)` | Lead / hook paragraphs |
| `--fs-500` | `1.125rem` | `h4`, emphasis |
| `--fs-400` | `1rem` | Body |
| `--fs-300` | `0.875rem` | Small / meta |
| `--fs-200` | `0.75rem` | Eyebrow, `kbd` — uppercase, `+0.06em` tracking |

Weights `--fw-normal|medium|semibold|bold` (400/500/600/700); line-heights `--lh-tight` 1.15,
`--lh-snug` 1.3, `--lh-body` 1.6. Spacing and radii are likewise tokens
(`--space-1`…`--space-10`, `--radius-sm|md|lg|pill`); never hardcode them. Content max widths:
`--maxw: 1100px` for layout, `--maxw-prose: 760px` for reading text.

[`site/styles.css`](../site/styles.css) is the source of truth — if these drift, that file wins.

### Logo

[`assets/logo.png`](../assets/logo.png) — a pixel-art sailboat sailing into the sunset, drawn as a
circular badge with a dark navy outline ring.

| Property | Value |
|---|---|
| Master | `assets/logo.png`, 672 × 672, PNG RGBA, transparent outside the badge |
| Small | `assets/logo-240.png`, 240 × 240 — used in the site footer at 32 px |
| Social | `assets/social-preview.png`, 1280 × 640, PNG RGB |
| Palette | 39 colours total — deliberately limited, in keeping with the pixel-art style |
| Style | Hard-edged pixel art. Chunky, aliased, no anti-aliasing on the pixel grid, no gradients other than the banded sky. |

Observed colours in the mark, for anyone producing a matching asset:

| Element | Hex |
|---|---|
| Outline ring | `#1A1F36` |
| Sky (banded gradient, top → horizon) | `#F6D9A2` → `#F2A75C` |
| Sun | `#FFC24A` |
| Sail highlight / shade | `#F7F0E0` / `#D6C5A8` |
| Mast | `#4A3826` |
| Pennant | `#E05A3C` |
| Hull (dark / light) | `#5C3A24` / `#8A5A3C` |
| Sea (surface / deep) | `#2E6E8E` / `#1E425C` |
| Foam | `#7FB8CC` |

Treatment rules:

- **Never resample with smoothing.** Scale by whole-number factors and use nearest-neighbour.
  A blurred Shipmates logo is a broken Shipmates logo.
- **Never recolour, rotate, add effects, or crop the badge.** Use it round and whole.
- **Alt text.** Where the logo carries meaning, describe it: `Shipmates — pixel-art sailboat
  sailing into a sunset`. Where it is purely decorative and adjacent to the wordmark (site
  footer), use `alt=""`.

> `TODO — needs an owner decision`: minimum display size, clear-space rule, and whether a
> monochrome / single-colour variant exists for contexts that need one (stickers, favicons,
> print). Nothing in the repo defines these.

> `TODO — needs an owner decision`: whether the logo palette above is a *brand* palette or
> art-only. It is a different family from the site tokens — the pennant `#E05A3C` is adjacent to
> the site accent `#D97757` but not identical. Right now the site tokens are the brand palette and
> the logo is an illustration; confirm or reconcile.

### Emoji

Emoji are part of the voice. They are also **strictly decorative** — on the site, every one is
marked `aria-hidden="true"`.

| Rule | Detail |
|---|---|
| Never load-bearing | No emoji may be the only carrier of meaning. Remove every emoji from a page and it must still make complete sense. |
| Marked hidden in HTML | `<span aria-hidden="true">⚓</span>`. Never inside an `alt` attribute or a link's accessible name. |
| One per heading, at most | Section headings take a leading emoji; body paragraphs almost never do. |
| Never mid-sentence in instructions | Install steps, error states, and prerequisites stay plain. |
| Never in code, frontmatter, filenames, or identifiers | `name:`, `description:`, slugs, and paths are plain ASCII. |

The established vocabulary:

| Emoji | Meaning | Where |
|---|---|---|
| 🚢 | Shipmates itself | Title, hero, sign-off |
| ⚓ | Coming aboard / install | Install headings, hero tagline |
| 🧭 | The crew | "Meet the crew" |
| 📜 | The orders | "The orders" |
| 🛠️ | How the voyage works | Mechanism sections |
| 🌊 | Roadmap | "On the horizon" |
| ⛵ | Forward-looking aside | "More crew and more orders are on the way." |
| 🫡 | Handoff to the captain | "You stay the captain." |

Each crew role also carries a fixed icon, used identically in the README table and the site crew
cards: 🏛️ `architect` · 🔧 `senior-engineer` · 🧪 `sdet` · 🛡️ `security-engineer` ·
🚨 `site-reliability-engineer` · ⚡ `performance-engineer` · 📋 `product-manager` ·
🎛️ `ux-ui-designer` · 🎨 `art-director` · 📖 `technical-writer` · 📊 `data-scientist`.

The `/ship-issue` stages likewise: 🗺️ Plan · ✏️ Design specs · 📦 Isolate · 🔨 Build ·
🚦 Self-check → CI gate · ⚖️ Acceptance board · 🔁 Remediate · 🏁 Deliver.

> `TODO — needs an owner decision`: whether the per-role and per-stage emoji mappings above are
> frozen. They currently agree across `README.md` and `site/index.html`, but nothing states that a
> new role must claim an unused emoji, or who arbitrates a clash.

---

## Naming things

### New subagent roles

| Rule | Detail |
|---|---|
| **Domain-neutral — the one hard rule** | A role must not name a language, framework, product, vendor, or project. It describes *how the role thinks and works*; the standard it enforces comes from the target repo's `README` / `CLAUDE.md` at run time. A PR that hardcodes a domain will be rejected. See [`AGENTS.md`](../AGENTS.md) and [`CONTRIBUTING.md`](../CONTRIBUTING.md). |
| Format | `lowercase-hyphenated`, matching the filename: `agents/<name>.md`, and the `name` field in frontmatter. |
| Shape | A real job title someone could hold. `senior-engineer`, `technical-writer`, `data-scientist`. Not a metaphor role (`first-mate`), not a verb (`reviewer-of-code`), not an adjective. |
| Length | One or two words, hyphenated. Initialisms allowed where they are the industry title (`sdet`). |
| Scope in the name | Say the discipline, not the seniority ladder — unless seniority is the point of the role, as in `senior-engineer`. |

Good: `security-engineer`, `site-reliability-engineer`, `performance-engineer`, `art-director`.
Bad: `react-expert`, `godot-reviewer`, `our-style-guardian`, `bosun`.

### New orders / skills

| Rule | Detail |
|---|---|
| Format | Imperative verb phrase, `lowercase-hyphenated`. Invoked with a leading slash: `/ship-issue`, `/fix-bug`, `/plan-epics`. |
| Filename | `skills/<name>/SKILL.md`, where `<name>` exactly equals the frontmatter `name` and the slash command. |
| Verb first | The name is an order you give the crew. `/harden`, `/migrate`, `/document`, `/release`, `/polish`, `/spike` — every one starts with the action. |
| Object second, if needed | `/ship-issue`, `/fix-bug`, `/plan-epics`. Singular or plural per what the command actually takes. |
| No nouns-as-names | Not `/quality-gate`, not `/pr-flow`. If you can't phrase it as an instruction, it isn't an order. |
| No harness names | Not `/claude-review`. Skills are meant to be portable. |

The nine that exist: `/ship-issue` · `/fix-bug` · `/plan-epics` · `/harden` · `/spike` ·
`/migrate` · `/document` · `/release` · `/polish`.

---

## Trademark and attribution

Use this wording, unchanged, wherever affiliation could be misread — README FAQ, site FAQ, site
footer, package metadata, social bios:

> Shipmates is an independent, MIT-licensed community project that builds on Claude Code's public
> subagent and skill features. "Claude" and "Claude Code" are trademarks of Anthropic.

The short form, for footers and constrained space (this is the wording live in the site footer):

> MIT License. Not affiliated with Anthropic. "Claude" and "Claude Code" are trademarks of
> Anthropic.

Rules:

- **Never imply endorsement or affiliation.** Not "official", not "partner", not "powered by
  Anthropic". "For Claude Code" and "built on Claude Code" are fine — the README badge
  `made for Claude Code` is the established form.
- **Never put "Claude" in a Shipmates identifier.** No `claude-shipmates`, no `/claude-review`,
  no Anthropic marks in the logo, favicon, or social preview.
- **Capitalise the marks correctly:** `Claude`, `Claude Code`, `Anthropic`.
- **Shipmates is MIT.** [`LICENSE`](../LICENSE). "Take it, fork it, crew up."

> `TODO — needs an owner decision`: whether "Shipmates" and the sailboat logo are claimed as
> marks of this project, and what third parties may do with them (forks, redistributions,
> merchandise). The MIT licence covers the code; it does not settle name and logo use, and
> nothing in the repo states a position.

> `TODO — needs an owner decision`: whether the site's user-visible order URLs
> (`site/commands/<name>/`, and the `#orders` anchor) should move to `skills` to match the
> artifact rename. Those are live URLs; changing them breaks inbound links and needs redirects.

---

## Open decisions

Every `TODO — needs an owner decision` in this document, in one place:

1. Logo minimum size, clear space, and whether a monochrome variant exists. → [Logo](#logo)
2. Whether the logo palette is a brand palette or art-only, and whether it should reconcile with
   the site tokens. → [Logo](#logo)
3. Whether the per-role and per-stage emoji mappings are frozen, and who arbitrates a clash.
   → [Emoji](#emoji)
4. Whether "Shipmates" and the logo are claimed as marks, and what third parties may do with
   them. → [Trademark and attribution](#trademark-and-attribution)
5. Whether the site's `commands/` URL segment and `#orders` anchor move to `skills`, and what
   redirects that needs. → [Trademark and attribution](#trademark-and-attribution)
