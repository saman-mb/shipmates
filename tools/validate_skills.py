#!/usr/bin/env python3
"""Validate the skills/ payload against the Agent Skills layout — stdlib only.

Runs identically in a local self-check and in CI, with no arguments and no
network: the skills/ tree is the thing the installer copies, so it is gated on
its own terms rather than only through the site it feeds.

Asserts the invariants a skill directory holds: skills/<slug>/SKILL.md exists
for every entry under skills/, its frontmatter opens on line 1 and declares
exactly name, description, argument-hint and allowed-tools in that order, the
name is its own directory name, the description is present and bounded, no
positional argument placeholder (`$1`) survives in prose outside a code fence, and the
retired commands/ directory is gone.

Exit 0 if all green; exit 1 if one or more failures (printed).
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SKILLS = ROOT / "skills"
COMMANDS = ROOT / "commands"

# Exact set AND exact order — a reader (and the installer) sees one shape.
FRONTMATTER_KEYS = ("name", "description", "argument-hint", "allowed-tools")

KEY_RE = re.compile(r"^[A-Za-z0-9_-]+$")
NAME_RE = re.compile(r"^[a-z0-9]+(-[a-z0-9]+)*$")
# Positional argument placeholder. Skills are invoked by description, not by
# argv, so `$1` in a skill body is a leftover that silently never expands.
POSITIONAL_RE = re.compile(r"\$\{?[0-9]")

MAX_NAME = 64
MAX_DESCRIPTION = 1024

KEY_LIST = ", ".join(FRONTMATTER_KEYS)

failures: list[str] = []
notes: list[str] = []


def fail(msg: str) -> None:
    failures.append(msg)


def ok(msg: str) -> None:
    notes.append(msg)


def parse_frontmatter(rel: str, lines: list[str]) -> tuple[dict, int] | None:
    """Return ({key: (lineno, value)}, index of the line after the closing '---').

    None when the block is unusable, in which case the failure is already
    reported and the body checks are skipped for this file.
    """
    if not lines or lines[0].strip() != "---":
        fail(
            f"{rel}:1: no opening frontmatter '---' — a SKILL.md must open with a "
            f"'---' line, then {KEY_LIST}, then a closing '---'"
        )
        return None

    entries: dict[str, tuple[int, str]] = {}
    order: list[str] = []
    for i in range(1, len(lines)):
        raw = lines[i]
        lineno = i + 1
        if raw.strip() == "---":
            check_key_set(rel, lineno, entries, order)
            return entries, i + 1
        if not raw.strip():
            continue
        key, sep, value = raw.partition(":")
        if not sep or not KEY_RE.fullmatch(key):
            fail(
                f"{rel}:{lineno}: not a 'key: value' line ({raw[:60]!r}) — write every "
                "frontmatter entry on a single line (allowed-tools is one "
                "comma-separated value, not a YAML list)"
            )
            continue
        if key in entries:
            fail(
                f"{rel}:{lineno}: duplicate frontmatter key '{key}' (first seen on line "
                f"{entries[key][0]}) — declare each of {KEY_LIST} exactly once"
            )
            continue
        entries[key] = (lineno, value.strip())
        order.append(key)

    fail(
        f"{rel}:{len(lines)}: unterminated frontmatter — close it with a '---' line "
        "before the '# /<skill>' heading"
    )
    return None


def check_key_set(rel: str, close_lineno: int, entries: dict, order: list[str]) -> None:
    """Exactly FRONTMATTER_KEYS, in that order. Duplicates are reported by the caller."""
    drift = False
    for key in order:
        if key not in FRONTMATTER_KEYS:
            fail(
                f"{rel}:{entries[key][0]}: unknown frontmatter key '{key}' — a SKILL.md "
                f"declares exactly {KEY_LIST}"
            )
            drift = True
    for key in FRONTMATTER_KEYS:
        if key not in entries:
            fail(
                f"{rel}:{close_lineno}: frontmatter is missing '{key}' — declare "
                f"{KEY_LIST}, in that order"
            )
            drift = True
    if drift:
        return  # an order report on top of a wrong key set is noise, not a second bug
    if tuple(order) != FRONTMATTER_KEYS:
        first = next(
            key for key, want in zip(order, FRONTMATTER_KEYS) if key != want
        )
        fail(
            f"{rel}:{entries[first][0]}: frontmatter keys out of order (got "
            f"{', '.join(order)}) — declare them in the order {KEY_LIST}"
        )


def check_values(rel: str, slug: str, entries: dict) -> None:
    if "name" in entries:
        lineno, name = entries["name"]
        if name != slug:
            fail(
                f"{rel}:{lineno}: name is {name!r} but the directory is skills/{slug}/ — "
                f"set `name: {slug}`, or rename the directory to skills/{name}/"
            )
        elif not NAME_RE.fullmatch(name):
            fail(
                f"{rel}:{lineno}: name {name!r} is not lowercase-hyphen — use lowercase "
                "letters and digits joined by single hyphens (e.g. ship-issue)"
            )
        if len(name) > MAX_NAME:
            fail(
                f"{rel}:{lineno}: name is {len(name)} characters, max {MAX_NAME} — "
                "shorten it (the directory name must match, so rename both)"
            )
    if "description" in entries:
        lineno, description = entries["description"]
        if not description:
            fail(
                f"{rel}:{lineno}: description is empty — write one line saying what the "
                "skill does and when to use it (this is what the model matches on)"
            )
        elif len(description) > MAX_DESCRIPTION:
            fail(
                f"{rel}:{lineno}: description is {len(description)} characters, max "
                f"{MAX_DESCRIPTION} — shorten it to a single summary line"
            )


def check_body(rel: str, lines: list[str], start: int) -> None:
    """No positional argument placeholder outside a fenced code block.

    The fence test strips leading whitespace first: a fence indented under a
    list item is still a fence, and `awk '{print $2}'` inside one is a shell
    field reference, not a leftover command argument. Mirrors the fence handling
    in .github/scripts/validate_site.py's check_fidelity.
    """
    in_fence = False
    for offset, raw in enumerate(lines[start:]):
        lineno = start + offset + 1
        s = raw.strip()
        if s.startswith("```"):
            in_fence = not in_fence
            continue
        if in_fence:
            continue
        hit = POSITIONAL_RE.search(raw)
        if hit:
            fail(
                f"{rel}:{lineno}: {hit.group(0)!r} outside a fenced code block — a skill "
                "has no positional arguments; write $ARGUMENTS, or describe the input in "
                "prose (shell field references belong inside a ``` fence)"
            )


def check_skill(directory: Path) -> None:
    slug = directory.name
    rel = f"skills/{slug}/SKILL.md"
    path = directory / "SKILL.md"
    before = len(failures)

    if not path.is_file():
        fail(
            f"{rel}:1: file not found — every directory under skills/ is a skill and "
            f"must contain a SKILL.md; add it, or delete skills/{slug}/"
        )
        return

    lines = path.read_text(encoding="utf-8").split("\n")
    parsed = parse_frontmatter(rel, lines)
    if parsed is None:
        return
    entries, start = parsed
    check_values(rel, slug, entries)
    check_body(rel, lines, start)

    if len(failures) == before:
        ok(f"{rel}: frontmatter ({KEY_LIST}), name matches directory, no bare '$n' in prose")


def main() -> int:
    if COMMANDS.exists():
        fail(
            "commands/: directory still exists — the workflows moved to "
            "skills/<slug>/SKILL.md; run `git rm -r commands/` so there is one source"
        )
    else:
        ok("commands/: absent — skills/ is the single source for the workflows")

    if not SKILLS.is_dir():
        fail(
            "skills/: directory not found — the workflows live in skills/<slug>/SKILL.md, "
            "one directory per skill"
        )
        return report()

    entries = sorted(SKILLS.iterdir())
    directories = [p for p in entries if p.is_dir()]
    for path in entries:
        if not path.is_dir():
            fail(
                f"skills/{path.name}:1: not a directory — every entry under skills/ is a "
                f"skill directory holding a SKILL.md; move it into skills/<slug>/ or delete it"
            )
    if not directories:
        fail("skills/: no skill directories — add skills/<slug>/SKILL.md, one per workflow")
        return report()

    before = len(failures)
    for directory in directories:
        check_skill(directory)
    if len(failures) == before:
        ok(f"skills/: {len(directories)} skill directory(ies), each with a valid SKILL.md")
    return report()


def report() -> int:
    for n in notes:
        print(f"  ok   {n}")
    if failures:
        print()
        for f in failures:
            print(f"  FAIL {f}")
        print(f"\n{len(failures)} failure(s).")
        return 1
    print(f"\nAll {len(notes)} checks passed.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
