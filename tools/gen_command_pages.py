#!/usr/bin/env python3
"""Generate site/commands/<slug>/index.html — one detail page per slash command.

Honest by construction: every command-specific sentence on a page is a
markdown-rendered projection of commands/<slug>.md — no invented stage names,
gates, crew, counts, durations or file names. Only command-agnostic chrome
("How to run it", "The stages", "Other orders") is authored here. Anything the
parser does not recognise raises with a file:line and a remedy rather than being
silently dropped or passed through, and every non-blank source line must be
claimed by a block. Deterministic and committed, matching the repo's other
generators. Regenerate with:  python3 tools/gen_command_pages.py

Layers, in file order, with hard import discipline:
  1 MODEL   frozen dataclasses only — no markup, no URLs, no I/O
  2 PARSE   commands/*.md -> model — no markup, no writing
  3 RENDER  model -> HTML/XML strings — the only layer that escapes or emits markup
  4 EMIT    paths + bytes — build_site() is pure, write_all() is the only writer
  5 CLI     argv -> exit code — the only layer that prints
"""

import argparse
import difflib
import html
import json
import os
import re
import sys
from dataclasses import dataclass
from pathlib import Path

# ---------------------------------------------------------------------------
# Constants (the generator's whole non-source input; see the drift invariant)
# ---------------------------------------------------------------------------

# Canonical order. Drives page order, the sitemap, and the sibling nav.
SLUGS = (
    "ship-issue",
    "fix-bug",
    "plan-epics",
    "harden",
    "spike",
    "migrate",
    "document",
    "release",
    "polish",
)
FLAGSHIP_SLUG = "ship-issue"

SITE_URL = "https://saman-mb.github.io/shipmates/"
SOCIAL_IMAGE = SITE_URL + "assets/social-preview.png"
REPO_BLOB_BASE = "https://github.com/saman-mb/shipmates/blob/main/"
# Wall clock is never read: a fixed constant keeps every run byte-identical.
LASTMOD = "2026-07-25"

# Section ids reserved by the page skeleton; a source heading may not claim one.
RESERVED_ANCHORS = frozenset(
    {"invoke", "stages", "config", "guardrails", "source", "other-orders", "main", "top"}
)

# Spelled out so the stages lead reads as prose; derived, never hand-typed.
NUMBER_WORDS = (
    "Zero", "One", "Two", "Three", "Four", "Five", "Six", "Seven", "Eight",
    "Nine", "Ten", "Eleven", "Twelve", "Thirteen", "Fourteen", "Fifteen",
    "Sixteen", "Seventeen", "Eighteen", "Nineteen", "Twenty",
)

MAX_META_DESCRIPTION = 158
MAX_JSONLD_TEXT = 300


class SourceError(Exception):
    """A command source file used a construct this generator does not support."""

    def __init__(self, src: str, lineno: int, what: str, line: str, remedy: str) -> None:
        self.src = src
        self.lineno = lineno
        self.what = what
        self.line = line
        self.remedy = remedy
        super().__init__(str(self))

    def __str__(self) -> str:
        return (
            f"{self.src}:{self.lineno}: {self.what}\n"
            f"    got: {self.line[:100]!r}\n"
            f"    {self.remedy}"
        )


# ---------------------------------------------------------------------------
# Layer 1 — MODEL
# Frozen dataclasses. No markup, no URLs, no escaping, no I/O.
# ---------------------------------------------------------------------------


@dataclass(frozen=True, slots=True)
class Para:
    lineno: int
    text: str  # raw inline markdown, lazy continuations already joined


@dataclass(frozen=True, slots=True)
class ListItem:
    lineno: int
    text: str  # raw inline markdown
    children: tuple  # tuple[Block, ...]


@dataclass(frozen=True, slots=True)
class ListBlock:
    lineno: int
    ordered: bool
    items: tuple  # tuple[ListItem, ...]


@dataclass(frozen=True, slots=True)
class Code:
    lineno: int
    lang: str
    lines: tuple  # tuple[str, ...] — verbatim, never inline-parsed


@dataclass(frozen=True, slots=True)
class Table:
    lineno: int
    header: tuple  # tuple[str, ...] — raw inline markdown per cell
    rows: tuple  # tuple[tuple[str, ...], ...]


@dataclass(frozen=True, slots=True)
class Quote:
    lineno: int
    blocks: tuple  # tuple[Block, ...]


@dataclass(frozen=True, slots=True)
class Subheading:
    lineno: int
    level: int
    text: str


@dataclass(frozen=True, slots=True)
class Frontmatter:
    description: str
    argument_hint: str
    allowed_tools: tuple  # tuple[str, ...]


@dataclass(frozen=True, slots=True)
class Section:
    lineno: int
    source_level: int
    title: str
    anchor: str
    blocks: tuple  # tuple[Block, ...]


@dataclass(frozen=True, slots=True)
class StageHeading:
    label: str
    sort_key: tuple  # tuple[int, ...]
    title: str
    gate: str
    annotation: str


@dataclass(frozen=True, slots=True)
class Stage:
    lineno: int
    heading_raw: str  # the source heading line, verbatim
    label: str  # the source's own label, displayed as authored
    sort_key: tuple  # tuple[int, ...]
    title: str  # gate and crew annotation removed
    gate: str  # verbatim text after the stop sign, or empty
    annotation: str  # verbatim trailing parenthetical, or empty
    crew: tuple  # tuple[str, ...] — ordered, de-duplicated known agent names
    anchor: str
    blocks: tuple  # tuple[Block, ...]


@dataclass(frozen=True, slots=True)
class Command:
    slug: str
    source_path: str  # repo-relative posix path
    tagline: str
    frontmatter: Frontmatter
    intro: tuple  # tuple[Block, ...]
    config: object  # Section | None
    stages: tuple  # tuple[Stage, ...]
    guardrails: object  # Section | None
    sections_before_stages: tuple  # tuple[Section, ...]
    sections_after_stages: tuple  # tuple[Section, ...]
    crew: tuple  # tuple[str, ...]


# ---------------------------------------------------------------------------
# Layer 2 — PARSE  (commands/*.md -> model)
# No markup literals, no escaping, no writing.
# ---------------------------------------------------------------------------

FRONTMATTER_KEYS = ("description", "argument-hint", "allowed-tools")

HEADING_RE = re.compile(r"^(?P<hashes>#{1,6})[ ](?P<title>.+?)[ ]*$")
TITLE_RE = re.compile(r"^#[ ]+/(?P<slug>[a-z0-9][a-z0-9-]*)[ ]*[—–-][ ]*(?P<tagline>.+?)[ ]*$")
STAGE_RE = re.compile(
    r"^##[ ]+Stage[ ]+(?P<label>[0-9]+(?:\.[0-9]+)?)[ ]*[—–-][ ]*(?P<rest>.+?)[ ]*$"
)
LIST_RE = re.compile(r"^(?P<ind> *)(?P<marker>[-*]|[0-9]+\.)[ ](?P<text>.*)$")
FENCE_RE = re.compile(r"^(?P<ind> *)```(?P<lang>[A-Za-z0-9_+.-]*)[ ]*$")
FENCE_ANY_RE = re.compile(r"^ *```")
RULE_RE = re.compile(r"^-{3,}[ ]*$")
DELIM_CELL_RE = re.compile(r"^:?-+:?$")
CODESPAN_RE = re.compile(r"`([^`]+)`")
GATE_MARK = "⛔"  # no-entry sign; introduces a stage's hard gate
TRAILING_PAREN_RE = re.compile(r"[ ]*\(([^()]*)\)[ ]*$")
AGENT_WORD_RE = re.compile(r"\bagents?\b")
LINK_RE = re.compile(r"!\[|\[[^\]]*\]\(|\[[^\]]*\]\[|\[\^|~~")
LASTMOD_RE = re.compile(r"^[0-9]{4}-[0-9]{2}-[0-9]{2}$")

LINKS_UNSUPPORTED = (
    "links are not supported in command sources — write the target as inline code instead"
)


def _indent_of(raw: str) -> int:
    return len(raw) - len(raw.lstrip(" "))


def _is_structural(raw: str) -> bool:
    """True when a line opens a block that must interrupt a paragraph or list item."""
    if raw.startswith("|"):
        return True
    stripped = raw.lstrip(" ")
    return bool(
        HEADING_RE.match(raw)
        or FENCE_ANY_RE.match(raw)
        or stripped.startswith(">")
        or RULE_RE.match(raw)
    )


def load_agent_names(agents_dir: Path) -> tuple:
    """The known crew roles, read from each agents/*.md `name:` frontmatter key."""
    names = []
    for path in sorted(agents_dir.glob("*.md")):
        src = f"agents/{path.name}"
        found = ""
        for lineno, raw in enumerate(path.read_text(encoding="utf-8").split("\n")[:10], start=1):
            if raw.startswith("name:"):
                found = raw[len("name:"):].strip()
                break
        if not found:
            raise SourceError(
                src, 1, "no `name:` key in the frontmatter", "",
                "every agents/*.md must open with a frontmatter block declaring `name: <role>`",
            )
        names.append(found)
    return tuple(dict.fromkeys(names))


def load_commands(commands_dir: Path, agents: tuple) -> tuple:
    """Every commands/*.md, in canonical SLUGS order. Raises on any drift from SLUGS."""
    on_disk = {p.stem: p for p in sorted(commands_dir.glob("*.md"))}
    for stem in sorted(on_disk):
        if stem not in SLUGS:
            raise SourceError(
                f"commands/{stem}.md", 1, "command file is not in SLUGS", "",
                f"commands/{stem}.md is not in SLUGS — add it to SLUGS in "
                "tools/gen_command_pages.py (canonical order drives the sitemap, the sibling nav "
                "and the homepage cards), then rerun the generator and commit site/",
            )
    for slug in SLUGS:
        if slug not in on_disk:
            raise SourceError(
                f"commands/{slug}.md", 1, "SLUGS entry has no command file", "",
                f"SLUGS lists {slug} but commands/{slug}.md does not exist — remove it from SLUGS "
                "in tools/gen_command_pages.py, delete site/commands/" + slug + "/, "
                "then rerun the generator and commit site/",
            )
    return tuple(parse_command(on_disk[slug], agents) for slug in SLUGS)


def split_frontmatter(lines: list, src: str, consumed: set) -> tuple:
    """Return (Frontmatter, index of the first line after the closing fence)."""
    if not lines or lines[0].strip() != "---":
        raise SourceError(
            src, 1, "missing frontmatter fence", lines[0] if lines else "",
            "a command file must open with a `---` line",
        )
    consumed.add(1)
    values = {}
    for i in range(1, len(lines)):
        raw = lines[i]
        lineno = i + 1
        if raw.strip() == "---":
            consumed.add(lineno)
            missing = [k for k in FRONTMATTER_KEYS if k not in values]
            if missing:
                raise SourceError(
                    src, lineno, f"frontmatter is missing {missing[0]}", raw,
                    "expected one of description, argument-hint, allowed-tools",
                )
            return (
                Frontmatter(
                    description=values["description"],
                    argument_hint=values["argument-hint"],
                    allowed_tools=tuple(
                        t.strip() for t in values["allowed-tools"].split(",") if t.strip()
                    ),
                ),
                i + 1,
            )
        if not raw.strip():
            continue
        key, sep, value = raw.partition(":")
        if not sep or key.strip() not in FRONTMATTER_KEYS:
            raise SourceError(
                src, lineno, "unknown frontmatter key", raw,
                "expected one of description, argument-hint, allowed-tools",
            )
        key = key.strip()
        if key in values:
            raise SourceError(
                src, lineno, f"duplicate frontmatter key {key}", raw,
                "declare each of description, argument-hint, allowed-tools exactly once",
            )
        values[key] = value.strip()
        consumed.add(lineno)
    raise SourceError(
        src, len(lines), "unterminated frontmatter fence", "",
        "close the frontmatter with a `---` line before the `# /<command>` heading",
    )


def stage_sort_key(label: str) -> tuple:
    """(1,) < (1,5) < (2,) — integer tuples, never floats."""
    return tuple(int(part) for part in label.split("."))


def slugify_anchor(title: str) -> str:
    return re.sub(r"-+", "-", re.sub(r"[^a-z0-9]+", "-", title.lower())).strip("-")


def _squeeze(text: str) -> str:
    """Collapse runs of spaces, as an HTML renderer would. Words are never altered."""
    return re.sub(r"[ \t]+", " ", text).strip()


def _is_crew_annotation(inner: str, agents: tuple) -> bool:
    """A trailing parenthetical is a crew annotation only when it names the crew."""
    if AGENT_WORD_RE.search(inner):
        return True
    return any(span in agents for span in CODESPAN_RE.findall(inner))


def parse_stage_heading(raw: str, src: str, lineno: int, agents: tuple) -> StageHeading:
    """Decompose `## Stage <n> — <title> [gate] [annotation]`, order-independently."""
    m = STAGE_RE.match(raw)
    if not m:
        raise SourceError(
            src, lineno, "stage heading does not match the supported grammar", raw,
            "write it as `## Stage <number> — <title>`, optionally followed by "
            f"`{GATE_MARK} <gate text>` and a trailing `(agent: ...)` parenthetical",
        )
    label = m.group("label")
    rest = m.group("rest")
    annotation = ""

    def take_annotation(text: str) -> tuple:
        hit = TRAILING_PAREN_RE.search(text)
        if hit and _is_crew_annotation(hit.group(1), agents):
            return text[: hit.start()], "(" + hit.group(1) + ")"
        return text, ""

    rest, annotation = take_annotation(rest)
    gate = ""
    mark = rest.find(GATE_MARK)
    if mark >= 0:
        gate = _squeeze(rest[mark + len(GATE_MARK):])
        rest = rest[:mark]
    if not annotation:
        rest, annotation = take_annotation(rest)
    title = _squeeze(rest)
    if not title:
        raise SourceError(
            src, lineno, "stage heading has no title", raw,
            "write it as `## Stage <number> — <title>`",
        )
    return StageHeading(
        label=label,
        sort_key=stage_sort_key(label),
        title=title,
        gate=gate,
        annotation=_squeeze(annotation),
    )


def find_crew(texts: tuple, agents: tuple) -> tuple:
    """Ordered, de-duplicated known agent names appearing in backticks."""
    found = []
    for text in texts:
        for span in CODESPAN_RE.findall(text):
            if span in agents:
                found.append(span)
    return tuple(dict.fromkeys(found))


def block_texts(blocks: tuple) -> tuple:
    """Every inline-markdown string in a block tree, in document order."""
    out = []
    for block in blocks:
        if isinstance(block, Para):
            out.append(block.text)
        elif isinstance(block, Subheading):
            out.append(block.text)
        elif isinstance(block, ListBlock):
            for item in block.items:
                out.append(item.text)
                out.extend(block_texts(item.children))
        elif isinstance(block, Table):
            out.extend(block.header)
            for row in block.rows:
                out.extend(row)
        elif isinstance(block, Quote):
            out.extend(block_texts(block.blocks))
    return tuple(out)


def parse_blocks(items: list, src: str, consumed: set) -> tuple:
    """Parse (lineno, text) pairs into blocks. Every non-blank line is consumed."""
    blocks = []
    i = 0
    n = len(items)
    while i < n:
        lineno, raw = items[i]
        if not raw.strip():
            i += 1
            continue
        if _indent_of(raw) >= 4:
            # A block cannot start this deep: list continuations are dedented to the
            # marker width before they get here, so 4 spaces can only mean an indented
            # code block, which would otherwise render as an ordinary paragraph.
            raise SourceError(
                src, lineno, f"block indented {_indent_of(raw)} spaces", raw,
                "4-space indented code blocks are not supported — use a ``` fenced block "
                "(list continuations indent to the marker width, 2 or 3 spaces)",
            )
        if FENCE_ANY_RE.match(raw):
            block, i = _parse_code(items, i, src, consumed)
        elif HEADING_RE.match(raw):
            block, i = _parse_subheading(items, i, src, consumed)
        elif raw.lstrip(" ").startswith(">"):
            block, i = _parse_quote(items, i, src, consumed)
        elif raw.startswith("|"):
            block, i = _parse_table(items, i, src, consumed)
        elif LIST_RE.match(raw):
            block, i = _parse_list(items, i, src, consumed)
        else:
            block, i = _parse_para(items, i, src, consumed)
        blocks.append(block)
    return tuple(blocks)


def _parse_code(items: list, i: int, src: str, consumed: set) -> tuple:
    lineno, raw = items[i]
    m = FENCE_RE.match(raw)
    if not m:
        raise SourceError(
            src, lineno, "code fence has an unsupported info string", raw,
            "open a fenced block with ``` optionally followed by a bare language name",
        )
    ind = len(m.group("ind"))
    lang = m.group("lang")
    consumed.add(lineno)
    body = []
    i += 1
    n = len(items)
    while i < n:
        ln, line = items[i]
        consumed.add(ln)
        i += 1
        if line.strip() == "```":
            return Code(lineno=lineno, lang=lang, lines=tuple(body)), i
        body.append(line[ind:] if not line[:ind].strip() else line.lstrip(" "))
    raise SourceError(
        src, lineno, "unclosed code fence at end of file", raw,
        "close the fenced block with a ``` line",
    )


def _parse_subheading(items: list, i: int, src: str, consumed: set) -> tuple:
    lineno, raw = items[i]
    m = HEADING_RE.match(raw)
    level = len(m.group("hashes"))
    if level != 3:
        raise SourceError(
            src, lineno, f"heading level {level} inside a section body", raw,
            "only `###` subheadings are supported here — C5 forbids heading level 4, "
            "and `##` opens a new section",
        )
    consumed.add(lineno)
    return Subheading(lineno=lineno, level=level, text=_squeeze(m.group("title"))), i + 1


def _parse_quote(items: list, i: int, src: str, consumed: set) -> tuple:
    lineno = items[i][0]
    inner = []
    n = len(items)
    while i < n:
        ln, raw = items[i]
        stripped = raw.lstrip(" ")
        if not stripped.startswith(">"):
            break
        consumed.add(ln)
        rest = stripped[1:]
        inner.append((ln, rest[1:] if rest.startswith(" ") else rest))
        i += 1
    return Quote(lineno=lineno, blocks=parse_blocks(inner, src, consumed)), i


def _split_row(raw: str) -> tuple:
    body = raw.strip()
    if body.startswith("|"):
        body = body[1:]
    if body.endswith("|"):
        body = body[:-1]
    return tuple(cell.strip() for cell in body.split("|"))


def _parse_table(items: list, i: int, src: str, consumed: set) -> tuple:
    lineno, raw = items[i]
    n = len(items)
    if i + 1 >= n or not items[i + 1][1].startswith("|"):
        raise SourceError(
            src, lineno, "table row without a header/delimiter pair", raw,
            "a GFM table needs a header row and a `|---|---|` delimiter row before its body rows",
        )
    header = _split_row(raw)
    delim_lineno, delim_raw = items[i + 1]
    delim = _split_row(delim_raw)
    if len(delim) != len(header) or not all(DELIM_CELL_RE.match(cell) for cell in delim):
        raise SourceError(
            src, delim_lineno, "table delimiter row does not match the header", delim_raw,
            f"write a delimiter row of {len(header)} cells, each of dashes (`|---|---|`)",
        )
    consumed.add(lineno)
    consumed.add(delim_lineno)
    rows = []
    i += 2
    while i < n and items[i][1].startswith("|"):
        row_lineno, row_raw = items[i]
        row = _split_row(row_raw)
        if len(row) != len(header):
            raise SourceError(
                src, row_lineno, f"table row has {len(row)} cells, header has {len(header)}",
                row_raw, "give every row the same number of `|`-separated cells as the header",
            )
        consumed.add(row_lineno)
        rows.append(row)
        i += 1
    return Table(lineno=lineno, header=header, rows=tuple(rows)), i


def _parse_para(items: list, i: int, src: str, consumed: set) -> tuple:
    lineno = items[i][0]
    parts = []
    n = len(items)
    while i < n:
        ln, raw = items[i]
        if not raw.strip() or _is_structural(raw) or LIST_RE.match(raw):
            break
        if _indent_of(raw) >= 4:
            # Do not swallow an over-indented line as a lazy continuation — hand it
            # back so parse_blocks raises about the unsupported indented code block.
            break
        consumed.add(ln)
        parts.append(raw.strip())
        i += 1
    return Para(lineno=lineno, text=" ".join(parts)), i


def _parse_list(items: list, i: int, src: str, consumed: set) -> tuple:
    lineno0, raw0 = items[i]
    first = LIST_RE.match(raw0)
    ind = len(first.group("ind"))
    ordered = first.group("marker").endswith(".")
    entries = []
    n = len(items)
    while i < n:
        lineno, raw = items[i]
        m = LIST_RE.match(raw)
        if (
            m is None
            or len(m.group("ind")) != ind
            or m.group("marker").endswith(".") != ordered
        ):
            break
        need = ind + len(m.group("marker")) + 1
        item_lines = [(lineno, m.group("text"))]
        consumed.add(lineno)
        i += 1
        while i < n:
            ln, line = items[i]
            if not line.strip():
                j = i
                while j < n and not items[j][1].strip():
                    j += 1
                if j < n and _indent_of(items[j][1]) >= need:
                    item_lines.append((ln, ""))
                    i = j
                    continue
                i = j
                break
            here = _indent_of(line)
            if here >= need:
                item_lines.append((ln, line[need:]))
                consumed.add(ln)
                i += 1
                continue
            if here > ind:
                raise SourceError(
                    src, ln, f"continuation indented {here}; expected {need}", line,
                    f"indent list continuations to {need} spaces (4-space indented code blocks "
                    "are not supported — use a fenced block)",
                )
            if LIST_RE.match(line) or _is_structural(line):
                break
            # Lazy continuation: an unindented prose line still belongs to the item.
            consumed.add(ln)
            item_lines.append((ln, line.strip()))
            i += 1
        item_blocks = parse_blocks(item_lines, src, consumed)
        if item_blocks and isinstance(item_blocks[0], Para):
            entries.append(
                ListItem(lineno=lineno, text=item_blocks[0].text, children=item_blocks[1:])
            )
        else:
            entries.append(ListItem(lineno=lineno, text="", children=item_blocks))
    return ListBlock(lineno=lineno0, ordered=ordered, items=tuple(entries)), i


@dataclass(frozen=True, slots=True)
class RawSection:
    lineno: int
    level: int
    title: str
    heading_raw: str
    lines: tuple  # tuple[tuple[int, str], ...]


def parse_sections(lines: list, start: int, src: str, consumed: set) -> tuple:
    """Split the body into (intro lines, raw sections) per THE SECTION RULE.

    A `---` line closes the current section. The next heading opens a new section at
    whatever level it is authored; absent a preceding `---`, only a level-2 heading
    opens a section — a level-3 heading stays a subheading inside the current one.
    """
    intro = []
    sections = []
    current = None
    after_rule = False
    in_fence = False
    pending_rule_lineno = 0

    for i in range(start, len(lines)):
        raw = lines[i]
        lineno = i + 1
        if FENCE_ANY_RE.match(raw):
            in_fence = not in_fence
        if not in_fence and RULE_RE.match(raw):
            if i > 0 and lines[i - 1].strip():
                raise SourceError(
                    src, lineno, "`---` is not the last non-blank line of its block", raw,
                    "leave a blank line before a `---` section terminator (a `---` directly "
                    "under text is a setext heading, which is not supported)",
                )
            consumed.add(lineno)
            after_rule = True
            pending_rule_lineno = lineno
            continue
        heading = None if in_fence else HEADING_RE.match(raw)
        if heading is not None:
            level = len(heading.group("hashes"))
            if level == 1:
                raise SourceError(
                    src, lineno, "second level-1 heading", raw,
                    "a command file has exactly one `# /<command> — <tagline>` heading; "
                    "use `##` for sections",
                )
            if level >= 4:
                raise SourceError(
                    src, lineno, f"heading level {level}", raw,
                    "C5 forbids heading level 4 — use a list or a bold lead-in instead",
                )
            if level == 2 or after_rule:
                consumed.add(lineno)
                current = RawSection(
                    lineno=lineno,
                    level=level,
                    title=_squeeze(heading.group("title")),
                    heading_raw=raw,
                    lines=[],
                )
                sections.append(current)
                after_rule = False
                continue
        if after_rule and raw.strip():
            raise SourceError(
                src, pending_rule_lineno, "`---` is not followed by a heading", lines[pending_rule_lineno - 1],
                "a top-level `---` terminates a section, so the next non-blank line must be a "
                "`##` or `###` heading",
            )
        if current is None:
            intro.append((lineno, raw))
        else:
            current.lines.append((lineno, raw))
    if in_fence:
        raise SourceError(
            src, len(lines), "unclosed code fence at end of file", "",
            "close the fenced block with a ``` line",
        )
    return tuple(intro), tuple(sections)


def _is_config_title(title: str) -> bool:
    return title == "Config" or title.startswith("Config ")


def parse_command(path: Path, agents: tuple) -> Command:
    src = f"commands/{path.name}"
    slug = path.stem
    lines = path.read_text(encoding="utf-8").split("\n")
    consumed = set()

    frontmatter, idx = split_frontmatter(lines, src, consumed)
    while idx < len(lines) and not lines[idx].strip():
        idx += 1
    if idx >= len(lines):
        raise SourceError(
            src, len(lines), "no `# /<command>` heading after the frontmatter", "",
            "add `# /<command> — <tagline>` below the frontmatter",
        )
    title_line = lines[idx]
    title_match = TITLE_RE.match(title_line)
    if title_match is None:
        raise SourceError(
            src, idx + 1, "first heading is not `# /<command> — <tagline>`", title_line,
            "write it as `# /<command> — <tagline>`",
        )
    if title_match.group("slug") != slug:
        raise SourceError(
            src, idx + 1, "heading command name does not match the file name", title_line,
            f"the heading must read `# /{slug} — <tagline>` to match commands/{slug}.md",
        )
    consumed.add(idx + 1)
    tagline = _squeeze(title_match.group("tagline"))

    intro_lines, raw_sections = parse_sections(lines, idx + 1, src, consumed)
    intro = parse_blocks(list(intro_lines), src, consumed)

    config = None
    guardrails = None
    stages = []
    before = []
    after = []
    seen_stage = False
    seen_labels = {}
    anchors = {}

    for raw_section in raw_sections:
        blocks = parse_blocks(list(raw_section.lines), src, consumed)
        if raw_section.title.startswith("Stage"):
            heading = parse_stage_heading(
                raw_section.heading_raw, src, raw_section.lineno, agents
            )
            if heading.label in seen_labels:
                raise SourceError(
                    src, raw_section.lineno,
                    f"duplicate stage label {heading.label} (first seen on line "
                    f"{seen_labels[heading.label]})",
                    raw_section.heading_raw,
                    "give every stage in a file a distinct number",
                )
            seen_labels[heading.label] = raw_section.lineno
            bad = next((b for b in blocks if isinstance(b, Subheading)), None)
            if bad is not None:
                raise SourceError(
                    src, bad.lineno, "`###` subheading inside a stage body", bad.text,
                    "a stage title already occupies heading level 3; C5 forbids level 4 — use a "
                    "list or a bold lead-in instead",
                )
            # Crew comes from the stage HEADING only. An agent merely named in the body is
            # usually being *discussed* ("Gates `ux-ui-designer`"), not convened at this stage —
            # listing it as crew would make the page claim something the source does not (C8).
            # The body mention still renders verbatim in the stage prose, so nothing is lost.
            texts = (raw_section.heading_raw,)
            stages.append(
                Stage(
                    lineno=raw_section.lineno,
                    heading_raw=raw_section.heading_raw,
                    label=heading.label,
                    sort_key=heading.sort_key,
                    title=heading.title,
                    gate=heading.gate,
                    annotation=heading.annotation,
                    crew=find_crew(texts, agents),
                    anchor="stage-" + heading.label.replace(".", "-"),
                    blocks=blocks,
                )
            )
            seen_stage = True
            continue

        anchor = slugify_anchor(raw_section.title)
        if not anchor:
            raise SourceError(
                src, raw_section.lineno, "section title has no anchorable characters",
                raw_section.heading_raw,
                "give the section a title containing letters or digits",
            )
        section = Section(
            lineno=raw_section.lineno,
            source_level=raw_section.level,
            title=raw_section.title,
            anchor=anchor,
            blocks=blocks,
        )
        if raw_section.title == "Guardrails":
            if guardrails is not None:
                raise SourceError(
                    src, raw_section.lineno, "second Guardrails section",
                    raw_section.heading_raw, "a command file has exactly one Guardrails section",
                )
            guardrails = section
        elif _is_config_title(raw_section.title) and not seen_stage:
            if config is not None:
                raise SourceError(
                    src, raw_section.lineno, "second Config section",
                    raw_section.heading_raw, "a command file has exactly one Config section",
                )
            config = section
        else:
            # An extra section renders as an <h3> inside #stages, so its anchor must
            # not collide with a skeleton id or another extra section's.
            if anchor in RESERVED_ANCHORS or anchor.startswith("stage-"):
                raise SourceError(
                    src, raw_section.lineno, f"section anchor `{anchor}` is reserved",
                    raw_section.heading_raw,
                    "rename the section — the page skeleton already owns the ids invoke, "
                    "stages, config, guardrails, source, other-orders and every stage-<n>",
                )
            if anchor in anchors:
                raise SourceError(
                    src, raw_section.lineno,
                    f"section anchor `{anchor}` collides with line {anchors[anchor]}",
                    raw_section.heading_raw, "give the two sections distinct titles",
                )
            anchors[anchor] = raw_section.lineno
            (after if seen_stage else before).append(section)

    if not stages:
        raise SourceError(
            src, 1, "no `## Stage <n> — <title>` sections", "",
            "a command file describes its run as numbered stages",
        )

    # [C-3] no-silent-drop invariant: every non-blank line must be claimed by a block.
    for lineno, raw in enumerate(lines, start=1):
        if raw.strip() and lineno not in consumed:
            raise SourceError(
                src, lineno, "line not consumed by any block", raw,
                "this generator supports paragraphs, `-`/`*` and `1.` lists, fenced code, "
                "GFM pipe tables, `>` blockquotes and `###` subheadings — rewrite the line as "
                "one of those",
            )

    stages.sort(key=lambda s: s.sort_key)
    crew = find_crew(tuple(stage.heading_raw for stage in stages), agents)
    return Command(
        slug=slug,
        source_path=f"commands/{slug}.md",
        tagline=tagline,
        frontmatter=frontmatter,
        intro=intro,
        config=config,
        stages=tuple(stages),
        guardrails=guardrails,
        sections_before_stages=tuple(before),
        sections_after_stages=tuple(after),
        crew=crew,
    )


# ---------------------------------------------------------------------------
# Layer 3 — RENDER  (model -> HTML/XML strings)
# The only layer allowed to escape or to write markup. No Path, no open, no os.
# ---------------------------------------------------------------------------


@dataclass(frozen=True, slots=True)
class PageContext:
    site_url: str
    lastmod: str
    social_image: str
    repo_blob_base: str


ALLOWED_LINK_HOSTS = frozenset({"saman-mb.github.io", "github.com"})
SCHEME_RE = re.compile(r"^([A-Za-z][A-Za-z0-9+.-]*):")

GITHUB_ICON = (
    '<svg class="btn__icon" width="20" height="20" viewBox="0 0 16 16" fill="currentColor" '
    'aria-hidden="true"><path d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55'
    "-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-."
    "52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64"
    "-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82.64-.18 1.32-"
    ".27 2-.27.68 0 1.36.09 2 .27 1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27."
    "82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.5"
    '5.38A8.013 8.013 0 0016 8c0-4.42-3.58-8-8-8z"/></svg>'
)

NAV_LINKS = (
    ("install", "Install"),
    ("crew", "Crew"),
    ("orders", "Orders"),
    ("how", "How"),
    ("faq", "FAQ"),
)

FOOTER_LINKS = (
    ("https://github.com/saman-mb/shipmates", "GitHub"),
    ("../../#install", "Install"),
    ("../../#crew", "Crew"),
    ("../../#orders", "Orders"),
    ("https://github.com/saman-mb/shipmates/blob/main/LICENSE", "License"),
    ("https://github.com/saman-mb/shipmates/blob/main/CONTRIBUTING.md", "Contributing"),
)


def esc(s: str) -> str:
    """One escaper for text and attributes alike, so no call site can pick the wrong one."""
    return html.escape(s, quote=True)


def link(url: str) -> str:
    """Assert a generator-authored URL is on the allowlist, then escape it.

    Allowed: a relative reference with no scheme, or https on an allowlisted host.
    Every href/src on these pages is authored here, never derived from source
    content — this asserts that claim instead of trusting it.
    """
    if url.startswith("//"):
        raise ValueError(f"protocol-relative URL is not allowed: {url}")
    scheme = SCHEME_RE.match(url)
    if scheme is not None:
        host = url[scheme.end():].lstrip("/").split("/", 1)[0]
        if scheme.group(1) != "https" or host not in ALLOWED_LINK_HOSTS:
            raise ValueError(f"URL is not on the allowlist: {url}")
    return esc(url)


def indent_html(text: str, prefix: str) -> str:
    """Indent generated markup, leaving preformatted content byte-identical.

    Lines inside a `<pre>` are never prefixed — indenting them would add leading
    whitespace the source never had, which is a content change, not a cosmetic one.
    """
    out = []
    in_pre = False
    for line in text.split("\n"):
        out.append(line if in_pre or not line else prefix + line)
        if "<pre" in line:
            in_pre = True
        if "</pre>" in line:
            in_pre = False
    return "\n".join(out)


def truncate_words(text: str, limit: int) -> str:
    """Word-boundary truncation, pinned here rather than borrowed from textwrap."""
    if len(text) <= limit:
        return text
    cut = text[: limit - 1]
    space = cut.rfind(" ")
    if space > 0:
        cut = cut[:space]
    return cut.rstrip() + "…"


def render_inline(md: str, src: str, lineno: int) -> str:
    """Tokenize inline markdown into literal/code/strong/em runs.

    Escape first, wrap second: every literal run is escaped before any tag is added, and
    code spans are escaped without further parsing. `_` is not an emphasis marker — every
    underscore in the corpus is inside an identifier. Angle brackets are placeholders
    (`<repo>`, `<PR#>`), so they escape rather than raise.
    """
    bad = LINK_RE.search(md)
    if bad is not None:
        raise SourceError(src, lineno, "markdown link, image or strikethrough", md, LINKS_UNSUPPORTED)
    out = []
    buf = []
    pos = 0
    end = len(md)

    def flush() -> None:
        if buf:
            out.append(esc("".join(buf)))
            buf.clear()

    while pos < end:
        ch = md[pos]
        if ch == "`":
            run = 1
            while pos + run < end and md[pos + run] == "`":
                run += 1
            fence = "`" * run
            close = md.find(fence, pos + run)
            if close < 0:
                raise SourceError(
                    src, lineno, "unbalanced backticks", md,
                    "close every inline code span with a matching run of backticks",
                )
            flush()
            out.append("<code>" + esc(md[pos + run: close]) + "</code>")
            pos = close + run
            continue
        if md.startswith("**", pos):
            close = md.find("**", pos + 2)
            if close < 0:
                raise SourceError(
                    src, lineno, "unbalanced `**` emphasis", md,
                    "close every `**strong**` span on the same paragraph",
                )
            flush()
            out.append("<strong>" + render_inline(md[pos + 2: close], src, lineno) + "</strong>")
            pos = close + 2
            continue
        if ch == "*":
            close = md.find("*", pos + 1)
            if close < 0:
                raise SourceError(
                    src, lineno, "unbalanced `*` emphasis", md,
                    "close every `*em*` span on the same paragraph",
                )
            flush()
            out.append("<em>" + render_inline(md[pos + 1: close], src, lineno) + "</em>")
            pos = close + 1
            continue
        buf.append(ch)
        pos += 1
    flush()
    return "".join(out)


def plain_inline(md: str) -> str:
    """The inline markdown with its markers removed — for JSON-LD, never for HTML."""
    return _squeeze(re.sub(r"\*\*|\*|`", "", md))


def render_block(b, src: str) -> str:
    if isinstance(b, Para):
        return "<p>" + render_inline(b.text, src, b.lineno) + "</p>"
    if isinstance(b, Subheading):
        return "<h3>" + render_inline(b.text, src, b.lineno) + "</h3>"
    if isinstance(b, Code):
        body = "\n".join(esc(line) for line in b.lines)
        return '<pre class="order-code" tabindex="0"><code>' + body + "</code></pre>"
    if isinstance(b, Quote):
        return "<blockquote>\n" + indent_html(render_blocks(b.blocks, src), "  ") + "\n</blockquote>"
    if isinstance(b, ListBlock):
        tag = "ol" if b.ordered else "ul"
        parts = []
        for item in b.items:
            inner = render_inline(item.text, src, item.lineno) if item.text else ""
            if item.children:
                inner += "\n" + indent_html(render_blocks(item.children, src), "    ") + "\n  "
            parts.append("  <li>" + inner + "</li>")
        return f"<{tag}>\n" + "\n".join(parts) + f"\n</{tag}>"
    if isinstance(b, Table):
        head = "".join(
            '<th scope="col">' + render_inline(cell, src, b.lineno) + "</th>" for cell in b.header
        )
        rows = "\n".join(
            "      <tr>"
            + "".join("<td>" + render_inline(cell, src, b.lineno) + "</td>" for cell in row)
            + "</tr>"
            for row in b.rows
        )
        return (
            '<div class="order-table" tabindex="0">\n'
            "  <table>\n"
            f"    <thead><tr>{head}</tr></thead>\n"
            "    <tbody>\n" + rows + "\n    </tbody>\n"
            "  </table>\n"
            "</div>"
        )
    raise ValueError(f"unrenderable block: {b!r}")


def render_blocks(bs: tuple, src: str) -> str:
    return "\n".join(render_block(b, src) for b in bs)


def render_prose(bs: tuple, src: str, prefix: str) -> str:
    if not bs:
        return ""
    return indent_html(
        '<div class="order-prose">\n'
        + indent_html(render_blocks(bs, src), "  ")
        + "\n</div>",
        prefix,
    )


def canonical_url(slug: str, ctx: PageContext) -> str:
    return f"{ctx.site_url}commands/{slug}/"


def page_title(cmd: Command) -> str:
    return f"/{cmd.slug} — {cmd.tagline}"


def render_head(cmd: Command, ctx: PageContext) -> str:
    url = canonical_url(cmd.slug, ctx)
    full_title = page_title(cmd) + " · Shipmates"
    social_title = page_title(cmd)
    description = truncate_words(cmd.frontmatter.description, MAX_META_DESCRIPTION)
    alt = "Shipmates — Custom sub-agents and slash-command workflows for Claude Code."
    return f"""<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>{esc(full_title)}</title>
  <meta name="description" content="{esc(description)}">
  <link rel="canonical" href="{link(url)}">
  <link rel="icon" href="{link("../../assets/logo-240.png")}" type="image/png">
  <meta name="theme-color" content="#FBFAF9" media="(prefers-color-scheme: light)">
  <meta name="theme-color" content="#14110F" media="(prefers-color-scheme: dark)">
  <meta property="og:type" content="article">
  <meta property="og:site_name" content="Shipmates">
  <meta property="og:title" content="{esc(social_title)}">
  <meta property="og:description" content="{esc(description)}">
  <meta property="og:url" content="{link(url)}">
  <meta property="og:image" content="{link(ctx.social_image)}">
  <meta property="og:image:width" content="1280">
  <meta property="og:image:height" content="640">
  <meta property="og:image:alt" content="{esc(alt)}">
  <meta name="twitter:card" content="summary_large_image">
  <meta name="twitter:title" content="{esc(social_title)}">
  <meta name="twitter:description" content="{esc(description)}">
  <meta name="twitter:image" content="{link(ctx.social_image)}">
  <meta name="twitter:image:alt" content="{esc(alt)}">
  <link rel="stylesheet" href="{link("../../styles.css")}">
{indent_html(render_jsonld(cmd, ctx), "  ")}
</head>"""


def _step_text(cmd: Command, stage: Stage) -> str:
    for text in block_texts(stage.blocks):
        summary = plain_inline(text)
        if summary:
            return truncate_words(summary, MAX_JSONLD_TEXT)
    for block in stage.blocks:
        if isinstance(block, Code) and block.lines:
            return truncate_words("\n".join(block.lines).strip(), MAX_JSONLD_TEXT)
    return stage.title


def render_jsonld(cmd: Command, ctx: PageContext) -> str:
    """Exactly one ld+json block per page — the site validator concatenates them."""
    url = canonical_url(cmd.slug, ctx)
    payload = {
        "@context": "https://schema.org",
        "@type": "HowTo",
        "name": f"/{cmd.slug}",
        "description": cmd.frontmatter.description,
        "url": url,
        "step": [
            {
                "@type": "HowToStep",
                "position": position,
                "name": f"Stage {stage.label} — {plain_inline(stage.title)}",
                "text": _step_text(cmd, stage),
                "url": f"{url}#{stage.anchor}",
            }
            for position, stage in enumerate(cmd.stages, start=1)
        ],
    }
    # JSON-LD is the one place HTML-escaping would corrupt the payload. Two
    # replacements close the </script> and <!-- breakouts; both stay valid JSON.
    body = json.dumps(payload, ensure_ascii=False, indent=2)
    body = body.replace("</", "<\\/").replace("<!--", "\\u003c!--")
    return '<script type="application/ld+json">\n' + body + "\n</script>"


def render_header() -> str:
    nav = "\n".join(
        f'          <li class="site-nav__item"><a class="site-nav__link" '
        f'href="{link("../../#" + anchor)}">{esc(label)}</a></li>'
        for anchor, label in NAV_LINKS
    )
    return f"""  <header class="site-header">
    <div class="container site-header__inner">
      <a class="site-header__brand" href="{link("../../")}">
        <img class="site-header__logo" src="{link("../../assets/logo-240.png")}" width="28" height="28" alt="">
        <span class="site-header__name">Shipmates</span>
      </a>
      <nav class="site-nav" aria-label="Primary">
        <ul class="site-nav__list">
{nav}
        </ul>
        <a class="btn btn--secondary site-nav__cta" href="{link("https://github.com/saman-mb/shipmates")}">
          {GITHUB_ICON}
          <span>GitHub</span>
        </a>
      </nav>
    </div>
  </header>"""


def render_footer() -> str:
    items = "\n".join(
        f'          <li><a href="{link(url)}">{esc(label)}</a></li>' for url, label in FOOTER_LINKS
    )
    legal = (
        "MIT License. Not affiliated with Anthropic. “Claude” and “Claude Code” "
        "are trademarks of Anthropic."
    )
    return f"""  <footer class="site-footer">
    <div class="container site-footer__inner">
      <div class="site-footer__brand">
        <img class="site-footer__logo" src="{link("../../assets/logo-240.png")}" width="32" height="32" alt="">
        <span class="site-footer__name">Shipmates</span>
        <p class="site-footer__tagline">Custom sub-agents &amp; slash-command workflows for Claude Code.</p>
      </div>
      <nav class="site-footer__nav" aria-label="Footer">
        <ul class="site-footer__links">
{items}
        </ul>
      </nav>
      <p class="site-footer__legal">{esc(legal)}</p>
    </div>
  </footer>"""


def render_back_link() -> str:
    return (
        f'<a class="order-back" href="{link("../../#orders")}">'
        '<span aria-hidden="true">←</span> All orders</a>'
    )


def render_hero(cmd: Command, src: str) -> str:
    flag = ""
    if cmd.slug == FLAGSHIP_SLUG:
        flag = '\n          <span class="order-detail__flag">Flagship</span>'
    # The frontmatter description is the one-line summary (it is also the meta
    # description); the intro blocks are the command file's own lede and follow it.
    intro = render_prose(cmd.intro, src, "          ")
    if intro:
        intro = "\n" + intro
    return f"""    <section class="section" aria-labelledby="order-title">
      <div class="container container--prose">
        {render_back_link()}
        <div class="order-detail">
          <p class="order-detail__eyebrow"><span aria-hidden="true">\U0001f4dc</span> Order</p>{flag}
          <h1 class="order-detail__title" id="order-title"><code>/{esc(cmd.slug)}</code></h1>
          <p class="order-detail__tagline">{esc(cmd.tagline)}</p>
          <p class="order-detail__desc">{esc(cmd.frontmatter.description)}</p>{intro}
        </div>
      </div>
    </section>"""


def render_invoke(cmd: Command) -> str:
    invocation = f"/{cmd.slug} {cmd.frontmatter.argument_hint}".strip()
    return f"""    <section class="section order-invoke" id="invoke" aria-labelledby="invoke-title">
      <div class="container container--prose">
        <div class="section__head">
          <h2 class="section__title" id="invoke-title">How to run it</h2>
        </div>
        <div class="codeblock">
          <p class="codeblock__label">Run it in Claude Code</p>
          <div class="codeblock__body">
            <pre class="codeblock__pre"><code class="codeblock__code">{esc(invocation)}</code></pre>
          </div>
        </div>
        <p class="order-invoke__hint"><code>&lt;angle brackets&gt;</code> = required · <code>[square brackets]</code> = optional</p>
      </div>
    </section>"""


def render_stage(st: Stage, src: str) -> str:
    """DOM order is visual order: num, title, gate, crew, body. No `order:` shuffling."""
    parts = [
        f'<li class="order-stage" id="{esc(st.anchor)}">',
        f'  <span class="order-stage__num" aria-hidden="true">{esc(st.label)}</span>',
        '  <h3 class="order-stage__title"><span class="visually-hidden">Stage '
        f'{esc(st.label)} — </span>{render_inline(st.title, src, st.lineno)}</h3>',
    ]
    if st.gate:
        parts.append(
            '  <p class="order-stage__gate"><span class="visually-hidden">Gate: </span>'
            f'<span aria-hidden="true">{GATE_MARK}</span> '
            f"{render_inline(st.gate, src, st.lineno)}</p>"
        )
    if st.crew or st.annotation:
        # [C-2] the source's own parenthetical is kept verbatim alongside the chips —
        # it carries detail the extracted crew list cannot ("x N, parallel", "fresh pass").
        bits = ["Crew:"]
        bits.extend(
            f'<span class="chip order-stage__crew-item"><code>{esc(name)}</code></span>'
            for name in st.crew
        )
        if st.annotation:
            bits.append(render_inline(st.annotation, src, st.lineno))
        parts.append('  <p class="order-stage__crew">' + " ".join(bits) + "</p>")
    if st.blocks:
        parts.append('  <div class="order-stage__body">')
        parts.append(render_prose(st.blocks, src, "    "))
        parts.append("  </div>")
    parts.append("</li>")
    return indent_html("\n".join(parts), "          ")


def _stages_lead(cmd: Command) -> str:
    total = len(cmd.stages)
    gates = sum(1 for stage in cmd.stages if stage.gate)
    lead = f"{_word(total)} stage{'' if total == 1 else 's'}, in order."
    if gates == 1:
        lead += " One is a hard gate — the run stops there until it passes."
    elif gates > 1:
        lead += f" {_word(gates)} are hard gates — the run stops there until they pass."
    return lead


def _word(n: int) -> str:
    return NUMBER_WORDS[n] if n < len(NUMBER_WORDS) else str(n)


def render_extra_sections(sections: tuple, src: str) -> str:
    """Sections that are neither Config, Guardrails nor a Stage — kept inside #stages.

    They render as an <h3> after the stage list rather than claiming a seventh
    section id, so nothing is dropped and no heading level is skipped.
    """
    out = []
    for section in sections:
        out.append(
            f'        <h3 id="{esc(section.anchor)}">{esc(section.title)}</h3>'
        )
        prose = render_prose(section.blocks, src, "        ")
        if prose:
            out.append(prose)
    return "\n".join(out)


def render_stages(cmd: Command, src: str) -> str:
    before = render_extra_sections(cmd.sections_before_stages, src)
    after = render_extra_sections(cmd.sections_after_stages, src)
    body = "\n".join(render_stage(stage, src) for stage in cmd.stages)
    parts = [
        '    <section class="section" id="stages" aria-labelledby="stages-title">',
        '      <div class="container container--prose">',
        '        <div class="section__head">',
        '          <p class="section__eyebrow">Step by step</p>',
        '          <h2 class="section__title" id="stages-title">The stages</h2>',
        f'          <p class="section__lead">{esc(_stages_lead(cmd))}</p>',
        "        </div>",
    ]
    if before:
        parts.append(before)
    parts.append('        <ol class="order-stages" role="list">')
    parts.append(body)
    parts.append("        </ol>")
    if after:
        parts.append(after)
    parts.append("      </div>")
    parts.append("    </section>")
    return "\n".join(parts)


def render_section(section, section_id: str, src: str):
    if section is None:
        return ""
    return f"""    <section class="section" id="{esc(section_id)}" aria-labelledby="{esc(section_id)}-title">
      <div class="container container--prose">
        <div class="section__head">
          <h2 class="section__title" id="{esc(section_id)}-title">{esc(section.title)}</h2>
        </div>
{render_prose(section.blocks, src, "        ")}
      </div>
    </section>"""


def render_source(cmd: Command, ctx: PageContext) -> str:
    blob = ctx.repo_blob_base + cmd.source_path
    return f"""    <section class="section order-source" id="source" aria-labelledby="source-title">
      <div class="container container--prose">
        <div class="section__head">
          <h2 class="section__title" id="source-title">Where this lives</h2>
        </div>
        <p>This page is generated from <code>{esc(cmd.source_path)}</code>. The installer copies it to <code>~/.claude/commands/{esc(cmd.slug)}.md</code> for every project, or <code>.claude/commands/{esc(cmd.slug)}.md</code> inside a single repo.</p>
        <a class="btn btn--secondary" href="{link(blob)}">
          {GITHUB_ICON}
          <span>View {esc(cmd.slug)}.md on GitHub</span>
        </a>
      </div>
    </section>"""


def render_siblings(cmd: Command, all_cmds: tuple) -> str:
    items = []
    for other in all_cmds:
        name = f"<code>/{esc(other.slug)}</code>"
        if other.slug == cmd.slug:
            inner = (
                '<span class="order-siblings__link order-siblings__link--current" '
                f'aria-current="page">{name}'
                '<span class="visually-hidden"> (current page)</span></span>'
            )
        else:
            inner = (
                f'<a class="order-siblings__link" href="{link("../" + other.slug + "/")}">'
                f"{name}</a>"
            )
        items.append(f'            <li class="order-siblings__item">{inner}</li>')
    listing = "\n".join(items)
    return f"""    <section class="section" id="other-orders" aria-labelledby="other-orders-title">
      <div class="container container--prose">
        <div class="section__head">
          <h2 class="section__title" id="other-orders-title">Other orders</h2>
        </div>
        <nav class="order-siblings" aria-label="Other orders">
          <ul class="order-siblings__list" role="list">
{listing}
          </ul>
        </nav>
        {render_back_link()}
      </div>
    </section>"""


def render_page(cmd: Command, all_cmds: tuple, ctx: PageContext) -> str:
    src = cmd.source_path
    sections = [
        render_hero(cmd, src),
        render_invoke(cmd),
        render_stages(cmd, src),
        render_section(cmd.config, "config", src),
        render_section(cmd.guardrails, "guardrails", src),
        render_source(cmd, ctx),
        render_siblings(cmd, all_cmds),
    ]
    body = "\n\n".join(part for part in sections if part)
    return f"""<!doctype html>
<html lang="en">
{render_head(cmd, ctx)}
<body>
  <a class="skip-link" href="#main">Skip to content</a>

{render_header()}

  <main class="main" id="main" tabindex="-1">

{body}

  </main>

{render_footer()}
</body>
</html>
"""


def render_sitemap(cmds: tuple, ctx: PageContext) -> str:
    entries = [
        "  <url>\n"
        f"    <loc>{esc(ctx.site_url)}</loc>\n"
        f"    <lastmod>{esc(ctx.lastmod)}</lastmod>\n"
        "    <changefreq>weekly</changefreq>\n"
        "    <priority>1.0</priority>\n"
        "  </url>"
    ]
    for cmd in cmds:
        entries.append(
            "  <url>\n"
            f"    <loc>{esc(canonical_url(cmd.slug, ctx))}</loc>\n"
            f"    <lastmod>{esc(ctx.lastmod)}</lastmod>\n"
            "    <changefreq>monthly</changefreq>\n"
            "    <priority>0.8</priority>\n"
            "  </url>"
        )
    return (
        '<?xml version="1.0" encoding="UTF-8"?>\n'
        '<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">\n'
        + "\n".join(entries)
        + "\n</urlset>\n"
    )


# ---------------------------------------------------------------------------
# Layer 4 — EMIT  (paths + bytes)
# No markup is built here. build_site() is pure; write_all() is the only writer.
# ---------------------------------------------------------------------------

SITE_DIR = "site"


def page_path(slug: str) -> str:
    return f"{SITE_DIR}/commands/{slug}/index.html"


SITEMAP_PATH = f"{SITE_DIR}/sitemap.xml"


def build_site(cmds: tuple, ctx: PageContext) -> dict:
    """Repo-relative posix path -> full file text. PURE: no I/O, no clock, no cwd.

    Every output is materialised in memory before anything is written, so a parse
    failure in any command leaves the tree completely untouched.
    """
    files = {page_path(cmd.slug): render_page(cmd, cmds, ctx) for cmd in cmds}
    files[SITEMAP_PATH] = render_sitemap(cmds, ctx)
    return files


def expected_paths(cmds: tuple) -> frozenset:
    return frozenset(page_path(cmd.slug) for cmd in cmds)


def write_all(files: dict, root: Path) -> list:
    """Write every file that differs, atomically. The only writer in this module."""
    written = []
    for rel in sorted(files):
        target = root / rel
        body = files[rel]
        if target.is_file() and target.read_text(encoding="utf-8") == body:
            continue
        target.parent.mkdir(parents=True, exist_ok=True)
        tmp = target.with_name(f"{target.name}.tmp-{os.getpid()}")
        with open(tmp, "w", encoding="utf-8", newline="\n") as handle:
            handle.write(body)
        os.replace(tmp, target)
        written.append(rel)
    return written


def find_orphans(root: Path, expected: frozenset) -> list:
    site = root / SITE_DIR
    return sorted(
        str(path.relative_to(root).as_posix())
        for path in sorted(site.glob("commands/*/index.html"))
        if path.relative_to(root).as_posix() not in expected
    )


def check_all(files: dict, root: Path) -> list:
    """Drift report lines. Writes NOTHING — this function never opens a path for writing."""
    report = []
    for rel in sorted(files):
        target = root / rel
        if not target.is_file():
            report.append(f"missing: {rel}")
            continue
        actual = target.read_text(encoding="utf-8")
        if actual == files[rel]:
            continue
        report.append(f"drift: {rel}")
        diff = difflib.unified_diff(
            actual.split("\n"),
            files[rel].split("\n"),
            fromfile=f"a/{rel}",
            tofile=f"b/{rel}",
            n=2,
            lineterm="",
        )
        for i, line in enumerate(diff):
            if i >= 20:
                report.append("    ... (diff truncated)")
                break
            report.append("    " + line)
    return report


# ---------------------------------------------------------------------------
# Layer 5 — CLI
# The only layer that prints or exits. Root comes from __file__, never from cwd.
# ---------------------------------------------------------------------------

ROOT = Path(__file__).resolve().parents[1]

REGENERATE_HINT = "run: python3 tools/gen_command_pages.py && git add site/"


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(
        description="Generate the per-command detail pages under site/commands/ and site/sitemap.xml."
    )
    parser.add_argument(
        "--check",
        action="store_true",
        help="report drift against the committed output and exit 1; write nothing",
    )
    parser.add_argument(
        "--lastmod",
        default=LASTMOD,
        metavar="YYYY-MM-DD",
        help=f"sitemap lastmod date (default: {LASTMOD})",
    )
    parser.add_argument(
        "--root",
        default=str(ROOT),
        metavar="PATH",
        help="repository root (default: the repo this script lives in)",
    )
    args = parser.parse_args(argv)

    if not LASTMOD_RE.match(args.lastmod):
        print(f"error: --lastmod must be YYYY-MM-DD, got {args.lastmod!r}", file=sys.stderr)
        return 2

    root = Path(args.root).resolve()
    ctx = PageContext(
        site_url=SITE_URL,
        lastmod=args.lastmod,
        social_image=SOCIAL_IMAGE,
        repo_blob_base=REPO_BLOB_BASE,
    )
    try:
        agents = load_agent_names(root / "agents")
        cmds = load_commands(root / "commands", agents)
        files = build_site(cmds, ctx)
    except SourceError as err:
        print(f"error: {err}", file=sys.stderr)
        return 1

    if args.check:
        report = check_all(files, root) + [
            f"unexpected generated file: {path} "
            "(renamed or removed a command? delete it and rerun)"
            for path in find_orphans(root, expected_paths(cmds))
        ]
        if report:
            for line in report:
                print(line)
            print(REGENERATE_HINT)
            return 1
        print(f"up to date: {len(files)} generated files, {len(cmds)} commands")
        return 0

    written = write_all(files, root)
    for path in find_orphans(root, expected_paths(cmds)):
        print(f"warning: unexpected generated file: {path} (renamed or removed a command?)")
    if written:
        for path in written:
            print(f"wrote {path}")
    print(f"{len(written)} of {len(files)} files updated ({len(cmds)} commands)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
