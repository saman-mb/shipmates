#!/usr/bin/env python3
"""gh — structured GitHub CLI wrapper for shipmates workflows.

Requires the GitHub CLI (``gh``) installed and authenticated. Reads a JSON spec
from stdin or ``--spec``, prints JSON result on stdout, diagnostics on stderr.

Self-contained aside from the external ``gh`` binary — stdlib only in Python.

Exit codes: 0 ok; 2 validation/usage; 3 gh failure.
"""
from __future__ import annotations

import json
import re
import shutil
import subprocess
import sys
import time
from pathlib import Path
from typing import Any

REPO_RE = re.compile(r"^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$")
NUMBER_RE = re.compile(r"^[0-9]+$")
MAX_INLINE_BODY = 200


class GhError(Exception):
    """Validation or operational failure surfaced to the user."""


def validate_repo(repo: str) -> str:
    if not REPO_RE.match(repo):
        raise GhError(f"invalid repo slug {repo!r} — want owner/name")
    return repo


def validate_number(value: Any, label: str = "number") -> int:
    if isinstance(value, bool) or not isinstance(value, int):
        if isinstance(value, str) and NUMBER_RE.match(value):
            return int(value)
        raise GhError(f"{label} must be a positive integer, got {value!r}")
    if value < 1:
        raise GhError(f"{label} must be >= 1, got {value}")
    return value


def read_body_file(spec: dict[str, Any], required: bool = True) -> str | None:
    path = spec.get("body_file")
    if path is None:
        inline = spec.get("body")
        if inline is not None:
            if not isinstance(inline, str):
                raise GhError("body must be a string when provided")
            if len(inline) > MAX_INLINE_BODY:
                raise GhError(
                    f"inline body exceeds {MAX_INLINE_BODY} chars — use body_file"
                )
            return inline
        if required:
            raise GhError("body_file is required for this operation")
        return None
    if not isinstance(path, str):
        raise GhError("body_file must be a string path")
    try:
        return Path(path).read_text(encoding="utf-8")
    except OSError as exc:
        raise GhError(f"could not read body_file {path!r}: {exc}") from exc


def repo_flag(spec: dict[str, Any]) -> list[str]:
    repo = spec.get("repo")
    if repo is None:
        return []
    return ["--repo", validate_repo(str(repo))]


def limit(spec: dict[str, Any], default: int = 100) -> int:
    raw = spec.get("limit", default)
    if isinstance(raw, bool) or not isinstance(raw, int):
        raise GhError(f"limit must be an integer, got {raw!r}")
    if raw < 1:
        raise GhError("limit must be >= 1")
    return min(raw, 1000)


def fields(spec: dict[str, Any], default: list[str]) -> list[str]:
    raw = spec.get("fields")
    if raw is None:
        return default
    if not isinstance(raw, list) or not all(isinstance(x, str) for x in raw):
        raise GhError("fields must be a list of strings")
    return raw


def run_gh(args: list[str], *, input_text: str | None = None) -> subprocess.CompletedProcess[str]:
    if shutil.which("gh") is None:
        raise GhError("gh not found on PATH — install GitHub CLI and run gh auth login")
    try:
        return subprocess.run(
            ["gh", *args],
            input=input_text,
            capture_output=True,
            text=True,
            check=False,
        )
    except OSError as exc:
        raise GhError(f"failed to spawn gh: {exc}") from exc


def gh_json(args: list[str]) -> Any:
    proc = run_gh(args)
    if proc.returncode != 0:
        msg = (proc.stderr or proc.stdout or "").strip() or f"gh exited {proc.returncode}"
        raise GhError(msg)
    text = proc.stdout.strip()
    if not text:
        return None
    try:
        return json.loads(text)
    except json.JSONDecodeError as exc:
        raise GhError(f"gh returned non-JSON output: {exc}") from exc


def gh_text(args: list[str], *, input_text: str | None = None) -> str:
    proc = run_gh(args, input_text=input_text)
    if proc.returncode != 0:
        msg = (proc.stderr or proc.stdout or "").strip() or f"gh exited {proc.returncode}"
        raise GhError(msg)
    return proc.stdout


def summarize_checks(data: Any) -> dict[str, Any]:
    if not isinstance(data, list):
        return {"checks": data, "rollup": "unknown", "pending": 0, "fail": 0, "pass": 0}
    pending = fail = passed = 0
    for row in data:
        if not isinstance(row, dict):
            continue
        state = str(row.get("state", "")).lower()
        bucket = str(row.get("bucket", "")).lower()
        if state in {"pending", "queued", "in_progress"} or bucket == "pending":
            pending += 1
        elif state in {"failure", "failed", "error", "cancelled", "timed_out"} or bucket == "fail":
            fail += 1
        else:
            passed += 1
    if pending:
        rollup = "pending"
    elif fail:
        rollup = "fail"
    else:
        rollup = "pass"
    return {
        "checks": data,
        "rollup": rollup,
        "pending": pending,
        "fail": fail,
        "pass": passed,
    }


def op_auth_status(_spec: dict[str, Any]) -> dict[str, Any]:
    proc = run_gh(["auth", "status"])
    text = (proc.stdout or "") + (proc.stderr or "")
    return {
        "ok": proc.returncode == 0,
        "authenticated": proc.returncode == 0,
        "status": text.strip(),
    }


def op_repo_view(spec: dict[str, Any]) -> dict[str, Any]:
    data = gh_json(
        [
            "repo",
            "view",
            *repo_flag(spec),
            "--json",
            "nameWithOwner,defaultBranchRef,url",
        ]
    )
    ref = (data or {}).get("defaultBranchRef") or {}
    return {
        "nameWithOwner": (data or {}).get("nameWithOwner"),
        "defaultBranch": ref.get("name"),
        "url": (data or {}).get("url"),
        "raw": data,
    }


def op_issue_view(spec: dict[str, Any]) -> dict[str, Any]:
    number = validate_number(spec.get("number"))
    flds = fields(
        spec,
        ["number", "title", "body", "labels", "state", "url", "author", "createdAt"],
    )
    data = gh_json(
        ["issue", "view", str(number), *repo_flag(spec), "--json", ",".join(flds)]
    )
    return {"issue": data}


def op_issue_list(spec: dict[str, Any]) -> dict[str, Any]:
    flds = fields(spec, ["number", "title", "labels", "state", "url"])
    args = [
        "issue",
        "list",
        *repo_flag(spec),
        "--limit",
        str(limit(spec)),
        "--json",
        ",".join(flds),
    ]
    state = spec.get("state")
    if state:
        args.extend(["--state", str(state)])
    labels = spec.get("labels")
    if labels:
        if not isinstance(labels, list) or not all(isinstance(x, str) for x in labels):
            raise GhError("labels must be a list of strings")
        for label in labels:
            args.extend(["--label", label])
    data = gh_json(args)
    return {"issues": data}


def op_issue_search(spec: dict[str, Any]) -> dict[str, Any]:
    query = spec.get("query")
    if not isinstance(query, str) or not query.strip():
        raise GhError("query is required for issue.search")
    flds = fields(spec, ["number", "title", "state", "url"])
    data = gh_json(
        [
            "search",
            "issues",
            query.strip(),
            *repo_flag(spec),
            "--limit",
            str(limit(spec, 20)),
            "--json",
            ",".join(flds),
        ]
    )
    return {"issues": data}


def op_issue_create(spec: dict[str, Any]) -> dict[str, Any]:
    title = spec.get("title")
    if not isinstance(title, str) or not title.strip():
        raise GhError("title is required for issue.create")
    body = read_body_file(spec)
    args = [
        "issue",
        "create",
        *repo_flag(spec),
        "--title",
        title.strip(),
        "--body-file",
        "-",
    ]
    labels = spec.get("labels")
    if labels:
        if not isinstance(labels, list) or not all(isinstance(x, str) for x in labels):
            raise GhError("labels must be a list of strings")
        for label in labels:
            args.extend(["--label", label])
    url = gh_text(args, input_text=body)
    return {"url": url.strip(), "title": title.strip()}


def op_issue_edit(spec: dict[str, Any]) -> dict[str, Any]:
    number = validate_number(spec.get("number"))
    body = read_body_file(spec)
    args = [
        "issue",
        "edit",
        str(number),
        *repo_flag(spec),
        "--body-file",
        "-",
    ]
    gh_text(args, input_text=body)
    return {"number": number, "edited": True}


def op_issue_comment(spec: dict[str, Any]) -> dict[str, Any]:
    number = validate_number(spec.get("number"))
    body = read_body_file(spec)
    args = [
        "issue",
        "comment",
        str(number),
        *repo_flag(spec),
        "--body-file",
        "-",
    ]
    url = gh_text(args, input_text=body)
    return {"number": number, "url": url.strip()}


def op_issue_close(spec: dict[str, Any]) -> dict[str, Any]:
    number = validate_number(spec.get("number"))
    gh_text(["issue", "close", str(number), *repo_flag(spec)])
    return {"number": number, "closed": True}


SUB_ISSUE_FIELDS = "number,title,state,subIssues,subIssuesSummary"


def sub_issue_pair(spec: dict[str, Any]) -> tuple[int, int]:
    parent = validate_number(spec.get("number"))
    child = validate_number(spec.get("sub_issue_number"), "sub_issue_number")
    if parent == child:
        raise GhError(f"issue #{parent} cannot be its own sub-issue")
    return parent, child


def sub_issue_children(sub_issues: Any) -> list[dict[str, Any]]:
    """Unwrap gh's `subIssues` JSON: a connection `{nodes, totalCount}`, not a list.

    `gh issue view --json subIssues` emits GraphQL connection shape. Iterating the
    object yields its keys and would silently report zero children.
    """
    if isinstance(sub_issues, dict):
        nodes = sub_issues.get("nodes")
        if isinstance(nodes, list):
            return [node for node in nodes if isinstance(node, dict)]
        return []
    if isinstance(sub_issues, list):
        return [node for node in sub_issues if isinstance(node, dict)]
    return []


def op_issue_sub_issue_list(spec: dict[str, Any]) -> dict[str, Any]:
    number = validate_number(spec.get("number"))
    data = (
        gh_json(
            ["issue", "view", str(number), *repo_flag(spec), "--json", SUB_ISSUE_FIELDS]
        )
        or {}
    )
    children = sub_issue_children(data.get("subIssues"))
    numbers = [
        child["number"]
        for child in children
        if isinstance(child.get("number"), int)
    ]
    return {
        "number": number,
        "title": data.get("title"),
        "state": data.get("state"),
        "subIssues": children,
        "subIssuesSummary": data.get("subIssuesSummary"),
        "numbers": numbers,
    }


def op_issue_sub_issue_add(spec: dict[str, Any]) -> dict[str, Any]:
    parent, child = sub_issue_pair(spec)
    replace_parent = spec.get("replace_parent", False)
    if not isinstance(replace_parent, bool):
        raise GhError("replace_parent must be a boolean")
    existing = op_issue_sub_issue_list({"number": parent, "repo": spec.get("repo")})
    if child in existing["numbers"]:
        return {
            "number": parent,
            "sub_issue_number": child,
            "attached": False,
            "reason": "already-child",
            "subIssuesSummary": existing["subIssuesSummary"],
        }
    if replace_parent:
        # Reassign the child away from whatever parent it already has: the
        # parent-side --add-sub-issue refuses a child owned elsewhere.
        gh_text(["issue", "edit", str(child), *repo_flag(spec), "--parent", str(parent)])
        mode = "parent"
    else:
        gh_text(
            [
                "issue",
                "edit",
                str(parent),
                *repo_flag(spec),
                "--add-sub-issue",
                str(child),
            ]
        )
        mode = "add-sub-issue"
    return {
        "number": parent,
        "sub_issue_number": child,
        "attached": True,
        "mode": mode,
    }


def op_issue_sub_issue_remove(spec: dict[str, Any]) -> dict[str, Any]:
    parent, child = sub_issue_pair(spec)
    gh_text(
        [
            "issue",
            "edit",
            str(parent),
            *repo_flag(spec),
            "--remove-sub-issue",
            str(child),
        ]
    )
    return {"number": parent, "sub_issue_number": child, "removed": True}


def op_pr_view(spec: dict[str, Any]) -> dict[str, Any]:
    number = validate_number(spec.get("number"))
    flds = fields(
        spec,
        [
            "number",
            "title",
            "body",
            "url",
            "headRefOid",
            "headRefName",
            "baseRefName",
            "state",
            "isCrossRepository",
            "author",
            "files",
        ],
    )
    data = gh_json(
        ["pr", "view", str(number), *repo_flag(spec), "--json", ",".join(flds)]
    )
    return {"pullRequest": data}


def op_pr_view_current(spec: dict[str, Any]) -> dict[str, Any]:
    flds = fields(spec, ["number", "title", "url", "headRefOid", "headRefName", "state"])
    data = gh_json(["pr", "view", *repo_flag(spec), "--json", ",".join(flds)])
    return {"pullRequest": data}


def op_pr_diff(spec: dict[str, Any]) -> dict[str, Any]:
    number = validate_number(spec.get("number"))
    diff = gh_text(["pr", "diff", str(number), *repo_flag(spec)])
    return {"number": number, "diff": diff}


def op_pr_create(spec: dict[str, Any]) -> dict[str, Any]:
    title = spec.get("title")
    if not isinstance(title, str) or not title.strip():
        raise GhError("title is required for pr.create")
    base = spec.get("base")
    head = spec.get("head")
    if not isinstance(base, str) or not base.strip():
        raise GhError("base branch is required for pr.create")
    if not isinstance(head, str) or not head.strip():
        raise GhError("head branch is required for pr.create")
    body = read_body_file(spec, required=False)
    args = [
        "pr",
        "create",
        *repo_flag(spec),
        "--base",
        base.strip(),
        "--head",
        head.strip(),
        "--title",
        title.strip(),
    ]
    if body is not None:
        args.extend(["--body-file", "-"])
        url = gh_text(args, input_text=body)
    else:
        url = gh_text(args)
    return {"url": url.strip(), "title": title.strip()}


def op_pr_checks(spec: dict[str, Any]) -> dict[str, Any]:
    number = validate_number(spec.get("number"))
    text = gh_text(["pr", "checks", str(number), *repo_flag(spec)])
    lines = [ln for ln in text.splitlines() if ln.strip()]
    rows = []
    for line in lines:
        parts = line.split("\t")
        if len(parts) >= 2:
            rows.append({"name": parts[0], "state": parts[1]})
        else:
            rows.append({"raw": line})
    summary = summarize_checks(rows)
    summary["text"] = text.strip()
    summary["number"] = number
    return summary


def op_pr_checks_poll(spec: dict[str, Any]) -> dict[str, Any]:
    interval = spec.get("interval_secs", 15)
    timeout = spec.get("timeout_secs", 3600)
    if isinstance(interval, bool) or not isinstance(interval, int) or interval < 1:
        raise GhError("interval_secs must be an integer >= 1")
    if isinstance(timeout, bool) or not isinstance(timeout, int) or timeout < 1:
        raise GhError("timeout_secs must be an integer >= 1")
    deadline = time.time() + timeout
    attempts = 0
    last: dict[str, Any] = {}
    while time.time() < deadline:
        attempts += 1
        last = op_pr_checks(spec)
        if last.get("rollup") != "pending":
            last["attempts"] = attempts
            last["timed_out"] = False
            return last
        time.sleep(interval)
    last["attempts"] = attempts
    last["timed_out"] = True
    return last


def op_pr_comment(spec: dict[str, Any]) -> dict[str, Any]:
    number = validate_number(spec.get("number"))
    body = read_body_file(spec)
    args = [
        "pr",
        "comment",
        str(number),
        *repo_flag(spec),
        "--body-file",
        "-",
    ]
    url = gh_text(args, input_text=body)
    return {"number": number, "url": url.strip()}


def op_pr_review(spec: dict[str, Any]) -> dict[str, Any]:
    number = validate_number(spec.get("number"))
    event = spec.get("event", "comment")
    if event not in {"approve", "request-changes", "comment"}:
        raise GhError("event must be approve, request-changes, or comment")
    body = read_body_file(spec)
    flag = {
        "approve": "--approve",
        "request-changes": "--request-changes",
        "comment": "--comment",
    }[event]
    args = [
        "pr",
        "review",
        str(number),
        *repo_flag(spec),
        flag,
        "--body-file",
        "-",
    ]
    gh_text(args, input_text=body)
    return {"number": number, "event": event, "submitted": True}


def op_pr_merge(spec: dict[str, Any]) -> dict[str, Any]:
    number = validate_number(spec.get("number"))
    squash = spec.get("squash", True)
    delete_branch = spec.get("delete_branch", True)
    args = ["pr", "merge", str(number), *repo_flag(spec)]
    if squash:
        args.append("--squash")
    if delete_branch:
        args.append("--delete-branch")
    head_sha = spec.get("head_sha")
    if head_sha is not None:
        if not isinstance(head_sha, str) or not head_sha.strip():
            raise GhError("head_sha must be a non-empty string")
        args.extend(["--match-head-commit", head_sha.strip()])
    url = gh_text(args)
    return {"number": number, "merged": True, "url": url.strip()}


def op_pr_list(spec: dict[str, Any]) -> dict[str, Any]:
    flds = fields(spec, ["number", "title", "url", "headRefName", "baseRefName", "state"])
    args = [
        "pr",
        "list",
        *repo_flag(spec),
        "--limit",
        str(limit(spec)),
        "--json",
        ",".join(flds),
    ]
    state = spec.get("state")
    if state:
        args.extend(["--state", str(state)])
    base = spec.get("base")
    if base:
        args.extend(["--base", str(base)])
    head = spec.get("head")
    if head:
        args.extend(["--head", str(head)])
    data = gh_json(args)
    return {"pullRequests": data}


def op_label_list(spec: dict[str, Any]) -> dict[str, Any]:
    data = gh_json(["label", "list", *repo_flag(spec), "--json", "name,color,description"])
    return {"labels": data}


def op_label_create(spec: dict[str, Any]) -> dict[str, Any]:
    name = spec.get("name")
    if not isinstance(name, str) or not name.strip():
        raise GhError("name is required for label.create")
    args = ["label", "create", name.strip(), *repo_flag(spec)]
    color = spec.get("color")
    if color:
        args.extend(["--color", str(color)])
    description = spec.get("description")
    if description:
        args.extend(["--description", str(description)])
    gh_text(args)
    return {"name": name.strip(), "created": True}


def op_release_list(spec: dict[str, Any]) -> dict[str, Any]:
    data = gh_json(
        [
            "release",
            "list",
            *repo_flag(spec),
            "--limit",
            str(limit(spec, 20)),
            "--json",
            "tagName,name,publishedAt,url",
        ]
    )
    return {"releases": data}


def op_release_create(spec: dict[str, Any]) -> dict[str, Any]:
    tag = spec.get("tag")
    if not isinstance(tag, str) or not tag.strip():
        raise GhError("tag is required for release.create")
    notes_path = spec.get("notes_file")
    args = ["release", "create", tag.strip(), *repo_flag(spec)]
    if notes_path:
        if not isinstance(notes_path, str):
            raise GhError("notes_file must be a string path")
        try:
            notes = Path(notes_path).read_text(encoding="utf-8")
        except OSError as exc:
            raise GhError(f"could not read notes_file {notes_path!r}: {exc}") from exc
        args.extend(["--notes-file", "-"])
        url = gh_text(args, input_text=notes)
    else:
        url = gh_text(args)
    return {"tag": tag.strip(), "url": url.strip()}


def op_run_view(spec: dict[str, Any]) -> dict[str, Any]:
    run_id = spec.get("run_id")
    if run_id is None:
        raise GhError("run_id is required for run.view")
    if isinstance(run_id, str) and NUMBER_RE.match(run_id):
        run_id = int(run_id)
    if isinstance(run_id, bool) or not isinstance(run_id, int):
        raise GhError(f"run_id must be an integer, got {run_id!r}")
    data = gh_json(
        [
            "run",
            "view",
            str(run_id),
            *repo_flag(spec),
            "--json",
            "databaseId,status,conclusion,url,workflowName,event",
        ]
    )
    return {"run": data}


def op_run_log_failed(spec: dict[str, Any]) -> dict[str, Any]:
    run_id = spec.get("run_id")
    if run_id is None:
        raise GhError("run_id is required for run.log_failed")
    if isinstance(run_id, str) and NUMBER_RE.match(run_id):
        run_id = int(run_id)
    if isinstance(run_id, bool) or not isinstance(run_id, int):
        raise GhError(f"run_id must be an integer, got {run_id!r}")
    log_lines = spec.get("log_lines", 60)
    if isinstance(log_lines, bool) or not isinstance(log_lines, int) or log_lines < 1:
        raise GhError("log_lines must be an integer >= 1")
    text = gh_text(["run", "view", str(run_id), *repo_flag(spec), "--log-failed"])
    lines = text.splitlines()
    interesting = [
        ln
        for ln in lines
        if re.search(r"FAIL|error|Parse|::error", ln, re.IGNORECASE)
    ]
    excerpt = interesting[:log_lines] if interesting else lines[:log_lines]
    return {
        "run_id": run_id,
        "log": "\n".join(excerpt),
        "truncated": len(excerpt) < len(lines),
    }


HANDLERS: dict[str, Any] = {
    "auth.status": op_auth_status,
    "repo.view": op_repo_view,
    "issue.view": op_issue_view,
    "issue.list": op_issue_list,
    "issue.search": op_issue_search,
    "issue.create": op_issue_create,
    "issue.edit": op_issue_edit,
    "issue.comment": op_issue_comment,
    "issue.close": op_issue_close,
    "issue.sub_issue_add": op_issue_sub_issue_add,
    "issue.sub_issue_list": op_issue_sub_issue_list,
    "issue.sub_issue_remove": op_issue_sub_issue_remove,
    "pr.view": op_pr_view,
    "pr.view_current": op_pr_view_current,
    "pr.diff": op_pr_diff,
    "pr.create": op_pr_create,
    "pr.checks": op_pr_checks,
    "pr.checks_poll": op_pr_checks_poll,
    "pr.comment": op_pr_comment,
    "pr.review": op_pr_review,
    "pr.merge": op_pr_merge,
    "pr.list": op_pr_list,
    "label.list": op_label_list,
    "label.create": op_label_create,
    "release.list": op_release_list,
    "release.create": op_release_create,
    "run.view": op_run_view,
    "run.log_failed": op_run_log_failed,
}


def execute(spec: dict[str, Any]) -> dict[str, Any]:
    if not isinstance(spec, dict):
        raise GhError("spec must be a JSON object")
    op = spec.get("op")
    if not isinstance(op, str) or not op.strip():
        raise GhError("spec.op is required")
    handler = HANDLERS.get(op.strip())
    if handler is None:
        supported = ", ".join(sorted(HANDLERS))
        raise GhError(f"unknown op {op!r} — supported: {supported}")
    result = handler(spec)
    return {"ok": True, "op": op.strip(), "result": result}


def main(argv: list[str] | None = None) -> int:
    import argparse

    ap = argparse.ArgumentParser(description="Structured GitHub CLI wrapper for shipmates.")
    ap.add_argument("--spec", help="path to JSON spec file (default: stdin)")
    ap.add_argument(
        "--list-ops",
        action="store_true",
        help="print supported operations and exit",
    )
    args = ap.parse_args(argv)

    if args.list_ops:
        print(json.dumps({"operations": sorted(HANDLERS)}, indent=2))
        return 0

    try:
        if args.spec:
            raw = Path(args.spec).read_text(encoding="utf-8")
        else:
            raw = sys.stdin.read()
        if not raw.strip():
            raise GhError("empty spec — pass JSON on stdin or --spec")
        spec = json.loads(raw)
        payload = execute(spec)
        sys.stdout.write(json.dumps(payload, indent=2) + "\n")
        return 0
    except GhError as exc:
        print(f"gh-tool: {exc}", file=sys.stderr)
        return 2
    except json.JSONDecodeError as exc:
        print(f"gh-tool: invalid JSON spec: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
