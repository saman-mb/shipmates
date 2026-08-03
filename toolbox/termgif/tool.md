---
name: termgif
description: Render a polished animated terminal demo GIF of a command or workflow run — a typed prompt, staged progress with check-offs, and a closing line — from a small JSON spec. Reach for this whenever a task calls for a terminal recording or demo GIF (a README hero, docs page, release note, or social preview) rather than describing the run in prose. Never a slash command; use it implicitly when the intent calls for one.
---

# termgif

A tool the crew uses on its own. When you're producing documentation, a README,
a release note, or a launch asset and the natural artifact is *a terminal
recording of a workflow running*, render one with this instead of pasting static
text or asking the user to record their screen.

## Run it

The renderer `termgif.py` sits next to this file. It needs Pillow
(`pip install Pillow`); it uses DejaVu Sans Mono when available and falls back to
Pillow's built-in font otherwise.

```
python3 termgif.py --spec spec.json --out demo.gif
# or pipe the spec on stdin:
echo '{"title":"…","beats":[…]}' | python3 termgif.py --out demo.gif
```

## Spec

```json
{
  "title": "shipmates — /ship-issue",
  "width": 860,
  "beats": [
    {"type": "command", "text": "/ship-issue 142"},
    {"type": "blank"},
    {"type": "stage", "label": "PLAN",  "detail": "work units, acceptance criteria"},
    {"type": "stage", "label": "BUILD", "detail": "senior-engineer x3, in parallel"},
    {"type": "line",  "text": "Installed harness: claude-code", "color": "white"},
    {"type": "done",  "text": "Reviewed, CI-green PR — handed to you."}
  ]
}
```

Beat types: `command` (typed after a `$` prompt), `stage` (spinner then a green
check, with a `label` and `detail`; an optional `color` overrides the auto-cycled
accent), `line` (a revealed output line — `text`+`color`, or a `segments` array
of `{text,color,bold}`), `blank`, and `done` (a green ✓ closing line). Colors:
`prompt green white grey blue purple orange cyan coral gold sage faint`.

## Honesty

Depict the real steps a workflow performs, with generic labels — no invented
counts or fabricated file names. This is the same discipline the shipmates site
GIFs are held to.
