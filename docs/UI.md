# UI & UX guidelines — the website

Rules for anything under `site/`. They are general on purpose: apply them to a new
page, a new component, or a fix, not just to the page that taught us them.

Two gates enforce part of this file mechanically —
`.github/scripts/validate_site.py` (structure, self-containment, SEO) and
`.github/scripts/validate_design_drift.py` (layout consistency, tokens, grid
slack, dead anchors). A rule here that a gate can check is checked there; the
rest is on the author, and no gate passing means a page looks right.

## 1. Layout and rhythm

- **Two widths, and only two.** `.container` (`--maxw`, 1100px) for a section's
  outer frame; `.container--prose` (`--maxw-prose`, 760px) for anything you
  read as prose. Every section shares one left edge — the drift check fails a
  page whose sections disagree.
- **Spacing comes from a container, not from hope.** Prose rhythm is
  `.order-prose > * + *`: only **direct children** of a prose block get the
  vertical measure. A component placed as a *sibling* of the prose block gets
  none, which is how a card grid ends up flush against the paragraph above it.
  Either put the block inside the prose container, or give it its own
  `margin-top` token. **Check the computed gap, do not eyeball it** — see §8.
- **Tokens only.** No raw colour or length outside the `:root` block; the drift
  check fails one. Structural exceptions (hairlines, outline offsets) are listed
  in that script rather than invented per page.
- **One step of space between groups, in one direction.** Prefer a grid `gap` or
  a single `margin-top` over stacking margins on both neighbours.

## 2. Type and colour

- **Use the scale:** `--fs-900` … `--fs-200`. Body copy is `--fs-400`; supporting
  text `--fs-300`; eyebrows and pills `--fs-200` or `--fs-300`. If a label looks
  small, first check whether it is being *scaled down* (§5) before shrinking the
  token further.
- **`--accent` is not a text colour** at small sizes. It is for UI and large text
  only (≥24px, or ≥18.66px bold). Small text and links use `--accent-strong`.
- **Never introduce a colour pair.** Use a pairing the system already validated:
  `--btn-primary-bg` with `--btn-primary-fg`, or `--on-accent` on an `--accent`
  fill. Both themes must hold, so check the dark override too.
- **Colour is never the only signal.** A state, tag or status carries its word
  first; colour and shape reinforce it. `local`/`cloud` is a filled pill versus a
  dashed outline *and* the literal words.

## 3. Components

- **One role, one implementation.** Two eyebrows, two badges or two card shells
  that render differently are the defect the drift check exists to catch. Reuse
  the existing class before inventing a sibling.
- **Multi-row grids set `align-content`.** With `align-items` alone, equal-height
  stretch leaks into the row gaps and the rhythm silently drifts.
- **Interactive and scrollable things are keyboard-reachable.** A scrollable
  region needs `tabindex="0"`; prefer native elements over styled divs.
- **No JavaScript, no external hosts.** The site is static and self-contained —
  no CDN fonts, styles, scripts or analytics. `validate_site.py` fails an
  external reference, and it is a privacy and offline-reading decision, not only
  a build constraint.
- **Respect reduced motion.** Anything that moves must be covered by the
  `prefers-reduced-motion` block, or have a static alternative.

## 4. Tables

- **One idea per column**, with a header cell for each.
- **Do not make a table carry prose.** If a cell needs a paragraph, the content
  wants a card or a definition list instead.
- **Wrap the table in `.order-table`** so the narrow-screen scroll behaviour and
  the focus ring come with it.

## 5. Images, diagrams and demos

- **A fixed-pixel image is scaled by the column, and so is its text.** A 1400px
  chart inside the 760px prose column renders at ~54%, so 15px labels land at
  about 8px and browser zoom does not help. Before shipping an image that
  contains text, compute its final size: **aim for labels ≥14px as rendered**,
  not in the source file.
- **Anything whose content is text should be built as HTML.** It stays crisp,
  zooms, wraps, is selectable, and a screen reader can read it. Reach for an
  image when the artifact genuinely is a picture — a chart, a screenshot, a
  recording.
- **Generate images with the repo's own tools**, never a hand-edited binary:
  `shipmates-diagram` for diagrams, `shipmates-termgif` for terminal demos,
  `shipmates-social-card`, `shipmates-sparkline`. Their output is deterministic,
  so a rebuild produces the same bytes and a diff means a real change.
- **Every image gets `width`, `height`, `alt` and, where it helps, a
  `<figcaption>`.** Dimensions prevent layout shift; the alt text describes what
  the thing shows, not that it exists.
- **A diagram that lies is worse than a paragraph that is vague.** Show the real
  components, states and transitions with plain labels.

## 6. Content and structure

- **Heading order is `h1` then `h2`, never skipping a level.** One `h1` per page.
- **Every page has a unique `<title>` and meta description** — both are gated.
- **SEO head is not optional:** canonical link, Open Graph and Twitter tags, and
  `TechArticle` JSON-LD whose `url` matches the canonical. Copying an existing
  page's head and rewriting the values is the fastest safe path.
- **State the limit, not just the feature.** A page that cannot do something says
  so plainly. An unproven claim is labelled as unproven, with the evidence
  record named instead of asserted.
- **Link the product it is about.** A page about a harness, tool or integration
  links that product's own site the first time it is named.

## 7. Adding a docs page

1. Create `site/docs/<slug>/index.html` (`site/docs/` leaves are hand-authored,
   never generated).
2. Add the slug to `DOCS_SLUGS` in `tools/gen_command_pages.py`, then run
   `python3 tools/gen_command_pages.py` — that writes the sitemap. A page that
   exists but is not in `DOCS_SLUGS` fails both the sitemap check and the docs
   coverage check.
3. Link it from `site/docs/index.html` (both the card grid and the "All docs
   pages" sibling list), and from wherever a reader would enter it — the harness
   row, the tool page, the homepage.
4. Put assets in `site/docs/<slug>/assets/`.

`validate_site.py` derives its expected leaf list from the directories on disk,
so a half-added page fails rather than shipping quietly.

## 8. Verify visually, and measure

Reading the CSS is not evidence. Before calling a page done:

```bash
python3 .github/scripts/validate_site.py
python3 .github/scripts/validate_design_drift.py
python3 tools/gen_command_pages.py --check
python3 tools/validate_docs.py
```

Then look at it, at **360, 768, 1200 and 1440 px**, in a real browser. Screenshot
and measure rather than trusting the source — a headless browser is enough:

```bash
# full-page screenshots at each width, plus overflow and gap measurements
"/Applications/Google Chrome.app/Contents/MacOS/Google Chrome" \
  --headless --disable-gpu --hide-scrollbars --window-size=1200,2400 \
  --screenshot=/tmp/page.png "http://127.0.0.1:8000/docs/<slug>/"
```

Check, at minimum:

- **page overflow** — `documentElement.scrollWidth - clientWidth` must be `0`;
- **gaps between groups** — a `0px` gap between a paragraph and the block under it
  is the bug this file was written for, and it is invisible in the CSS;
- **rendered font sizes** — `getComputedStyle(el).fontSize` of a label, in px, on
  a page whose images have been scaled;
- **both themes** — light and dark, since every colour pair must hold in both.

A pre-existing defect on an unrelated page is not yours to fix in passing; name it
instead of silently widening scope.
