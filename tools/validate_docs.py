#!/usr/bin/env python3
"""Validate README.md, CONTRIBUTING.md and AGENTS.md against commands/ — stdlib only.

Runs identically in a local self-check and in CI, with no arguments and no
network: the three root documents are what a contributor — and the model reading
them — uses to decide what to run, so a stale claim there is a wrong instruction
delivered to everyone who reads it. Two mechanical claims are checked, both at
grep level, deliberately without any prose understanding:

1. Config tokens (`MODE`, `MERGE_MODE`, `PUBLISH_MODE`, `NITS_MODE`, `DRY_RUN`)
   referenced in a root document. The token must be declared by at least one
   commands/<slug>.md, and a value the document assigns to it (`MODE=pr`) must be
   a value that declaration allows. Declarations are read from the command
   sources — the `## Config` section and the `## Parameters` table — and are
   never restated here, so a new mode or a renamed value is picked up
   automatically and only the documents can drift. A bare token with no value
   (`DRY_RUN`) carries no value to resolve; only its declaration is required.
   Out of scope on purpose: environment variables that configure a *test
   harness* (`SHIPMATES_SRC`, `CODEX_SMOKE`) are not command config.

2. Command references. A literal `/slug` in a root document must be a current
   `commands/<slug>.md`, or a name in FOREIGN below — the explicit list of slash
   tokens that are not Shipmates commands, one reason per entry. Anything shaped
   like an invocation and not on that list fails, so a typo and a retired slug
   are caught the same way.

   Retired slugs are not commands. `REDIRECTS` in tools/gen_command_pages.py
   emits static-site meta-refresh stubs for renamed commands; nothing in a stub
   resolves an invocation, so `/document` or `/ship-issue` in a root document is
   a stale reference that fails here, exactly like a typo.

   A slash token that is not invocation-shaped — a path, a URL tail, an HTML tag
   (`</p>`), a path segment (`<name>/index.html`) — is not a reference and is
   skipped. FOREIGN is where a genuine exception goes, never the regex.

This checker does not re-implement the SLUGS-versus-tree gate: `load_skills()`
in tools/gen_command_pages.py already enforces it in both directions — a command
on disk missing from SLUGS, and a SLUGS entry with no command file — and CI runs
`python3 tools/gen_command_pages.py --check` for that. Neither does this tool
read prose: a re-told process, or a behaviour claim about another workflow, is
invisible to a token scan. That half is author discipline, which is why AGENTS.md
carries the link-don't-restate rule beside this gate.

Exit 0 if all green; exit 1 if one or more failures (printed).
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
COMMANDS = ROOT / "commands"

# The root documents: what a contributor reads before touching anything, and the
# three files that can quote a command without being one.
DOCS = ("README.md", "CONTRIBUTING.md", "AGENTS.md")

# The config vocabulary these three documents speak in. An explicit list, the
# way the generator keeps SLUGS explicit — but only the *names* live here: which
# commands ship each token, and which values they allow, is derived from
# commands/*.md (see declared_values()), so this file cannot become a second
# source of truth for a mode or a value.
CONFIG_TOKENS = ("MODE", "MERGE_MODE", "PUBLISH_MODE", "NITS_MODE", "DRY_RUN")

# Slash tokens in the root documents that are deliberately not command
# references, each with the single-sentence reason it is exempt — an entry with
# no reason is how a real typo gets waved through for good.
FOREIGN = {
    "review": (
        "Claude Code's own built-in code review — the collision this project's "
        "naming gate exists to avoid, quoted in CONTRIBUTING.md as the example"
    ),
    "skill": (
        "Pi's native invocation syntax, `/skill:<name>` — a harness surface, not "
        "a Shipmates command"
    ),
    "harness-watch": (
        "this repo's own dev wrapper for the manual harness-docs drift check "
        "(`tools/harness_watch.py`) — maintainer tooling, never installed"
    ),
    "name": (
        "the placeholder for a command's name in a root document (`/name "
        "invocation`), not a reference to any command"
    ),
}

# Section tracking. Declarations are read from the `## Config` section and the
# `## Parameters` table only — never from prose, where the same token appears in
# stage instructions that are not declarations.
HEADING_RE = re.compile(r"^##\s")
CONFIG_HEADING_RE = re.compile(r"^##\s+Config\b", re.IGNORECASE)
PARAMETERS_HEADING_RE = re.compile(r"^##\s+Parameters\b", re.IGNORECASE)
TABLE_ROW_RE = re.compile(r"^\s*\|")

# `TOKEN` = `value` (a Config bullet) — the declaration form the commands use.
CONFIG_DECL_RE = re.compile(
    r"`([A-Z][A-Z0-9_]*)`\s*=\s*(?:`([^`]+)`|([a-z][a-z0-9-]*))"
)
# `TOKEN=value` — the same declaration written as an assignment. The lookbehind
# keeps `MODE` from matching inside `MERGE_MODE`.
ASSIGN_RE = re.compile(r"(?<![A-Za-z0-9_])([A-Z][A-Z0-9_]+)=([a-z][a-z0-9-]*)")
# One value out of a Parameters `Values` cell: "manual  /  auto", "on  /  off".
# Matched whole, not scanned for words, so a spelled-out alternative inside a
# cell (`dry-run` / `dry run` / `preview`) contributes the values that are
# actually assignable and not the words `dry` and `run` out of the middle.
ENUM_VALUE_RE = re.compile(r"[a-z][a-z0-9-]*")

# Config token referenced in a root document, with the value assigned to it when
# the reference carries one (`MODE=pr`); a bare reference (`DRY_RUN`) has none.
# Built from CONFIG_TOKENS so the vocabulary has one home; the lookbehind keeps
# `MODE` from matching inside `MERGE_MODE`.
CONFIG_REF_RE = re.compile(
    r"(?<![A-Za-z0-9_])(" + "|".join(CONFIG_TOKENS) + r")"
    r"(?![A-Za-z0-9_])(?:=([a-z][a-z0-9-]*))?"
)

# `/slug` in a root document. The lookbehind rejects a slash that continues a
# path or URL (`.github/scripts`, `https://…`, `src/adapters/`) and an HTML tag
# (`</p>`); the lookahead rejects one that opens a path segment or carries a
# file extension (`<name>/index.html`). What survives is invocation-shaped.
SLASH_REF_RE = re.compile(
    r"(?<![\w./~$:<>-])/([a-z][a-z0-9]*(?:-[a-z0-9]+)*)(?![A-Za-z0-9_/.\-])"
)

failures: list[str] = []
notes: list[str] = []


def fail(msg: str) -> None:
    failures.append(msg)


def ok(msg: str) -> None:
    notes.append(msg)


def declared_values() -> dict[str, set[str]]:
    """Token -> the values commands/*.md declare for it.

    Every token in CONFIG_TOKENS gets an entry, empty when no command declares
    it — so a token nobody ships is reported rather than silently skipped.
    """
    values: dict[str, set[str]] = {t: set() for t in CONFIG_TOKENS}

    def add(token: str, value: str) -> None:
        if token in values and value:
            values[token].add(value)

    for path in sorted(COMMANDS.glob("*.md")):
        section = ""
        for line in path.read_text(encoding="utf-8").splitlines():
            if HEADING_RE.match(line):
                if CONFIG_HEADING_RE.match(line):
                    section = "config"
                elif PARAMETERS_HEADING_RE.match(line):
                    section = "parameters"
                else:
                    section = ""
                continue
            if section == "config":
                for m in CONFIG_DECL_RE.finditer(line):
                    add(m.group(1), m.group(2) or m.group(3))
                for m in ASSIGN_RE.finditer(line):
                    add(m.group(1), m.group(2))
            elif section == "parameters" and TABLE_ROW_RE.match(line):
                # Name | Required | Token | Values | Default | Help
                cells = [c.strip() for c in line.strip().strip("|").split("|")]
                if len(cells) > 4 and cells[0].upper() in values:
                    for value in cells[3].split("/"):
                        value = value.strip().strip("`").strip()
                        if ENUM_VALUE_RE.fullmatch(value):
                            add(cells[0].upper(), value)
                for m in ASSIGN_RE.finditer(line):
                    add(m.group(1), m.group(2))
    return values


def check_config_tokens(rel: str, lines: list[str], declared: dict[str, set[str]]) -> int:
    """Every config token reference must resolve. Returns the reference count."""
    count = 0
    for lineno, line in enumerate(lines, 1):
        for m in CONFIG_REF_RE.finditer(line):
            count += 1
            token, value = m.group(1), m.group(2)
            known = sorted(declared[token])
            if not known:
                fail(
                    f"{rel}:{lineno}: `{token}` is declared by no command — name a token a "
                    f"commands/<slug>.md actually ships (its `## Config` section or "
                    f"`## Parameters` table), or drop the reference"
                )
            elif value and value not in declared[token]:
                fail(
                    f"{rel}:{lineno}: `{token}={value}` is not a value any command declares — "
                    f"commands/*.md declare {token} as {', '.join(known)}"
                )
    return count


def check_command_refs(rel: str, lines: list[str], current: set[str]) -> int:
    """Every invocation-shaped reference must be a current command. Returns the count."""
    count = 0
    for lineno, line in enumerate(lines, 1):
        for m in SLASH_REF_RE.finditer(line):
            count += 1
            name = m.group(1)
            if name in current or name in FOREIGN:
                continue
            fail(
                f"{rel}:{lineno}: `/{name}` is not a current command — point at the existing "
                f"commands/<slug>.md instead; a renamed command leaves only a redirect stub "
                f"(REDIRECTS in tools/gen_command_pages.py), which resolves nothing, and a "
                f"token that is genuinely not a Shipmates command belongs in FOREIGN in "
                f"tools/validate_docs.py with the reason"
            )
    return count


def check_doc(rel: str, declared: dict[str, set[str]], current: set[str]) -> None:
    path = ROOT / rel
    if not path.is_file():
        fail(f"{rel}: file not found — the root documents are README.md, CONTRIBUTING.md and AGENTS.md")
        return
    lines = path.read_text(encoding="utf-8").split("\n")

    before = len(failures)
    tokens = check_config_tokens(rel, lines, declared)
    if len(failures) == before:
        ok(f"{rel}: {tokens} config token reference(s) resolve to a command declaration")

    before = len(failures)
    refs = check_command_refs(rel, lines, current)
    if len(failures) == before:
        ok(
            f"{rel}: {refs} command reference(s) are current commands or declared foreign syntax"
        )


def main() -> int:
    if not COMMANDS.is_dir():
        fail(
            "commands/: directory not found — the workflows live in commands/<slug>.md, "
            "one file per command"
        )
        return report()

    current = {p.stem for p in sorted(COMMANDS.glob("*.md"))}
    if not current:
        fail("commands/: no command files — nothing for the root documents to reference")
        return report()

    declared = declared_values()
    ok(
        f"commands/: {len(current)} command file(s) declare "
        + ", ".join(f"{t} ({'/'.join(sorted(declared[t])) or 'never'})" for t in CONFIG_TOKENS)
    )

    for rel in DOCS:
        check_doc(rel, declared, current)
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
