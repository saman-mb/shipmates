#!/usr/bin/env python3
"""Assert no workflow step above `actions/checkout` reads a path in the repo.

An `actions/checkout` step materialises the repository. Before it runs, the
runner's workspace is empty, so a step listed above it can only call the shell,
an installer, or an `${{ }}` expression — never a script or a file from the
tree. Cargo-dist generates the one legitimate exception, the Windows longpaths
git config, and it inlines the command rather than calling a script precisely
because that file would not exist yet.

That exception is why this gate exists. Extracting the step's body into
`.github/scripts/` looks like the house style everywhere else in a workflow,
and it works in every job that checks out first — but here it dies with exit
127 on a clean runner, and on the release path the failure is invisible from a
pull request because the release jobs are skipped there. It only shows up on
the push that was supposed to publish, which is a whole release too late.

The rule: in any job that has a checkout, a step above the checkout must not
name a path in this repository, and must not use a local action. Escape hatch
for a step that genuinely has to run first: keep its command self-contained, or
mark it with a `# pre-checkout-ok` comment and say why.

Stdlib only. Exposes validate(root) -> list[str] for the regression tests.
"""

from __future__ import annotations

import re
import sys
from dataclasses import dataclass
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
WORKFLOWS_REL = Path(".github/workflows")
CHECKOUT_PREFIX = "actions/checkout@"
ALLOW_MARKER = "pre-checkout-ok"

# A job key sits at two spaces inside the top-level `jobs:` block.
JOB_RE = re.compile(r"^  ([A-Za-z0-9_.-]+):\s*$")
STEPS_RE = re.compile(r"^    steps:\s*$")
# A step entry opens with `- ` at six spaces, and may carry its first key inline.
STEP_RE = re.compile(r"^ {6}- ")
STEP_KEY_RE = re.compile(r"^ {6}- ([A-Za-z0-9_-]+):\s?(.*)$")
KEY_RE = re.compile(r"^ {8}([A-Za-z0-9_-]+):\s?(.*)$")
BLOCK_SCALAR_CHARS = {"", "|", "|-", "|+", ">", ">-", ">+"}
TOKEN_SPLIT_RE = re.compile(r"[\s\"'`]+")
# A relative-looking path token: at least one slash, no scheme, no shell prefix.
PATH_TOKEN_RE = re.compile(r"^(?:\./)?[A-Za-z0-9_.-][A-Za-z0-9_.*/-]*$")
TOKEN_TRIM = "()[]{},;:'\""


@dataclass(frozen=True)
class Step:
    """One `steps:` entry, flattened to the keys this gate cares about."""

    line: int  # 1-based line of the `- ` marker
    name: str
    uses: str
    run: str
    text: str  # whole entry, for the escape-hatch marker


def _indent(line: str) -> int:
    return len(line) - len(line.lstrip(" "))


def _job_blocks(lines: list[str]) -> list[tuple[str, int, int]]:
    """Return (job_id, start, end) line indexes for every job in the file."""
    headers: list[tuple[int, str]] = []
    in_jobs = False
    for i, line in enumerate(lines):
        stripped = line.strip()
        if _indent(line) == 0 and stripped and not stripped.startswith("#"):
            in_jobs = stripped == "jobs:"
            continue
        if in_jobs:
            match = JOB_RE.match(line)
            if match:
                headers.append((i, match.group(1)))
    blocks: list[tuple[str, int, int]] = []
    for idx, (start, job_id) in enumerate(headers):
        end = headers[idx + 1][0] if idx + 1 < len(headers) else len(lines)
        blocks.append((job_id, start, end))
    return blocks


def _step_entries(lines: list[str], start: int, end: int) -> list[tuple[int, list[str]]]:
    """Return (line index, entry lines) for each step in a job block.

    An entry keeps the comment block directly above its `- ` marker, so the
    escape-hatch marker reads the way a person writes it.
    """
    steps_line = next((i for i in range(start, end) if STEPS_RE.match(lines[i])), None)
    if steps_line is None:
        return []
    starts = [i for i in range(steps_line + 1, end) if STEP_RE.match(lines[i])]
    bounds: list[int] = []
    for step_start in starts:
        begin = step_start
        while (
            begin - 1 > steps_line
            and _indent(lines[begin - 1]) == 6
            and lines[begin - 1].lstrip().startswith("#")
        ):
            begin -= 1
        bounds.append(begin)
    entries: list[tuple[int, list[str]]] = []
    for idx, step_start in enumerate(starts):
        step_end = bounds[idx + 1] if idx + 1 < len(bounds) else end
        entries.append((step_start, lines[bounds[idx] : step_end]))
    return entries


def _scalar(entry: list[str], index: int, value: str) -> str:
    """Return an inline scalar, or a block scalar's indented body."""
    if value not in BLOCK_SCALAR_CHARS:
        return value
    body: list[str] = []
    for line in entry[index + 1 :]:
        if line.strip() and _indent(line) <= 8:
            break
        body.append(line.strip())
    return "\n".join(body).strip()


def _parse_step(entry: list[str], line_index: int) -> Step:
    dash = next((i for i, line in enumerate(entry) if STEP_RE.match(line)), 0)
    name = uses = run = ""
    head = STEP_KEY_RE.match(entry[dash])
    if head:
        key, value = head.group(1), head.group(2).strip()
        if key == "name":
            name = value
        elif key == "uses":
            uses = value
        elif key == "run":
            run = _scalar(entry, dash, value)
    for offset in range(dash + 1, len(entry)):
        match = KEY_RE.match(entry[offset])
        if not match:
            continue
        line = entry[offset]
        key, value = match.group(1), match.group(2).strip()
        if key == "name" and not name:
            name = value
        elif key == "uses" and not uses:
            uses = value
        elif key == "run" and not run:
            run = _scalar(entry, offset, value)
    return Step(
        line=line_index + 1,
        name=name,
        uses=uses,
        run=run,
        text="\n".join(entry),
    )


def _repo_paths(command: str, root: Path, top_level: frozenset[str]) -> list[str]:
    """Path tokens in a command that name something in this repository."""
    hits: list[str] = []
    for raw in TOKEN_SPLIT_RE.split(command):
        token = raw.strip(TOKEN_TRIM)
        if not token or token[0] in "-/~$=":
            continue
        if "://" in token or token.startswith("${{"):
            continue
        if token.startswith("./"):
            hits.append(token)
            continue
        if "/" not in token or not PATH_TOKEN_RE.match(token):
            continue
        first = token.split("/", 1)[0].rstrip("*")
        if first in top_level or (root / token).exists():
            hits.append(token)
    return hits


def validate(root: Path | None = None) -> list[str]:
    repo_root = (root or ROOT).resolve()
    workflows_dir = repo_root / WORKFLOWS_REL
    if not workflows_dir.is_dir():
        return [f"{WORKFLOWS_REL.as_posix()}: directory not found under {repo_root}"]

    top_level = frozenset(path.name for path in repo_root.iterdir())
    errors: list[str] = []

    for path in sorted(workflows_dir.glob("*.y*ml")):
        text = path.read_text(encoding="utf-8")
        lines = text.splitlines()
        display = path.relative_to(repo_root).as_posix()
        for job_id, job_start, job_end in _job_blocks(lines):
            entries = _step_entries(lines, job_start, job_end)
            parsed = [_parse_step(entry, index) for index, entry in entries]
            checkout_at = next(
                (i for i, step in enumerate(parsed) if step.uses.startswith(CHECKOUT_PREFIX)),
                None,
            )
            if checkout_at is None:
                continue
            for step in parsed[:checkout_at]:
                if ALLOW_MARKER in step.text:
                    continue
                if step.uses.startswith("./") or step.uses.startswith("../"):
                    errors.append(
                        f"{display}:{step.line}: step {step.name or step.uses!r} in job "
                        f"{job_id!r} uses a local action before actions/checkout, which "
                        f"cannot exist on the runner yet"
                    )
                    continue
                hits = sorted(set(_repo_paths(step.run, repo_root, top_level)))
                if hits:
                    errors.append(
                        f"{display}:{step.line}: step {step.name or '(unnamed)'!r} in job "
                        f"{job_id!r} runs before actions/checkout but reads "
                        f"{', '.join(hits)} — the workspace is empty until checkout runs. "
                        f"Inline the command, or mark the step `# {ALLOW_MARKER}` and say why."
                    )
    return errors


def main(argv: list[str] | None = None) -> int:
    del argv
    errors = validate(ROOT)
    if errors:
        for err in errors:
            print(f"error: {err}", file=sys.stderr)
        return 1
    print("ok: no pre-checkout workflow step reads the repository")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
