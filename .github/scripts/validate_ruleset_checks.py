#!/usr/bin/env python3
"""Assert ruleset required checks match pull_request job names.

Stdlib only. GitHub matches a required status check on `jobs.<id>.name` (or
the job id when `name:` is omitted). A committed ruleset that names a check
the workflows no longer emit is a silent merge-gate hole; this script fails
CI with both the ruleset file:line and the workflow file:line / job.
"""

from __future__ import annotations

import json
import re
import sys
from dataclasses import dataclass
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
RULESET_REL = Path(".github/rulesets/main.json")
WORKFLOWS_REL = Path(".github/workflows")

CONTEXT_RE = re.compile(r'"context"\s*:\s*"((?:\\.|[^"\\])*)"')
YAML_KEY_RE = re.compile(r"^([A-Za-z0-9_-]+)\s*:(?:\s*(.*))?$")
BLOCK_SCALAR_RE = re.compile(r"[|>][+-]?\s*$")


@dataclass(frozen=True)
class Context:
    value: str
    path: Path
    line: int


@dataclass(frozen=True)
class JobCheck:
    check_name: str
    job_id: str
    path: Path
    line: int


def _rel(path: Path, root: Path) -> str:
    try:
        return path.resolve().relative_to(root.resolve()).as_posix()
    except ValueError:
        return str(path)


def _strip_yaml_comment(line: str) -> str:
    in_single = False
    in_double = False
    escaped = False
    out: list[str] = []
    for ch in line:
        if escaped:
            out.append(ch)
            escaped = False
            continue
        if ch == "\\" and in_double:
            out.append(ch)
            escaped = True
            continue
        if ch == "'" and not in_double:
            in_single = not in_single
        elif ch == '"' and not in_single:
            in_double = not in_double
        elif ch == "#" and not in_single and not in_double:
            break
        out.append(ch)
    return "".join(out).rstrip()


def _unquote(value: str) -> str:
    value = value.strip()
    if len(value) >= 2 and value[0] == value[-1] and value[0] in "'\"":
        return value[1:-1]
    return value


def _flow_has_pull_request(value: str) -> bool:
    value = value.strip()
    if not value:
        return False
    if value.startswith("[") and value.endswith("]"):
        items = [_unquote(part) for part in value[1:-1].split(",")]
        return "pull_request" in items
    return _unquote(value) == "pull_request"


def _parse_contexts(ruleset: Path) -> tuple[list[Context], list[str]]:
    """Return (contexts with file:line, errors)."""
    errors: list[str] = []
    text = ruleset.read_text(encoding="utf-8")
    try:
        data = json.loads(text)
    except json.JSONDecodeError as exc:
        return [], [f"{ruleset}: invalid JSON: {exc}"]

    expected: list[str] = []
    for rule in data.get("rules") or []:
        if not isinstance(rule, dict):
            continue
        if rule.get("type") != "required_status_checks":
            continue
        params = rule.get("parameters") or {}
        for item in params.get("required_status_checks") or []:
            if isinstance(item, dict) and item.get("context"):
                expected.append(str(item["context"]))

    lined: list[Context] = []
    for n, line in enumerate(text.splitlines(), 1):
        for match in CONTEXT_RE.finditer(line):
            lined.append(Context(match.group(1), ruleset, n))

    if [ctx.value for ctx in lined] != expected:
        errors.append(
            f"{ruleset}: 'context' line scan {[c.value for c in lined]!r} "
            f"does not match required_status_checks {expected!r}"
        )
        return lined, errors
    if not lined:
        errors.append(f"{ruleset}: no required_status_checks contexts found")
    return lined, errors


def parse_workflow_pr_jobs(path: Path) -> tuple[bool, list[JobCheck]]:
    """Subset YAML parser for GHA `on:` + `jobs:` in this repo's workflow files."""
    has_pr = False
    jobs: list[JobCheck] = []
    section: str | None = None
    current_id: str | None = None
    current_id_line = 0
    current_indent = 0
    current_name: str | None = None
    current_name_line = 0
    in_block = False
    block_indent = 0

    def flush() -> None:
        nonlocal current_id, current_name
        if current_id is None:
            return
        if current_name:
            jobs.append(JobCheck(current_name, current_id, path, current_name_line))
        else:
            jobs.append(JobCheck(current_id, current_id, path, current_id_line))
        current_id = None
        current_name = None

    for n, raw in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        if in_block:
            if raw.strip():
                indent = len(raw) - len(raw.lstrip(" "))
                if indent <= block_indent:
                    in_block = False
                else:
                    continue
            else:
                continue

        stripped = _strip_yaml_comment(raw)
        if not stripped.strip():
            continue
        indent = len(stripped) - len(stripped.lstrip(" "))
        content = stripped.strip()
        if BLOCK_SCALAR_RE.search(content):
            in_block = True
            block_indent = indent

        if indent == 0:
            match = YAML_KEY_RE.match(content)
            if match:
                flush()
                key, rest = match.group(1), (match.group(2) or "").strip()
                if key == "on":
                    section = "on"
                    if rest and _flow_has_pull_request(rest):
                        has_pr = True
                elif key == "jobs":
                    section = "jobs"
                else:
                    section = None
                continue

        if section == "on" and indent > 0:
            match = YAML_KEY_RE.match(content)
            if match and match.group(1) == "pull_request":
                has_pr = True
            elif _flow_has_pull_request(content):
                has_pr = True
            continue

        if section != "jobs":
            continue

        if indent == 2 and not content.startswith("-"):
            match = YAML_KEY_RE.match(content)
            if match:
                flush()
                current_id = match.group(1)
                current_id_line = n
                current_indent = indent
                current_name = None
                current_name_line = 0
            continue

        if current_id is not None and indent == current_indent + 2:
            match = YAML_KEY_RE.match(content)
            if match and match.group(1) == "name":
                current_name = _unquote((match.group(2) or "").strip())
                current_name_line = n

    flush()
    return has_pr, jobs


def _collect_pr_jobs(workflows_dir: Path) -> list[JobCheck]:
    found: list[JobCheck] = []
    files = sorted(workflows_dir.glob("*.yml")) + sorted(workflows_dir.glob("*.yaml"))
    for path in files:
        has_pr, jobs = parse_workflow_pr_jobs(path)
        if has_pr:
            found.extend(jobs)
    return found


def _format_jobs(jobs: list[JobCheck], root: Path) -> str:
    if not jobs:
        return "  (none)"
    lines = []
    for job in jobs:
        lines.append(
            f"  {_rel(job.path, root)}:{job.line}: job {job.job_id} -> {job.check_name!r}"
        )
    return "\n".join(lines)


def validate(repo_root: Path | None = None) -> list[str]:
    root = (repo_root or ROOT).resolve()
    ruleset = root / RULESET_REL
    workflows_dir = root / WORKFLOWS_REL
    errors: list[str] = []

    if not ruleset.is_file():
        return [f"{RULESET_REL.as_posix()}: file not found under {root}"]
    if not workflows_dir.is_dir():
        return [f"{WORKFLOWS_REL.as_posix()}: directory not found under {root}"]

    contexts, ctx_errors = _parse_contexts(ruleset)
    errors.extend(
        e.replace(str(ruleset), _rel(ruleset, root), 1) if str(ruleset) in e else e
        for e in ctx_errors
    )
    pr_jobs = _collect_pr_jobs(workflows_dir)
    names = {job.check_name for job in pr_jobs}
    job_list = _format_jobs(pr_jobs, root)
    ruleset_disp = _rel(ruleset, root)

    for ctx in contexts:
        if ctx.value in names:
            continue
        errors.append(
            f"{ruleset_disp}:{ctx.line}: required status check {ctx.value!r} is not a "
            f"jobs.*.name (or job id, when name is omitted) in any workflow whose on: "
            f"includes pull_request\n"
            f"pull_request jobs:\n{job_list}"
        )
    return errors


def main(argv: list[str] | None = None) -> int:
    del argv
    errors = validate(ROOT)
    if errors:
        for err in errors:
            print(f"error: {err}", file=sys.stderr)
        return 1
    print("ok: required status checks match pull_request job names")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
