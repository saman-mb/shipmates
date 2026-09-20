#!/usr/bin/env python3
"""Validate canonical commands/ and crew/ — stdlib only.

Runs identically in a local self-check and in CI, with no arguments and no
network: commands/*.md and crew/*.md are the installer sources of truth, so they
are gated on their own terms rather than only through compiled payloads or the
site they feed. This checker does not walk a skills/ or harnesses/ tree, and it
does not lint rendered harness output.

For every commands/<slug>.md: frontmatter opens on line 1 and declares name then
description first (the standard's two required keys, in that order) followed by
any of the standard's optional keys and the vendor extensions we use, in any
order; the name is the filename stem; every declared value carries content (and
the description is bounded); no unescaped positional argument placeholder (`$1`)
survives anywhere in the file; every fenced code block is closed; shell fences
carry no inline `--body`/`-b` (and no command-substituted `--body-file` path);
prose and inline-backticks carry no invocation-shaped `--body` unless the same
line has `<!-- shipmates:body-ok -->`; every fan-out names its crew role; and a
strictly-read-only command (allowed-tools contains neither Write nor Edit) that
seats a write-capable crew role carries `<!-- shipmates:briefed-read-only:<role> -->`.

crew/*.md is scanned after frontmatter for the same `--body` rules (blunt inside
shell fences, invocation-shaped in prose). Role files are also the roster:
`writes: true` or `edit` in `capabilities` marks a write-capable seat. Compiled
payloads are not a second source of truth here.

The key set is a superset check, not an exact-set check. An earlier revision
demanded exactly name, description, argument-hint, allowed-tools in that fixed
order, which rejected the Agent Skills standard's own optional keys (license,
compatibility, metadata) — so a standard-legal SKILL.md written anywhere else
failed this gate, and `compatibility`, the first key a multi-harness adapter
would reach for, was unusable. Unknown keys are still rejected, so a typo like
`descrition` fails loudly rather than being ignored by every reader.

Positionals are flagged inside fenced code blocks too. An earlier revision
exempted fences, on the assumption that a `$2` between ``` markers is read as a
shell field reference rather than substituted. That assumption was never
verified — nothing documents a fence exemption, and the documented way to keep a
literal `$` before a digit is the backslash escape `\\$1`, which would be
pointless if fences were exempt — so it was removed. Substitution is treated as
textual over the whole file: `/shipmates-issue 42 focus on retries` binds `$2` to
`on` and rewrites `awk '{print $2}'` to `awk '{print on}'`.

Exit 0 if all green; exit 1 if one or more failures (printed).
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
COMMANDS = ROOT / "commands"

# The Agent Skills standard's required keys — present, and first, in this order.
REQUIRED_KEYS = ("name", "description")
# The standard's optional keys. Permitted in any order; none of the nine use them
# yet, but a SKILL.md that carries them is legal and must pass this gate.
STANDARD_OPTIONAL_KEYS = ("license", "compatibility", "metadata")
# Claude Code extensions the standard does not define. Permitted in any order.
EXTENSION_KEYS = ("argument-hint", "allowed-tools", "disable-model-invocation")
# Everything a SKILL.md may declare. Anything else is a typo or a private key
# no reader honours, and is rejected.
ALLOWED_KEYS = REQUIRED_KEYS + STANDARD_OPTIONAL_KEYS + EXTENSION_KEYS
# The canonical order for the keys the nine ship — recommended, and what the ok()
# note describes, but only the REQUIRED_KEYS prefix is enforced.
FRONTMATTER_KEYS = REQUIRED_KEYS + EXTENSION_KEYS

# crew/*.md — the source of truth for role names (see crew_roles()).
CREW = ROOT / "crew"

# `metadata:` is the one standard key defined as a nested mapping, so its value
# may live on indented continuation lines. They are opaque here (this is a
# line-oriented reader, not a YAML parser) but still scanned for placeholders.
INDENTED_RE = re.compile(r"^[ \t]")
BLOCK_KEY = "metadata"

# Claude Code reads these as booleans, in any letter case.
BOOLEAN_VALUES = frozenset({"true", "false", "yes", "no", "on", "off", "1", "0"})

KEY_RE = re.compile(r"^[A-Za-z0-9_-]+$")
NAME_RE = re.compile(r"^[a-z0-9]+(-[a-z0-9]+)*$")
# Positional argument placeholder, anywhere in the file. Skills are invoked by
# description, not by argv, so `$1` is either a leftover that never expands or —
# worse, inside a fence — live text the invocation rewrites. A backslash-escaped
# `\$1` is the documented literal, so the lookbehind lets it through.
POSITIONAL_RE = re.compile(r"(?<!\\)\$\{?[0-9]")

# `--body <anything>` puts content inside a shell command string, where a
# crafted title/body/diff/comment can break out of the quoting — the exact
# defect fixed twice already (#82 in ship-issue, #138 in ship-pr-review).
# `--body-file <path>` is the only form these skills may document.
#
# Deliberately blunt: it matches *any* value form, not just a double-quoted
# one. An earlier version anchored on `\"` and so waved through `--body '...'`,
# `--body $BODY` and `-b "$BODY"` — and the unquoted-variable spelling is
# strictly more dangerous than the one it caught, since it adds word-splitting
# and globbing on top. That means a correctly-quoted `--body "$CREW_AUTHORED"`
# is rejected too; that is the policy, not an oversight — a reviewer cannot
# tell from one line whether the variable holds crew text or PR text, so the
# skills route every body through a file.
BODY_FLAG_RE = re.compile(r"--body(?!-file)\b\s*=?\s*\S")

# `gh`'s short spelling of the same flag. Scoped to lines that invoke `gh`,
# because `-b` belongs to other tools too: `git worktree add -b <BRANCH>`
# appears in nine of these skills and is a branch name, not a body.
SHORT_BODY_RE = re.compile(r"(?<![\w-])-b\b\s*=?\s*\S")
GH_INVOCATION_RE = re.compile(r"(?<![\w-])gh\s")

# `--body-file` is only safe when the *path* is a literal or a plain variable.
# `--body-file "$(gh pr view <PR#> -q .title)"` is the original defect wearing
# the safe flag's name.
BODY_FILE_SUBST_RE = re.compile(r"--body-file\b\s*=?\s*\S*(?:\$\(|`)")

# Prose / inline-backticks: only an *invocation-shaped* `--body` is a hit.
# `--body=` or `--body` plus whitespace plus a quote, `$`, `<`, or ident is a
# value being passed; naming the flag in a never/don't sentence is not.
# `--body-file` stays the sanctioned form. Same-line `<!-- shipmates:body-ok -->`
# is the explicit escape hatch for a genuine exception.
PROSE_BODY_RE = re.compile(r"--body(?!-file)\b(?:\s*=|\s+[\"'`$</A-Za-z_])")
BODY_OK_RE = re.compile(r"<!--\s*shipmates:body-ok\s*-->")
NEVER_BODY_RE = re.compile(r"(?i)\b(never|don't|do not)\b")

# A strictly-read-only command that seats a write-capable role must carry one
# of these per role, repeatable. Unknown HTML comments are not renderer markers.
BRIEFED_READ_ONLY_RE = re.compile(
    r"<!--\s*shipmates:briefed-read-only:([a-z0-9]+(?:-[a-z0-9]+)*)\s*-->"
)
WRITE_TRUE = frozenset({"true", "yes", "on", "1"})

# Fence languages whose contents are shell the crew will run. A bare ``` fence
# counts: an unlabelled block of commands is still commands.
SHELL_FENCE_LANGS = frozenset({"", "bash", "sh", "shell", "zsh", "console", "shell-session"})

# Opening/closing fence, tracked by backtick count so a nested fence inside a
# heredoc cannot silently close the outer one.
FENCE_RE = re.compile(r"^(`{3,})(.*)$")

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
    "disable-model-invocation": (
        "write `true` to keep the skill user-invoked only, or drop the key to let "
        "the model load it on its own"
    ),
    "license": "name the licence (e.g. `MIT`), or drop the key",
    "compatibility": (
        "state what the skill needs from its environment (e.g. `Requires git and "
        "the GitHub CLI`), or drop the key"
    ),
}

KEY_LIST = ", ".join(FRONTMATTER_KEYS)
REQUIRED_LIST = ", ".join(REQUIRED_KEYS)
OPTIONAL_LIST = ", ".join(STANDARD_OPTIONAL_KEYS + EXTENSION_KEYS)

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
            f"'---' line, then {REQUIRED_LIST} in that order (optionally followed by "
            f"any of {OPTIONAL_LIST}), then a closing '---'"
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
        if INDENTED_RE.match(raw):
            if order and order[-1] == BLOCK_KEY:
                continue  # nested mapping under `metadata:` — opaque to this reader
            fail(
                f"{rel}:{lineno}: indented frontmatter line ({raw[:60]!r}) — only "
                f"`{BLOCK_KEY}:` takes a nested block; write every other entry on a "
                "single unindented line (allowed-tools is one comma-separated value, "
                "not a YAML list)"
            )
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
                f"{entries[key][0]}) — declare each frontmatter key exactly once"
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
    """REQUIRED_KEYS present and first, then any of ALLOWED_KEYS in any order.

    Order past the required prefix is a recommendation (KEY_LIST is what the nine
    ship), not a failure: the optional keys are the standard's, and a file that
    declares them in its own order is still legal. Duplicates are reported by the
    caller.
    """
    drift = False
    for key in order:
        if key not in ALLOWED_KEYS:
            fail(
                f"{rel}:{entries[key][0]}: unknown frontmatter key '{key}' — check the "
                f"spelling; a SKILL.md declares {REQUIRED_LIST} and may add any of "
                f"{OPTIONAL_LIST}"
            )
            drift = True
    for key in REQUIRED_KEYS:
        if key not in entries:
            fail(
                f"{rel}:{close_lineno}: frontmatter is missing '{key}' — declare "
                f"{REQUIRED_LIST} first, in that order (we ship {KEY_LIST})"
            )
            drift = True
    if drift:
        return  # an order report on top of a wrong key set is noise, not a second bug
    if tuple(order[: len(REQUIRED_KEYS)]) != REQUIRED_KEYS:
        first = next(
            key for key, want in zip(order, REQUIRED_KEYS) if key != want
        )
        fail(
            f"{rel}:{entries[first][0]}: frontmatter does not open with {REQUIRED_LIST} "
            f"(got {', '.join(order)}) — move them to the top in that order; the "
            "optional keys follow in any order"
        )


def check_values(rel: str, slug: str, entries: dict) -> None:
    if "name" in entries:
        lineno, name = entries["name"]
        if name != slug:
            fail(
                f"{rel}:{lineno}: name is {name!r} but the file is commands/{slug}.md — "
                f"set `name: {slug}`, or rename the file to commands/{name}.md"
            )
        elif not NAME_RE.fullmatch(name):
            fail(
                f"{rel}:{lineno}: name {name!r} is not lowercase-hyphen — use lowercase "
                "letters and digits joined by single hyphens (e.g. shipmates-issue)"
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
    if entries.get("disable-model-invocation", (0, ""))[1]:
        lineno, value = entries["disable-model-invocation"]
        if value.lower() not in BOOLEAN_VALUES:
            fail(
                f"{rel}:{lineno}: disable-model-invocation is {value!r}, which is not a "
                "boolean — write `true` to keep the skill user-invoked only; a value "
                "Claude Code cannot read silently restores model invocation"
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
                "``` fence does not protect it (`/shipmates-issue 42 focus on retries` would "
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


def _shell_blocks(lines: list[str], start: int):
    """Yield (lineno, logical line, fence_lineno) for every command line in a
    shell fence.

    Fences are matched by backtick count and only an info-string-free run of at
    least as many backticks closes one, so a nested ``` fence inside a heredoc
    no longer flips the tracker off and blinds the rest of the file. Backslash
    continuations are joined, so a flag and its argument split across two lines
    are scanned as the one command they are.
    """
    open_ticks = 0
    fence_lineno = 0
    lang = ""
    pending = ""
    pending_lineno = 0
    for offset, raw in enumerate(lines[start:]):
        lineno = start + offset + 1
        hit = FENCE_RE.match(raw.strip())
        if hit:
            ticks, info = len(hit.group(1)), hit.group(2).strip()
            if not fence_lineno:
                open_ticks, fence_lineno = ticks, lineno
                lang = info.split()[0].lower() if info else ""
            elif ticks >= open_ticks and not info:
                open_ticks = fence_lineno = 0
                lang = ""
                pending, pending_lineno = "", 0
            continue
        if not fence_lineno or lang not in SHELL_FENCE_LANGS:
            continue
        body = raw.rstrip()
        if not pending_lineno:
            pending_lineno = lineno
        if body.endswith("\\"):
            pending += body[:-1]
            continue
        yield pending_lineno, pending + body, fence_lineno
        pending, pending_lineno = "", 0
    if pending_lineno:
        yield pending_lineno, pending, fence_lineno


def _unfenced_lines(lines: list[str], start: int):
    """Yield (lineno, raw) for body lines that are not inside a fenced block."""
    open_ticks = 0
    fence_lineno = 0
    for offset, raw in enumerate(lines[start:]):
        lineno = start + offset + 1
        hit = FENCE_RE.match(raw.strip())
        if hit:
            ticks, info = len(hit.group(1)), hit.group(2).strip()
            if not fence_lineno:
                open_ticks, fence_lineno = ticks, lineno
            elif ticks >= open_ticks and not info:
                open_ticks = fence_lineno = 0
            continue
        if fence_lineno:
            continue
        yield lineno, raw


def check_no_inline_body(rel: str, lines: list[str], start: int) -> None:
    """No `--body`/`-b` inside a shell fence, and no invocation-shaped `--body`
    in prose.

    A shell-fence `--body`/`-b` puts content (a PR/issue title, body, diff, or
    review comment — all attacker-controlled on anything the crew didn't write)
    inside a shell command string, where a crafted value can break out of the
    quoting. This is the same defect fixed twice already, once per command
    (#82, #138) — a lint that fails the build is cheaper than a third fix.
    Fence checks stay blunt: BODY_FLAG_RE, gh-scoped SHORT_BODY_RE,
    BODY_FILE_SUBST_RE. `--body-file <path>` is the sanctioned form, but only
    with a literal or plain-variable path: substituting a command into the
    *filename* smuggles the same defect back in under the safe flag's name.

    Prose and inline-backticks are narrower: only `--body=` or `--body` plus a
    value starter (quote, `$`, `<`, ident) is a hit. Naming the flag inside a
    never/don't sentence is not an invocation. Same-line
    `<!-- shipmates:body-ok -->` is the explicit escape hatch.
    """
    for lineno, line, fence_lineno in _shell_blocks(lines, start):
        short_hit = GH_INVOCATION_RE.search(line) and SHORT_BODY_RE.search(line)
        if BODY_FLAG_RE.search(line) or short_hit:
            fail(
                f"{rel}:{lineno}: {line.strip()[:70]!r} passes content to `--body` "
                f"inside the shell fence opened on line {fence_lineno} — a crafted "
                "title/body/diff/comment can break out of the shell quoting (the #82 / #138 "
                "defect class); write the content to a temp file and use `--body-file <path>` "
                "instead"
            )
        if BODY_FILE_SUBST_RE.search(line):
            fail(
                f"{rel}:{lineno}: {line.strip()[:70]!r} substitutes a command into the "
                f"`--body-file` path inside the shell fence opened on line {fence_lineno} — "
                "that is the #82 / #138 defect wearing the safe flag's name; capture the "
                "value into a quoted variable on its own line and pass the variable"
            )
    for lineno, line in _unfenced_lines(lines, start):
        if BODY_OK_RE.search(line):
            continue
        for hit in PROSE_BODY_RE.finditer(line):
            if NEVER_BODY_RE.search(line[: hit.start()]):
                continue
            fail(
                f"{rel}:{lineno}: {line.strip()[:70]!r} passes content to `--body` in "
                "prose — an invocation-shaped `--body=` / `--body <value>` in running text "
                "is the same defect class as a shell fence; write the content to a temp file "
                "and use `--body-file <path>`, or add `<!-- shipmates:body-ok -->` on this "
                "line if it is not an invocation"
            )
            break


def crew_roles() -> tuple:
    """The shipped crew role names, read from crew/*.md.

    Read from disk rather than hardcoded: a second hand-maintained copy of the
    roster would drift, and a spawn naming a role that was renamed or deleted
    must fail this check — that is the defect it guards. Returns () when crew/ is
    absent; the caller fails loudly rather than passing, because a check that
    cannot see the roster cannot certify the command.
    """
    if not CREW.is_dir():
        return ()
    return tuple(sorted(p.stem for p in CREW.glob("*.md")))


def check_spawn_binds_crew_role(rel: str, lines: list[str], start: int) -> None:
    """A command that fans work out to workers must declare which crew role does it.

    The failure this guards is silent and expensive: a command declares
    `MAX_CONCURRENT_WORKERS` and tells the reader to "spawn workers", naming no role.
    The harness has nothing to resolve, falls back to a general-purpose agent, and the
    run looks healthy while every finding came from a generic agent instead of the
    specialist the class needed. The payload is well-formed, the digests match, CI is
    green, and a captain gets worse work than they paid for.

    **Structured, not prose.** This reads declarations, never sentences:

      * a stage heading carrying the catalogue's `(agent: \\`role\\`)` annotation — the
        shape 8 of the 9 fan-out commands already use, and the shape a reader and a
        renderer both already understand; or
      * a Config binding — an ALLCAPS knob whose value *is* a crew role
        (`BUILDER = senior-engineer`), which is how a command that composes another
        command instead of spawning a role declares who owns the work; or
      * a stage heading that names a composed command (`(composes: /ship-issue)`) for
        the same case, when the composed command owns the crew rather than a role.

    An earlier revision of this check scanned prose for role names near spawn verbs. It
    was defeated four times in review — by a file-wide anchor, by a one-word insert in an
    unrelated stage, by the substring `architect` inside `architectural`, and by a
    *negative* mention ("do not compose /ship-issue") satisfying it. Every patch closed
    the named hole and opened an equivalent one, because the input was prose. Reading a
    declaration instead of a sentence is what makes the check mean something.
    """
    text = "\n".join(lines)
    if "MAX_CONCURRENT_WORKERS" not in text:
        return

    roles = crew_roles()
    if not roles:
        fail(
            f"{rel}:{start + 1}: cannot check that this fan-out declares a crew role — "
            "crew/*.md is missing or unreadable, so the roster is unknown. A broken "
            "environment is not a pass: restore crew/ (or run the validator from the "
            "repository root) so the check can see the roles"
        )
        return

    # A role name must match whole: `architect` is a crew role, `architectural` is a
    # word this catalogue uses constantly, and treating one as the other is how a
    # prose scan passed a command that named nobody.
    role_alt = "|".join(re.escape(role) for role in roles)
    role_re = re.compile(r"(?<![\w-])(?:" + role_alt + r")(?![\w-])")

    # Scope (1) and (3) to the stage that declares the fan-out. A heading anywhere in
    # the file is not an answer: `ship-epic`'s fan-out stage says `(orchestrator)` and
    # the command happens to carry `(agent: architect)` on its planning stage, so an
    # unscoped scan passed it on an annotation that has nothing to do with who runs
    # the wave. The declaration must sit on the stage that fans out.
    body_lines = lines[start:]
    section_of: list[int] = []
    current = 0
    for raw in body_lines:
        if raw.startswith("#"):
            current += 1
        section_of.append(current)
    knob_sections = {
        section_of[i]
        for i, raw in enumerate(body_lines)
        if "MAX_CONCURRENT_WORKERS" in raw
    }

    heading_ann = re.compile(r"^#+.*\((?:agents?)\s*:(?P<body>[^)]*)\)")
    composes = re.compile(r"^#+.*\((?:composes)\s*:\s*/ship-[a-z-]+", re.IGNORECASE)
    for section in sorted(knob_sections):
        for i, raw in enumerate(body_lines):
            if section_of[i] != section:
                continue
            ann = heading_ann.match(raw)
            if ann and role_re.search(ann.group("body")):
                return
            if composes.match(raw):
                return

    # (2) A Config binding whose value IS a role — how a command that names its worker
    # through a knob declares who owns the work. Scoped to the Config block and the
    # fan-out stages, like the heading anchors: a file-wide search here would be the last
    # escape hatch of the same class this check exists to close, since any unrelated knob
    # bound to a role name anywhere in the document would satisfy it.
    binding = re.compile(
        r"`?[A-Z][A-Z_]+`?\s*=\s*`?(?:" + role_alt + r")`?(?![\w-])"
    )
    config_lines = [
        raw
        for i, raw in enumerate(body_lines)
        if section_of[i] == 0 or section_of[i] in knob_sections
    ]
    if any(binding.search(raw) for raw in config_lines):
        return

    fail(
        f"{rel}:{start + 1}: fans out to workers (`MAX_CONCURRENT_WORKERS`) but declares no "
        "crew role to do the work — a harness has nothing to resolve and silently falls back "
        "to a general-purpose agent, discarding the specialism the findings need. Declare it "
        "structurally: annotate the fan-out stage heading `(agent: `role`)`, bind a Config "
        "knob to a role (`BUILDER` = `senior-engineer`), or annotate a stage that composes "
        "another command as `(composes: /ship-issue)` when that command owns the crew. Naming "
        "the role in prose alone does not count — this check reads declarations, and a "
        "sentence can be edited without changing what the command does"
    )


def _frontmatter_entries(lines: list[str]) -> dict[str, str]:
    """Line-oriented key: value map up to the closing '---'. Unknown keys kept."""
    entries: dict[str, str] = {}
    if not lines or lines[0].strip() != "---":
        return entries
    for raw in lines[1:]:
        if raw.strip() == "---":
            break
        if not raw.strip() or INDENTED_RE.match(raw):
            continue
        key, sep, value = raw.partition(":")
        if sep:
            entries[key] = value.strip()
    return entries


def _body_start(lines: list[str]) -> int:
    """Index of the first body line after closing frontmatter, or 0."""
    if lines and lines[0].strip() == "---":
        for i in range(1, len(lines)):
            if lines[i].strip() == "---":
                return i + 1
    return 0


def write_capable_roles() -> frozenset[str]:
    """Crew stems that can write: `writes: true` or `edit` in capabilities."""
    capable: set[str] = set()
    if not CREW.is_dir():
        return frozenset()
    for path in CREW.glob("*.md"):
        entries = _frontmatter_entries(path.read_text(encoding="utf-8").split("\n"))
        writes = entries.get("writes", "").lower()
        caps = {
            part.strip().lower()
            for part in entries.get("capabilities", "").split(",")
            if part.strip()
        }
        if writes in WRITE_TRUE or "edit" in caps:
            capable.add(path.stem)
    return frozenset(capable)


def _strictly_read_only(entries: dict) -> bool:
    """True only when allowed-tools is present and contains neither Write nor Edit."""
    if "allowed-tools" not in entries:
        return False
    tokens = {
        part.strip().lower()
        for part in entries["allowed-tools"][1].split(",")
        if part.strip()
    }
    return "write" not in tokens and "edit" not in tokens


def check_read_only_write_seats(
    rel: str, lines: list[str], start: int, entries: dict
) -> None:
    """A strictly-read-only command must brief every write-capable seat it names.

    Inferred from allowed-tools (no extra frontmatter key): neither Write nor
    Edit. A seated role is a backtick-wrapped crew stem. Write-capable means
    the role's own `writes: true` or `edit` in capabilities. Mixed-mode
    commands that keep Write/Edit do not trip. Crew files are not gated.
    """
    if not _strictly_read_only(entries):
        return
    roles = crew_roles()
    if not roles:
        fail(
            f"{rel}:{start + 1}: cannot check write-capable seats on a strictly-read-only "
            "command — crew/*.md is missing or unreadable, so the roster is unknown. "
            "Restore crew/ (or run the validator from the repository root) so the check "
            "can see which roles write"
        )
        return
    capable = write_capable_roles()
    body = "\n".join(lines[start:])
    role_alt = "|".join(re.escape(role) for role in roles)
    seated = set(re.findall(r"`(" + role_alt + r")`", body))
    briefed = set(BRIEFED_READ_ONLY_RE.findall(body))
    for role in sorted(seated & capable):
        if role not in briefed:
            fail(
                f"{rel}:{start + 1}: strictly-read-only command seats write-capable "
                f"`{role}` without `<!-- shipmates:briefed-read-only:{role} -->` — a role "
                "that can write will, so the command's read-only promise holds only while "
                "that seat is briefed to report and write nothing. Add one marker per "
                "write-capable role, with <role> equal to the crew file stem"
            )


def check_command(path: Path) -> None:
    slug = path.stem
    rel = f"commands/{path.name}"
    before = len(failures)

    lines = path.read_text(encoding="utf-8").split("\n")
    parsed = parse_frontmatter(rel, lines)
    if parsed is None:
        return
    entries, start = parsed
    check_values(rel, slug, entries)
    check_frontmatter(rel, lines, start)
    check_body(rel, lines, start)
    check_no_inline_body(rel, lines, start)
    check_spawn_binds_crew_role(rel, lines, start)
    check_read_only_write_seats(rel, lines, start, entries)

    if len(failures) == before:
        ok(
            f"{rel}: frontmatter opens with {REQUIRED_LIST}, every key known and non-empty, "
            "name matches filename, no unescaped '$n' anywhere, fences closed, no inline "
            "--body in a shell fence or invocation-shaped in prose, every fan-out names its "
            "crew role, read-only write-capable seats are briefed"
        )


def check_crew(path: Path) -> None:
    rel = f"crew/{path.name}"
    before = len(failures)
    lines = path.read_text(encoding="utf-8").split("\n")
    start = _body_start(lines)
    check_no_inline_body(rel, lines, start)
    if len(failures) == before:
        ok(f"{rel}: no inline --body in a shell fence or invocation-shaped in prose")


def main() -> int:
    if not COMMANDS.is_dir():
        fail(
            "commands/: directory not found — the workflows live in commands/<slug>.md, "
            "one file per command"
        )
        return report()

    files = sorted(COMMANDS.glob("*.md"))
    if not files:
        fail("commands/: no command files — add commands/<slug>.md, one per workflow")
        return report()

    before = len(failures)
    for path in files:
        check_command(path)
    if len(failures) == before:
        ok(f"commands/: {len(files)} command files, each valid")

    if CREW.is_dir():
        crew_files = sorted(CREW.glob("*.md"))
        before = len(failures)
        for path in crew_files:
            check_crew(path)
        if len(failures) == before:
            ok(f"crew/: {len(crew_files)} role files, --body scan clean")
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
