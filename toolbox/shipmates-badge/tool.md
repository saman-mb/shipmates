---
name: shipmates-badge
description: Shipmates: Render a shields-style status badge — a grey label paired with a coloured message — as an offline SVG committed straight into the repo, with no network round-trip to shields.io. Reach for this whenever a task calls for a README, docs, or release-note status badge (build passing, version, coverage, license) and the badge should live in git rather than hot-linking an external service. Never a slash command; use it implicitly when the intent calls for one.
---

# badge

A tool the crew uses on its own. When you're producing a README, a docs page, or
a release note and the natural artifact is *a status badge* — build passing, a
version tag, a coverage number, a licence — render one with this instead of
hot-linking `img.shields.io`. The result is a small, self-contained SVG you
commit next to the doc, so it renders forever, offline, and diffs cleanly.

## Run it

The renderer `badge.py` sits next to this file. It is pure standard library —
no dependencies, no network — so a run is deterministic: the same arguments
always produce byte-for-byte the same SVG.

```
python3 badge.py --label build --message passing --color green --out badge.svg
# omit --out to write the SVG to stdout instead:
python3 badge.py --label coverage --message 98% --color brightgreen
```

`--label` is the grey left segment, `--message` the coloured right segment, and
`--color` (or `--colour`) tints the message. Colours are either a named value —
`brightgreen`, `green`, `yellowgreen`, `yellow`, `orange`, `red`, `blue`,
`purple`, `lightgrey`, `grey`, plus the semantic aliases `success`, `important`,
`critical`, `informational`, `inactive` — or any `#rgb` / `#rrggbb` hex value.
Exit codes: `0` ok; `2` on a bad colour, a missing argument, or a write error.

## Example

```
python3 badge.py --label version --message v0.1.3 --color blue --out version.svg
```

produces the classic two-segment flat badge: a grey `version` on the left, a
blue `v0.1.3` on the right, height 20, with rounded outer corners and a square
inner join. Segment widths are sized from a baked DejaVu Sans advance-width table
(the same metrics shields uses) and locked with SVG `textLength`, so the text is
never clipped and never loose no matter which font the viewer has installed.

## Honesty

A badge is a *static snapshot*, not a live status: it shows exactly the label,
message, and colour you pass, frozen at the moment you render it. It does not
poll CI, read a coverage report, or update itself. Set the message to a value
that is true when you commit it, and re-render it when that value changes —
don't imply a `passing` or `98%` you haven't actually confirmed.
