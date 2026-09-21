#!/usr/bin/env python3
"""Validate the harness roster audit (`tools/harness_roster.json`).

The roster answers one question — which Agent Skills-compatible clients could be
Shipmates targets, and what each one costs — so it is only worth anything if it
cannot silently drift away from what the repo actually ships. This check keeps
the four things that rot fastest honest:

1. **The shipped set is the real one.** Every target in `tools/manifest.json`
   has a `shipped` row, and no row claims `shipped` while the binary does not
   install it. A rename lands in one place and is caught in the other.
2. **Every row carries first-party evidence.** A URL, a date, and — for a row
   that claims the shared tree — the literal `.agents/skills` path string it was
   read from. A claim with no evidence is a guess, and this repo records guesses
   as `watch`.
3. **A deferred decision is never silent.** `skip`, `watch` and `native-adapter`
   rows must say why in prose. A blank reason would let a client quietly vanish
   from consideration.
4. **The census still reconciles.** The showcase rows plus the showcase clients
   already covered by a shipped target must add up to the client count recorded
   from the source page, so a client added upstream is noticed rather than
   assumed.

Config-only: no network. Run it after touching the roster, the target list, or
a harness name.
"""

from __future__ import annotations

import json
import re
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).parent.parent
ROSTER = REPO_ROOT / "tools" / "harness_roster.json"
MANIFEST = REPO_ROOT / "tools" / "manifest.json"
WATCH = REPO_ROOT / "tools" / "harness_watch.json"

VERDICTS = {"shipped", "shared-free", "native-adapter", "watch", "skip"}
TREES = {"shared", "native", "commands", "unknown"}
SUBAGENTS = {"yes", "no", "not emitted", "unknown"}
SHARED_TREE_PATH = ".agents/skills"
DATE = re.compile(r"^\d{4}-\d{2}-\d{2}$")
SLUG = re.compile(r"^[a-z0-9]+(?:-[a-z0-9]+)*$")
MIN_REASON = 20

errors: list[str] = []


def fail(message: str) -> None:
    errors.append(message)


def main() -> int:
    roster = json.loads(ROSTER.read_text(encoding="utf-8"))
    manifest = json.loads(MANIFEST.read_text(encoding="utf-8"))
    watch = json.loads(WATCH.read_text(encoding="utf-8"))

    if roster.get("schema_version") != 1:
        fail(f"schema_version must be 1, found {roster.get('schema_version')!r}")

    policy = roster.get("policy") or {}
    for key in ("shared-tree", "named-target", "evidence", "promotion"):
        if not policy.get(key):
            fail(f"policy.{key} is missing — the audit's conclusions must be stated, not implied")

    source = roster.get("source") or {}
    for key in ("url", "checked_on", "clients_listed", "covered_by_shipped"):
        if key not in source:
            fail(f"source.{key} is missing")
    if source.get("checked_on") and not DATE.match(str(source["checked_on"])):
        fail(f"source.checked_on must be YYYY-MM-DD, found {source['checked_on']!r}")

    rows = roster.get("harnesses")
    if not isinstance(rows, list) or not rows:
        fail("harnesses must be a non-empty list")
        return report()

    targets = list(manifest.get("targets") or [])
    seen: set[str] = set()
    shipped: set[str] = set()
    showcase_rows = 0

    for index, row in enumerate(rows):
        where = f"harnesses[{index}]"
        ident = row.get("id", "")
        where = f"{where} ({ident or 'no id'})"

        if not ident or not SLUG.match(str(ident)):
            fail(f"{where}: id must be a kebab-case slug")
        elif ident in seen:
            fail(f"{where}: duplicate id")
        else:
            seen.add(ident)

        if not row.get("name"):
            fail(f"{where}: name is required")

        verdict = row.get("verdict")
        if verdict not in VERDICTS:
            fail(f"{where}: verdict {verdict!r} is not one of {sorted(VERDICTS)}")
        if verdict == "shipped":
            shipped.add(str(ident))

        tree = row.get("tree")
        if tree not in TREES:
            fail(f"{where}: tree {tree!r} is not one of {sorted(TREES)}")

        subagents = row.get("subagents")
        if subagents not in SUBAGENTS:
            fail(f"{where}: subagents {subagents!r} is not one of {sorted(SUBAGENTS)}")

        evidence = row.get("evidence") or {}
        url = str(evidence.get("url", ""))
        if not url.startswith("https://"):
            fail(f"{where}: evidence.url must be an https first-party URL, found {url!r}")
        if not DATE.match(str(evidence.get("checked_on", ""))):
            fail(f"{where}: evidence.checked_on must be YYYY-MM-DD")
        quote = str(evidence.get("quote", ""))
        if tree == "shared" and SHARED_TREE_PATH not in quote:
            fail(
                f"{where}: tree is 'shared' but the evidence quote does not contain "
                f"{SHARED_TREE_PATH!r} — a shared-tree claim needs the literal path it came from"
            )

        if verdict in {"skip", "watch", "native-adapter"}:
            reason = str(row.get("reason", ""))
            if len(reason) < MIN_REASON:
                fail(f"{where}: verdict {verdict!r} needs a reason (>= {MIN_REASON} chars), found {reason!r}")

        if row.get("source") == "showcase":
            showcase_rows += 1

    missing = sorted(set(targets) - shipped)
    extra = sorted(shipped - set(targets))
    if missing:
        fail(f"targets with no `shipped` roster row: {', '.join(missing)}")
    if extra:
        fail(f"rows marked `shipped` that tools/manifest.json does not install: {', '.join(extra)}")

    for target in sorted(shipped):
        if target not in (watch.get("harnesses") or {}):
            fail(f"shipped target {target!r} has no tools/harness_watch.json entry")

    covered = source.get("covered_by_shipped") or []
    unknown_covered = sorted(set(map(str, covered)) - shipped)
    if unknown_covered:
        fail(
            "source.covered_by_shipped names clients that are not shipped rows: "
            + ", ".join(unknown_covered)
        )
    listed = source.get("clients_listed")
    if isinstance(listed, int) and showcase_rows + len(covered) != listed:
        fail(
            f"the census no longer reconciles: {showcase_rows} showcase rows + "
            f"{len(covered)} covered by a shipped target != {listed} clients listed at the source"
        )

    return report(rows=len(rows), shipped=len(shipped), showcase=showcase_rows)


def report(rows: int = 0, shipped: int = 0, showcase: int = 0) -> int:
    if errors:
        for error in errors:
            print(f"error: {error}", file=sys.stderr)
        print(f"\nharness roster: {len(errors)} problem(s)", file=sys.stderr)
        return 1
    print(
        f"harness roster: ok ({rows} rows, {shipped} shipped targets, "
        f"{showcase} showcase clients, evidence + verdicts consistent)"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
