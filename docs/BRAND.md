# Brand

A writing guide for Shipmates' identity, voice, and naming. It is a reference for people making
copy decisions, not an authority over what is shipped.

Who this is for: anyone (human or agent) writing README copy, site copy, skill and agent
descriptions, release notes, issue titles, or social text for this project.

**What owns what.** Two things here have a canonical home elsewhere, and that home wins:

| Subject | Canonical source |
|---|---|
| Colour, type scale, spacing, radius, shadow values | [`site/styles.css`](../site/styles.css), section 1 "Design tokens" |
| The exact wording of any shipped string | The file that ships it (`README.md`, `site/index.html`, `crew/*.md`, `commands/*.md`) |

Everything else — the metaphor, the naming register, voice, emoji, naming rules for new roles and
commands, trademark wording — is stated here and nowhere else, so this is where to change it.

`AGENTS.md` and `CONTRIBUTING.md` carry a condensed version of the
[naming register](#naming-register) and link here for the full statement.

Open brand questions — logo clear-space and monochrome variant, the logo palette's status, whether
the emoji mappings are frozen, and the name/logo trademark position — are tracked in issue #85, not
as TODOs in this file.

The published `commands/<name>/` URL segment is **settled**, not open: under the three-tier naming
above, a page documenting one of the twelve documents a *command*, so the path is correct. That
supersedes an earlier decision to move it to `skills/`, which was taken before "command" and "order"
were distinguished. Recorded on issue #72; do not reopen it here.

---

## What Shipmates is

Shipmates is an open-source (MIT) crew of specialist AI agents on
[Claude Code](https://code.claude.com/docs) today: **12 domain-neutral subagents** and
**12 reusable commands**. Its flagship, `/ship-issue`, takes a GitHub issue all the way to a
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
| **crew** / **shipmates** | the 12 subagents, authored in `crew/*.md` | README "Meet the crew"; site `#crew`; `crew-card` components |
| **shipmate** | a single subagent role | README crew table column header |
| **command** | one of the 12 workflows, authored in `commands/*.md`, as the captain issues it | README "The commands"; site `#commands`; `order-card` components |
| **order** | what one subagent is told to do *inside* a command | Nowhere yet — reserved for per-subagent copy; see [The narrow sense of "order"](#the-narrow-sense-of-order) |
| **voyage** | one end-to-end run of a command | README + site "How the voyage works" |
| **come aboard** | installing Shipmates | README + site "Come aboard" / "Get the crew aboard" |
| **weigh anchor** | running your first command | README "Weigh anchor (use it)" |
| **on the horizon** | roadmap, not-yet-shipped work | README "On the horizon" |

### Where the metaphor stops

The metaphor is a wrapper, never a substitute. It must never be the only way a reader learns
something they need in order to act. Drop it — immediately and without apology — for:

- **Install paths and file layout.** `~/.claude/skills/<name>/SKILL.md`, not "where the commands
  are stowed."
- **Precedence and scoping rules.** Name the two scopes and say plainly which one wins. No
  harbour metaphors.
- **Error states, failure modes, and gates.** "CI must go green before the acceptance board
  runs." Not "the tide must turn."
- **Requirements and prerequisites.** `git`, an authenticated `gh` CLI, a repo with CI.
- **Scope limits and honesty statements.** See ["Scope & honesty" in `AGENTS.md`](../AGENTS.md#scope--honesty).
  Those bullets say what is and isn't shipped in plain words, on purpose.
- **Anything a screen reader must convey.** Emoji are decorative; see
  [Emoji](#emoji).

Rule of thumb: **headings and hooks can sail; instructions dock.** If a sentence tells the
reader to do something or warns them about something, write it plainly.

Do not extend the metaphor with new coinages. The nine terms above are the vocabulary. No
"port", "cargo", "manifest", "first mate", "logbook", "swab", "ahoy", "arr", "matey".

---

## Naming register

**The load-bearing section.** Three nouns, three jobs. They are not synonyms and they may not be
swapped at will.

| Term | Register | What it names |
|---|---|---|
| **skill** | technical | The artifact on disk — `skills/<name>/SKILL.md`, in the [Agent Skills](https://agentskills.io) open-standard shape. |
| **command** | brand | A whole workflow the captain issues to the crew — `/ship-issue`, `/fix-bug`. The twelve of them are **the commands**. |
| **order** | brand | What a single subagent is told to do *within* a command — one specialist's instruction. |

The metaphor holds the three together: the captain issues a **command** to the crew, and carrying
it out means individual crew members receive **orders**. On disk, that command is a **skill**.

### The artifact

Shipmates ships reusable **skills** — the [Agent Skills](https://agentskills.io) open-standard
artifact, one directory per skill with a `SKILL.md` inside:

```
.claude/skills/ship-issue/SKILL.md
.claude/skills/fix-bug/SKILL.md
…
```

That is the *installed* shape on Claude Code. In this repository the authored
source is `commands/ship-issue.md`; the `skills/` layout is produced by the
exporter at install time and is never committed.

`install.sh` copies them to `~/.claude/skills/<name>/SKILL.md` (global) or
`<repo>/.claude/skills/<name>/SKILL.md` (project-scoped). In the Shipmates product domain,
these same things are **commands you give the crew** — you invoke one by typing `/ship-issue 42`.

Both names are correct for that one object. They are not synonyms you may swap at will; they
belong to different registers.

### The narrow sense of "order"

**"Order" is not a synonym for "command", and it is not a name for the twelve.** It is deliberately
smaller: an order is what one crew member is told to do inside a run. `/ship-issue` is a command;
"root-cause the failure and write the minimal fix", handed to `senior-engineer` at the remediation
loop, is an order. Test it by rewriting: if the sentence still works with "one of the twelve
workflows", the word is **command**; if it only works with "one specialist's instruction", the word
is **order**.

This is a narrowing. Until this taxonomy landed, "orders" was the brand plural for the
workflows — README's `## 📜 The orders (commands)`, the site's `#orders` section, the detail pages'
`← All orders`. Those surfaces say **commands** now, and the `#orders` anchor no longer exists;
`#commands` replaces it.

Worked examples:

- ✅ "Install the crew, then run your first **command**: `/ship-issue 42`." — brand, the whole workflow.
- ✅ "`/ship-issue` is defined by the `ship-issue` **skill**, installed at `.claude/skills/ship-issue/SKILL.md`." — technical, the file on disk.
- ✅ "Every reviewer on the acceptance board gets the same **order**: judge the pushed PR head, independently." — brand, one subagent's instruction inside one command.
- ❌ "Shipmates ships twelve **orders**." — they are **commands**.
- ❌ "Install the **orders** into `~/.claude/`." — wrong twice: wrong tier, and install paths are tech-leading, so it is **skills**.

### Which register

| Register | Use | Because |
|---|---|---|
| **Brand-leading** — "command", "order" | Site hero and section headings, card labels, README intro and tagline, example-usage prose, social copy, release announcements | The reader is being sold an idea. "Give the crew a command" is the product. |
| **Tech-leading** — "skill" | Install paths, repo layout and reference docs, frontmatter documentation, contributor instructions, validation tooling, portability / cross-harness material | The reader is being told a fact they must act on. The fact is the open standard. |

The two overlap in one place and that is fine: a doc may say "the `/ship-issue` **command**,
defined by the `ship-issue` **skill**, installed at `.claude/skills/ship-issue/SKILL.md`." Naming the bridge once,
where the reader first meets it, is better than picking a side.

### Retired terms

| Retired | Use instead | Why |
|---|---|---|
| **slash command**, **slash-command workflow** | "command" (brand-leading) or "skill" (tech-leading) | Anthropic's old label. Custom commands were merged into skills. It is also Claude-Code-only, and Shipmates' artifact is a portable standard. |
| `.claude/commands/<name>.md` as the current layout | `.claude/skills/<name>/SKILL.md` | The flat layout is legacy. `install.sh` sweeps it aside on upgrade; document it only in migration notes. |
| **sub-agent**, **sub agent** (hyphenated or spaced) | **subagent**, one word | Matches the Claude Code, Cursor, and OpenCode docs. One spelling, everywhere. |
| **the orders**, **an order** *meaning one of the twelve workflows* | **the commands**, **a command** | "Order" was narrowed to a single subagent's instruction inside a command. Using it for the twelve collapses two tiers of the taxonomy. |

Plain **"command"** as the product noun is correct and wanted. The retirement is of the
*compound* "slash command", not of the word "command".

**"Order" is not retired — it is narrowed.** It stays in the vocabulary in its new, smaller sense
([above](#the-narrow-sense-of-order)); what is retired is using it for the twelve workflows.

`subagent` is the technical noun. "The crew" / "shipmates" / "a shipmate" is the brand
equivalent — use those in brand-leading copy, `subagent` in tech-leading copy.

### The hard rule

> **Never bulk-substitute one noun for another.** No `sed -i 's/command/skill/g'`, no
> `s/orders/commands/g`. Decide per passage: is this sentence selling the idea, or telling the
> reader a fact they must act on — and does it mean the whole workflow or one specialist's
> instruction?

A find-and-replace across the repo will produce "install the commands" and "the /ship-issue skill
takes a GitHub issue" in the same document. Both are wrong. It will also rewrite the English word
— "the stages, in order", "in order to act", the `order-card` CSS class — which is a different word
that happens to be spelled the same. Read every hit.

### Do / don't

Worked examples, not prescriptions. The middle column is wording this repo used and no longer
ships — from the slash-command rename (commit `9a78bc7`) and the later narrowing of "order"; the
right column is the string the repo actually ships today. If a surface is rewritten again, the
surface is right and this table is stale — fix the table.

| Surface | Retired | Shipped today | Register |
|---|---|---|---|
| Site commands-section heading (`h2#commands-title`) | `Slash command workflows` | `Command workflows` | Brand-leading |
| README subtitle | `Custom sub-agents & slash-command workflows for Claude Code.` | `Custom subagents & command workflows for Claude Code.` | Brand-leading |
| Site `<title>` | `Shipmates — Claude Code sub-agents & slash-command workflows` | `Shipmates — Claude Code subagents & command workflows` | Brand-leading |
| README FAQ answer | `slash commands are reusable workflows in .claude/commands/*.md` | `skills are reusable workflows defined in .claude/skills/<name>/SKILL.md and invoked as commands, like /ship-issue` | Tech-leading |
| `CONTRIBUTING.md` heading | `## Adding a command` | `## Adding a skill (workflow)` | Tech-leading |
| Site FAQ | `builds on Claude Code's public sub-agent and slash-command features` | `builds on Claude Code's public subagent and skill features` | Tech-leading |
| README section heading | `## 📜 The orders (commands)` | `## 📜 The commands` | Brand-leading |
| `AGENTS.md` layout bullet | `commands/*.md are the slash-command workflows` | `skills/<name>/SKILL.md are the workflows, in the Agent Skills format` | Tech-leading |
| `AGENTS.md` section heading | `## The orders (12)` | `## The commands (12)` | Brand-leading |
| Site nav and footer link | `Orders` → `#orders` | `Commands` → `#commands` | Brand-leading |
| Detail-page back link | `← All orders` | `← All commands` | Brand-leading |
| Site commands-section lead | `Open any order for its full stage-by-stage breakdown.` | `Open any command for its full stage-by-stage breakdown.` | Brand-leading |

---

## Voice and tone

Derived from the copy that already works. Four rules:

1. **Direct.** Second person, imperative, active. The reader is doing something; say what.
   Short sentences. Em dashes over semicolons.
2. **Wry, not zany.** One dry joke per section at most, and it earns its place by being true
   ("Then go get a coffee ☕"). Never a pun for its own sake.
3. **Nautical but never twee.** The metaphor lives in headings, taglines, and section names. It
   does not live in instructions. See [Where the metaphor stops](#where-the-metaphor-stops).
4. **Confident without overclaiming.** State what the thing does. Then state, in the same
   breath, where it doesn't. ["Scope & honesty" in `AGENTS.md`](../AGENTS.md#scope--honesty) is
   the precedent: three blunt bullets about Claude-Code-only support, prompt-driven rather than
   prompt-driven gates, and no Anthropic affiliation. Every surface owes the reader that
   treatment.

### Say / don't say

| Say | Don't say | Why |
|---|---|---|
| "Stop being your AI's for-loop. Give it a crew." | "Supercharge your AI-powered development workflow." | Names the reader's actual pain. No marketing verbs. |
| "Running the crew on opencode / Cursor / Copilot / Codex is on the roadmap, not shipped." | "Works with any coding agent." | Honest about the boundary. Overclaiming is the one unrecoverable brand error. |
| "The gates are defined by the structured workflow and explicit quality checks; no code-enforced state machine or tool-boundary hook is shipped." | "Enforced, guaranteed gates." | Precise about the mechanism, so nobody is surprised by it. |
| "You stay the captain. The shipmates do the twenty steps in between." | "Fully autonomous, zero-touch AI engineering." | The metaphor doing real work — it says who decides. |
| "Visual specialists flat-out flag *'needs a human visual pass'* when they can't render." | "AI-reviewed pixel-perfect UI." | Says what the tool can't do, unprompted. |

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

### Design tokens

Every colour, font, size, spacing, radius, and shadow value lives in
[`site/styles.css`](../site/styles.css), section 1 "Design tokens", as CSS custom properties with a
dark-mode override block. That file is the canonical source and the only place to change one; no
selector hardcodes a value, everything references `var(--…)`. The values are deliberately not
copied into this document — a second copy would drift and nothing would catch it.

Two rules that belong here, because they are judgement rather than values:

- **`--accent` is not a text colour.** It is for UI and large text only (≥24px, or ≥18.66px
  bold). For anything small — links, labels, inline emphasis — use `--accent-strong`, the
  contrast-safe variant. Getting this wrong is the most common accessibility regression on the site.
- **No webfonts.** The site uses the platform font stack: instant first paint, native rendering, and
  nothing to license. The emoji families in `--font-sans` are there on purpose, so the brand's emoji
  render consistently — they still never carry meaning. See [Emoji](#emoji).

### Logo

[`site/assets/logo.png`](../site/assets/logo.png) — a pixel-art sailboat sailing into the sunset, drawn as a
circular badge with a dark navy outline ring.

| Property | Value |
|---|---|
| Master | `site/assets/logo.png`, 672 × 672, PNG RGBA, transparent outside the badge |
| Small | `site/assets/logo-240.png`, 240 × 240 — used in the site footer at 32 px |
| Social | `site/assets/social-preview.png`, 1280 × 640, PNG RGB |
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
| 📜 | The commands | "The commands" |
| 🛠️ | How the voyage works | Mechanism sections |
| 🌊 | Roadmap | "On the horizon" |
| ⛵ | Forward-looking aside | "More crew and more commands are on the way." |
| 🫡 | Handoff to the captain | "You stay the captain." |

Each crew role also carries a fixed icon, used identically in the README table and the site crew
cards: 🏛️ `architect` · 🔧 `senior-engineer` · 🧪 `sdet` · 🛡️ `security-engineer` ·
🚨 `site-reliability-engineer` · ⚡ `performance-engineer` · 📋 `product-manager` ·
🎛️ `ux-ui-designer` · 🎨 `art-director` · 📖 `technical-writer` · 📊 `data-scientist`.

The `/ship-issue` stages likewise: 🗺️ Plan · ✏️ Design specs · 📦 Isolate · 🔨 Build ·
🚦 Self-check → CI gate · ⚖️ Acceptance board · 🔁 Remediate · 🏁 Deliver.

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

### New commands / skills

| Rule | Detail |
|---|---|
| Format | Imperative verb phrase, `lowercase-hyphenated`. Invoked with a leading slash: `/ship-issue`, `/fix-bug`, `/plan-epics`. |
| Filename | `skills/<name>/SKILL.md`, where `<name>` exactly equals the frontmatter `name` and the command you type (`/<name>`). |
| Verb first | The name is a command you give the crew. `/harden`, `/migrate`, `/document`, `/release`, `/polish`, `/spike` — every one starts with the action. |
| Object second, if needed | `/ship-issue`, `/fix-bug`, `/plan-epics`. Singular or plural per what the command actually takes. |
| No nouns-as-names | Not `/quality-gate`, not `/pr-flow`. If you can't phrase it as an instruction, it isn't a command. |
| No harness names | Not `/claude-review`. Skills are meant to be portable. |

The twelve that exist: `/ship-issue` · `/fix-bug` · `/plan-epics` · `/harden` · `/spike` ·
`/migrate` · `/document` · `/release` · `/polish` · `/pr-review` · `/onboard` · `/refactor`.

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

The MIT licence covers the code. It does not settle name and logo use, and this project has not
stated a position on that; that question is one of the open brand issues on the tracker.
