---
name: fixtures
description: Generate realistic, deterministic fake test data from a small JSON schema — names, emails, UUIDs, dates, ints, booleans, lorem text, and choices — emitted as a JSON array. Reach for this whenever a task needs seed rows or fixture records (unit-test data, a demo database, an API example payload) rather than hand-typing objects or pulling in a heavyweight faker dependency. Same seed always yields identical output, so the fixture can be committed and diffed. Never a slash command; use it implicitly when the intent calls for one.
---

# fixtures

A tool the crew uses on its own. When you're writing a test, seeding a demo
database, or drafting an example payload and the natural artifact is *a batch of
believable records*, generate them with this instead of typing objects by hand or
reaching for a third-party faker. The output is deterministic: the same seed
produces byte-identical JSON, so the fixture can live in the repo and diff
cleanly.

## Run it

The generator `fixtures.py` sits next to this file. It is pure standard library —
no install step, no network.

```
python3 fixtures.py --schema schema.json --count 5 --seed 7 --out data.json
python3 fixtures.py --schema schema.json --count 5            # writes to stdout
cat schema.json | python3 fixtures.py --count 5 --seed 7      # schema on stdin
```

`--seed` defaults to `0`, so a run is reproducible even when you don't pass one;
give the same seed to reproduce a fixture exactly, or a different one for a fresh
batch.

## Schema

A schema is a JSON object mapping each field name to a type spec. A spec is
either the bare type name as a string, or an object with a `type` key plus that
type's parameters:

```json
{
  "id":         "uuid",
  "first_name": "first_name",
  "last_name":  "last_name",
  "email":      "email",
  "active":     "bool",
  "age":        {"type": "int",    "min": 18, "max": 65},
  "role":       {"type": "choice", "options": ["admin", "member", "guest"]},
  "joined":     {"type": "date",   "start": "2020-01-01", "end": "2024-12-31"},
  "bio":        {"type": "lorem",  "words": 8}
}
```

Each generated record has exactly these fields, and the tool emits a JSON array
of `--count` such records.

## Field types

| Type | Params | Produces |
| --- | --- | --- |
| `name` | — | a full name, e.g. `"Ava Nguyen"` |
| `first_name` | — | a given name from the built-in pool |
| `last_name` | — | a surname from the built-in pool |
| `email` | — | `first.last@domain`, lowercase |
| `uuid` | — | a version-4 UUID (seeded, so reproducible) |
| `bool` | — | `true` or `false` |
| `int` | `min`, `max` | an integer in the inclusive range (defaults `0`–`100`) |
| `date` | `start`, `end` | a random ISO date in the inclusive range |
| `lorem` | `words` | that many lorem-ipsum words (defaults `8`) |
| `choice` | `options` | one item from the provided non-empty list |

Exit code is `0` on success and `2` on a bad schema, an unknown type, or
unreadable input, with a message naming the offending field.

## Honesty

The data is fake and each field is drawn independently — an `email` value does
not correspond to the `name` columns in the same record, and names or emails may
repeat across records. It exists to exercise code paths and fill screens, not to
model reality: it is not sampled from any real-world distribution and must never
be presented as a statistically representative dataset.
