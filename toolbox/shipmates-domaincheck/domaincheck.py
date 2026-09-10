#!/usr/bin/env python3
"""domaincheck — RDAP domain availability checks (registry-authoritative).

Uses https://rdap.org/domain/<name> and follows redirects to the authoritative
registry. A 404 from the registry means unregistered; 200 means registered.
This is RDAP, not DNS — no API keys, stdlib only (urllib).

Usage:
    python3 domaincheck.py github.com
    python3 domaincheck.py example.com example.org
    python3 domaincheck.py --tld com,app,io shipmates
    python3 domaincheck.py --detail github.com
    python3 domaincheck.py --whois github.com   # optional whois(1) passthrough

Exit codes: 0 on success; 2 on usage/validation error.
"""
from __future__ import annotations

import argparse
import json
import random
import re
import shutil
import subprocess
import sys
import time
import urllib.error
import urllib.request
from typing import Any

RDAP_BOOTSTRAP = "https://rdap.org/domain/"
DEFAULT_DELAY = 0.6
MAX_RETRIES = 4
USER_AGENT = "shipmates-domaincheck/1.0 (+https://github.com/saman-mb/shipmates)"


def _normalize_domain(raw: str) -> str:
    d = raw.strip().lower()
    d = d.removeprefix("http://").removeprefix("https://")
    if d.startswith("www."):
        d = d[4:]
    d = d.rstrip(".")
    if not d or "." not in d or not re.fullmatch(r"[a-z0-9.-]+", d):
        raise ValueError(f"invalid domain: {raw!r}")
    if d.startswith(".") or d.endswith(".") or ".." in d:
        raise ValueError(f"invalid domain: {raw!r}")
    return d


def _expand_tld(name: str, tld_csv: str) -> list[str]:
    label = name.strip().lower()
    if not label or "." in label:
        raise ValueError("--tld expects a bare label (no dots), e.g. shipmates")
    if not re.fullmatch(r"[a-z0-9-]+", label):
        raise ValueError(f"invalid label for --tld: {name!r}")
    tlds = [t.strip().lower().lstrip(".") for t in tld_csv.split(",") if t.strip()]
    if not tlds:
        raise ValueError("--tld requires a comma-separated suffix list, e.g. com,app,io")
    return [_normalize_domain(f"{label}.{t}") for t in tlds]


def _fetch_rdap(domain: str, delay: float) -> tuple[str, dict[str, Any] | None, str | None]:
    """Return (verdict, json_body_or_none, error_note_or_none). verdict in available|registered|unknown."""
    url = RDAP_BOOTSTRAP + domain
    backoff = delay
    last_err: str | None = None

    for attempt in range(MAX_RETRIES):
        if attempt:
            time.sleep(backoff)
            backoff = min(backoff * 2, 8.0)
        req = urllib.request.Request(url, headers={"Accept": "application/rdap+json", "User-Agent": USER_AGENT})
        try:
            with urllib.request.urlopen(req, timeout=30) as resp:
                code = resp.getcode()
                body = resp.read()
        except urllib.error.HTTPError as exc:
            if exc.code == 404:
                return "available", None, None
            if exc.code == 429:
                last_err = "rate limited (429)"
                jitter = random.uniform(0, 0.25)
                time.sleep(backoff + jitter)
                continue
            last_err = f"HTTP {exc.code}"
            if 500 <= exc.code < 600 and attempt + 1 < MAX_RETRIES:
                continue
            return "unknown", None, last_err
        except urllib.error.URLError as exc:
            last_err = str(exc.reason)
            if attempt + 1 < MAX_RETRIES:
                continue
            return "unknown", None, last_err

        if code == 404:
            return "available", None, None
        if code != 200:
            return "unknown", None, f"HTTP {code}"

        try:
            data = json.loads(body.decode("utf-8"))
        except (UnicodeDecodeError, json.JSONDecodeError) as exc:
            return "unknown", None, str(exc)
        return "registered", data, None

    return "unknown", None, last_err or "retries exhausted"


def _vcard_fn(entity: dict[str, Any]) -> str | None:
    vcard = entity.get("vcardArray")
    if not isinstance(vcard, list) or len(vcard) < 2:
        return None
    for row in vcard[1:]:
        if isinstance(row, list) and len(row) >= 4 and row[0] == "fn":
            return str(row[3])
    return None


def _extract_detail(data: dict[str, Any]) -> dict[str, Any]:
    out: dict[str, Any] = {"ldhName": data.get("ldhName") or data.get("unicodeName")}
    statuses = data.get("status")
    if isinstance(statuses, list):
        out["status"] = statuses
    events: dict[str, str] = {}
    for ev in data.get("events") or []:
        if not isinstance(ev, dict):
            continue
        action = ev.get("eventAction")
        when = ev.get("eventDate")
        if action and when:
            events[str(action)] = str(when)
    if events:
        out["events"] = events
    registrar = None
    for ent in data.get("entities") or []:
        if not isinstance(ent, dict):
            continue
        roles = ent.get("roles") or []
        if "registrar" in roles:
            registrar = _vcard_fn(ent) or ent.get("handle")
            break
    if registrar:
        out["registrar"] = registrar
    return out


def _run_whois(domain: str) -> None:
    whois = shutil.which("whois")
    if not whois:
        print("domaincheck: whois not installed — skipping whois output", file=sys.stderr)
        return
    try:
        proc = subprocess.run([whois, domain], capture_output=True, text=True, timeout=60, check=False)
    except (OSError, subprocess.TimeoutExpired) as exc:
        print(f"domaincheck: whois failed: {exc}", file=sys.stderr)
        return
    if proc.stdout:
        print(proc.stdout.rstrip())


def _print_result(domain: str, verdict: str, detail: dict[str, Any] | None, note: str | None, show_detail: bool) -> None:
    if show_detail and verdict == "registered" and detail:
        print(f"{domain}\t{verdict}")
        for key in ("registrar", "ldhName", "status", "events"):
            if key in detail:
                print(f"  {key}: {detail[key]}")
        if note:
            print(f"  note: {note}", file=sys.stderr)
    else:
        line = f"{domain}\t{verdict}"
        if note and verdict == "unknown":
            line += f"\t({note})"
        print(line)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description="Check domain availability via RDAP (rdap.org bootstrap)."
    )
    parser.add_argument("domains", nargs="*", help="domain names, e.g. example.com")
    parser.add_argument(
        "--tld",
        metavar="SUFFIXES",
        help="comma-separated TLDs; first positional arg is a bare label (shipmates + com,io)",
    )
    parser.add_argument(
        "--detail",
        action="store_true",
        help="show registrar, dates, and statuses for registered domains",
    )
    parser.add_argument(
        "--whois",
        action="store_true",
        help="after RDAP, run whois(1) for registered domains when available",
    )
    parser.add_argument(
        "--delay",
        type=float,
        default=DEFAULT_DELAY,
        metavar="SEC",
        help=f"seconds between queries in batch mode (default {DEFAULT_DELAY})",
    )
    args = parser.parse_args(argv)

    if args.delay < 0:
        print("domaincheck: --delay must be non-negative", file=sys.stderr)
        return 2

    try:
        if args.tld:
            if not args.domains:
                print("domaincheck: --tld requires a bare label argument", file=sys.stderr)
                return 2
            if len(args.domains) > 1:
                print("domaincheck: --tld accepts one label only", file=sys.stderr)
                return 2
            domains = _expand_tld(args.domains[0], args.tld)
        else:
            if not args.domains:
                parser.print_help(sys.stderr)
                return 2
            domains = [_normalize_domain(d) for d in args.domains]
    except ValueError as exc:
        print(f"domaincheck: {exc}", file=sys.stderr)
        return 2

    for i, domain in enumerate(domains):
        if i:
            time.sleep(args.delay)
        verdict, data, note = _fetch_rdap(domain, args.delay)
        detail = _extract_detail(data) if data else None
        _print_result(domain, verdict, detail, note, args.detail)
        if args.whois and verdict == "registered":
            _run_whois(domain)

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
