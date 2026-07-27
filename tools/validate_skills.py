#!/usr/bin/env python3
"""Validate the skills/ payload against the Agent Skills layout — stdlib only.

Runs identically in a local self-check and in CI, with no arguments and no
network: the skills/ tree is the thing the installer copies, so it is gated on
its own terms rather than only through the site it feeds.

Asserts the invariants a skill directory holds: skills/<slug>/SKILL.md exists
for every entry under skills/, its frontmatter opens on line 1 and declares
exactly name, description, argument-hint and allowed-tools in that order, the
name is its own directory name, every declared value carries content (and the
description is bounded), no unescaped positional argument placeholder (`$1`)
survives anywhere in the file, every fenced code block is closed, and the
retired commands/ directory is gone.

Positionals are flagged inside fenced code blocks too. An earlier revision
exempted fences, on the assumption that a `$2` between ``` markers is read as a
shell field reference rather than substituted. That assumption was never
verified — nothing documents a fence exemption, and the documented way to keep a
literal `$` before a digit is the backslash escape `\\$1`, which would be
pointless if fences were exempt — so it was removed. Substitution is treated as
textual over the whole file: `/ship-issue 42 focus on retries` binds `$2` to
`on` and rewrites `awk '{print $2}'` to `awk '{print on}'`.

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
# Positional argument placeholder, anywhere in the file. Skills are invoked by
# description, not by argv, so `$1` is either a leftover that never expands or —
# worse, inside a fence — live text the invocation rewrites. A backslash-escaped
# `\$1` is the documented literal, so the lookbehind lets it through.
POSITIONAL_RE = re.compile(r"(?<!\\)\$\{?[0-9]")

MAX_NAME = 64
MAX_DESCRIPTION = 1024

# Presence and order are not enough: a declared-but-empty value ships a broken
# skill. One remedy per key, phrased as what to write instead.
EMPTY_REMEDY = {
    "description": (
        "write one line saying what the skill does and when to use it (this is "
        "what the model matches on)"
    ),
    "argument-hint": (
        "write the shape of the invocation text (e.g. `<issue-number> [optional "
        "extra guidance]`)"
    ),
    "allowed-tools": (
        "list the tools the skill needs as one comma-separated value (e.g. "
        "`Bash, Read, Write, Edit, Agent`)"
    ),
}

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
    for key, remedy in EMPTY_REMEDY.items():
        if key in entries and not entries[key][1]:
            fail(f"{rel}:{entries[key][0]}: {key} is empty — {remedy}")
    if entries.get("description", (0, ""))[1]:
        lineno, description = entries["description"]
        if len(description) > MAX_DESCRIPTION:
            fail(
                f"{rel}:{lineno}: description is {len(description)} characters, max "
                f"{MAX_DESCRIPTION} — shorten it to a single summary line"
            )


def check_frontmatter(rel: str, lines: list[str], start: int) -> None:
    """No positional placeholder in the frontmatter either.

    Substitution is textual over the whole file, so `argument-hint: <... $1>` is
    rewritten exactly like a body line. Scanned raw, over the whole block
    including its '---' delimiters, so a line that failed to parse as
    'key: value' is still covered.
    """
    for lineno in range(1, start + 1):
        hit = POSITIONAL_RE.search(lines[lineno - 1])
        if hit:
            fail(
                f"{rel}:{lineno}: {hit.group(0)!r} in the frontmatter — a skill has no "
                "positional arguments, and frontmatter is substituted like the body; "
                "describe the input shape instead (e.g. `<issue-number> [optional extra "
                "guidance]`), or escape a literal as `\\$1`"
            )


def check_body(rel: str, lines: list[str], start: int) -> None:
    """No positional placeholder in the body, fenced or not, and no open fence.

    Fences are still tracked, but only so the report can name where the hit is:
    a fence does not exempt anything (see the module docstring). The fence test
    strips leading whitespace first — a fence indented under a list item is
    still a fence — mirroring the fence handling in
    .github/scripts/validate_site.py's check_fidelity.
    """
    fence_lineno = 0
    for offset, raw in enumerate(lines[start:]):
        lineno = start + offset + 1
        if raw.strip().startswith("```"):
            fence_lineno = lineno if not fence_lineno else 0
            continue
        hit = POSITIONAL_RE.search(raw)
        if not hit:
            continue
        if fence_lineno:
            fail(
                f"{rel}:{lineno}: {hit.group(0)!r} inside the fenced code block opened on "
                f"line {fence_lineno} — substitution is textual over the whole file, so a "
                "``` fence does not protect it (`/ship-issue 42 focus on retries` would "
                "make `awk '{print $2}'` read `awk '{print on}'`); restructure to avoid "
                "`$` before a digit (e.g. `cut -f2` instead of `awk '{print $2}'`), or "
                "escape it as `\\$2`"
            )
        else:
            fail(
                f"{rel}:{lineno}: {hit.group(0)!r} in the body — a skill has no positional "
                "arguments; write $ARGUMENTS, describe the input in prose, or escape a "
                "literal as `\\$1`"
            )

    if fence_lineno:
        fail(
            f"{rel}:{fence_lineno}: unterminated fenced code block — close it with a "
            "matching ``` line; an open fence swallows the rest of the file for the site "
            "generator and the site validator too"
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
    check_frontmatter(rel, lines, start)
    check_body(rel, lines, start)

    if len(failures) == before:
        ok(
            f"{rel}: frontmatter ({KEY_LIST}) present, ordered and non-empty, name matches "
            "directory, no unescaped '$n' anywhere, fences closed"
        )


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
