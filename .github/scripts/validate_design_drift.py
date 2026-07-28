#!/usr/bin/env python3
"""Catch design-system drift in site/ that the other gates cannot see.

The existing gates check shape (`validate_skills.py`), source-to-page fidelity
(`gen_command_pages.py --check`) and site structure (`validate_site.py`). None of
them notices that one component has been built two different ways, or that a
section quietly stopped sharing the page's left edge. Those are the defects a
human spots in seconds and a reader feels without being able to name.

Three checks, described in #131:

  1. section rhythm   — within a page, every section shares one container width
  2. component drift  — one component role, one implementation
  3. token bypass     — no raw colour/length literal outside the token block

Each gates on NEW violations only. Everything currently known and accepted lives
in BASELINE below, so the check can land without a flag day; a deviation that is
deliberate is recorded there with its reason, which doubles as the decision log.
Shrinking BASELINE is the point — an entry removed can never come back silently.

Exit 0 clean, 1 on a new violation.
"""
from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SITE = ROOT / "site"
CSS = SITE / "styles.css"

failures: list[str] = []
notes: list[str] = []


def fail(msg: str) -> None:
    failures.append(msg)
    print(f"  FAIL {msg}")


def ok(msg: str) -> None:
    print(f"  ok   {msg}")


def note(msg: str) -> None:
    notes.append(msg)
    print(f"  ~    {msg}")


# --- accepted deviations -----------------------------------------------------
# Each entry needs a reason. An entry with no reason is a bug someone hid.
BASELINE = {
    # check 1: sections whose container legitimately differs
    "section-width": {
        # Known-bad, being fixed in #130: both use --maxw-prose (760px) against
        # 1100px everywhere else, so they start 170px further in and visibly
        # break the page's left edge. Listed so this check can land before the
        # fix does; DELETE BOTH when #130 merges.
        "next",
        "faq",
    },
    # check 2: component roles known to have divergent implementations
    "component-drift": {
        # Known-bad, being fixed in #130: .section__eyebrow spaces its emoji
        # with a literal space in the HTML while .order-detail__eyebrow uses a
        # real `gap` token, so spacing cannot match across the two surfaces.
        # DELETE when #130 merges.
        "eyebrow",
    },
    # check 3: selectors allowed to carry a raw literal
    "token-bypass": {
        # Known-bad, being fixed in #128: `max-width: 940px` is the demo asset's
        # native width hardcoded into layout. DELETE when #128 merges.
        ".hero__demo",
        # `.visually-hidden` needs the exact 1px clip rect from the a11y recipe;
        # naming it as a token would imply it is tunable, and it is not.
        ".visually-hidden",
    },
}


def read(p: Path) -> str:
    if not p.is_file():
        print(f"  FAIL missing file: {p.relative_to(ROOT)}")
        sys.exit(1)
    return p.read_text(encoding="utf-8")


# --- check 1: section rhythm -------------------------------------------------
def check_section_rhythm(pages: list[Path]) -> None:
    """Within one page, every <section> should share a container width.

    A section that starts further in than its neighbours breaks the page's left
    edge — the reader sees the misalignment even when they cannot name it.

    The comparison is per page, not across the site, because a page that is
    entirely prose may legitimately be narrow throughout; what it may not do is
    change its mind halfway down. Checking only the landing page would have
    repeated this gate's own founding mistake — reviewing part of a surface and
    reporting on all of it.
    """
    print("\nsection rhythm")
    for page in pages:
        _rhythm_one(page.relative_to(SITE).as_posix(), page.read_text(encoding="utf-8"))


def _rhythm_one(rel: str, html: str) -> None:
    # <section ... id="x" ...> followed by its first container div
    pattern = re.compile(
        r'<section[^>]*\bid="(?P<id>[a-z0-9-]+)"[^>]*>\s*'
        r'<div class="(?P<cls>[^"]*\bcontainer\b[^"]*)"',
        re.IGNORECASE,
    )
    found = list(pattern.finditer(html))
    if not found:
        return  # not every page is section-structured; nothing to compare

    widths: dict[str, list[str]] = {}
    for m in found:
        classes = set(m.group("cls").split())
        modifiers = tuple(sorted(c for c in classes if c.startswith("container--")))
        key = modifiers or ("container",)
        widths.setdefault(" ".join(key), []).append(m.group("id"))

    if len(widths) == 1:
        ok(f"{rel}: all {len(found)} sections share one container width")
        return

    dominant = max(widths, key=lambda k: len(widths[k]))
    for key, ids in widths.items():
        if key == dominant:
            continue
        for sid in ids:
            if sid in BASELINE["section-width"]:
                note(f"{rel} #{sid}: container '{key}' — accepted deviation")
                continue
            fail(
                f"{rel} #{sid}: container '{key}' but {len(widths[dominant])} sections use "
                f"'{dominant}' — this section will not share the page's left edge. "
                f"Constrain the prose block instead, or record the deviation in BASELINE."
            )


# --- check 2: component drift ------------------------------------------------
PROP = re.compile(r"^\s*([a-z-]+)\s*:", re.MULTILINE)
RULE = re.compile(r"(?P<sel>^\.[^{@}\n]+?)\{(?P<body>[^}]*)\}", re.MULTILINE)

# Properties that decide how a component lays its children out. Two selectors
# playing the same role should agree here; they may freely differ on colour.
LAYOUT_PROPS = {"display", "gap", "align-items", "flex-direction", "column-gap"}

# Roles that MUST agree across surfaces, named explicitly.
#
# Inferring the role from the BEM suffix alone is wrong: `.hero__copy` is a text
# column and `.codeblock__copy` is a button, and `__inner` means only "the inner
# wrapper of whatever this is". A shared suffix is not a shared role, so guessing
# produces confident nonsense. This list is the claim being enforced — a role
# lands here when we have decided it should look the same everywhere.
SHARED_ROLES = {
    "eyebrow": "the small uppercase label above a section or page title",
    "icon": "an emoji/glyph sitting next to adjacent text",
}


def check_component_drift(css: str) -> None:
    """One component role should have one implementation.

    `.section__eyebrow` spacing its emoji with a literal space in the HTML while
    `.order-detail__eyebrow` uses a real `gap` token is the exact defect this
    catches: same thing on screen, two mechanisms, spacing that cannot match.
    """
    print("\ncomponent drift")
    roles: dict[str, dict[str, frozenset]] = {}
    for m in RULE.finditer(css):
        sel = m.group("sel").strip()
        if "," in sel or ":" in sel or " " in sel.strip("."):
            continue  # only simple single-class rules
        if "__" not in sel:
            continue
        role = sel.rsplit("__", 1)[1]
        if role not in SHARED_ROLES:
            continue
        props = frozenset(p for p in PROP.findall(m.group("body")) if p in LAYOUT_PROPS)
        roles.setdefault(role, {})[sel] = props

    drifted = 0
    for role, impls in sorted(roles.items()):
        if len(impls) < 2:
            continue
        distinct = set(impls.values())
        if len(distinct) == 1:
            continue
        if role in BASELINE["component-drift"]:
            note(f"__{role}: divergent implementations — accepted deviation")
            continue
        drifted += 1
        detail = "; ".join(
            f"{sel} {{{', '.join(sorted(p)) or 'no layout props'}}}"
            for sel, p in sorted(impls.items())
        )
        fail(
            f"__{role}: one component role, {len(distinct)} different layout "
            f"implementations — {detail}. They cannot render identically. "
            f"Converge them, or record the deviation in BASELINE."
        )
    if not drifted:
        checked = ", ".join(f"__{r}" for r in sorted(roles)) or "none present"
        ok(f"shared roles agree across surfaces ({checked})")


# --- check 3: token bypass ---------------------------------------------------
LITERAL = re.compile(
    r":\s*[^;{}]*?(?<![-\w])(?P<lit>#[0-9a-fA-F]{3,8}\b|\d+px\b)",
)
# Properties where a raw length is structural rather than design intent — a
# hairline border or a 2px underline offset is not a design token waiting to
# happen, and pretending otherwise buries the real findings in noise.
STRUCTURAL_OK = {"border", "border-top", "border-bottom", "border-left",
                 "border-right", "outline", "outline-offset", "border-width",
                 "border-left-width", "border-right-width", "border-top-width",
                 "border-bottom-width", "text-underline-offset",
                 "box-shadow", "text-shadow", "filter", "backdrop-filter",
                 "stroke-width", "flex-basis", "background-image"}

COMMENT = re.compile(r"/\*.*?\*/", re.DOTALL)


def check_token_bypass(css: str) -> None:
    """No raw colour or length outside the token block.

    The stylesheet's own header claims this invariant. Nothing enforced it, so
    the claim was true only for as long as everyone remembered.
    """
    print("\ntoken bypass")
    root_end = css.find("}", css.find(":root{"))
    body = css[root_end:] if root_end != -1 else css

    offenders = 0
    for m in RULE.finditer(body):
        sel = m.group("sel").strip()
        if sel in BASELINE["token-bypass"]:
            continue
        for decl in m.group("body").split(";"):
            if ":" not in decl:
                continue
            prop = decl.split(":", 1)[0].strip()
            if prop in STRUCTURAL_OK or prop.startswith("--"):
                continue
            hit = LITERAL.search(":" + decl.split(":", 1)[1])
            if hit and "var(" not in decl:
                offenders += 1
                fail(
                    f"{sel} {{ {prop} }}: raw literal '{hit.group('lit')}' outside the "
                    f"token block — use an existing token or add one."
                )
                break
    if not offenders:
        ok("no raw colour/length literals outside the token block")


def main() -> int:
    # Strip comments first — prose inside a comment is not a declaration, and
    # parsing it produced a confident false positive on a hex value in a note.
    css = COMMENT.sub("", read(CSS))
    pages = sorted(SITE.rglob("*.html"))
    if not pages:
        print("  FAIL no HTML pages found under site/")
        return 1

    print(f"design drift  ({len(pages)} pages)")
    check_section_rhythm(pages)
    check_component_drift(css)
    check_token_bypass(css)

    print()
    if failures:
        print(f"{len(failures)} drift violation(s).")
        print("A deliberate exception goes in BASELINE with its reason — that list is")
        print("the decision log, and it should only ever get shorter.")
        return 1
    accepted = sum(len(v) for v in BASELINE.values())
    print(f"No new drift. {accepted} accepted deviation(s) in BASELINE.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
