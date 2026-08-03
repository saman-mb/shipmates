---
name: social-card
description: Render a polished 1280x640 social / Open Graph preview card — an eyebrow kicker, a large wrapping title, a muted subtitle, an accent rule, and a footer wordmark — from a small JSON spec. Reach for this whenever a task produces something worth sharing (a repo or product launch, a release, a docs or guide page) and the natural artifact is a share image rather than a described one. Never a slash command; the crew reach for it implicitly when the intent of a prompt calls for one.
requires: pillow
---

# social-card

A tool the crew uses on its own. When you're shipping a launch, a release, or a
docs page and the natural artifact is *the preview image that shows up when the
link is shared* — the Open Graph / Twitter card — render one with this instead
of hand-composing an image or asking the user to open a design app.

## Run it

The renderer `social_card.py` sits next to this file. It needs Pillow, which it
installs for itself the first time it runs (into a private cache) if it is not
already present — you never have to install anything. It uses DejaVu Sans /
DejaVu Sans Bold when available and falls back to Pillow's built-in font otherwise.

```
python3 social_card.py --spec spec.json --out card.png
# or pipe the spec on stdin:
echo '{"title":"…"}' | python3 social_card.py --out card.png
```

The output is always a 1280x640 PNG — the standard Open Graph aspect that
Twitter, Slack, Discord, LinkedIn, and iMessage crop from.

## Spec

```json
{
  "eyebrow":  "Now open source",
  "title":    "Aurora — a typed HTTP client that reads like prose",
  "subtitle": "Composable requests, first-class retries, and a test recorder built in.",
  "accent":   "#7c8cff",
  "wordmark": "github.com/aurora-http/aurora"
}
```

Keys: `eyebrow` (a small kicker, drawn uppercased inside an accent-tinted pill),
`title` (large, bold, wraps — the only required key), `subtitle` (muted, wraps),
`accent` (a hex colour that drives the pill and the footer dot), and `wordmark`
(footer text). Two optional keys override the palette: `bg` and `fg` (hex). Long
`title` and `subtitle` text wraps and the type auto-fits, so copy of any length
stays inside the frame.

## Honesty

Write real, specific card copy — the actual product name, the actual version,
the real one-line pitch — not placeholder lorem. The renderer lays out exactly
what the spec says; it does not invent a headline, fetch a logo, or embed
imagery, and it draws no metrics you did not put in the subtitle. It produces a
single static PNG (no animation, no reduced-motion variant). If you need the
card to carry a logo or screenshot, say so — that is outside what this draws.
