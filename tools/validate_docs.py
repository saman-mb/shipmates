#!/usr/bin/env python3
"""Validate README.md, CONTRIBUTING.md and AGENTS.md against commands/ — stdlib only.

Runs identically in a local self-check and in CI, with no arguments and no
network: the three root documents are what a contributor — and the model reading
them — uses to decide what to run, so a stale claim there is a wrong instruction
delivered to everyone who reads it. Three mechanical claims are checked:

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

3. Shared bootstrap doctrine sync. The doctrine blocks shared between
   docs/COST.md and steering/bootstrap.md (six-principles, command-preamble,
   model-routing, acceptance-board, epic-integration-board, subagent-preamble,
   why-merge-pr) must be identical byte-for-byte so agents receive one
   unambiguous ruleset.

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
# `MODE` from matching inside `MERGE_MODE`. The value is matched case-insensitively
# on its first character so a mistyped assignment (`MODE=EDIT`) is captured and
# resolved (and fails) rather than read as a bare token and waved through.
CONFIG_REF_RE = re.compile(
    r"(?<![A-Za-z0-9_])(" + "|".join(CONFIG_TOKENS) + r")"
    r"(?![A-Za-z0-9_])(?:=([A-Za-z][A-Za-z0-9-]*))?"
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


def find_drift_lines(
    c_text: str, c_start: int, c_end: int, c_content: str,
    b_text: str, b_start: int, b_end: int, b_content: str,
) -> tuple[int, int]:
    """Return 1-based (cost_lineno, boot_lineno) where drift starts."""
    c_lines = c_content.splitlines()
    b_lines = b_content.splitlines()
    for cl, bl in zip(c_lines, b_lines):
        if cl != bl:
            c_idx = c_text.find(cl, c_start)
            b_idx = b_text.find(bl, b_start)
            c_line = c_text[:c_idx].count("\n") + 1 if c_idx != -1 else c_text[:c_start].count("\n") + 1
            b_line = b_text[:b_idx].count("\n") + 1 if b_idx != -1 else b_text[:b_start].count("\n") + 1
            return c_line, b_line
    if len(c_lines) > len(b_lines):
        extra = c_lines[len(b_lines)]
        c_idx = c_text.find(extra, c_start)
        c_line = c_text[:c_idx].count("\n") + 1 if c_idx != -1 else c_text[:c_end].count("\n") + 1
        b_line = b_text[:b_end].count("\n") + 1
        return c_line, b_line
    elif len(b_lines) > len(c_lines):
        extra = b_lines[len(c_lines)]
        b_idx = b_text.find(extra, b_start)
        b_line = b_text[:b_idx].count("\n") + 1 if b_idx != -1 else b_text[:b_end].count("\n") + 1
        c_line = c_text[:c_end].count("\n") + 1
        return c_line, b_line
    return c_text[:c_start].count("\n") + 1, b_text[:b_start].count("\n") + 1


def check_bootstrap_cost_sync() -> None:
    """Shared doctrine blocks must be identical between docs/COST.md and steering/bootstrap.md."""
    cost_path = ROOT / "docs/COST.md"
    boot_path = ROOT / "steering/bootstrap.md"

    if not cost_path.is_file():
        fail("docs/COST.md: file not found — required for shared bootstrap doctrine validation")
        return
    if not boot_path.is_file():
        fail("steering/bootstrap.md: file not found — required for shared bootstrap doctrine validation")
        return

    cost_text = cost_path.read_text(encoding="utf-8")
    boot_text = boot_path.read_text(encoding="utf-8")

    markers = [
        ("command-preamble", "<!-- command-preamble:start -->", "<!-- command-preamble:end -->"),
        ("acceptance-board", "<!-- acceptance-board:start -->", "<!-- acceptance-board:end -->"),
        ("epic-integration-board", "<!-- epic-integration-board:start -->", "<!-- epic-integration-board:end -->"),
        ("subagent-preamble", "<!-- subagent-preamble:start -->", "<!-- subagent-preamble:end -->"),
        ("model-routing", "<!-- model-routing:start -->", "<!-- model-routing:end -->"),
        ("why-merge-pr", "<!-- why-merge-pr:start -->", "<!-- why-merge-pr:end -->"),
    ]
    cost_blocks: dict[str, tuple[str, int, int]] = {}
    for name, start_m, end_m in markers:
        if start_m not in cost_text or end_m not in cost_text:
            fail(f"docs/COST.md: missing markers for block `{name}`")
            continue
        s = cost_text.index(start_m)
        e = cost_text.index(end_m, s)
        content = cost_text[s + len(start_m):e].strip()
        cost_blocks[name] = (content, s, e)

    if "## Six principles" in cost_text:
        s = cost_text.index("## Six principles")
        e = cost_text.find("\n## ", s + len("## Six principles"))
        if e == -1:
            e = len(cost_text)
        content = cost_text[s + len("## Six principles"):e].strip()
        cost_blocks["six-principles"] = (content, s, e)
    else:
        fail("docs/COST.md: missing `## Six principles` section")

    if "command-preamble" in cost_blocks and "model-routing" in cost_blocks:
        cp_content, cp_s, cp_e = cost_blocks["command-preamble"]
        mr_content, _, _ = cost_blocks["model-routing"]
        expanded_cp = cp_content.replace("<!-- shipmates:model-routing -->", mr_content).strip()
        cost_blocks["command-preamble"] = (expanded_cp, cp_s, cp_e)

    sections = [
        ("six-principles", "## Six principles", "## Cost discipline", False),
        ("command-preamble", "## Cost discipline", "## Reusable acceptance board", True),
        ("model-routing", "## Model routing — what the orchestrator may pick from", "## Reusable acceptance board", True),
        ("acceptance-board", "## Reusable acceptance board", "## Reusable epic integration board", False),
        ("epic-integration-board", "## Reusable epic integration board", "## Reusable subagent preamble", False),
        ("subagent-preamble", "## Reusable subagent preamble", "## Why merge this (PR body)", False),
        ("why-merge-pr", "## Why merge this (PR body)", None, False),
    ]
    boot_blocks: dict[str, tuple[str, int, int]] = {}
    for name, start_h, end_h, include_heading in sections:
        if start_h not in boot_text:
            fail(f"steering/bootstrap.md: missing section `{start_h}` for block `{name}`")
            continue
        s = boot_text.index(start_h)
        content_start = s if include_heading else s + len(start_h)
        if end_h and end_h in boot_text:
            e = boot_text.index(end_h, s)
            content = boot_text[content_start:e].strip()
        else:
            e = len(boot_text)
            content = boot_text[content_start:].strip()
        boot_blocks[name] = (content, s, e)

    matched = 0
    for name in sorted(cost_blocks.keys()):
        if name not in boot_blocks:
            continue
        c_content, c_s, c_e = cost_blocks[name]
        b_content, b_s, b_e = boot_blocks[name]
        if c_content == b_content:
            matched += 1
        else:
            c_drift_line, b_drift_line = find_drift_lines(
                cost_text, c_s, c_e, c_content,
                boot_text, b_s, b_e, b_content,
            )
            fail(
                f"docs/COST.md:{c_drift_line}: block `{name}` drifted from "
                f"steering/bootstrap.md:{b_drift_line} — shared doctrine blocks must stay identical"
            )
            fail(
                f"steering/bootstrap.md:{b_drift_line}: block `{name}` drifted from "
                f"docs/COST.md:{c_drift_line} — shared doctrine blocks must stay identical"
            )

    if matched == len(cost_blocks) and len(cost_blocks) == len(boot_blocks):
        ok(
            f"steering/bootstrap.md and docs/COST.md: {matched} shared doctrine "
            f"block(s) are synchronized"
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

    check_bootstrap_cost_sync()
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
