#!/usr/bin/env python3
"""Watch each AI harness's first-party docs for skill-discovery drift.

Repo-local dev tool (NOT a shipped shipmates resource). It answers one question:
"has any harness changed how it discovers skills since we last verified it?" —
so we can update the adapter to the right approach before users hit a silently
broken install.

Everything harness-specific lives in the injected config `tools/harness_watch.json`
(one entry per harness: docs URL, the strings its docs must still contain/omit,
what our adapter assumes, and when it was last verified). This script hard-codes
NOTHING about any harness; it iterates the config. To re-baseline a harness after
a review, edit the JSON, never this file.

Usage:
    python3 tools/harness_watch.py            # fetch docs, report drift (exit 1 on drift)
    python3 tools/harness_watch.py --offline  # skip network; config self-consistency only
    python3 tools/harness_watch.py --strict   # unreachable docs also fail
    python3 tools/harness_watch.py --json      # machine-readable report
    python3 tools/harness_watch.py --only codex,cursor
"""
from __future__ import annotations

import argparse
import json
import sys
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path

CONFIG = Path(__file__).with_name("harness_watch.json")
UA = "shipmates-harness-watch/1 (+https://github.com/saman-mb/shipmates)"

OK, DRIFT, UNREACHABLE, MISCONFIGURED = "OK", "DRIFT", "UNREACHABLE", "MISCONFIGURED"


def fetch(url: str, timeout: float = 12.0, max_redirects: int = 6) -> str:
    """GET a URL, following redirects (including 307/308 that older urllib raises)."""
    current = url
    for _ in range(max_redirects + 1):
        req = urllib.request.Request(current, headers={"User-Agent": UA})
        try:
            with urllib.request.urlopen(req, timeout=timeout) as resp:
                return resp.read().decode("utf-8", "replace")
        except urllib.error.HTTPError as exc:
            location = exc.headers.get("Location") if exc.headers else None
            if exc.code in (301, 302, 303, 307, 308) and location:
                current = urllib.parse.urljoin(current, location)
                continue
            raise
    raise urllib.error.URLError(f"too many redirects (>{max_redirects})")


def check_config_consistency(name: str, cfg: dict) -> list[str]:
    """Offline: does the entry's `tree` agree with its adapter path + expect rules?

    Catches a config typo (e.g. a `shared` harness whose adapter path still points
    at a private tree) without touching the network.
    """
    problems: list[str] = []
    tree = cfg.get("tree")
    path = cfg.get("adapter_skill_path", "")
    contains = cfg.get("expect_contains", [])
    if tree == "shared":
        if not path.startswith(".agents/skills"):
            problems.append(f"tree=shared but adapter_skill_path is {path!r} (expected .agents/skills/...)")
        if ".agents/skills" not in contains:
            problems.append("tree=shared but expect_contains does not list '.agents/skills'")
    elif tree == "native":
        if "/skills/" not in path:
            problems.append(f"tree=native but adapter_skill_path {path!r} has no /skills/ segment")
    elif tree == "commands":
        if "/commands/" not in path:
            problems.append(f"tree=commands but adapter_skill_path {path!r} has no /commands/ segment")
    else:
        problems.append(f"unknown tree {tree!r} (expected shared|native|commands)")
    if not cfg.get("docs_url"):
        problems.append("no docs_url")
    return problems


def check_docs(cfg: dict) -> tuple[str, list[str]]:
    """Fetch the docs and apply the expect rules. Returns (status, details).

    `check` mode (from the config, default "content"):
      - "content": the docs' raw HTML must contain/omit the `expect_*` strings.
      - "reachable": the page is JS-rendered (its content isn't in the static
        HTML — e.g. Google's antigravity.google app), so we only confirm it still
        loads and flag it for manual content review. Content checks there would be
        a permanent false positive.
    """
    url = cfg["docs_url"]
    try:
        body = fetch(url).lower()
    except (urllib.error.URLError, urllib.error.HTTPError, TimeoutError, OSError) as exc:
        return UNREACHABLE, [f"could not fetch {url}: {exc}"]

    if cfg.get("check", "content") == "reachable":
        return OK, ["reachable; docs are JS-rendered so content is not auto-checked — review by hand"]

    details: list[str] = []
    for needle in cfg.get("expect_contains", []):
        if needle.lower() not in body:
            details.append(f"expected to find {needle!r} in the docs — it is GONE (path may have changed)")
    for needle in cfg.get("expect_absent", []):
        if needle.lower() in body:
            details.append(f"{needle!r} now APPEARS in the docs (it was absent) — reconsider the approach")
    return (DRIFT if details else OK), details


def main() -> int:
    ap = argparse.ArgumentParser(description="Watch harness docs for skill-discovery drift.")
    ap.add_argument("--offline", action="store_true", help="skip network; run config self-consistency only")
    ap.add_argument("--strict", action="store_true", help="treat UNREACHABLE docs as a failure")
    ap.add_argument("--json", action="store_true", help="machine-readable output")
    ap.add_argument("--only", default="", help="comma-separated harness names to check (default: all)")
    args = ap.parse_args()

    config = json.loads(CONFIG.read_text(encoding="utf-8"))
    harnesses: dict[str, dict] = config["harnesses"]
    wanted = {h.strip() for h in args.only.split(",") if h.strip()}
    if wanted:
        unknown = wanted - harnesses.keys()
        if unknown:
            print(f"unknown harness(es): {', '.join(sorted(unknown))}", file=sys.stderr)
            return 2

    results: list[dict] = []
    for name, cfg in harnesses.items():
        if wanted and name not in wanted:
            continue
        misconfig = check_config_consistency(name, cfg)
        if misconfig:
            results.append({"harness": name, "status": MISCONFIGURED, "details": misconfig,
                            "tree": cfg.get("tree"), "docs_url": cfg.get("docs_url")})
            continue
        if args.offline:
            results.append({"harness": name, "status": OK, "details": ["config consistent (offline)"],
                            "tree": cfg.get("tree"), "docs_url": cfg.get("docs_url")})
            continue
        status, details = check_docs(cfg)
        results.append({"harness": name, "status": status, "details": details,
                        "tree": cfg.get("tree"), "docs_url": cfg.get("docs_url"),
                        "verified_on": cfg.get("verified_on")})

    if args.json:
        print(json.dumps({"results": results}, indent=2))
    else:
        _render(results, offline=args.offline)

    bad = {DRIFT, MISCONFIGURED}
    if args.strict:
        bad = bad | {UNREACHABLE}
    return 1 if any(r["status"] in bad for r in results) else 0


def _render(results: list[dict], offline: bool) -> None:
    icon = {OK: "ok  ", DRIFT: "DRIFT", UNREACHABLE: "unrch", MISCONFIGURED: "CONFIG"}
    print(f"Harness skill-discovery watch{' (offline: config only)' if offline else ''}\n")
    for r in results:
        print(f"  {icon.get(r['status'], '?'):6} {r['harness']:15} {r.get('tree','') :8} {r.get('docs_url','')}")
        for d in r["details"]:
            if r["status"] in (DRIFT, MISCONFIGURED):
                print(f"         └─ {d}")
            elif "review by hand" in d:
                print(f"         ~  {d}")
    drift = [r["harness"] for r in results if r["status"] in (DRIFT, MISCONFIGURED)]
    unreach = [r["harness"] for r in results if r["status"] == UNREACHABLE]
    print()
    if drift:
        print(f"  {len(drift)} need review: {', '.join(drift)}")
        print("  → re-verify the harness's first-party docs, update the adapter if its approach")
        print("    changed, then bump `verified_on`/`expect_*` in tools/harness_watch.json.")
    if unreach:
        print(f"  {len(unreach)} unreachable (network?): {', '.join(unreach)}")
    if not drift and not unreach:
        print("  all harnesses match their recorded expectations.")


if __name__ == "__main__":
    sys.exit(main())
