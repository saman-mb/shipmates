#!/usr/bin/env python3
"""Validate the static Shipmates site (site/) with stdlib only.

Runs identically in local self-check and in CI. Asserts the invariants the
landing page must hold: it exists, is self-contained (no external CSS/JS/font
hosts), every local asset it references resolves on disk, it has exactly one
<h1>, the expected component counts, valid JSON-LD, and a token-driven CSS.

Exit 0 = all green; exit 1 = one or more failures (printed)."""

from __future__ import annotations
import json
import re
import sys
from html.parser import HTMLParser
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SITE = ROOT / "site"
INDEX = SITE / "index.html"
CSS = SITE / "styles.css"

failures: list[str] = []
notes: list[str] = []


def fail(msg: str) -> None:
    failures.append(msg)


def ok(msg: str) -> None:
    notes.append(msg)


class Collector(HTMLParser):
    """Collects refs + the JSON-LD payload without external deps."""

    def __init__(self) -> None:
        super().__init__(convert_charrefs=True)
        self.refs: list[tuple[str, dict]] = []  # (tag, attrs)
        self.h1 = 0
        self._in_ldjson = False
        self.ldjson = ""

    def handle_starttag(self, tag, attrs):
        a = {k.lower(): (v or "") for k, v in attrs}
        self.refs.append((tag.lower(), a))
        if tag.lower() == "h1":
            self.h1 += 1
        if tag.lower() == "script" and a.get("type", "").lower() == "application/ld+json":
            self._in_ldjson = True

    def handle_endtag(self, tag):
        if tag.lower() == "script" and self._in_ldjson:
            self._in_ldjson = False

    def handle_data(self, data):
        if self._in_ldjson:
            self.ldjson += data


def is_external(url: str) -> bool:
    return bool(re.match(r"^(https?:)?//", url.strip()))


def is_local_asset(url: str) -> bool:
    u = url.strip()
    if not u or u.startswith(("#", "mailto:", "tel:", "data:", "javascript:")):
        return False
    return not is_external(u)


def main() -> int:
    # --- existence ---
    if not INDEX.is_file():
        fail(f"missing {INDEX.relative_to(ROOT)}")
        return report()
    if not CSS.is_file():
        fail(f"missing {CSS.relative_to(ROOT)}")

    html = INDEX.read_text(encoding="utf-8")
    css = CSS.read_text(encoding="utf-8") if CSS.is_file() else ""

    p = Collector()
    p.feed(html)

    # --- exactly one h1 ---
    if p.h1 == 1:
        ok("exactly one <h1>")
    else:
        fail(f"expected exactly one <h1>, found {p.h1}")

    # --- no '../' escaping the published root ---
    if "../" in html:
        fail("index.html contains '../' path(s) — would escape the Pages root")
    else:
        ok("no '../' path traversal")

    # --- external CSS / JS / @import (self-contained requirement) ---
    for tag, a in p.refs:
        if tag == "link" and "stylesheet" in a.get("rel", "").lower() and is_external(a.get("href", "")):
            fail(f"external stylesheet: {a.get('href')}")
        if tag == "script" and is_external(a.get("src", "")):
            fail(f"external script: {a.get('src')}")
    if "@import" in css:
        fail("styles.css uses @import")
    if re.search(r"url\(\s*['\"]?https?:", css):
        fail("styles.css references an external url(http...)")
    if not any(f.startswith("external") for f in failures):
        ok("self-contained (no external CSS/JS/font hosts)")

    # --- every local asset the page references resolves on disk ---
    missing = []
    checked = 0
    for tag, a in p.refs:
        for attr in ("src", "href"):
            url = a.get(attr, "")
            if is_local_asset(url):
                checked += 1
                target = (SITE / url.split("?")[0].split("#")[0]).resolve()
                if not target.is_file():
                    missing.append(url)
    for m in sorted(set(missing)):
        fail(f"referenced local asset not found: {m}")
    if not missing:
        ok(f"all {checked} local asset reference(s) resolve on disk")

    # --- JSON-LD parses and is a SoftwareApplication ---
    if not p.ldjson.strip():
        fail("no JSON-LD <script type=application/ld+json> block found")
    else:
        try:
            data = json.loads(p.ldjson)
            if data.get("@type") != "SoftwareApplication":
                fail(f"JSON-LD @type is {data.get('@type')!r}, expected SoftwareApplication")
            else:
                ok("JSON-LD parses (SoftwareApplication)")
        except json.JSONDecodeError as e:
            fail(f"JSON-LD does not parse: {e}")

    # --- required <head> SEO tags ---
    if not re.search(r"<title>[^<]+</title>", html):
        fail("missing non-empty <title>")
    if 'name="description"' not in html:
        fail("missing meta description")
    if 'rel="canonical"' not in html:
        fail("missing canonical link")
    if 'property="og:image"' not in html:
        fail("missing og:image")
    if not any(f for f in failures if "title" in f or "description" in f or "canonical" in f or "og:image" in f):
        ok("head SEO tags present (title, description, canonical, og:image)")

    # --- component counts (acceptance criteria) ---
    counts = {
        "crew-card": (len(re.findall(r'class="crew-card"', html)), 11),
        "order-card": (len(re.findall(r'class="order-card(?:\s|")', html)), 9),
        "order-card--flagship": (html.count("order-card--flagship"), 1),
        "how-step": (len(re.findall(r'class="how-step"', html)), 8),
        "faq__item": (len(re.findall(r'class="faq__item"', html)), 6),
    }
    for name, (got, want) in counts.items():
        if got == want:
            ok(f"{name}: {got}")
        else:
            fail(f"{name}: expected {want}, found {got}")

    # --- CSS is token-driven ---
    if ":root" not in css:
        fail("styles.css has no :root token block")
    else:
        ok(":root token block present")
    if "prefers-color-scheme: dark" not in css:
        fail("styles.css has no dark-theme override")
    else:
        ok("dark-theme override present")

    # --- reduced-motion demo poster wired (a11y / WCAG 2.2.2) ---
    if "hero__demo-poster" in html and "hero__demo-poster" in css and "prefers-reduced-motion" in css:
        ok("reduced-motion demo poster wired")
    else:
        fail("reduced-motion demo poster not fully wired (need the poster <img> in HTML + a prefers-reduced-motion swap in CSS)")

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
