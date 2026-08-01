#!/usr/bin/env python3
"""Generate harnesses/<target>/ payload trees — stdlib only.

Every target with a registered adapter in tools/adapters/registry.py is compiled
from canonical/ by tools/export.py, and is delegated to it here once
canonical/manifest.json enables the target. The legacy matrix transformer below
remains for the targets canonical has not taken over yet.

Reads tools/harness_matrix.json, the declarative feature x harness matrix, and
projects those remaining source documents into one payload tree per target
harness:

    harnesses/<target>/skills/<slug>/SKILL.md   (frontmatter transformed)
    harnesses/<target>/agents/<name>.md         (only when the matrix says the
                                                 harness supports agents)

Frontmatter is transformed per the matrix. For each (feature, target) pair:

  supported: true   emit the key, respelled if the matrix names a different
                    "spelling", and with a comma-separated value rewritten to
                    the Agent Skills standard's space-separated form when the
                    entry declares "separator": "space" (the canonical sources
                    stay comma-separated because that is what Claude Code
                    parses).
  policy: drop      omit the key silently — cosmetic loss only.
  policy: warn      omit the key, print one machine-readable JSON line (stderr)
                    recording exactly what was lost.
  policy: emulate   omit the key and append a deterministic "Harness
                    adaptation" section to the body expressing it in prose.
  policy: refuse    fail the whole target before anything is written. Reserved
                    for safety properties: every shipped command sets
                    disable-model-invocation: true, and a harness with no
                    equivalent gets no payload rather than an auto-invocable
                    command that creates worktrees, pushes branches and opens
                    pull requests.

A frontmatter key the matrix does not cover (wrong spelling, or a skills-only
key showing up in agents/*.md) is a hard error, not a silent passthrough — the
matrix is the single source of truth for what may leave the repo, so an
uncovered key fails the build instead of drifting past it. The matrix itself is
validated at load: version, known policies, every feature covering every
harness exactly, "emulation" present iff policy is emulate.

Matrix extensions beyond the task's minimal shape, all additive: a top-level
"harnesses" block (whether the target gets an agents/ tree), a per-feature
"applies_to" list ("skills" / "agents"), per-entry "separator" ("comma" |
"space"), and a free-text "reason" on refuse entries.

Deterministic and committed, matching the repo's other generators. Regenerate
with:  python3 tools/build_harness_payloads.py            (all targets)
       python3 tools/build_harness_payloads.py --target claude-code
CI gate:  python3 tools/build_harness_payloads.py --check [--target X]

Layers, in file order:
  1 MODEL   frozen dataclasses only — no I/O
  2 PARSE   matrix JSON + frontmatter documents -> model
  3 TRANSFORM  (document, target) -> rendered text + records — pure
  4 EMIT    files dict + atomic writes + drift check — build_target() is pure
  5 CLI     argv -> exit code — the only layer that prints

Exit 0 on success; 1 on refusal, drift, or unreadable sources; 2 on a malformed
matrix or bad usage. Warnings and refusals are JSON lines on stderr so stdout
stays human-readable.
"""

from __future__ import annotations

import argparse
import difflib
import json
import os
import re
import sys
from dataclasses import dataclass
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
MATRIX_REL = "tools/harness_matrix.json"
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

SKILLS = "skills"
AGENTS = "agents"
KINDS = (SKILLS, AGENTS)

POLICIES = ("drop", "warn", "emulate", "refuse")
SEPARATORS = ("comma", "space")

KEY_RE = re.compile(r"^[A-Za-z0-9_-]+$")
HARNESS_RE = re.compile(r"^[a-z0-9][a-z0-9-]*$")
INDENTED_RE = re.compile(r"^[ \t]")

# Diff context lines shown per drifted file before truncating, mirroring
# tools/gen_command_pages.py's check_all.
MAX_DIFF_LINES = 20


class MatrixError(Exception):
    """tools/harness_matrix.json failed validation."""


class SourceError(Exception):
    """A skills/ or agents/ source file could not be parsed."""


# ---------------------------------------------------------------------------
# Layer 1 — MODEL
# Frozen dataclasses. No I/O.
# ---------------------------------------------------------------------------


@dataclass(frozen=True, slots=True)
class Policy:
    """One (feature, harness) cell of the matrix."""

    supported: bool
    spelling: str | None  # target's key name when it differs from ours
    policy: str | None  # required when supported is False
    emulation: str | None  # required when policy == "emulate"
    reason: str | None  # optional prose, carried into refuse records
    separator: str  # "comma" (canonical) or "space" (the standard's form)


@dataclass(frozen=True, slots=True)
class Feature:
    name: str
    applies_to: frozenset  # subset of {"skills", "agents"}
    harnesses: dict  # target name -> Policy


@dataclass(frozen=True, slots=True)
class Harness:
    name: str
    agents: bool  # whether the target gets an agents/ tree at all


@dataclass(frozen=True, slots=True)
class Matrix:
    harnesses: tuple  # tuple[Harness, ...], declaration order
    features: dict  # feature name -> Feature


@dataclass(frozen=True, slots=True)
class Entry:
    """One frontmatter key, keeping its raw physical lines for verbatim re-emit.

    `block` holds indented continuation lines (the `metadata:` nested mapping);
    opaque here, preserved verbatim either way.
    """

    key: str
    value: str
    raw: str  # the physical 'key: value' line
    block: tuple  # tuple[str, ...] continuation lines


@dataclass(frozen=True, slots=True)
class Document:
    entries: tuple  # tuple[Entry, ...], source order
    body: str  # everything after the closing '---' line, verbatim


@dataclass(frozen=True, slots=True)
class BuildResult:
    files: dict  # repo-relative posix path -> full file text
    warnings: tuple  # tuple[dict, ...] JSON-line records
    refusals: tuple  # tuple[dict, ...] — non-empty fails the target
    infos: tuple  # tuple[dict, ...]


# ---------------------------------------------------------------------------
# Layer 2 — PARSE
# Matrix JSON and frontmatter documents -> model. No rendering.
# ---------------------------------------------------------------------------


def _policy(feature: str, target: str, raw: dict) -> Policy:
    where = f"features.{feature}.harnesses.{target}"
    if not isinstance(raw, dict) or not isinstance(raw.get("supported"), bool):
        raise MatrixError(f"{where}: must be an object with a boolean 'supported'")
    supported = raw["supported"]
    spelling = raw.get("spelling")
    policy = raw.get("policy")
    emulation = raw.get("emulation")
    reason = raw.get("reason")
    separator = raw.get("separator", "comma")
    if spelling is not None and not KEY_RE.fullmatch(spelling):
        raise MatrixError(f"{where}: spelling {spelling!r} is not a valid frontmatter key")
    if separator not in SEPARATORS:
        raise MatrixError(f"{where}: separator must be one of {SEPARATORS}, got {separator!r}")
    if reason is not None and not isinstance(reason, str):
        raise MatrixError(f"{where}: reason must be a string")
    if supported:
        if policy is not None or emulation is not None:
            raise MatrixError(
                f"{where}: supported entries take no policy/emulation — support is native"
            )
    else:
        if policy not in POLICIES:
            raise MatrixError(f"{where}: unsupported entries need a policy in {POLICIES}")
        if policy == "emulate" and not emulation:
            raise MatrixError(
                f"{where}: policy 'emulate' needs the emulation prose strategy to apply"
            )
        if emulation and policy != "emulate":
            raise MatrixError(f"{where}: emulation text without policy 'emulate'")
        if spelling is not None or separator != "comma":
            raise MatrixError(
                f"{where}: spelling/separator transform a supported key; this entry is omitted"
            )
    return Policy(supported, spelling, policy, emulation, reason, separator)


def load_matrix(root: Path) -> Matrix:
    path = root / MATRIX_REL
    if not path.is_file():
        raise MatrixError(f"{MATRIX_REL}: file not found")
    try:
        raw = json.loads(path.read_text(encoding="utf-8"))
    except json.JSONDecodeError as err:
        raise MatrixError(f"{MATRIX_REL}:{err.lineno}: invalid JSON — {err.msg}") from err
    if not isinstance(raw, dict) or raw.get("version") != 1:
        raise MatrixError(f"{MATRIX_REL}: top-level object with \"version\": 1 expected")
    raw_harnesses = raw.get("harnesses")
    if not isinstance(raw_harnesses, dict) or not raw_harnesses:
        raise MatrixError(f"{MATRIX_REL}: 'harnesses' must be a non-empty object")
    harnesses = []
    for name, cell in raw_harnesses.items():
        if not isinstance(cell, dict) or not isinstance(cell.get("agents"), bool):
            raise MatrixError(f"harnesses.{name}: must be an object with a boolean 'agents'")
        if not HARNESS_RE.fullmatch(name):
            raise MatrixError(
                f"harnesses.{name}: name must match {HARNESS_RE.pattern} "
                "(lowercase alphanumerics and hyphens only — no path traversal)"
            )
        harnesses.append(Harness(name, cell["agents"]))
    raw_features = raw.get("features")
    if not isinstance(raw_features, dict) or not raw_features:
        raise MatrixError(f"{MATRIX_REL}: 'features' must be a non-empty object")
    features = {}
    for fname, fcell in raw_features.items():
        if not isinstance(fcell, dict):
            raise MatrixError(f"features.{fname}: must be an object")
        applies_to = fcell.get("applies_to")
        if (
            not isinstance(applies_to, list)
            or not applies_to
            or any(k not in KINDS for k in applies_to)
        ):
            raise MatrixError(
                f"features.{fname}.applies_to: non-empty list drawn from {list(KINDS)} expected"
            )
        cells = fcell.get("harnesses")
        if not isinstance(cells, dict):
            raise MatrixError(f"features.{fname}.harnesses: must be an object")
        missing = [h.name for h in harnesses if h.name not in cells]
        extra = [k for k in cells if k not in raw_harnesses]
        if missing or extra:
            raise MatrixError(
                f"features.{fname}.harnesses: must cover every harness exactly "
                f"(missing: {missing or 'none'}; unknown: {extra or 'none'})"
            )
        features[fname] = Feature(
            fname,
            frozenset(applies_to),
            {t: _policy(fname, t, cells[t]) for t in raw_harnesses},
        )
    return Matrix(tuple(harnesses), features)


def parse_document(rel: str, text: str) -> Document:
    """Frontmatter -> ordered Entries; body kept verbatim for byte-exact re-emit.

    Line-oriented, mirroring tools/validate_skills.py's reader: 'key: value'
    per line, indented continuations belong to the key above (the `metadata:`
    nested mapping). Unknown keys are NOT rejected here — they are rejected at
    transform time against the matrix, which is the coverage gate.
    """
    lines = text.split("\n")
    if not lines or lines[0].strip() != "---":
        raise SourceError(f"{rel}:1: no opening frontmatter '---' line")
    close = None
    for i in range(1, len(lines)):
        if lines[i].strip() == "---":
            close = i
            break
    if close is None:
        raise SourceError(f"{rel}:{len(lines)}: unterminated frontmatter — close it with '---'")

    entries: list[Entry] = []
    for i in range(1, close):
        raw = lines[i]
        lineno = i + 1
        if not raw.strip():
            continue
        if INDENTED_RE.match(raw):
            if not entries:
                raise SourceError(f"{rel}:{lineno}: indented line before any frontmatter key")
            last = entries[-1]
            entries[-1] = Entry(last.key, last.value, last.raw, last.block + (raw,))
            continue
        key, sep, value = raw.partition(":")
        if not sep or not KEY_RE.fullmatch(key):
            raise SourceError(f"{rel}:{lineno}: not a 'key: value' line ({raw[:60]!r})")
        if any(e.key == key for e in entries):
            raise SourceError(f"{rel}:{lineno}: duplicate frontmatter key '{key}'")
        entries.append(Entry(key, value.strip(), raw, ()))

    body = "\n".join(lines[close + 1 :])
    return Document(tuple(entries), body)


def load_sources(root: Path) -> tuple[dict, dict]:
    """({slug: Document}, {agent-name: Document}) from the canonical trees."""
    skills_dir = root / "skills"
    agents_dir = root / "agents"
    if not skills_dir.is_dir():
        raise SourceError("skills/: directory not found — the canonical command sources")
    if not agents_dir.is_dir():
        raise SourceError("agents/: directory not found — the canonical agent sources")
    skills = {}
    for directory in sorted(skills_dir.iterdir()):
        if not directory.is_dir():
            continue
        path = directory / "SKILL.md"
        if not path.is_file():
            raise SourceError(f"skills/{directory.name}/SKILL.md: file not found")
        skills[directory.name] = parse_document(
            f"skills/{directory.name}/SKILL.md", path.read_text(encoding="utf-8")
        )
    agents = {}
    for path in sorted(agents_dir.glob("*.md")):
        agents[path.stem] = parse_document(
            f"agents/{path.name}", path.read_text(encoding="utf-8")
        )
    if not skills:
        raise SourceError("skills/: no skill directories found")
    if not agents:
        raise SourceError("agents/: no agent files found")
    return skills, agents


# ---------------------------------------------------------------------------
# Layer 3 — TRANSFORM
# (document, kind, target) -> rendered text + records. Pure.
# ---------------------------------------------------------------------------


def respell_list(value: str, separator: str) -> str:
    """comma-separated canonical -> the target's separator. Identity for comma."""
    if separator == "comma":
        return value
    return " ".join(part.strip() for part in value.split(",") if part.strip())


def emulation_block(target: str, entry: Entry, emulation: str) -> str:
    """The prose stand-in appended to the body. Deterministic, one per key."""
    return (
        f"\n\n---\n\n## Harness adaptation (`{entry.key}`)\n\n"
        f"`{entry.key}` is not supported by {target}; {emulation}.\n\n"
        f"Canonical value: `{entry.value}`.\n"
    )


def transform_document(
    rel: str, doc: Document, kind: str, target: str, matrix: Matrix
) -> tuple[str, list, list]:
    """(rendered text, warnings, refusals). Raises SourceError on uncovered keys.

    Untransformed entries re-emit their raw physical lines, so a target whose
    matrix is all-supported (claude-code) reproduces its source byte-for-byte.
    """
    out_lines: list[str] = []
    warnings: list[dict] = []
    refusals: list[dict] = []
    emulations: list[str] = []
    for entry in doc.entries:
        feature = matrix.features.get(entry.key)
        if feature is None or kind not in feature.applies_to:
            raise SourceError(
                f"{rel}: frontmatter key '{entry.key}' is not covered by "
                f"{MATRIX_REL} for {kind} — add it to the matrix (every feature x "
                "every harness) or fix the key spelling"
            )
        cell = feature.harnesses[target]
        if cell.supported:
            key = cell.spelling or entry.key
            value = respell_list(entry.value, cell.separator)
            if key == entry.key and value == entry.value:
                out_lines.append(entry.raw)
            else:
                out_lines.append(f"{key}: {value}")
            out_lines.extend(entry.block)
            continue
        if cell.policy == "drop":
            continue
        if cell.policy == "warn":
            warnings.append(
                {
                    "type": "warning",
                    "policy": "warn",
                    "target": target,
                    "file": rel,
                    "key": entry.key,
                    "value": entry.value,
                    "action": "omitted",
                }
            )
            continue
        if cell.policy == "emulate":
            emulations.append(emulation_block(target, entry, cell.emulation))
            continue
        refusals.append(
            {
                "type": "error",
                "policy": "refuse",
                "target": target,
                "file": rel,
                "key": entry.key,
                "value": entry.value,
                "reason": cell.reason or f"'{entry.key}' cannot be honoured by {target}",
            }
        )
    body = doc.body
    if emulations:
        body = body.rstrip("\n") + "".join(emulations)
    text = "---\n" + "\n".join(out_lines) + "\n---\n" + body
    return text, warnings, refusals


# ---------------------------------------------------------------------------
# Layer 4 — EMIT
# build_target() is pure; write_all() is the only writer.
# ---------------------------------------------------------------------------


def build_target(
    matrix: Matrix, harness: Harness, skills: dict, agents: dict
) -> BuildResult:
    """Repo-relative posix path -> full file text, plus policy records.

    Every output is materialised in memory before anything is written, so a
    refusal in any source leaves the tree completely untouched.
    """
    files: dict[str, str] = {}
    warnings: list[dict] = []
    refusals: list[dict] = []
    infos: list[dict] = []
    for slug in sorted(skills):
        rel = f"skills/{slug}/SKILL.md"
        text, w, r = transform_document(rel, skills[slug], SKILLS, harness.name, matrix)
        files[f"harnesses/{harness.name}/skills/{slug}/SKILL.md"] = text
        warnings.extend(w)
        refusals.extend(r)
    if harness.agents:
        for name in sorted(agents):
            rel = f"agents/{name}.md"
            text, w, r = transform_document(rel, agents[name], AGENTS, harness.name, matrix)
            files[f"harnesses/{harness.name}/agents/{name}.md"] = text
            warnings.extend(w)
            refusals.extend(r)
    else:
        infos.append(
            {
                "type": "info",
                "target": harness.name,
                "message": f"{harness.name} declares no agents tree — "
                f"{len(agents)} agent(s) skipped",
            }
        )
    return BuildResult(files, tuple(warnings), tuple(refusals), tuple(infos))


def expected_paths(result: BuildResult) -> frozenset:
    return frozenset(result.files)


def write_all(files: dict, root: Path) -> list:
    """Write every file that differs, atomically. The only writer in this module."""
    written = []
    harnesses_root = (root / "harnesses").resolve()
    for rel in sorted(files):
        target = (root / rel).resolve()
        if not str(target).startswith(str(harnesses_root) + os.sep):
            raise MatrixError(f"refusing to write outside harnesses/: {rel}")
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


def find_orphans(root: Path, harness: str, expected: frozenset) -> list:
    base = root / "harnesses" / harness
    if not base.is_dir():
        return []
    return sorted(
        str(path.relative_to(root).as_posix())
        for path in sorted(base.rglob("*"))
        if path.is_file() and path.relative_to(root).as_posix() not in expected
    )


def find_orphan_targets(root: Path, known: frozenset) -> list:
    base = root / "harnesses"
    if not base.is_dir():
        return []
    return sorted(
        f"harnesses/{path.name}/"
        for path in sorted(base.iterdir())
        if path.is_dir() and path.name not in known
    )


def check_all(files: dict, root: Path) -> list:
    """Drift report lines. Writes NOTHING — this function never opens a path for writing."""
    report = []
    harnesses_root = (root / "harnesses").resolve()
    for rel in sorted(files):
        target = (root / rel).resolve()
        if not str(target).startswith(str(harnesses_root) + os.sep):
            report.append(f"refusing to check outside harnesses/: {rel}")
            continue
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
            if i >= MAX_DIFF_LINES:
                report.append("    ... (diff truncated)")
                break
            report.append("    " + line)
    return report


# ---------------------------------------------------------------------------
# Layer 5 — CLI
# The only layer that prints or exits. Root comes from __file__, never from cwd.
# ---------------------------------------------------------------------------


def emit_jsonl(records: tuple) -> None:
    for record in records:
        print(json.dumps(record, sort_keys=True, ensure_ascii=False), file=sys.stderr)


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(
        description=(
            "Generate per-harness payload trees under harnesses/<target>/ from "
            "skills/ + agents/ according to tools/harness_matrix.json."
        )
    )
    parser.add_argument(
        "--target",
        default="all",
        metavar="HARNESS",
        help="harness to generate (a name from the matrix, or 'all'; default: all)",
    )
    parser.add_argument(
        "--check",
        action="store_true",
        help="report drift against the committed output and exit 1; write nothing",
    )
    parser.add_argument(
        "--root",
        default=str(ROOT),
        metavar="PATH",
        help="repository root (default: the repo this script lives in)",
    )
    args = parser.parse_args(argv)

    root = Path(args.root).resolve()
    try:
        matrix = load_matrix(root)
    except MatrixError as err:
        print(f"error: {err}", file=sys.stderr)
        return 2

    known = {h.name: h for h in matrix.harnesses}
    if args.target == "all":
        targets = list(matrix.harnesses)
    elif args.target in known:
        targets = [known[args.target]]
    else:
        print(
            f"error: unknown target {args.target!r} — the matrix declares: "
            f"{', '.join(known)} (or 'all')",
            file=sys.stderr,
        )
        return 2

    # Canonical ownership. Any target with a registered adapter is compiled from
    # canonical/ by tools/export.py, not from skills/ + agents/ by the legacy
    # transformer below. Targets canonical/manifest.json has *enabled* are
    # delegated; a registered-but-not-yet-enabled one is skipped outright rather
    # than built here, because a second, differently-shaped tree under the same
    # harnesses/<target>/ path would put this generator's --check permanently at
    # odds with the canonical export's.
    from tools import export as canonical_export
    from tools.adapters.registry import ADAPTERS

    try:
        enabled = frozenset(canonical_export.load_manifest(root)["targets"])
    except canonical_export.ExportError as err:
        print(f"error: {err}", file=sys.stderr)
        return 2
    unknown = sorted(enabled - set(known))
    if unknown:
        print(
            f"error: canonical/manifest.json enables target(s) absent from "
            f"{MATRIX_REL}: {', '.join(unknown)} — add them to the matrix",
            file=sys.stderr,
        )
        return 2

    canonical_owned = frozenset(ADAPTERS) | enabled
    for harness in targets:
        if harness.name not in enabled:
            continue
        canonical_rc = canonical_export.run(root, harness.name, args.check)
        if canonical_rc:
            return canonical_rc
    for harness in targets:
        if harness.name in canonical_owned and harness.name not in enabled:
            print(
                f"skipped: harnesses/{harness.name}/ — a canonical adapter is "
                "registered but canonical/manifest.json has not enabled the target"
            )
    targets = [target for target in targets if target.name not in canonical_owned]

    try:
        skills, agents = load_sources(root)
    except SourceError as err:
        print(f"error: {err}", file=sys.stderr)
        return 1

    failures = 0
    for harness in targets:
        try:
            result = build_target(matrix, harness, skills, agents)
        except SourceError as err:
            print(f"error: {err}", file=sys.stderr)
            failures += 1
            continue
        emit_jsonl(result.infos)
        emit_jsonl(result.warnings)
        if result.refusals:
            emit_jsonl(result.refusals)
            if args.check:
                # Refusal is expected for harnesses the matrix gates — no tree
                # was committed, so there is nothing to check. Report but don't
                # fail.
                print(
                    f"refused (expected): harnesses/{harness.name}/ — "
                    f"{len(result.refusals)} safety polic(ies) cannot be honoured"
                )
            else:
                print(
                    f"harnesses/{harness.name}/: refused — {len(result.refusals)} "
                    "safety polic(ies) cannot be honoured; nothing written",
                    file=sys.stderr,
                )
                failures += 1
            continue
        hint = (
            f"run: python3 tools/build_harness_payloads.py --target {harness.name} "
            "&& git add harnesses/"
        )
        if args.check:
            report = check_all(result.files, root) + [
                f"unexpected generated file: {path} "
                "(renamed or removed a source? delete it and rerun)"
                for path in find_orphans(root, harness.name, expected_paths(result))
            ]
            if report:
                for line in report:
                    print(line)
                print(hint)
                failures += 1
            else:
                print(
                    f"up to date: harnesses/{harness.name}/ — "
                    f"{len(result.files)} generated files"
                )
            continue
        written = write_all(result.files, root)
        for path in find_orphans(root, harness.name, expected_paths(result)):
            print(
                f"warning: unexpected generated file: {path} "
                "(renamed or removed a source?)"
            )
        for path in written:
            print(f"wrote {path}")
        print(
            f"harnesses/{harness.name}/: {len(written)} of {len(result.files)} "
            "files updated"
        )

    if args.target == "all" and args.check:
        for path in find_orphan_targets(root, frozenset(known)):
            print(f"unexpected generated tree: {path} (not in the matrix; delete it)")
            failures += 1

    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
