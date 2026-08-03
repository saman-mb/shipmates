#!/usr/bin/env python3
"""fixtures — generate realistic, deterministic fake test data from a small schema.

Self-contained: the standard library is the only dependency (``random``, ``json``,
``uuid``, ``datetime``). No network, no external wordlists — the name and lorem
pools are baked into this file. Given the same ``--seed`` it emits byte-identical
output every run, so a fixture can be committed and diffed like any other source.

This is the runnable payload of the shipmates ``fixtures`` tool. An agent reaches
for it, per tool.md, when a task implies authoring test data or seed rows — a unit
test needing a handful of user records, a demo database, an API example — instead
of hand-typing objects or pulling a heavyweight faker dependency. It is never a
slash command.

Usage:
    python3 fixtures.py --schema schema.json --count 5 --seed 7 --out data.json
    python3 fixtures.py --schema schema.json --count 5      # writes to stdout
    cat schema.json | python3 fixtures.py --count 5 --seed 7 # schema on stdin

Schema (JSON): a map of field name -> type spec. A spec is either the bare type
name as a string, or an object with a ``type`` key plus that type's parameters:

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

Supported types:
    name, first_name, last_name  — drawn from built-in name pools
    email                        — first.last@domain, lowercase
    uuid                         — a version-4 UUID (seeded, so reproducible)
    bool                         — true / false
    int      {min, max}          — inclusive, defaults 0..100
    date     {start, end}        — random ISO date in an inclusive range
    lorem    {words}             — n lorem-ipsum words, defaults 8
    choice   {options}           — one item from a provided non-empty list

The data is fake and independent per field (an email need not match the name
columns). It is for exercising code paths, not for statistics — it is not drawn
from any real-world distribution and should never stand in for a representative
sample.

Exit codes: 0 ok; 2 on a bad schema, unknown type, or unreadable input.
"""
import argparse
import datetime
import json
import random
import sys
import uuid

# --- Built-in wordlists (small, baked in — no external files) --------------

FIRST_NAMES = [
    "Ava", "Liam", "Noah", "Mia", "Ethan", "Sofia", "Lucas", "Isla", "Mason",
    "Emma", "Leo", "Aria", "Kai", "Nora", "Omar", "Zoe", "Ivan", "Maya",
    "Theo", "Ruby", "Priya", "Diego", "Hana", "Yusuf", "Elena", "Marcus",
    "Wren", "Sana", "Felix", "Nadia",
]
LAST_NAMES = [
    "Nguyen", "Patel", "Kim", "Garcia", "Okafor", "Silva", "Haddad", "Rossi",
    "Novak", "Larsson", "Chen", "Mbeki", "Fischer", "Costa", "Ali", "Ivanov",
    "Sato", "Moreau", "Khan", "Weber", "Reyes", "Andersson", "Dubois",
    "Bauer", "Santos", "Yilmaz", "Petrov", "Kowalski", "Mensah", "Romano",
]
EMAIL_DOMAINS = [
    "example.com", "test.dev", "mail.example", "demo.io", "sample.org",
    "inbox.test", "acme.example", "post.dev",
]
LOREM = [
    "lorem", "ipsum", "dolor", "sit", "amet", "consectetur", "adipiscing",
    "elit", "sed", "do", "eiusmod", "tempor", "incididunt", "labore", "dolore",
    "magna", "aliqua", "enim", "minim", "veniam", "quis", "nostrud",
    "exercitation", "ullamco", "laboris", "aliquip", "commodo", "consequat",
]

DEFAULT_START = "2000-01-01"
DEFAULT_END = "2025-12-31"


class SchemaError(Exception):
    """The schema, or a field spec inside it, is malformed or references an
    unknown type. Surfaced to the user as a helpful message and exit code 2."""


# --- Field generators ------------------------------------------------------
# Each takes the seeded RNG and the field's parameter dict, and returns a
# JSON-serialisable value. Every draw goes through `rng`, so the whole output
# is a pure function of the seed.


def _gen_first_name(rng, spec):
    return rng.choice(FIRST_NAMES)


def _gen_last_name(rng, spec):
    return rng.choice(LAST_NAMES)


def _gen_name(rng, spec):
    return f"{rng.choice(FIRST_NAMES)} {rng.choice(LAST_NAMES)}"


def _gen_email(rng, spec):
    first = rng.choice(FIRST_NAMES).lower()
    last = rng.choice(LAST_NAMES).lower()
    domain = rng.choice(EMAIL_DOMAINS)
    return f"{first}.{last}@{domain}"


def _gen_uuid(rng, spec):
    # uuid.uuid4() reads os.urandom and would ignore the seed; build a
    # version-4 UUID from the seeded RNG instead so runs stay reproducible.
    return str(uuid.UUID(int=rng.getrandbits(128), version=4))


def _gen_bool(rng, spec):
    return rng.random() < 0.5


def _gen_int(rng, spec):
    lo = _as_int(spec.get("min", 0), "min")
    hi = _as_int(spec.get("max", 100), "max")
    if lo > hi:
        raise SchemaError(f"int min ({lo}) is greater than max ({hi})")
    return rng.randint(lo, hi)


def _gen_lorem(rng, spec):
    n = _as_int(spec.get("words", 8), "words")
    if n < 1:
        raise SchemaError(f"lorem 'words' must be >= 1, got {n}")
    words = [rng.choice(LOREM) for _ in range(n)]
    words[0] = words[0].capitalize()
    return " ".join(words)


def _gen_date(rng, spec):
    start = _parse_date(spec.get("start", DEFAULT_START), "start")
    end = _parse_date(spec.get("end", DEFAULT_END), "end")
    if start > end:
        raise SchemaError(f"date start ({start.isoformat()}) is after end ({end.isoformat()})")
    ordinal = rng.randint(start.toordinal(), end.toordinal())
    return datetime.date.fromordinal(ordinal).isoformat()


def _gen_choice(rng, spec):
    options = spec.get("options")
    if not isinstance(options, list) or not options:
        raise SchemaError("choice needs a non-empty 'options' list")
    return rng.choice(options)


GENERATORS = {
    "name": _gen_name,
    "first_name": _gen_first_name,
    "last_name": _gen_last_name,
    "email": _gen_email,
    "uuid": _gen_uuid,
    "bool": _gen_bool,
    "int": _gen_int,
    "lorem": _gen_lorem,
    "date": _gen_date,
    "choice": _gen_choice,
}


# --- Helpers ---------------------------------------------------------------


def _as_int(value, label):
    if isinstance(value, bool) or not isinstance(value, int):
        raise SchemaError(f"'{label}' must be an integer, got {value!r}")
    return value


def _parse_date(value, label):
    if not isinstance(value, str):
        raise SchemaError(f"date '{label}' must be an ISO date string, got {value!r}")
    try:
        return datetime.date.fromisoformat(value)
    except ValueError:
        raise SchemaError(f"date '{label}' is not a valid ISO date (YYYY-MM-DD): {value!r}")


def _normalize(field, spec):
    """Resolve one field spec to (type_name, params_dict)."""
    if isinstance(spec, str):
        return spec, {}
    if isinstance(spec, dict):
        type_name = spec.get("type")
        if not isinstance(type_name, str):
            raise SchemaError(
                f"field {field!r}: an object spec needs a string 'type' key"
            )
        return type_name, spec
    raise SchemaError(
        f"field {field!r}: spec must be a type name (string) or an object with a 'type'"
    )


def generate(schema, count, seed):
    """Return a list of `count` records built from `schema`, seeded by `seed`.

    Same (schema, count, seed) always yields identical output. Raises
    SchemaError on any malformed field spec or unknown type.
    """
    if not isinstance(schema, dict):
        raise SchemaError(
            "schema must be a JSON object mapping field names to type specs"
        )
    if not schema:
        raise SchemaError("schema is empty — add at least one field")

    # Resolve and validate every field up front so a bad type fails before we
    # emit anything, with the offending field named.
    plan = []
    supported = ", ".join(sorted(GENERATORS))
    for field, spec in schema.items():
        type_name, params = _normalize(field, spec)
        generator = GENERATORS.get(type_name)
        if generator is None:
            raise SchemaError(
                f"field {field!r}: unknown type {type_name!r}. supported types: {supported}"
            )
        plan.append((field, type_name, generator, params))

    rng = random.Random(seed)
    records = []
    for _ in range(count):
        record = {}
        for field, type_name, generator, params in plan:
            try:
                record[field] = generator(rng, params)
            except SchemaError as exc:
                raise SchemaError(f"field {field!r} ({type_name}): {exc}")
        records.append(record)
    return records


# --- CLI -------------------------------------------------------------------


def main(argv=None):
    ap = argparse.ArgumentParser(
        description="Generate deterministic fake test data from a small JSON schema."
    )
    ap.add_argument("--schema", help="path to a JSON schema file (default: stdin)")
    ap.add_argument("--count", type=int, default=10, help="number of records (default: 10)")
    ap.add_argument("--seed", type=int, default=0,
                    help="RNG seed; same seed => identical output (default: 0)")
    ap.add_argument("--out", help="output JSON path (default: stdout)")
    args = ap.parse_args(argv)

    if args.count < 0:
        print("fixtures: --count must be >= 0", file=sys.stderr)
        return 2

    try:
        raw = open(args.schema, encoding="utf-8").read() if args.schema else sys.stdin.read()
        schema = json.loads(raw)
    except (OSError, json.JSONDecodeError) as exc:
        print(f"fixtures: could not read schema: {exc}", file=sys.stderr)
        return 2

    try:
        records = generate(schema, args.count, args.seed)
    except SchemaError as exc:
        print(f"fixtures: bad schema: {exc}", file=sys.stderr)
        return 2

    text = json.dumps(records, indent=2) + "\n"
    if args.out:
        try:
            with open(args.out, "w", encoding="utf-8") as fh:
                fh.write(text)
        except OSError as exc:
            print(f"fixtures: could not write output: {exc}", file=sys.stderr)
            return 2
        print(f"fixtures: wrote {args.out} ({len(records)} records)")
    else:
        sys.stdout.write(text)
    return 0


if __name__ == "__main__":
    sys.exit(main())
