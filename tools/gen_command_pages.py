#!/usr/bin/env python3
"""Generate site/commands/<slug>/index.html and site/agents/<role>/index.html —
one detail page per command, one per agent.

Honest by construction: every command-specific sentence on a command page is a
markdown-rendered projection of skills/<slug>/SKILL.md — no invented stage names,
gates, crew, counts, durations or file names. Only command-agnostic chrome
("How to run it", "The stages", "Other commands") is authored here. Anything the
parser does not recognise raises with a file:line and a remedy rather than being
silently dropped or passed through, and every non-blank source line must be
claimed by a block. Deterministic and committed, matching the repo's other
generators. Regenerate with:  python3 tools/gen_command_pages.py

Agent pages project the agents/<role>.md frontmatter (name/description/tools)
verbatim; their editorial copy (tagline, what/scenarios/checks/crew-fit) is
authored here in AGENT_COPY, and drift between the copy table and agents/*.md
on disk raises in either direction — an agent with no copy, or copy with no
agent, is a hard error. Which commands call an agent is DERIVED by scanning the
skill sources for the role name, never hand-maintained.

Layers, in file order, with hard import discipline:
  1 MODEL   frozen dataclasses only — no markup, no URLs, no I/O
  2 COPY    AGENT_COPY — the agent pages' authored editorial text (model-layer data)
  3 PARSE   skills/*/SKILL.md + agents/*.md -> model — no markup, no writing
  4 RENDER  model -> HTML/XML strings — the only layer that escapes or emits markup
  5 EMIT    paths + bytes — build_site() is pure, write_all() is the only writer
  6 CLI     argv -> exit code — the only layer that prints
"""

import argparse
import contextlib
import difflib
import html
import io
import json
import os
import re
import sys
import tempfile
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
    "pr-review",
    "onboard",
    "refactor",
)

# Hand-authored docs pages under site/docs/. The generator discovers them on
# disk and includes them in the sitemap — it never generates them.
DOCS_SLUGS = ("install", "harnesses", "troubleshooting", "architecture")
FLAGSHIP_SLUG = "ship-issue"

# Canonical crew order — the homepage crew grid's order. Drives the agent page
# sibling nav and the sitemap. Every agents/<role>.md on disk must appear here.
AGENT_ROLES = (
    "architect",
    "senior-engineer",
    "sdet",
    "security-engineer",
    "site-reliability-engineer",
    "performance-engineer",
    "devops-engineer",
    "product-manager",
    "ux-ui-designer",
    "art-director",
    "technical-writer",
    "data-scientist",
)

SITE_URL = "https://saman-mb.github.io/shipmates/"
SOCIAL_IMAGE = SITE_URL + "assets/social-preview.png"
REPO_BLOB_BASE = "https://github.com/saman-mb/shipmates/blob/main/"
# Wall clock is never read: a fixed constant keeps every run byte-identical.
LASTMOD = "2026-07-25"

# Section ids reserved by the page skeleton; a source heading may not claim one.
RESERVED_ANCHORS = frozenset(
    {"invoke", "stages", "config", "guardrails", "source", "other-commands", "main", "top"}
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
    name: str
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


@dataclass(frozen=True, slots=True)
class AgentFrontmatter:
    """agents/<role>.md uses `tools:` (comma-separated), not a skill's `allowed-tools:`."""
    name: str
    description: str
    tools: tuple  # tuple[str, ...]


@dataclass(frozen=True, slots=True)
class AgentScenario:
    title: str  # 3-6 words, rendered as the card's heading
    desc: str  # 1-2 sentences


@dataclass(frozen=True, slots=True)
class AgentCheck:
    lead: str  # the bold lead-in
    text: str  # the explanation — at most 25 words


@dataclass(frozen=True, slots=True)
class CrewFit:
    paragraphs: tuple  # tuple[str, ...] — 1-2 paragraphs naming the handoff roles
    related: tuple  # tuple[str, ...] — roles it hands off to or works alongside


@dataclass(frozen=True, slots=True)
class AgentCopy:
    """The authored editorial half of an agent page; the other half is parsed frontmatter."""
    tagline: str
    what: tuple  # tuple[str, ...] — 1-2 paragraphs
    scenarios: tuple  # tuple[AgentScenario, ...] — 2-3 cards
    checks: tuple  # tuple[AgentCheck, ...] — 5-8 bullets
    crew_fit: CrewFit


@dataclass(frozen=True, slots=True)
class Agent:
    slug: str
    source_path: str  # repo-relative posix path
    name: str
    description: str
    tools: tuple  # tuple[str, ...]
    tagline: str
    what: tuple  # tuple[str, ...]
    scenarios: tuple  # tuple[AgentScenario, ...]
    checks: tuple  # tuple[AgentCheck, ...]
    crew_fit: CrewFit
    called_by: tuple  # tuple[str, ...] — command slugs, derived from the skill sources


@dataclass(frozen=True, slots=True)
class ToolExample:
    """One example on a tool page: a `src` asset (png/gif/svg) with alt/caption.
    `poster` is a static final-frame shown under prefers-reduced-motion — set it
    only for animated GIFs; leave it empty for a static PNG or SVG."""
    src: str  # filename, relative to the tool page (e.g. "examples/deploy.gif")
    width: int
    height: int
    alt: str
    caption: str
    poster: str = ""  # static reduced-motion poster for an animated GIF; "" if static


@dataclass(frozen=True, slots=True)
class ToolCopy:
    """The authored editorial half of a tool page; the other half is the
    parsed `toolbox/<slug>/tool.md` frontmatter (name + description)."""
    tagline: str
    what: tuple  # tuple[str, ...] — 1-2 paragraphs
    usage: tuple  # tuple[str, ...] — 1-2 paragraphs on how the crew reach for it
    examples: tuple = ()  # tuple[ToolExample, ...] — optional demo gallery
    sample: str = ""  # optional plain-text input->output block for non-visual tools


@dataclass(frozen=True, slots=True)
class Tool:
    slug: str
    source_path: str  # repo-relative posix path to tool.md
    name: str
    description: str
    tagline: str
    what: tuple  # tuple[str, ...]
    usage: tuple  # tuple[str, ...]
    bundled: tuple  # tuple[str, ...] — bundled asset filenames besides tool.md
    examples: tuple = ()  # tuple[ToolExample, ...]
    sample: str = ""  # optional plain-text input->output block for non-visual tools


# ---------------------------------------------------------------------------
# Layer 2 — COPY  (AGENT_COPY)
# The agent pages' authored editorial text: model-layer data — no markup, no
# URLs, no I/O. Every string is inline markdown, rendered by the RENDER layer.
# Drift against agents/*.md raises in load_agents, in either direction; the
# shape rules (2-3 scenarios, 5-8 checks, 25-word explanations) are enforced by
# check_agent_copy, so a copy defect fails the build rather than shipping.
# ---------------------------------------------------------------------------

# Source name attached to copy errors — the copy lives here, not in a repo source file.
AGENT_COPY_SRC = "tools/gen_command_pages.py"

AGENT_COPY = {
    "architect": AgentCopy(
        tagline="Structure & schema — coupling, boundaries, migration safety.",
        what=(
            "The architect reviews the structure of a change rather than its lines: whether "
            "the change fits the system's existing boundaries, layering, and single sources of "
            "truth, or forks logic that already lives elsewhere. A one-off exception that fights "
            "the established pattern is a reject even when it works.",
            "One-way doors — public APIs, persisted schemas, data migrations, hard-to-drop "
            "dependencies — get scrutinised hard, while reversible changes wave through. Every "
            "finding is grounded in specific `file:line` evidence, and significant decisions "
            "capture the trade-off and the rejected alternative, so the why survives.",
        ),
        scenarios=(
            AgentScenario(
                "A design plan needs vetting",
                "Before a big build starts, the architect checks the plan against the system's "
                "real boundaries and quality attributes — coupling, reversibility, and what "
                "breaks at ten times the load.",
            ),
            AgentScenario(
                "A schema or API is changing",
                "Cross-cutting and schema-level changes get a structural review before merge: "
                "backward and forward compatibility, versioning, and a concrete migration path "
                "for existing data and callers.",
            ),
            AgentScenario(
                "Complexity is creeping in",
                "When a change adds abstraction, the architect asks whether that complexity is "
                "essential to the problem — and prefers removing the need over adding a clever "
                "layer.",
            ),
        ),
        checks=(
            AgentCheck(
                "Fit and duplication.",
                "Whether the change respects existing boundaries and single sources of truth, "
                "or quietly forks logic that already lives elsewhere.",
            ),
            AgentCheck(
                "Coupling and blast radius.",
                "What the change makes harder to change later — dependency direction, cohesion, "
                "and how far a future edit would have to reach.",
            ),
            AgentCheck(
                "Reversibility.",
                "Two-way doors get waved through; one-way doors like public APIs, persisted "
                "schemas, and migrations get scrutinised hard.",
            ),
            AgentCheck(
                "Quality attributes.",
                "Real risks to security, performance, scalability, reliability, and observability "
                "— including what breaks at ten times the load, data, or users.",
            ),
            AgentCheck(
                "Data and schema evolution.",
                "Backward and forward compatibility, versioning, and a concrete migration path "
                "for existing data and its callers.",
            ),
            AgentCheck(
                "Essential complexity.",
                "Whether new complexity is essential to the problem or accidental — and whether "
                "the need could be removed instead.",
            ),
        ),
        crew_fit=CrewFit(
            paragraphs=(
                "The architect usually runs early and late: vetting a design plan before the "
                "`senior-engineer` builds from it, then reviewing the finished structure before "
                "merge. A REJECT hands the engineer the specific structural concern and a "
                "concrete alternative, and the `sdet` later proves the rework actually runs.",
            ),
            related=("senior-engineer", "sdet", "security-engineer"),
        ),
    ),
    "senior-engineer": AgentCopy(
        tagline="Builds to spec, fixes red CI, clears review defects.",
        what=(
            "The senior engineer is the crew's builder. It implements features to a plan or "
            "spec, fixes failing tests and red CI, and clears the defects reviewers flag — "
            "working in the codebase's existing style and reusing what's already there before "
            "adding anything new.",
            "It implements exactly what the task asks, treats tests as part of done, and "
            "verifies its own work — running the relevant tests, build, and lint, then "
            "re-reading its diff before reporting. It never commits or opens pull requests: the "
            "orchestrator owns git, and the engineer reports what changed and exactly how it "
            "was verified.",
        ),
        scenarios=(
            AgentScenario(
                "A planned feature needs building",
                "Hand it a spec or plan and it implements exactly that — matching the codebase's "
                "idioms, staying in scope, and adding tests for the failure and edge paths, not "
                "just the happy path.",
            ),
            AgentScenario(
                "CI is red",
                "Failing tests, lint, or build errors get diagnosed and fixed at the cause, "
                "then re-run to prove the pipeline goes green.",
            ),
            AgentScenario(
                "Review came back with defects",
                "The board's defect list gets cleared item by item, each fix verified — with "
                "adjacent problems noted for follow-up rather than silently folded in.",
            ),
        ),
        checks=(
            AgentCheck(
                "Codebase match.",
                "New code follows the project's existing style, idioms, and patterns, reusing "
                "what's already there before adding anything new.",
            ),
            AgentCheck(
                "Scope discipline.",
                "Exactly what the task, acceptance criteria, or defect list asks — adjacent "
                "problems get noted for follow-up, not silently fixed.",
            ),
            AgentCheck(
                "Test coverage.",
                "Tests for the failure and edge paths, not just the happy path — meaningful "
                "coverage, not coverage theatre.",
            ),
            AgentCheck(
                "Security hygiene.",
                "External input validated, no secrets committed, least privilege honoured, and "
                "no injection or path-traversal footguns introduced.",
            ),
            AgentCheck(
                "Verified done.",
                "Relevant tests, build, and lint actually run, the diff re-read, and each "
                "criterion confirmed before claiming completion.",
            ),
            AgentCheck(
                "Surfaced ambiguity.",
                "Underspecified tasks or conflicts with the code get called out, never silently "
                "guessed around.",
            ),
        ),
        crew_fit=CrewFit(
            paragraphs=(
                "Nearly every command routes through the senior engineer at some point: it "
                "builds what the `architect` and `product-manager` shape, and fixes what the "
                "`sdet`, `security-engineer`, and the review board find. Its handback is a "
                "verified diff and a precise report, which the sdet then re-verifies "
                "independently.",
            ),
            related=("architect", "sdet", "product-manager"),
        ),
    ),
    "sdet": AgentCopy(
        tagline="Runs the real tests/build and reports pass/fail with a defect list.",
        what=(
            "The sdet proves a change actually works — by running it, not by judging whether "
            "the code looks right. It executes the real tests, linters, type-checks, and build "
            "against the pushed head, testing to break rather than to confirm.",
            "Tests are designed deliberately — boundary values, decision tables, state "
            "transitions, malformed input — and every acceptance criterion is traced to a test "
            "that exercises it. Defects come back severity-tagged, with the exact command, "
            "exact output, and `file:line`, so nobody has to reproduce its work. It verifies; "
            "it never edits code.",
        ),
        scenarios=(
            AgentScenario(
                "A change is about to ship",
                "Before a PR opens, and again against the pushed head before merge, the sdet "
                "runs the full validation plan and returns PASS or FAIL with the defect list.",
            ),
            AgentScenario(
                "The happy path looks fine",
                "Boundary values, empty and maximum inputs, and malformed cases get probed — "
                "along with error paths, concurrency, and whether nearby behaviour silently "
                "broke.",
            ),
            AgentScenario(
                "A test suite looks flaky",
                "A pass on a non-deterministic test is not a pass; the sdet re-runs to confirm "
                "before anything is reported green.",
            ),
        ),
        checks=(
            AgentCheck(
                "Real execution.",
                "Tests, linters, type-checks, and a real build or run where the toolchain "
                "exists — a static read-through only as a stated fallback.",
            ),
            AgentCheck(
                "Boundary values.",
                "Empty, one, many, maximum, and just-past-maximum inputs, plus decision tables "
                "for branching logic.",
            ),
            AgentCheck(
                "Criteria traceability.",
                "Every acceptance criterion mapped to a test that exercises it — an untested "
                "criterion is itself a finding.",
            ),
            AgentCheck(
                "Failure paths.",
                "Error handling, concurrency and ordering, malformed input, and whether nearby "
                "existing behaviour silently broke.",
            ),
            AgentCheck(
                "Flakiness.",
                "Any non-deterministic pass re-run to confirm — a flaky green is never reported "
                "as green.",
            ),
            AgentCheck(
                "Actionable defects.",
                "Every finding severity-tagged blocking, high, or low, with the exact command, "
                "exact output, and `file:line`.",
            ),
        ),
        crew_fit=CrewFit(
            paragraphs=(
                "The sdet is the crew's independent verification pass: it re-runs what the "
                "`senior-engineer` claims is done, and its verdict gates the `product-manager`'s "
                "acceptance. A FAIL hands the engineer a severity-tagged defect list to clear; "
                "a PASS is the evidence the board can trust.",
            ),
            related=("senior-engineer", "product-manager", "site-reliability-engineer"),
        ),
    ),
    "security-engineer": AgentCopy(
        tagline="Threat-models the change — authz, injection, secrets, vulnerable deps.",
        what=(
            "The security engineer threat-models a change against the project's actual threat "
            "model — proportionate, not paranoid. It walks the trust boundaries with STRIDE, "
            "asking per boundary what an attacker controls and what that buys them, then "
            "reviews against OWASP fundamentals.",
            "Every finding shows the exploit path, not a lint hit: the concrete attack scenario "
            "from inputs to impact, the exact location, and a specific fix, ranked from "
            "Critical to Low. A credible Critical or High is blocking. It hands the engineer a "
            "precise, testable remediation rather than writing the product fix itself.",
        ),
        scenarios=(
            AgentScenario(
                "A change touches a trust boundary",
                "Authentication, authorisation, input handling, crypto, or a new external "
                "surface gets threat-modelled before it ships, with findings ranked by "
                "severity.",
            ),
            AgentScenario(
                "Secrets might be leaking",
                "Code, commits, logs, and error bodies get hunted for tokens and keys — a "
                "hardcoded credential is a blocking finding.",
            ),
            AgentScenario(
                "Dependencies are changing",
                "Known-vulnerable, unpinned, or abandoned packages and risky install-time "
                "scripts get flagged; lockfiles and minimal, current versions are preferred.",
            ),
        ),
        checks=(
            AgentCheck(
                "Authorisation.",
                "Identity actually verified, and every privileged action checked server-side "
                "against this principal — broken access control and IDOR hunted first.",
            ),
            AgentCheck(
                "Injection.",
                "Untrusted input reaching any interpreter parameterised, never concatenated; "
                "output encoded for its sink to stop XSS, SSRF, and path traversal.",
            ),
            AgentCheck(
                "Secrets and crypto.",
                "No secrets in code, commits, logs, or errors; vetted primitives only, strong "
                "salted password hashing, TLS in transit.",
            ),
            AgentCheck(
                "Supply chain.",
                "Known-vulnerable, unpinned, or abandoned dependencies flagged, along with "
                "risky install-time scripts.",
            ),
            AgentCheck(
                "Secure defaults.",
                "Least privilege, deny-by-default, fail closed, server-side validation, and no "
                "stack traces leaking in error bodies.",
            ),
            AgentCheck(
                "Exploit paths.",
                "Findings show the concrete attack scenario from inputs to impact — reasoned "
                "data flow, not pattern-match lint hits.",
            ),
        ),
        crew_fit=CrewFit(
            paragraphs=(
                "The security engineer reviews risky changes before merge and thinks about "
                "trust boundaries alongside the `architect`. Its REJECT hands the "
                "`senior-engineer` a specific, testable remediation, and the `sdet` confirms "
                "the fix holds. On delivery plumbing it sets the severity of an exposure, while "
                "the `devops-engineer` owns where the wiring changes.",
            ),
            related=("architect", "senior-engineer", "devops-engineer"),
        ),
    ),
    "site-reliability-engineer": AgentCopy(
        tagline="Reliability, failure modes, rollback safety — and bug root-cause.",
        what=(
            "The site reliability engineer judges a change by how it behaves when things go "
            "wrong, not just on the happy path. It is also the crew's root-cause specialist: "
            "reproduce the failure deterministically, work back to the mechanism, then specify "
            "the minimal fix and the regression check that would have caught it.",
            "On review it walks the failure surface — timeouts and retries, idempotency, "
            "observability, and whether the rollout can be undone. Findings are ranked by "
            "severity, with blockers like data loss, no rollback path, or unbounded failure "
            "separated from nits.",
        ),
        scenarios=(
            AgentScenario(
                "A bug needs a real cause",
                "The SRE finds the smallest input that triggers the failure, names the "
                "mechanism, and hands back the minimal fix plus the regression check — "
                "blameless, fix once.",
            ),
            AgentScenario(
                "A change is going to production",
                "Failure modes, rollback safety, and migration compatibility get reviewed "
                "before deploy; a one-way, irreversible deploy is a blocking concern unless "
                "justified.",
            ),
            AgentScenario(
                "Something wakes people up",
                "Toil and unobservable systems get flagged: if it can't be diagnosed at 3am "
                "from what it emits, it can't be operated.",
            ),
        ),
        checks=(
            AgentCheck(
                "Reproduction.",
                "The smallest input or state that triggers the failure, captured "
                "deterministically — no reproduction, no confirmed root cause.",
            ),
            AgentCheck(
                "Root cause.",
                "Work backwards from the failure with logs, traces, and bisection until the "
                "actual defect is named, not the place it surfaced.",
            ),
            AgentCheck(
                "Failure modes.",
                "Slow or dead dependencies, malformed input, full disks, and mid-operation "
                "crashes — every remote call gets a timeout and sane retries.",
            ),
            AgentCheck(
                "Idempotency.",
                "Running twice — retry, redelivery, restart — must not double-apply; partial "
                "failures left recoverable, resources bounded.",
            ),
            AgentCheck(
                "Observability.",
                "Meaningful logs, metrics, and traces at the right boundaries, with no secrets "
                "in them — diagnosable at 3am.",
            ),
            AgentCheck(
                "Safe delivery.",
                "A rollback path, backward-compatible migrations, and flag or canary guards — "
                "irreversible deploys blocked unless justified.",
            ),
        ),
        crew_fit=CrewFit(
            paragraphs=(
                "On `/fix-bug` the SRE owns the root cause and hands the `senior-engineer` the "
                "minimal fix and its regression check; the `sdet` then proves the fix. On "
                "`/release` it gates deploy safety. Build-time questions — pipelines, caching, "
                "pinning — belong to the `devops-engineer`, and the SRE defers there "
                "explicitly.",
            ),
            related=("senior-engineer", "sdet", "devops-engineer"),
        ),
    ),
    "performance-engineer": AgentCopy(
        tagline="Profiles, benchmarks, and proves the win.",
        what=(
            "The performance engineer optimises to the project's stated bar — target latency, "
            "throughput, frame budget, or memory ceiling — with correctness first: a fast "
            "wrong answer is a bug. If no bar is stated, it establishes the current baseline "
            "and improves against that.",
            "The discipline is measure, don't guess: record a baseline, profile to the real "
            "bottleneck, attack the biggest cost, then re-measure with the same benchmark to "
            "prove the win. No measured improvement means it was not an optimisation.",
        ),
        scenarios=(
            AgentScenario(
                "Something is too slow",
                "The bottleneck gets found with a profiler rather than intuition, then fixed "
                "and proven with before-and-after numbers on the same benchmark.",
            ),
            AgentScenario(
                "A change might regress",
                "Hot paths get reviewed for complexity, N+1 patterns, and unbounded growth — a "
                "real regression against the bar is blocking.",
            ),
            AgentScenario(
                "No way to measure exists",
                "If the repo has no benchmark or timing harness, a minimal one gets built "
                "first — nothing is optimised on intuition.",
            ),
        ),
        checks=(
            AgentCheck(
                "Repeatable baseline.",
                "A benchmark, profile, or timing harness with a number recorded before anything "
                "gets touched.",
            ),
            AgentCheck(
                "The real bottleneck.",
                "Profiling to where time and memory actually go — Amdahl's law: speeding up "
                "code that isn't the bottleneck buys nothing.",
            ),
            AgentCheck(
                "Biggest cost first.",
                "Algorithmic wins before micro-tuning — complexity reductions, collapsed N+1 "
                "queries, cached repeated work, fewer allocations and copies.",
            ),
            AgentCheck(
                "The right goal.",
                "Latency, throughput, and memory are different targets — watch the tail "
                "percentiles, not just the average.",
            ),
            AgentCheck(
                "Proof of the win.",
                "Before-and-after numbers on the same benchmark, with the target met and the "
                "tests still green.",
            ),
            AgentCheck(
                "Premature optimisation.",
                "Cold paths left simple — flagged when the simpler code is the right call at "
                "this scale.",
            ),
        ),
        crew_fit=CrewFit(
            paragraphs=(
                "The performance engineer is called when a bottleneck needs fixing or a change "
                "needs a regression check, and hands the `senior-engineer` a precise, measured "
                "fix to apply. Its remit is the shipped system's behaviour — build and pipeline "
                "speed belongs to the `devops-engineer`.",
            ),
            related=("senior-engineer", "devops-engineer", "sdet"),
        ),
    ),
    "devops-engineer": AgentCopy(
        tagline="Build & delivery — reproducibility, pinning, environment parity.",
        what=(
            "The devops engineer reviews the delivery system as a codebase: the pipeline and "
            "build definitions, image and environment definitions, config and secret plumbing, "
            "and dependency pinning that construct, configure, and ship the software. Its line "
            "is sharp — it owns build time, not run time.",
            "It judges reproducibility, pinning, environment parity, pipeline correctness, and "
            "the speed of the feedback loop — and where feasible it exercises the definitions "
            "rather than reasoning about them: run the build twice, build from a clean "
            "checkout, inspect what a step really resolved.",
        ),
        scenarios=(
            AgentScenario(
                "The build can't be trusted",
                "Floating versions, runner-dependent steps, and unchecked network fetches get "
                "hunted until the same commit produces the same artifact, on any machine, six "
                "months from now.",
            ),
            AgentScenario(
                "A pipeline change needs review",
                "Jobs get checked for correct wiring, real gating, idempotent re-runs, and safe "
                "secret plumbing — a green check that can't fail is worse than no check.",
            ),
            AgentScenario(
                "The feedback loop is slow",
                "Cache correctness comes before cache presence, then redundant work and "
                "needless serialisation — a loosely-keyed cache is a correctness bug wearing a "
                "performance costume.",
            ),
        ),
        checks=(
            AgentCheck(
                "Reproducibility.",
                "Same commit, same artifact — no floating versions, machine-specific paths, or "
                "steps whose result depends on what ran before.",
            ),
            AgentCheck(
                "Pinning.",
                "Base images, toolchains, and build dependencies pinned to immutable "
                "references, with a stated route to updating them.",
            ),
            AgentCheck(
                "Environment parity.",
                "Drift between environments named specifically — versions, config shape, "
                "resource limits, flags — and the bug class it hides.",
            ),
            AgentCheck(
                "Secret plumbing.",
                "Where values are injected, how they're scoped per environment, and whether one "
                "can reach a log, artifact, or untrusted pull request.",
            ),
            AgentCheck(
                "Pipeline correctness.",
                "Jobs wired to the right events, gates that can genuinely fail, and re-runs "
                "that are idempotent and side-effect free.",
            ),
            AgentCheck(
                "Feedback-loop time.",
                "What a contributor waits for and why — cache correctness, redundant work, and "
                "jobs that could be conditional.",
            ),
        ),
        crew_fit=CrewFit(
            paragraphs=(
                "Three verdicts get deferred explicitly: rollout and rollback safety to the "
                "`site-reliability-engineer`, the severity of a secret exposure to the "
                "`security-engineer`, and the shipped system's performance to the "
                "`performance-engineer`. File location never decides ownership — the question "
                "being asked does.",
            ),
            related=("site-reliability-engineer", "security-engineer", "performance-engineer"),
        ),
    ),
    "product-manager": AgentCopy(
        tagline="Accepts or rejects against the acceptance criteria and your bar.",
        what=(
            "The product manager guards user value and the quality bar — not the code. It "
            "checks every acceptance criterion individually against the actual current state of "
            "the pushed change, running whatever is checkable rather than trusting the PR's "
            "claims.",
            "Beyond the ticket, it holds the change to the project's Definition of Done: tests "
            "present, docs updated, and the non-functional expectations the product implies. It "
            "guards scope in both directions — rejecting under-delivery and gold-plating alike. "
            "During planning, it surfaces hidden requirements and names ambiguity rather than "
            "letting it slide.",
        ),
        scenarios=(
            AgentScenario(
                "A PR needs a verdict",
                "Each acceptance criterion gets verified against the real, pushed state of the "
                "change — ACCEPT, ACCEPT-WITH-NITS, or REJECT with the specific unmet criteria "
                "listed.",
            ),
            AgentScenario(
                "Requirements are fuzzy",
                "During planning, the product manager asks the why behind the request, "
                "surfacing hidden requirements and edge cases before they turn into rework.",
            ),
            AgentScenario(
                "The ticket passed, the point didn't",
                "Outcome over output: whether the change actually solves the user's underlying "
                "problem, judged from the real journey rather than one screen in isolation.",
            ),
        ),
        checks=(
            AgentCheck(
                "Every criterion.",
                "Each acceptance criterion checked individually against the pushed change's "
                "actual state — run when runnable, never taken on claims.",
            ),
            AgentCheck(
                "Definition of Done.",
                "The project's stated bar beyond the ticket: tests present, docs updated, and "
                "implied non-functional expectations held.",
            ),
            AgentCheck(
                "Real user value.",
                "Whether the change solves the underlying problem from the user's perspective, "
                "not merely the letter of the ticket.",
            ),
            AgentCheck(
                "Under-delivery.",
                "Placeholders, obviously-wrong defaults, and corner cases that will bite "
                "immediately get rejected.",
            ),
            AgentCheck(
                "Gold-plating.",
                "Unrequested extra surface that adds risk and maintenance for no agreed value "
                "gets rejected too.",
            ),
        ),
        crew_fit=CrewFit(
            paragraphs=(
                "The product manager books the ends of a run: clarifying requirements with the "
                "`architect` during planning, then accepting or rejecting the finished PR after "
                "the `sdet` has verified it. A REJECT lists the specific unmet criteria and "
                "routes the work back to the `senior-engineer`.",
            ),
            related=("sdet", "architect", "senior-engineer"),
        ),
    ),
    "ux-ui-designer": AgentCopy(
        tagline="Specs & reviews on-screen UI — tokens, layout, focus, a11y.",
        what=(
            "The ux-ui designer works to the project's stated design system — matching it, "
            "never inventing a competing style. Before anything is built, it produces a design "
            "spec: text wireframes of every screen and state, a tokens plan, responsive layout, "
            "focus order, feedback, and WCAG 2.2 AA accessibility.",
            "After the build, it reviews the real interface against that spec, Nielsen's "
            "heuristics, and WCAG — stating its coverage up front and naming anything unseen as "
            "unreviewed. Layout is never trusted from a static read: the UI gets rendered, or "
            "the change is flagged as needing a human visual pass.",
        ),
        scenarios=(
            AgentScenario(
                "UI is about to be built",
                "The spec comes first: every screen and every state wireframed, tokens planned, "
                "layout responsive, and accessibility designed in rather than bolted on.",
            ),
            AgentScenario(
                "Built UI needs a verdict",
                "The interface gets reviewed against the spec, the heuristics, and the WCAG "
                "thresholds — findings grouped by category, each with evidence, the expected "
                "pattern, and a severity.",
            ),
            AgentScenario(
                "Consistency is drifting",
                "Token bypasses, components built two different ways, and rhythm that doesn't "
                "share an edge get named — framed by the confusion they cause, not consistency "
                "for its own sake.",
            ),
        ),
        checks=(
            AgentCheck(
                "Every state.",
                "Default, empty, loading, error, and success states for each screen — plus the "
                "coverage actually seen, stated before any verdict.",
            ),
            AgentCheck(
                "Token discipline.",
                "Shared design tokens as the single source of truth — raw colour, size, and "
                "spacing values bypassing the layer get rejected.",
            ),
            AgentCheck(
                "Responsive layout.",
                "Platform layout primitives and constraints, never hardcoded absolute "
                "positions, so the interface adapts across viewports.",
            ),
            AgentCheck(
                "Keyboard and focus.",
                "Full keyboard operability, a sensible focus order, and a visible focus "
                "indicator that nothing obscures.",
            ),
            AgentCheck(
                "WCAG 2.2 AA.",
                "Contrast ratios, target sizes, colour never the only signal, and honest "
                "reading order — checked against the thresholds.",
            ),
            AgentCheck(
                "Rendering honesty.",
                "Layout never trusted from a static read alone — render the real UI, or "
                "explicitly flag that a human visual pass is needed.",
            ),
        ),
        crew_fit=CrewFit(
            paragraphs=(
                "The ux-ui designer specs before the `senior-engineer` builds and reviews "
                "after, looping until the interface holds up. Rendered visuals beyond UI — "
                "imagery, brand, art — belong to the `art-director`, and acceptance against the "
                "product bar stays with the `product-manager`.",
            ),
            related=("senior-engineer", "art-director", "product-manager"),
        ),
    ),
    "art-director": AgentCopy(
        tagline="Directs & reviews rendered visuals — judges the picture.",
        what=(
            "The art director judges the produced output, not the source that made it. If the "
            "project has a render, export, or screenshot path, it runs it, looks at the result "
            "at real resolution, and critiques what it actually sees — anything not rendered is "
            "named as unreviewed.",
            "The judgement follows the fundamentals: value and readability first, then "
            "hierarchy and composition, colour, cohesion with the project's other assets, and "
            "craft. Before work begins, it specs a concrete, measurable direction — exact "
            "colour values, dimensions, light, and reference examples — so the result is what "
            "was meant, not a guess.",
        ),
        scenarios=(
            AgentScenario(
                "Visuals need a direction",
                "Before work starts, the art director writes a concrete direction — exact "
                "values, dimensions, light angle and intensity, and references — concrete "
                "enough to verify against.",
            ),
            AgentScenario(
                "Rendered work needs review",
                "The real artifact gets produced and viewed at real resolution, then judged on "
                "value, composition, colour, cohesion, and craft — with a decisive verdict.",
            ),
            AgentScenario(
                "Something looks imported",
                "Cohesion checks catch assets that don't sit in the project's visual world: "
                "shared palette, line weight, lighting, and scale.",
            ),
        ),
        checks=(
            AgentCheck(
                "The real artifact.",
                "The render or export actually produced and viewed at real resolution — never "
                "the source reviewed in its place.",
            ),
            AgentCheck(
                "Stated coverage.",
                "Exactly which assets were seen and at what resolution named before the verdict "
                "— anything unseen called unreviewed.",
            ),
            AgentCheck(
                "Value and readability.",
                "The composition still reads when detail blurs — strong light-dark structure "
                "and clear silhouettes over rendering polish.",
            ),
            AgentCheck(
                "Hierarchy and colour.",
                "A clear focal point and eye path; a deliberate, harmonious palette with "
                "consistent light direction.",
            ),
            AgentCheck(
                "Cohesion and craft.",
                "One visual world with the project's other assets, plus spacing, alignment, "
                "edge quality, and no unwanted repetition or artifacts.",
            ),
            AgentCheck(
                "No rubber-stamping.",
                "Loops run produce, look, critique, refine until genuinely signed off — never "
                "ended early to close the loop.",
            ),
        ),
        crew_fit=CrewFit(
            paragraphs=(
                "The art director is the visual counterpart to the `ux-ui-designer`: the "
                "designer owns interface usability and tokens, the art director owns the "
                "picture itself. Findings hand back to the `senior-engineer` or the asset's "
                "author as concrete fixes, blockers separated from nits — and a review loop "
                "never rubber-stamps early to end.",
            ),
            related=("ux-ui-designer", "senior-engineer", "product-manager"),
        ),
    ),
    "technical-writer": AgentCopy(
        tagline="Writes docs from the real code; proves them with a reader test.",
        what=(
            "The technical writer gets a reader to done, not to informed. It picks the right "
            "kind of doc for the job — tutorial, how-to guide, reference, or explanation — and "
            "writes it in the project's own voice and format, next to what it describes.",
            "Every claim is verified against the actual code: commands, paths, parameters, and "
            "output must match what the repo does today, and a fresh reader following the steps "
            "verbatim must reach the stated result. When reviewing docs, a factually wrong or "
            "non-completable instruction is blocking.",
        ),
        scenarios=(
            AgentScenario(
                "A change needs docs",
                "READMEs, how-tos, reference pages, changelogs, and migration guides get "
                "written from the real source — ready to commit, in the repo's format.",
            ),
            AgentScenario(
                "Docs might have drifted",
                "Every claim gets checked against the code and every procedure walked — drift, "
                "broken samples, and missing prerequisites get flagged with specific fixes.",
            ),
            AgentScenario(
                "A doc type is blurry",
                "Tutorials, how-tos, reference, and explanation serve different readers; the "
                "writer splits them instead of blending one doc that serves none.",
            ),
        ),
        checks=(
            AgentCheck(
                "Zero drift.",
                "Every command, path, parameter, and output read from the real source and "
                "matching what the repo actually does today.",
            ),
            AgentCheck(
                "The reader test.",
                "A fresh reader with no prior context, following the steps verbatim, must reach "
                "the stated result — executed personally where possible.",
            ),
            AgentCheck(
                "The right doc type.",
                "Tutorial, how-to, reference, or explanation chosen for the reader's goal — "
                "never blended into one doc.",
            ),
            AgentCheck(
                "Minimalism.",
                "The least that gets the reader to done — throat-clearing, obvious statements, "
                "and duplication cut.",
            ),
            AgentCheck(
                "Consistent terminology.",
                "One name per concept, matching the code and UI — terms defined once, never "
                "drifting synonyms.",
            ),
            AgentCheck(
                "Honest changelogs.",
                "Breaking changes and the upgrade path stated plainly; scannable structure with "
                "descriptive link text and alt text.",
            ),
        ),
        crew_fit=CrewFit(
            paragraphs=(
                "The technical writer runs near the end of a change, turning the "
                "`senior-engineer`'s verified diff into docs that match it, and reviews docs "
                "the way the `sdet` reviews code — by walking the steps. The `product-manager` "
                "holds docs to the Definition of Done like any other deliverable.",
            ),
            related=("senior-engineer", "product-manager", "sdet"),
        ),
    ),
    "data-scientist": AgentCopy(
        tagline="Data/model work — metrics, leakage, validation, reproducibility.",
        what=(
            "The data scientist engages where the actual deliverable is analysis or a model — "
            "experiments, pipelines, metrics, statistical claims. A model that scores well but "
            "answers the wrong question, or scores well only because of a leak, counts as a "
            "failure.",
            "Framing and metric choice come first, then the ways results lie: target leakage, "
            "train-test contamination, missing baselines, and evaluation on data the model has "
            "seen. Where feasible, the pipeline gets rerun or the metric recomputed rather than "
            "trusting the reported figure. On a conventional app with no data deliverable, it "
            "says so instead of inventing work.",
        ),
        scenarios=(
            AgentScenario(
                "A model claims a number",
                "The evaluation gets audited — an honest split, a sensible baseline, metrics on "
                "unseen data — before the number is believed.",
            ),
            AgentScenario(
                "An experiment needs designing",
                "Problem framing, metric choice, and validation strategy get set up front, "
                "matched to the real-world cost of the errors that matter.",
            ),
            AgentScenario(
                "Results look too good",
                "Target leakage and train-test contamination get hunted first — the number one "
                "way results lie.",
            ),
        ),
        checks=(
            AgentCheck(
                "Problem framing.",
                "The question well-posed and the metric reflecting real success — accuracy on "
                "imbalanced data is a trap.",
            ),
            AgentCheck(
                "Leakage.",
                "Features encoding the label or the future, tuning on the test set, duplicates, "
                "and shift between train and serve.",
            ),
            AgentCheck(
                "Honest evaluation.",
                "Held-out or cross-validated correctly, a sensible baseline to beat, and "
                "metrics reported only on unseen data.",
            ),
            AgentCheck(
                "Statistical soundness.",
                "Real effect or noise — sample size, variance across runs, multiple "
                "comparisons, and correlation not read as cause.",
            ),
            AgentCheck(
                "Bias and fairness.",
                "Where decisions affect people, disparate performance across relevant groups "
                "and unrepresentative training data.",
            ),
            AgentCheck(
                "Reproducibility.",
                "Fixed seeds, pinned data and dependencies, and a runnable path from raw data "
                "to result — an irreproducible number is a finding.",
            ),
        ),
        crew_fit=CrewFit(
            paragraphs=(
                "The data scientist is the crew's specialist for data-and-model deliverables — "
                "designing experiments in `/spike`, reviewing analysis and model changes in "
                "`/pr-review`. Findings hand to the `senior-engineer` as specific fixes, and "
                "anything outside data work routes back to the rest of the crew.",
            ),
            related=("senior-engineer", "sdet", "product-manager"),
        ),
    ),
}


# ---------------------------------------------------------------------------
# TOOL COPY — the authored editorial half of each tool page. Its factual half
# (name, description) is parsed from toolbox/<slug>/tool.md. TOOLS is the
# canonical order (homepage grid, sibling nav, sitemap); every toolbox/<slug>/
# on disk must appear here and vice versa (drift raises in load_tools).
# ---------------------------------------------------------------------------

TOOL_COPY_SRC = "tools/gen_command_pages.py"

# Canonical tool order — matches the homepage `#tools` grid.
TOOLS = ("termgif", "social-card", "pixelart", "svgflow", "badge", "sparkline", "scrub", "fixtures")

TOOL_COPY = {
    "termgif": ToolCopy(
        tagline="Renders an animated terminal demo GIF of a workflow run from a small JSON spec.",
        what=(
            "`termgif` turns a small JSON spec — a typed prompt, staged progress with "
            "check-offs, and a closing line — into a polished animated terminal GIF. It is a "
            "self-contained Python renderer that provisions its one dependency (Pillow) itself, so a run produces "
            "the recording deterministically rather than asking anyone to capture their screen.",
            "It is a *tool*, not a command: you never type it. The crew reach for it on their "
            "own when the natural artifact of a task is a terminal recording — a README hero, a "
            "docs page, a release note, or a social preview — instead of static text.",
        ),
        usage=(
            "Tools are opt-in. Add it at install time with `shipmates install --harness <name> "
            "--with-tools termgif`, or omit the flag and pick it from the interactive list. Once "
            "installed it sits alongside the crew as a capability they invoke implicitly; there "
            "is no slash command to type.",
            "The renderer and its instructions are bundled together, so wherever the tool lands "
            "the agent has both the `termgif.py` script and the spec format it needs to drive it.",
        ),
        # These three are just termgif driving different JSON specs — nothing to do
        # with shipmates. Each spec lives beside its GIF under examples/.
        examples=(
            ToolExample(
                src="examples/deploy.gif", poster="examples/deploy.png", width=820, height=370,
                alt="Terminal recording of a production deploy: BUILD, TEST and RELEASE stages check off, then a green 'Shipped v2.4.0 to production' line.",
                caption="A production deploy — build, test, then a zero-downtime rollout to three regions.",
            ),
            ToolExample(
                src="examples/train.gif", poster="examples/train.png", width=820, height=370,
                alt="Terminal recording of a model training run: DATASET, TRAIN and EVAL stages, a top-1 98.7% metric line, then a green checkpoint-saved line.",
                caption="A model-training run — dataset to eval, with the headline metrics and a saved checkpoint.",
            ),
            ToolExample(
                src="examples/launch.gif", poster="examples/launch.png", width=820, height=370,
                alt="Terminal recording of a rocket launch sequence: FUEL, GUIDANCE and IGNITION stages, a T-minus countdown, then a green liftoff line.",
                caption="A launch countdown — fuel, guidance, ignition, and liftoff, in warm mission-control colours.",
            ),
        ),
    ),
    "social-card": ToolCopy(
        tagline="Renders a shippable 1280×640 social / Open Graph preview card from a small JSON spec.",
        what=(
            "`social-card` turns a small JSON spec — an eyebrow kicker, a title, a subtitle, an accent colour, and a footer wordmark — into a polished 1280×640 PNG, the standard Open Graph aspect that Slack, Discord, Twitter, LinkedIn, and iMessage crop from a shared link. It is a self-contained Python renderer that provisions its one dependency (Pillow) itself the first time it runs, so a run produces the card deterministically on a deep, tasteful dark canvas — nothing to install, no design app to open.",
            "It is a *tool*, not a command: you never type it. The crew reach for it on their own when the natural artifact of a task is *a share image* — a repo or product launch, a release, a docs or guide page — instead of a described one. Long titles wrap and the type auto-fits, so copy of any length stays inside the frame.",
        ),
        usage=(
            "Tools are opt-in. Add it at install time with `shipmates install --harness <name> --with-tools social-card`, or omit the flag and pick it from the interactive list. Once installed it sits alongside the crew as a capability they invoke implicitly; there is no slash command to type.",
            "The renderer and its instructions are bundled together, so wherever the tool lands the agent has both the `social_card.py` script and the spec format it needs to drive it — `python3 social_card.py --spec spec.json --out card.png`.",
        ),
        examples=(
            ToolExample(
                src="examples/launch.png", width=1280, height=640,
                alt="A dark launch card: an indigo 'Now open source' pill above a large white title 'Aurora — a typed HTTP client that reads like prose', a muted subtitle, and a footer repo URL.",
                caption="A product launch — an eyebrow pill, a wrapping headline, and the repo in the footer.",
            ),
            ToolExample(
                src="examples/release.png", width=1280, height=640,
                alt="A dark release card: a green 'Release · v2.4.0' pill above the title 'Streaming responses, 40% smaller binary', a subtitle of highlights, and a green footer dot with a changelog line.",
                caption="A release note — the version in the kicker and the headline changes below it.",
            ),
            ToolExample(
                src="examples/docs.png", width=1280, height=640,
                alt="A dark docs card: an orange 'Guide' pill above the title 'Deploying to production, end to end', a muted one-line summary, and a footer docs URL with an orange accent dot.",
                caption="A docs guide — a short kicker, the guide's title, and the page it links to.",
            ),
        ),
    ),
    "pixelart": ToolCopy(
        tagline="Pixel art the way the shipmates logo is made — a limited palette upscaled by a whole-number factor with nearest-neighbour, never smoothed.",
        what=(
            "`pixelart` turns a small JSON spec — a limited palette plus a grid of single-character rows — into a crisp pixel-art asset: a static PNG, or an animated GIF when the spec carries `frames`. It is a self-contained Python renderer that provisions its one dependency (Pillow) itself the first time it runs, so a run produces the image deterministically — nothing to install — rather than leaning on a model to imagine one.",
            "It reproduces the technique behind the shipmates logo, which is a 48×48 grid on ~39 colours scaled up ×14 with nearest-neighbour and no smoothing. Every upscale here is `Image.NEAREST`, so each logical pixel becomes a solid block — hard-edged and aliased on purpose, never a gradient. It is a tool, not a command: the crew reach for it on their own when the natural artifact is an icon, sprite, favicon, or badge in that style.",
        ),
        usage=(
            "Tools are opt-in. Add it at install time with `shipmates install --harness <name> --with-tools pixelart`, or omit the flag and pick it from the interactive list. Once installed it sits alongside the crew as a capability they invoke implicitly; there is no slash command to type.",
            "The renderer and its instructions travel together, so wherever the tool lands the agent has both the `pixelart.py` script and the spec format it needs to drive it — a `grid` and `palette` for a still, or `frames` and `durations` for an animation, with `--poster` to drop a reduced-motion still beside a GIF.",
        ),
        examples=(
            ToolExample(
                src="examples/harbour.png", width=672, height=528,
                alt="A pixel-art sunset seascape: a banded amber sky, a glowing low sun with its light glittering on a deep-blue sea, a small sailboat in silhouette on the horizon, and a few birds.",
                caption="A sunset voyage — a banded sky, a glowing sun glittering on the sea, and a sailboat on the horizon, in ~40 colours.",
            ),
            ToolExample(
                src="examples/planet.png", width=576, height=576,
                alt="A pixel-art ringed gas giant: a banded amber planet shaded like a lit sphere with a dark limb, encircled by a thin tilted ring, over a dark starfield.",
                caption="A ringed gas giant — a banded atmosphere shaded as a lit sphere, a tilted ring, and a starfield.",
            ),
            ToolExample(
                src="examples/mountain.png", width=660, height=462,
                alt="A pixel-art night scene: a glowing moon and stars over three depth-layered mountain silhouettes, a snow-capped central peak, and two pine trees.",
                caption="A moonlit mountain night — depth-layered ridges, a clean snow cap, and pines under a glowing moon.",
            ),
            ToolExample(
                src="examples/voyage.gif", width=682, height=440,
                alt="An animated pixel-art sailboat bobbing on scrolling waves under a warm sky, the low sun glittering on the water.",
                caption="A sailboat bobbing on scrolling waves — the animated cousin of the static seascape, an eight-frame loop.",
                poster="examples/voyage_poster.png",
            ),
            ToolExample(
                src="examples/lighthouse.gif", width=576, height=576,
                alt="An animated pixel-art lighthouse at night, its lamp sweeping a bright warm beam back and forth across a starry sky above a shimmering sea.",
                caption="A lighthouse sweeping its beam across the night — a bright additive light cone over a shimmering sea.",
                poster="examples/lighthouse_poster.png",
            ),
            ToolExample(
                src="examples/shootingstar.gif", width=720, height=432,
                alt="An animated pixel-art night sky with a crescent moon and twinkling stars, a shooting star streaking across with a glowing tail.",
                caption="A shooting star crossing a twinkling sky past a crescent moon — a glowing tail, looped over eight frames.",
                poster="examples/shootingstar_poster.png",
            ),
        ),
    ),
    "svgflow": ToolCopy(
        tagline="Draws a small flow, pipeline, or state diagram as a committed SVG from a tiny JSON spec.",
        what=(
            "`svgflow` turns a small JSON spec — a list of `nodes` and the `edges` between them — into a clean box-and-arrow diagram, emitted as a self-contained SVG. There is no browser, no mermaid runtime, and no headless renderer: the SVG is assembled as text and is byte-for-byte deterministic, so the same spec always produces the same file and it is safe to commit next to the docs it illustrates.",
            "It does one thing well. Nodes lay out in a single column (`\"direction\": \"down\"`) or row (`\"direction\": \"right\"`) in declaration order, each box sized to its label; adjacent nodes join with a straight spine arrow while skips, back-edges, and self-loops bow out to the side so they stay legible. It is a *tool*, not a command: the crew reach for it on their own when the natural artifact of a task is a diagram of how the pieces connect — a CI pipeline, a request path, a state machine — instead of a paragraph of prose or a mermaid block something else has to render.",
        ),
        usage=(
            "Tools are opt-in. Add it at install time with `shipmates install --harness <name> --with-tools svgflow`, or omit the flag and pick it from the interactive list. Once installed it sits alongside the crew as a capability they invoke implicitly; there is no slash command to type.",
            "The renderer depends only on the Python standard library, so wherever the tool lands the agent can run `python3 svgflow.py --spec spec.json --out flow.svg` with nothing to install. The `svgflow.py` script and the spec format it needs are bundled together in `tool.md`.",
        ),
        examples=(
            ToolExample(
                src="examples/pipeline.svg", width=274, height=384,
                alt="A vertical flow diagram titled 'CI pipeline': Build, Test and Deploy boxes joined by downward teal arrows labelled 'on push' and 'green', with a back-edge from Test to Build labelled 'red → retry'.",
                caption="A CI pipeline as a column — build to deploy, with a labelled retry edge looping back on failure.",
            ),
            ToolExample(
                src="examples/request.svg", width=830, height=203,
                alt="A left-to-right flow diagram titled 'Request flow': Client, API gateway, Service and Database boxes joined by teal arrows labelled 'HTTPS', 'authz' and 'query', with a return edge from Service to Client labelled '200 JSON'.",
                caption="A request path as a row — client through gateway and service to the database, and the response back.",
            ),
            ToolExample(
                src="examples/state.svg", width=354, height=508,
                alt="A vertical state diagram titled 'Order state machine': Pending, Paid, Shipped and Delivered states joined by teal arrows, a back-edge from Shipped to Pending labelled 'cancelled', and a self-loop on Delivered labelled 're-notify'.",
                caption="An order state machine — the forward path plus a cancelled transition and a self-loop, all auto-routed.",
            ),
        ),
    ),
    "badge": ToolCopy(
        tagline="Turns a label, a message, and a colour into a shields-style flat badge you commit as offline SVG.",
        what=(
            "`badge` renders the classic two-segment flat status badge — a grey left segment carrying a `label`, a coloured right segment carrying a `message` — straight to an SVG file. It is pure standard library: no dependencies and no network, so there is no round-trip to `shields.io` and the badge renders forever, offline, and diffs cleanly in git. Output is deterministic, so the same arguments always produce byte-for-byte the same file.",
            "It is a *tool*, not a command: you never type it. The crew reach for it on their own when the natural artifact of a task is a small status badge — build `passing`, a `version` tag, a `coverage` number, a licence — for a README, a docs page, or a release note, and that badge should live in the repo rather than hot-linking an external service. Colours are a named palette (`green`, `blue`, `red`, `brightgreen`, `orange`, `yellow`, ...) or any `#rrggbb` hex value, and segment widths are sized from a baked DejaVu Sans width table and locked with SVG `textLength` so text is never clipped.",
        ),
        usage=(
            "Tools are opt-in. Add it at install time with `shipmates install --harness <name> --with-tools badge`, or omit the flag and pick it from the interactive list. Once installed it sits alongside the crew as a capability they invoke implicitly; there is no slash command to type.",
            "The renderer and its instructions are bundled together, so wherever the tool lands the agent has both the `badge.py` script and the CLI it needs to drive it: `python3 badge.py --label build --message passing --color green --out badge.svg` (or omit `--out` to write the SVG to stdout).",
        ),
        examples=(
            ToolExample(
                src="examples/build.svg", width=89, height=20,
                alt="A flat status badge: a grey 'build' segment on the left and a green 'passing' segment on the right.",
                caption="A build-status badge — grey label, green message, the classic shields flat style.",
            ),
            ToolExample(
                src="examples/version.svg", width=95, height=20,
                alt="A flat status badge: a grey 'version' segment on the left and a blue 'v0.1.3' segment on the right.",
                caption="A version badge — a release tag tinted blue, sized to fit its text exactly.",
            ),
            ToolExample(
                src="examples/coverage.svg", width=96, height=20,
                alt="A flat status badge: a grey 'coverage' segment on the left and a bright-green '98%' segment on the right.",
                caption="A coverage badge — a bright-green percentage, rendered offline and committed to the repo.",
            ),
        ),
    ),
    "sparkline": ToolCopy(
        tagline="Turns a short run of numbers into a tiny inline SVG that shows the trend at a glance.",
        what=(
            "`sparkline` takes a short series of numbers — `--data \"12,18,9,22,15,27\"` — and draws its trend as one tiny inline chart: a smooth polyline over a faint gradient fill, scaled to its own min/max, with the last point marked by a dot. The output is a self-contained SVG (scalable, self-sizing, verified as valid XML), a light teal stroke on a subtle dark panel that reads on the tool pages; `--color`, `--label`, `--width`, `--height`, `--no-baseline` and `--bare` tune it.",
            "It is a *tool*, not a command: you never type it. The crew reach for it on their own when a task hands over a run of numbers — a benchmark trend, a metrics readout, a latency/throughput/error-rate history — and the honest artifact is the shape of the curve rather than a wall of digits. It has no dependencies beyond `python3` and the standard library, so a run is deterministic and offline.",
        ),
        usage=(
            "Tools are opt-in. Add it at install time with `shipmates install --harness <name> --with-tools sparkline`, or omit the flag and pick it from the interactive list. Once installed it sits alongside the crew as a capability they invoke implicitly; there is no slash command to type.",
            "The renderer and its instructions are bundled together, so wherever the tool lands the agent has both the `sparkline.py` script and the CLI it needs to drive it: `python3 sparkline.py --data \"…\" --out spark.svg`.",
        ),
        examples=(
            ToolExample(
                src="examples/latency.svg", width=240, height=60,
                alt="A declining sparkline in coral: p95 latency falling from 182ms to 88ms across a release, the last point marked with a dot.",
                caption="A latency history that improves across a release — p95 falling from 182ms to 88ms.",
            ),
            ToolExample(
                src="examples/throughput.svg", width=240, height=60,
                alt="A rising sparkline in teal: throughput climbing from 420 to 845 requests per second, the last point marked with a dot.",
                caption="Throughput climbing as autoscaling kicks in — 420 to 845 req/s.",
            ),
            ToolExample(
                src="examples/error-rate.svg", width=240, height=60,
                alt="A sparkline in gold with a single sharp spike: error rate jumping to 2.1% during an incident, then recovering to 0.2%.",
                caption="An error-rate spike during an incident, then recovery — a lone peak at 2.1%.",
            ),
        ),
    ),
    "scrub": ToolCopy(
        tagline="Strips credentials and personal data out of text before it leaves the repo.",
        what=(
            "`scrub` redacts secrets and PII from a log, paste, or bug report before it's shared. It swaps each match for a typed placeholder — emails become `[REDACTED_EMAIL]`, AWS access keys `[REDACTED_AWS_KEY]`, `api_key=`/`token=`/`secret=` assignments and Bearer tokens `[REDACTED_TOKEN]`, JWTs `[REDACTED_JWT]`, IPv4 addresses `[REDACTED_IP]`, and PEM private-key blocks `[REDACTED_PRIVATE_KEY]` — so the shape of the redaction stays visible. It is pure standard library: no dependencies, no network, and the same input always produces the same cleaned output.",
            "It is a *tool*, not a command: you never type it. The crew reach for it on their own whenever text is about to leave the repo — pasted into an issue, a bug report, a chat message, or a PR body — and might carry a credential or someone's personal data. The detectors are deliberately conservative, leaving ordinary prose and code identifiers like `token = response.data.token` alone, which makes it a strong first pass rather than a guarantee — read the cleaned text before sharing.",
        ),
        usage=(
            "Tools are opt-in. Add it at install time with `shipmates install --harness <name> --with-tools scrub`, or omit the flag and pick it from the interactive list. Once installed it sits alongside the crew as a capability they invoke implicitly; there is no slash command to type.",
            "Run it on a file with `python3 scrub.py --in log.txt --out clean.txt`, or pipe text through it stdin-to-stdout with `cat log.txt | python3 scrub.py`. The cleaned text goes to `--out` or stdout; a per-category redaction summary goes to stderr so it never pollutes the output. Exit code is `0` on success and `2` on a usage error.",
        ),
        examples=(
            ToolExample(
                src="examples/scrub.gif", width=860, height=340,
                alt="Terminal recording of scrub redacting a log: an email, AWS key, bearer token, JWT and IP each replaced with a typed [REDACTED_*] placeholder, ending 'redacted 9 items across 7 categories'.",
                caption="scrub cleaning a log — each secret class swapped for a typed placeholder, counted at the end.",
                poster="examples/scrub_poster.png",
            ),
        ),
        sample="$ python3 scrub.py --in app.log\n\nBEFORE\n  env AWS_ACCESS_KEY_ID=AKIAIOSFODNN7EXAMPLE\n  operator jane.doe@example.com from 203.0.113.42\n  Authorization: Bearer abcDEF0123456789xyzABCDEF\n\nAFTER  (summary on stderr)\n  env AWS_ACCESS_KEY_ID=[REDACTED_AWS_KEY]\n  operator [REDACTED_EMAIL] from [REDACTED_IP]\n  Authorization: Bearer [REDACTED_TOKEN]",
    ),
    "fixtures": ToolCopy(
        tagline="Turns a tiny field-to-type schema into a reproducible JSON array of believable fake records.",
        what=(
            "`fixtures` reads a small JSON schema — a map of field name to type, like `uuid`, `email`, `bool`, `int` or `choice` — and emits a JSON array of that many fake records. It is a self-contained Python script with no dependencies beyond the standard library, and it is deterministic: the same `--seed` always produces byte-identical output, so a generated fixture can be committed to the repo and diff cleanly like any other source file.",
            "It is a *tool*, not a command: you never type it. The crew reach for it on their own when a task needs seed rows or example records — unit-test data, a demo database, an API sample payload — instead of hand-typing objects or pulling in a heavyweight faker dependency. The data is fake and drawn independently per field, meant to exercise code paths rather than model any real-world distribution.",
        ),
        usage=(
            "Tools are opt-in. Add it at install time with `shipmates install --harness <name> --with-tools fixtures`, or omit the flag and pick it from the interactive list. Once installed it sits alongside the crew as a capability they invoke implicitly; there is no slash command to type.",
            "The script and its instructions are bundled together, so wherever the tool lands the agent has both the `fixtures.py` generator and the schema format it needs to drive it. Run it as `python3 fixtures.py --schema schema.json --count 5 --seed 7 --out data.json`, or omit `--out` to write the array to stdout.",
        ),
        examples=(
            ToolExample(
                src="examples/fixtures.gif", width=900, height=370,
                alt="Terminal recording of fixtures generating three JSON user records from a schema with --seed 7, ending 'same seed produces the same bytes'.",
                caption="fixtures generating three records from a schema — deterministic, so the same seed yields the same bytes.",
                poster="examples/fixtures_poster.png",
            ),
        ),
        sample="schema.json\n{\n  \"id\":    \"uuid\",\n  \"email\": \"email\",\n  \"age\":   {\"type\": \"int\", \"min\": 18, \"max\": 65},\n  \"role\":  {\"type\": \"choice\", \"options\": [\"admin\", \"member\"]}\n}\n\n$ python3 fixtures.py --schema schema.json --count 2 --seed 7\n[\n  {\n    \"id\": \"6513270e-269e-4d37-b2a7-4de452e6b438\",\n    \"email\": \"priya.patel@test.dev\",\n    \"age\": 52,\n    \"role\": \"admin\"\n  },\n  {\n    \"id\": \"e8e25d94-0ed9-4475-9531-985d5d9dc9f8\",\n    \"email\": \"ivan.haddad@example.com\",\n    \"age\": 23,\n    \"role\": \"member\"\n  }\n]",
    ),
}


# ---------------------------------------------------------------------------
# Layer 3 — PARSE  (skills/*/SKILL.md + agents/*.md -> model)
# No markup literals, no escaping, no writing.
# ---------------------------------------------------------------------------

# The Agent Skills standard's required keys — present, and first, in this order.
REQUIRED_FRONTMATTER_KEYS = ("name", "description")
# The standard's optional keys, then the Claude Code extensions it does not
# define. Both groups are permitted in any order; only `argument-hint` reaches a
# page, the rest are parsed so a standard-legal SKILL.md renders instead of
# raising. Keep in step with tools/validate_skills.py.
STANDARD_OPTIONAL_FRONTMATTER_KEYS = ("license", "compatibility", "metadata")
EXTENSION_FRONTMATTER_KEYS = ("argument-hint", "allowed-tools", "disable-model-invocation", "arguments", "loop_max", "stages", "invocation", "board")
ALLOWED_FRONTMATTER_KEYS = (
    REQUIRED_FRONTMATTER_KEYS
    + STANDARD_OPTIONAL_FRONTMATTER_KEYS
    + EXTENSION_FRONTMATTER_KEYS
)
# Canonical order for the keys the nine ship — recommended, not enforced past the
# required prefix. Drives the remedies below.
FRONTMATTER_KEYS = REQUIRED_FRONTMATTER_KEYS + EXTENSION_FRONTMATTER_KEYS
FRONTMATTER_KEY_LIST = ", ".join(FRONTMATTER_KEYS)
REQUIRED_FRONTMATTER_KEY_LIST = ", ".join(REQUIRED_FRONTMATTER_KEYS)
ALLOWED_FRONTMATTER_KEY_LIST = ", ".join(ALLOWED_FRONTMATTER_KEYS)
# `metadata:` is the one standard key defined as a nested mapping, so its value
# may live on indented continuation lines. They are consumed and ignored — this
# is a line-oriented reader, not a YAML parser, and no page reads them.
BLOCK_FRONTMATTER_KEY = "metadata"
INDENTED_RE = re.compile(r"^[ \t]")

# Agent frontmatter (crew/<role>.md): name and description required and first,
# in that order; `tools` optional. No other keys — a typo fails here rather than
# reaching a page in silence, same rule as the skill parser.
AGENT_REQUIRED_FRONTMATTER_KEYS = ("name", "description")
AGENT_FRONTMATTER_KEYS = AGENT_REQUIRED_FRONTMATTER_KEYS + ("tools", "capabilities", "writes", "web-scopes", "read-scopes", "tool-order")
AGENT_FRONTMATTER_KEY_LIST = ", ".join(AGENT_FRONTMATTER_KEYS)

# Agent Skills limits on the two standard keys.
SKILL_NAME_RE = re.compile(r"^[a-z0-9]+(-[a-z0-9]+)*$")
MAX_SKILL_NAME = 64
MAX_SKILL_DESCRIPTION = 1024

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
# A role opening a list item / table cell, optionally bold: `role` or **`role`**.
LEADING_ROLE_RE = re.compile(r"^\*{0,2}`([^`]+)`\*{0,2}")
# What can chain two roles named together ("`a` or `b`", "`a`, `b`"), consumed between hits.
ROLE_CHAIN_SEP_RE = re.compile(r"^\s*(?:,|/|or|and)\s*", re.IGNORECASE)
GATE_MARK = "⛔"  # no-entry sign; introduces a stage's hard gate
TRAILING_PAREN_RE = re.compile(r"[ ]*\(([^()]*)\)[ ]*$")
AGENT_WORD_RE = re.compile(r"\bagents?\b")
LINK_RE = re.compile(r"!\[|\[[^\]]*\]\(|\[[^\]]*\]\[|\[\^|~~")
LASTMOD_RE = re.compile(r"^[0-9]{4}-[0-9]{2}-[0-9]{2}$")

LINKS_UNSUPPORTED = (
    "links are not supported in skill sources — write the target as inline code instead"
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


def parse_agent(path: Path) -> AgentFrontmatter:
    """The frontmatter of one crew/<role>.md."""
    slug = path.stem
    src = f"crew/{path.name}"
    lines = path.read_text(encoding="utf-8").split("\n")
    if not lines or lines[0].strip() != "---":
        raise SourceError(
            src, 1, "missing frontmatter fence", lines[0] if lines else "",
            "an agent file must open with a `---` line",
        )
    values = {}
    order = []
    for i in range(1, len(lines)):
        raw = lines[i]
        lineno = i + 1
        if raw.strip() == "---":
            missing = [k for k in AGENT_REQUIRED_FRONTMATTER_KEYS if k not in values]
            if missing:
                raise SourceError(
                    src, lineno, f"frontmatter is missing {missing[0]}", raw,
                    f"an agent file requires {', '.join(AGENT_REQUIRED_FRONTMATTER_KEYS)}, "
                    "in that order, at the top of the block (we ship "
                    f"{AGENT_FRONTMATTER_KEY_LIST})",
                )
            if tuple(order[: len(AGENT_REQUIRED_FRONTMATTER_KEYS)]) != AGENT_REQUIRED_FRONTMATTER_KEYS:
                raise SourceError(
                    src, _key_lineno(lines, i, order[0] if order else "name"),
                    "frontmatter does not open with "
                    f"{', '.join(AGENT_REQUIRED_FRONTMATTER_KEYS)}", raw,
                    "move name and description to the top of the block, in that order; "
                    "tools follows",
                )
            fm = AgentFrontmatter(
                name=values["name"],
                description=values["description"],
                # No fallback to `capabilities`. The generator reads a rendered
                # payload, where `tools:` always carries the harness's real tool
                # names; falling back would publish semantic capability names
                # ("read, bash") as if they were tools, which is what #158 was.
                tools=tuple(
                    t.strip() for t in values["tools"].split(",") if t.strip()
                ),
            )
            lineno = _key_lineno(lines, i, "name")
            if not SKILL_NAME_RE.match(fm.name):
                raise SourceError(
                    src, lineno, "frontmatter name is not a slug", lines[lineno - 1],
                    "agent names use lowercase letters, digits and single hyphens "
                    "(`^[a-z0-9]+(-[a-z0-9]+)*$`) — rename the agent and its file to match",
                )
            if fm.name != slug:
                raise SourceError(
                    src, lineno, "frontmatter name does not match the file name", lines[lineno - 1],
                    f"the `name:` key must equal the file name — write `name: {slug}` here, "
                    f"or rename the file to crew/{fm.name}.md",
                )
            return fm
        if not raw.strip():
            continue
        if INDENTED_RE.match(raw):
            raise SourceError(
                src, lineno, "indented frontmatter line", raw,
                "write every entry on a single unindented `key: value` line",
            )
        key, sep, value = raw.partition(":")
        if not sep or key.strip() not in AGENT_FRONTMATTER_KEYS:
            raise SourceError(
                src, lineno, "unknown frontmatter key", raw,
                f"expected one of {AGENT_FRONTMATTER_KEY_LIST}",
            )
        key = key.strip()
        if key in values:
            raise SourceError(
                src, lineno, f"duplicate frontmatter key {key}", raw,
                "declare each frontmatter key exactly once",
            )
        values[key] = value.strip()
        order.append(key)
    raise SourceError(
        src, len(lines), "unterminated frontmatter fence", "",
        "close the frontmatter with a `---` line before the system-prompt body",
    )


def check_agent_copy(role: str, copy: AgentCopy) -> None:
    """Fail-fast shape rules on the authored copy — the UX spec's bounds, enforced
    at build time so a copy defect can never ship on a page."""
    src = AGENT_COPY_SRC

    def bad(what: str, remedy: str) -> None:
        raise SourceError(src, 1, f"AGENT_COPY['{role}']: {what}", "", remedy)

    if not copy.tagline.strip():
        bad("empty tagline", "give the agent its one-line tagline (it matches the homepage crew card)")
    if not 1 <= len(copy.what) <= 2:
        bad(f"{len(copy.what)} 'what' paragraphs", "the spec allows 1-2 paragraphs")
    if not 2 <= len(copy.scenarios) <= 3:
        bad(f"{len(copy.scenarios)} scenarios", "the spec allows 2-3 scenario cards")
    for scenario in copy.scenarios:
        words = len(scenario.title.split())
        if not 3 <= words <= 6:
            bad(
                f"scenario title {scenario.title!r} is {words} words",
                "scenario titles are 3-6 words",
            )
        if not scenario.desc.strip():
            bad(f"scenario {scenario.title!r} has an empty description", "write 1-2 sentences")
    if not 5 <= len(copy.checks) <= 8:
        bad(f"{len(copy.checks)} checks", "the spec allows 5-8 check bullets")
    for check in copy.checks:
        if not check.lead.strip():
            bad("a check has an empty lead-in", "bold lead-in, then the explanation")
        words = len(check.text.split())
        if words > 25:
            bad(f"check {check.lead!r} runs {words} words", "check explanations are at most 25 words")
    if not 1 <= len(copy.crew_fit.paragraphs) <= 2:
        bad(f"{len(copy.crew_fit.paragraphs)} crew-fit paragraphs", "the spec allows 1-2")
    if not copy.crew_fit.related:
        bad("no related roles", "name the roles it hands off to or works alongside")
    for rel in copy.crew_fit.related:
        if rel not in AGENT_ROLES:
            bad(f"related role {rel!r} is not a known agent", f"use one of: {', '.join(AGENT_ROLES)}")


def derive_called_by(skills_dir: Path, roles: tuple) -> dict:
    """role -> slugs of the commands mentioning the role."""
    called = {role: [] for role in roles}
    for slug in SLUGS:
        path = skills_dir / f"{slug}.md"
        if not path.is_file():
            path = skills_dir / slug / "SKILL.md"
        if not path.is_file():
            continue
        text = path.read_text(encoding="utf-8")
        for role in roles:
            if re.search(r"\b" + re.escape(role) + r"\b", text):
                called[role].append(slug)
    return {role: tuple(slugs) for role, slugs in called.items()}


def load_agents(agents_dir: Path, skills_dir: Path) -> tuple:
    """Every agents/<role>.md joined with its AGENT_COPY entry and the derived
    called_by, in canonical AGENT_ROLES order. Raises on any drift between the
    disk, the copy table, and AGENT_ROLES — in every direction."""
    on_disk = {p.stem: p for p in sorted(agents_dir.glob("*.md"))}
    for role in sorted(on_disk):
        if role not in AGENT_COPY:
            raise SourceError(
                f"agents/{role}.md", 1, "agent file has no AGENT_COPY entry", "",
                f"agents/{role}.md exists but AGENT_COPY in tools/gen_command_pages.py has no "
                f"'{role}' entry — add the editorial copy (tagline, what, scenarios, checks, "
                "crew_fit), then rerun the generator and commit site/",
            )
    for role in sorted(AGENT_COPY):
        if role not in on_disk:
            raise SourceError(
                "tools/gen_command_pages.py", 1, "AGENT_COPY entry has no agent file", "",
                f"AGENT_COPY lists '{role}' but agents/{role}.md does not exist — write the "
                "agent file or remove the copy entry, then rerun the generator and commit site/",
            )
    for role in sorted(AGENT_ROLES):
        if role not in on_disk:
            raise SourceError(
                f"agents/{role}.md", 1, "AGENT_ROLES entry has no agent file", "",
                f"AGENT_ROLES lists {role} but agents/{role}.md does not exist — remove it from "
                "AGENT_ROLES in tools/gen_command_pages.py, delete site/agents/" + role + "/, "
                "then rerun the generator and commit site/",
            )
    parsed = {role: parse_agent(path) for role, path in on_disk.items()}
    called_by = derive_called_by(skills_dir, tuple(parsed))
    agents = []
    for role in AGENT_ROLES:
        if role not in parsed:
            raise SourceError(
                f"agents/{role}.md", 1, "agent file is not in AGENT_ROLES", "",
                f"agents/{role}.md is not in AGENT_ROLES — add it to AGENT_ROLES in "
                "tools/gen_command_pages.py (canonical order drives the sitemap and the "
                "sibling nav), then rerun the generator and commit site/",
            )
        copy = AGENT_COPY[role]
        check_agent_copy(role, copy)
        fm = parsed[role]
        agents.append(
            Agent(
                slug=role,
                source_path=f"crew/{role}.md",
                name=fm.name,
                description=fm.description,
                tools=fm.tools,
                tagline=copy.tagline,
                what=copy.what,
                scenarios=copy.scenarios,
                checks=copy.checks,
                crew_fit=copy.crew_fit,
                called_by=called_by[role],
            )
        )
    return tuple(agents)


def parse_tool_frontmatter(path: Path) -> tuple:
    """Read `name` + `description` from a toolbox `tool.md` frontmatter block."""
    rel = f"toolbox/{path.parent.name}/tool.md"
    lines = path.read_text(encoding="utf-8").split("\n")
    if not lines or lines[0].strip() != "---":
        raise SourceError(rel, 1, "tool.md has no frontmatter", lines[0] if lines else "",
                          "start the file with a `---` frontmatter block carrying `name` and `description`")
    fm, closed = {}, False
    for raw in lines[1:]:
        if raw.strip() == "---":
            closed = True
            break
        if ":" in raw:
            key, val = raw.split(":", 1)
            fm[key.strip()] = val.strip()
    if not closed:
        raise SourceError(rel, 1, "tool.md frontmatter is not closed", "", "close the frontmatter with a `---` line")
    for key in ("name", "description"):
        if not fm.get(key):
            raise SourceError(rel, 1, f"tool.md frontmatter missing `{key}`", "", f"add a `{key}:` line to the frontmatter")
    return fm["name"], fm["description"]


def load_tools(toolbox_dir: Path) -> tuple:
    """Every toolbox/<slug>/tool.md joined with its TOOL_COPY entry, in TOOLS
    order. Raises on drift between disk, TOOL_COPY, and TOOLS — every direction,
    the same contract load_agents holds for the crew."""
    on_disk = (
        {p.name: p for p in sorted(toolbox_dir.iterdir()) if p.is_dir() and (p / "tool.md").is_file()}
        if toolbox_dir.is_dir() else {}
    )
    for slug in sorted(on_disk):
        if slug not in TOOL_COPY:
            raise SourceError(
                f"toolbox/{slug}/tool.md", 1, "tool has no TOOL_COPY entry", "",
                f"toolbox/{slug}/ exists but TOOL_COPY in tools/gen_command_pages.py has no "
                f"'{slug}' entry — add the editorial copy (tagline, what, usage), then rerun and commit site/",
            )
    for slug in sorted(TOOL_COPY):
        if slug not in on_disk:
            raise SourceError(
                "tools/gen_command_pages.py", 1, "TOOL_COPY entry has no tool", "",
                f"TOOL_COPY lists '{slug}' but toolbox/{slug}/tool.md does not exist — add the tool "
                "or remove the copy entry, then rerun and commit site/",
            )
    for slug in TOOLS:
        if slug not in on_disk:
            raise SourceError(
                "tools/gen_command_pages.py", 1, "TOOLS entry has no tool", "",
                f"TOOLS lists {slug} but toolbox/{slug}/tool.md does not exist — remove it from TOOLS, "
                "delete site/tools/" + slug + "/, then rerun and commit site/",
            )
    tools = []
    for slug in TOOLS:
        directory = on_disk[slug]
        name, description = parse_tool_frontmatter(directory / "tool.md")
        bundled = tuple(sorted(
            p.name for p in directory.iterdir() if p.is_file() and p.name != "tool.md"
        ))
        copy = TOOL_COPY[slug]
        tools.append(Tool(
            slug=slug,
            source_path=f"toolbox/{slug}/tool.md",
            name=name,
            description=description,
            tagline=copy.tagline,
            what=copy.what,
            usage=copy.usage,
            bundled=bundled,
            examples=copy.examples,
            sample=copy.sample,
        ))
    return tuple(tools)


def load_skills(skills_dir: Path, agents: tuple) -> tuple:
    """Every command, in canonical SLUGS order. Raises on any drift from SLUGS.

    Accepts either layout: flat `<slug>.md` (the authored `commands/` tree, and
    opencode's rendered payload) or nested `<slug>/SKILL.md` (Claude's rendered
    payload). The site is generated from a rendered payload, so it has to read
    the shape that target actually ships.
    """
    on_disk = {p.stem: p for p in sorted(skills_dir.glob("*.md"))}
    on_disk.update({p.parent.name: p for p in sorted(skills_dir.glob("*/SKILL.md"))})
    for slug in sorted(on_disk):
        if slug not in SLUGS:
            raise SourceError(
                f"commands/{slug}.md", 1, "command file is not in SLUGS", "",
                f"commands/{slug}.md is not in SLUGS — add it to SLUGS in "
                "tools/gen_command_pages.py (canonical order drives the sitemap, the sibling nav "
                "and the homepage cards), then rerun the generator and commit site/",
            )
    for slug in SLUGS:
        if slug not in on_disk:
            raise SourceError(
                f"commands/{slug}.md", 1, "SLUGS entry has no command file", "",
                f"SLUGS lists {slug} but commands/{slug}.md does not exist — remove it from "
                "SLUGS in tools/gen_command_pages.py, delete site/commands/" + slug + "/, "
                "then rerun the generator and commit site/",
            )
    return tuple(parse_skill(on_disk[slug], agents) for slug in SLUGS)


def split_frontmatter(lines: list, src: str, consumed: set) -> tuple:
    """Return (Frontmatter, index of the first line after the closing fence).

    `name` and `description` are required and must come first, in that order —
    the standard's own rule. Everything else in ALLOWED_FRONTMATTER_KEYS is
    optional and unordered, so a SKILL.md carrying the standard's `license`,
    `compatibility` or `metadata` parses here instead of raising. Unknown keys
    still raise: a typo no reader honours must not reach a page in silence.
    """
    if not lines or lines[0].strip() != "---":
        raise SourceError(
            src, 1, "missing frontmatter fence", lines[0] if lines else "",
            "a skill file must open with a `---` line",
        )
    consumed.add(1)
    values = {}
    order = []
    for i in range(1, len(lines)):
        raw = lines[i]
        lineno = i + 1
        if raw.strip() == "---":
            consumed.add(lineno)
            missing = [k for k in REQUIRED_FRONTMATTER_KEYS if k not in values]
            if missing:
                raise SourceError(
                    src, lineno, f"frontmatter is missing {missing[0]}", raw,
                    f"the Agent Skills standard requires {REQUIRED_FRONTMATTER_KEY_LIST}, "
                    f"in that order, at the top of the block (we ship {FRONTMATTER_KEY_LIST})",
                )
            prefix = tuple(order[: len(REQUIRED_FRONTMATTER_KEYS)])
            if prefix != REQUIRED_FRONTMATTER_KEYS:
                first = next(
                    k for k, want in zip(order, REQUIRED_FRONTMATTER_KEYS) if k != want
                )
                raise SourceError(
                    src, _key_lineno(lines, i, first), "frontmatter does not open with "
                    f"{REQUIRED_FRONTMATTER_KEY_LIST}", raw,
                    f"move {REQUIRED_FRONTMATTER_KEY_LIST} to the top of the block, in "
                    "that order; the optional keys follow in any order",
                )
            return (
                Frontmatter(
                    name=values["name"],
                    description=values["description"],
                    argument_hint=values.get("argument-hint", ""),
                    allowed_tools=tuple(
                        t.strip()
                        for t in values.get("allowed-tools", "").split(",")
                        if t.strip()
                    ),
                ),
                i + 1,
            )
        if not raw.strip():
            continue
        if INDENTED_RE.match(raw):
            if order and order[-1] == BLOCK_FRONTMATTER_KEY:
                consumed.add(lineno)  # nested mapping under `metadata:`
                continue
            raise SourceError(
                src, lineno, "indented frontmatter line", raw,
                f"only `{BLOCK_FRONTMATTER_KEY}:` takes a nested block — write every "
                "other entry on a single unindented `key: value` line",
            )
        key, sep, value = raw.partition(":")
        if not sep or key.strip() not in ALLOWED_FRONTMATTER_KEYS:
            raise SourceError(
                src, lineno, "unknown frontmatter key", raw,
                f"expected one of {ALLOWED_FRONTMATTER_KEY_LIST}",
            )
        key = key.strip()
        if key in values:
            raise SourceError(
                src, lineno, f"duplicate frontmatter key {key}", raw,
                "declare each frontmatter key exactly once",
            )
        values[key] = value.strip()
        order.append(key)
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


def _leading_roles(text: str, agents: tuple) -> tuple:
    """Roles named at the very start of a list item / table cell, chained by 'or'/','/'and'.

    Empty if the text doesn't open on a known role — i.e. it isn't a declaration at all,
    just prose that happens to mention one later.
    """
    roles = []
    rest = text.lstrip()
    while True:
        hit = LEADING_ROLE_RE.match(rest)
        if not hit or hit.group(1) not in agents:
            break
        roles.append(hit.group(1))
        rest = rest[hit.end():]
        sep = ROLE_CHAIN_SEP_RE.match(rest)
        if not sep:
            break
        rest = rest[sep.end():]
    return tuple(roles)


def find_declared_crew(blocks: tuple, agents: tuple) -> tuple:
    """Crew a stage body *declares*, as distinct from one it merely *mentions* (C8).

    A list or a table row counts as a declaration only when every item / row opens on a
    role name — "`sdet` (always): ..." — rather than naming one in passing partway through
    a sentence — "dispatch a `senior-engineer` fixer". A block with any non-role-led item
    is enumerating something else (flags, config) and contributes nothing, even for the
    items that do happen to start with a role name.
    """
    found = []
    for block in blocks:
        if isinstance(block, ListBlock):
            roles = []
            for item in block.items:
                item_roles = _leading_roles(item.text, agents)
                if not item_roles:
                    roles = []
                    break
                roles.extend(item_roles)
            found.extend(roles)
        elif isinstance(block, Table):
            roles = []
            for row in block.rows:
                cell = row[0].strip() if row else ""
                hit = re.fullmatch(r"`([^`]+)`", cell)
                if not hit or hit.group(1) not in agents:
                    roles = []
                    break
                roles.append(hit.group(1))
            found.extend(roles)
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
                    "a skill file has exactly one `# /<command> — <tagline>` heading; "
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


def _key_lineno(lines: list, end: int, key: str) -> int:
    """1-based line of a frontmatter key already validated as present."""
    for i in range(end):
        if lines[i].partition(":")[0].strip() == key:
            return i + 1
    return 1


def check_frontmatter(fm: Frontmatter, slug: str, lines: list, end: int, src: str) -> None:
    """Agent Skills rules on `name` and `description`.

    Not in split_frontmatter: only the caller knows which directory the file
    came from, and `name` is only meaningful against that directory.
    """
    lineno = _key_lineno(lines, end, "name")
    if not SKILL_NAME_RE.match(fm.name):
        raise SourceError(
            src, lineno, "frontmatter name is not a slug", lines[lineno - 1],
            "the Agent Skills standard restricts `name:` to lowercase letters, digits and single "
            "hyphens (`^[a-z0-9]+(-[a-z0-9]+)*$`) — rename the skill and its directory to match",
        )
    if len(fm.name) > MAX_SKILL_NAME:
        raise SourceError(
            src, lineno, f"frontmatter name is {len(fm.name)} characters", lines[lineno - 1],
            f"the Agent Skills standard caps `name:` at {MAX_SKILL_NAME} characters — shorten it "
            "and rename its directory to match",
        )
    if fm.name != slug:
        raise SourceError(
            src, lineno, "frontmatter name does not match the directory", lines[lineno - 1],
            "the Agent Skills standard requires `name:` to equal the parent directory name — "
            f"write `name: {slug}` here, or move the file to skills/{fm.name}/SKILL.md",
        )

    lineno = _key_lineno(lines, end, "description")
    if len(fm.description) > MAX_SKILL_DESCRIPTION:
        raise SourceError(
            src, lineno, f"frontmatter description is {len(fm.description)} characters",
            lines[lineno - 1],
            f"the Agent Skills standard caps `description:` at {MAX_SKILL_DESCRIPTION} characters "
            "— trim it to the sentence that tells a model when to reach for this skill",
        )


def parse_skill(path: Path, agents: tuple) -> Command:
    # Both layouts carry the slug in a different place: flat `<slug>.md` puts it
    # in the stem, nested `<slug>/SKILL.md` in the parent directory. Taking the
    # stem unconditionally reads every nested command as "SKILL".
    slug = path.parent.name if path.name == "SKILL.md" else path.stem
    src = f"commands/{slug}.md"
    lines = path.read_text(encoding="utf-8").split("\n")
    consumed = set()

    frontmatter, idx = split_frontmatter(lines, src, consumed)
    check_frontmatter(frontmatter, slug, lines, idx, src)
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
            src, idx + 1, "heading command name does not match the filename", title_line,
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
            # Crew comes from the stage HEADING plus any body list/table that *declares* it
            # (every item opens on a role name). An agent merely named in running prose is
            # usually being *discussed* ("Gates `ux-ui-designer`", "dispatch a
            # `senior-engineer` fixer"), not convened at this stage — listing it as crew
            # would make the page claim something the source does not (C8). The prose
            # mention still renders verbatim in the stage body, so nothing is lost.
            texts = (raw_section.heading_raw,)
            crew = find_crew(texts, agents) + find_declared_crew(blocks, agents)
            stages.append(
                Stage(
                    lineno=raw_section.lineno,
                    heading_raw=raw_section.heading_raw,
                    label=heading.label,
                    sort_key=heading.sort_key,
                    title=heading.title,
                    gate=heading.gate,
                    annotation=heading.annotation,
                    crew=tuple(dict.fromkeys(crew)),
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
                    raw_section.heading_raw, "a skill file has exactly one Guardrails section",
                )
            guardrails = section
        elif _is_config_title(raw_section.title) and not seen_stage:
            if config is not None:
                raise SourceError(
                    src, raw_section.lineno, "second Config section",
                    raw_section.heading_raw, "a skill file has exactly one Config section",
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
                    "stages, config, guardrails, source, other-commands and every stage-<n>",
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
            "a skill file describes its run as numbered stages",
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
# Layer 4 — RENDER  (model -> HTML/XML strings)
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
    ("commands", "Commands"),
    ("tools", "Tools"),
    ("how", "How"),
    ("next", "What's next"),
    ("faq", "FAQ"),
)

FOOTER_LINKS = (
    ("https://github.com/saman-mb/shipmates", "GitHub"),
    ("../../#install", "Install"),
    ("../../#crew", "Crew"),
    ("../../#commands", "Commands"),
    ("../../#next", "What's next"),
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


def _head(
    full_title: str, social_title: str, description: str, url: str, jsonld: str, ctx: PageContext
) -> str:
    """The one <head> every detail page shares — command and agent pages differ
    only in the five values the callers pass in."""
    alt = "Shipmates — Custom subagents and command workflows for your AI coding harness."
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
{indent_html(jsonld, "  ")}
</head>"""


def render_head(cmd: Command, ctx: PageContext) -> str:
    return _head(
        full_title=page_title(cmd) + " · Shipmates",
        social_title=page_title(cmd),
        description=truncate_words(cmd.frontmatter.description, MAX_META_DESCRIPTION),
        url=canonical_url(cmd.slug, ctx),
        jsonld=render_jsonld(cmd, ctx),
        ctx=ctx,
    )


def _jsonld_script(payload: dict) -> str:
    """Serialise one ld+json block. JSON-LD is the one place HTML-escaping would
    corrupt the payload. Two replacements close the </script> and <!-- breakouts;
    both stay valid JSON."""
    body = json.dumps(payload, ensure_ascii=False, indent=2)
    body = body.replace("</", "<\\/").replace("<!--", "\\u003c!--")
    return '<script type="application/ld+json">\n' + body + "\n</script>"


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
    return _jsonld_script(payload)


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
        <a class="btn btn--primary site-nav__cta--mobile" href="{link("../../#install")}">Install</a>
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
        <p class="site-footer__tagline">Custom subagents &amp; command workflows — for your AI coding harness.</p>
      </div>
      <nav class="site-footer__nav" aria-label="Footer">
        <ul class="site-footer__links">
{items}
        </ul>
      </nav>
      <p class="site-footer__legal">{esc(legal)}</p>
    </div>
  </footer>"""


def _back_link(href: str, label: str) -> str:
    return (
        f'<a class="order-back" href="{link(href)}">'
        f'<span aria-hidden="true">←</span> {esc(label)}</a>'
    )


def render_back_link() -> str:
    return _back_link("../../#commands", "All commands")


def render_agent_back_link() -> str:
    return _back_link("../../#crew", "All crew")


def render_hero(cmd: Command, src: str) -> str:
    flag = ""
    if cmd.slug == FLAGSHIP_SLUG:
        flag = '\n          <span class="order-detail__flag">Flagship</span>'
    # The frontmatter description is the one-line summary (it is also the meta
    # description); the intro blocks are the skill file's own lede and follow it.
    intro = render_prose(cmd.intro, src, "          ")
    if intro:
        intro = "\n" + intro
    return f"""    <section class="section" aria-labelledby="order-title">
      <div class="container container--prose">
        {render_back_link()}
        <div class="order-detail">
          <p class="section__eyebrow"><span aria-hidden="true">\U0001f4dc</span> Command</p>{flag}
          <h1 class="order-detail__title" id="order-title"><code>/{esc(cmd.slug)}</code></h1>
          <p class="order-detail__tagline">{esc(cmd.tagline)}</p>
          <p class="order-detail__desc">{esc(cmd.frontmatter.description)}</p>{intro}
        </div>
      </div>
    </section>"""


def _png_size(path: Path) -> tuple:
    """(width, height) from a PNG's IHDR — no Pillow dependency, because this
    generator's --check runs in CI before the demo step installs Pillow."""
    with open(path, "rb") as fh:
        head = fh.read(24)
    if head[:8] != b"\x89PNG\r\n\x1a\n":
        raise SourceError("assets", 0, str(path), "", "not a PNG (bad signature)")
    return int.from_bytes(head[16:20], "big"), int.from_bytes(head[20:24], "big")


def _demo_assets(slug: str) -> tuple:
    """(gif, poster) filenames for a command's demo. `/ship-issue` reuses the
    flagship demo.gif rather than shipping a second near-identical asset."""
    if slug == FLAGSHIP_SLUG:
        return "demo.gif", "demo-poster.png"
    return f"command-{slug}.gif", f"command-{slug}-poster.png"


def render_demo(cmd: Command) -> str:
    """A playable terminal figure at the top of the page: poster by default, a
    button swaps in the animated GIF (WCAG 2.2.2 — motion is user-initiated)."""
    _gif, poster = _demo_assets(cmd.slug)
    w, h = _png_size(ROOT / "site" / "assets" / poster)
    return f"""    <section class="section" id="demo" aria-labelledby="demo-title">
      <div class="container container--prose">
        <div class="section__head">
          <h2 class="section__title" id="demo-title">See it run</h2>
        </div>
        <figure class="demo">
          <img class="demo__media" id="demo-media" src="{link('../../assets/' + poster)}" width="{w}" height="{h}" alt="Illustrative terminal recording of the stages /{esc(cmd.slug)} runs, in order." loading="lazy">
          <button type="button" class="btn btn--secondary demo__toggle" id="demo-toggle" aria-pressed="false" aria-controls="demo-media">Play the run</button>
          <figcaption class="demo__caption">Illustrative — the stages <code>/{esc(cmd.slug)}</code> runs, in order.</figcaption>
        </figure>
      </div>
    </section>"""


def render_demo_script(cmd: Command) -> str:
    """Inline, self-contained toggle: no external src, so the page stays
    self-contained per the site's own gate."""
    gif, _poster = _demo_assets(cmd.slug)
    return f"""  <script>
  (function () {{
    var img = document.getElementById('demo-media');
    var toggle = document.getElementById('demo-toggle');
    if (!img || !toggle) return;
    var poster = img.getAttribute('src');
    var gif = '../../assets/{gif}';
    var playing = false;
    toggle.addEventListener('click', function () {{
      playing = !playing;
      img.src = playing ? gif : poster;
      toggle.setAttribute('aria-pressed', String(playing));
      toggle.textContent = playing ? 'Pause the run' : 'Play the run';
    }});
  }})();
  </script>"""


def render_invoke(cmd: Command) -> str:
    invocation = f"/{cmd.slug} {cmd.frontmatter.argument_hint}".strip()
    return f"""    <section class="section order-invoke" id="invoke" aria-labelledby="invoke-title">
      <div class="container container--prose">
        <div class="section__head">
          <h2 class="section__title" id="invoke-title">How to run it</h2>
        </div>
        <div class="codeblock">
          <p class="codeblock__label">Run it in your harness</p>
          <div class="codeblock__body">
            <pre class="codeblock__pre"><code class="codeblock__code">{esc(invocation)}</code></pre>
          </div>
        </div>
        <p class="order-invoke__hint"><code>&lt;angle brackets&gt;</code> = required · <code>[square brackets]</code> = optional</p>
      </div>
    </section>"""


ANNOTATION_PREFIX_RE = re.compile(r"^agents?:\s*")
ANNOTATION_CONNECTOR_RE = r",\s*(?:or|and)\s*"


def annotation_residue(st: Stage) -> str:
    """The part of a crew annotation the chips can't already say.

    Strips the outer parens, the leading `agent:`/`agents:` scaffolding, and
    each chip's own codespan (plus one adjacent ", or"/", and" connector, so
    removing a name doesn't leave a dangling conjunction) — what's left is
    the genuine qualifier ("fresh pass", "x N, parallel", "for runtime/ops
    bugs" ...). Empty when the annotation said nothing beyond the names.
    """
    inner = st.annotation.strip()
    if inner.startswith("(") and inner.endswith(")"):
        inner = inner[1:-1]
    inner = ANNOTATION_PREFIX_RE.sub("", inner)
    for name in st.crew:
        inner = re.sub(
            rf"(?:{ANNOTATION_CONNECTOR_RE})?`{re.escape(name)}`(?:{ANNOTATION_CONNECTOR_RE})?",
            " ",
            inner,
        )
    return re.sub(r"\s+", " ", inner).strip(" ,")


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
        # [C-2] Names live in the chips now, so the source's parenthetical only
        # earns a place beside them when it says something the chips can't — a
        # qualifier ("fresh pass", "x N, parallel", "for runtime/ops bugs"). An
        # annotation that reduces to nothing but the names and the agent:/
        # agents: scaffolding is dropped instead of repeating the chips in
        # prose (#136). Annotations with no recognised crew (MODE=pr notes,
        # "specialist agents, in parallel") have nothing to de-duplicate
        # against and still render verbatim.
        bits = ["Crew:"]
        bits.extend(
            f'<span class="chip order-stage__crew-item"><code>{esc(name)}</code></span>'
            for name in st.crew
        )
        if st.annotation:
            if st.crew:
                residue = annotation_residue(st)
                if residue:
                    bits.append(render_inline(f"({residue})", src, st.lineno))
            else:
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
        <p>This page is generated from <code>{esc(cmd.source_path)}</code>. The installer copies it to <code>~/.claude/skills/{esc(cmd.slug)}/SKILL.md</code> for every project, or <code>.claude/skills/{esc(cmd.slug)}/SKILL.md</code> inside a single repo.</p>
        <a class="btn btn--secondary" href="{link(blob)}">
          {GITHUB_ICON}
          <span>View {esc(cmd.source_path)} on GitHub</span>
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
    return f"""    <section class="section" id="other-commands" aria-labelledby="other-commands-title">
      <div class="container container--prose">
        <div class="section__head">
          <h2 class="section__title" id="other-commands-title">Other commands</h2>
        </div>
        <nav class="order-siblings" aria-label="Other commands">
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
        render_demo(cmd),
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
<body class="page--doc">
  <a class="skip-link" href="#main">Skip to content</a>

{render_header()}

  <main class="main" id="main" tabindex="-1">

{body}

  </main>

{render_footer()}
{render_demo_script(cmd)}
</body>
</html>
"""


# --- agent detail pages ------------------------------------------------------


def canonical_agent_url(agent: Agent, ctx: PageContext) -> str:
    return f"{ctx.site_url}agents/{agent.slug}/"


def agent_page_title(agent: Agent) -> str:
    return f"{agent.name} — {agent.tagline}"


def render_agent_jsonld(agent: Agent, ctx: PageContext) -> str:
    """TechArticle — the same shape the hand-authored docs leaves carry."""
    payload = {
        "@context": "https://schema.org",
        "@type": "TechArticle",
        "name": agent.name,
        "description": agent.description,
        "url": canonical_agent_url(agent, ctx),
        "isPartOf": {"@type": "CollectionPage", "url": ctx.site_url + "#crew"},
    }
    return _jsonld_script(payload)


def render_agent_head(agent: Agent, ctx: PageContext) -> str:
    return _head(
        full_title=agent_page_title(agent) + " · Shipmates",
        social_title=agent_page_title(agent),
        description=truncate_words(agent.description, MAX_META_DESCRIPTION),
        url=canonical_agent_url(agent, ctx),
        jsonld=render_agent_jsonld(agent, ctx),
        ctx=ctx,
    )


def render_agent_hero(agent: Agent) -> str:
    return f"""    <section class="section" aria-labelledby="order-title">
      <div class="container container--prose">
        {render_agent_back_link()}
        <div class="order-detail">
          <p class="section__eyebrow"><span aria-hidden="true">\U0001f9ed</span> Subagent</p>
          <h1 class="order-detail__title" id="order-title"><code>{esc(agent.name)}</code></h1>
          <p class="order-detail__tagline">{esc(agent.tagline)}</p>
          <p class="order-detail__desc">{esc(agent.description)}</p>
        </div>
      </div>
    </section>"""


def _copy_prose(texts: tuple, prefix: str) -> str:
    """Authored copy paragraphs through the same prose renderer the sources use."""
    return render_prose(
        tuple(Para(lineno=0, text=text) for text in texts), AGENT_COPY_SRC, prefix
    )


def _agent_section(section_id: str, title: str, inner: str) -> str:
    return f"""    <section class="section" id="{esc(section_id)}" aria-labelledby="{esc(section_id)}-title">
      <div class="container container--prose">
        <div class="section__head">
          <h2 class="section__title" id="{esc(section_id)}-title">{esc(title)}</h2>
        </div>
{inner}
      </div>
    </section>"""


def render_agent_what(agent: Agent) -> str:
    return _agent_section("what", "What this agent does", _copy_prose(agent.what, "        "))


def render_agent_scenarios(agent: Agent) -> str:
    cards = []
    for scenario in agent.scenarios:
        cards.append(
            '          <div class="agent-scenario">\n'
            f'            <h3 class="agent-scenario__title">'
            f'{render_inline(scenario.title, AGENT_COPY_SRC, 0)}</h3>\n'
            f'            <p class="agent-scenario__desc">'
            f'{render_inline(scenario.desc, AGENT_COPY_SRC, 0)}</p>\n'
            "          </div>"
        )
    inner = '        <div class="agent-scenarios">\n' + "\n".join(cards) + "\n        </div>"
    return _agent_section("scenarios", "When you'd want it", inner)


def render_agent_checks(agent: Agent) -> str:
    items = tuple(
        ListItem(lineno=0, text=f"**{check.lead}** {check.text}", children=())
        for check in agent.checks
    )
    prose = render_prose(
        (ListBlock(lineno=0, ordered=False, items=items),), AGENT_COPY_SRC, "        "
    )
    return _agent_section("checks", "What it checks", prose)


def render_agent_crew_fit(agent: Agent) -> str:
    related = " ".join(
        f'<a class="chip order-stage__crew-item" href="{link("../" + role + "/")}">'
        f"<code>{esc(role)}</code></a>"
        for role in agent.crew_fit.related
    )
    called = " ".join(
        f'<a class="chip order-stage__crew-item" href="{link("../../commands/" + slug + "/")}">'
        f"<code>/{esc(slug)}</code></a>"
        for slug in agent.called_by
    )
    inner = (
        _copy_prose(agent.crew_fit.paragraphs, "        ")
        + f'\n        <p class="order-stage__crew">Related roles: {related}</p>'
        + f'\n        <p class="order-stage__crew">Called in by: {called}</p>'
    )
    return _agent_section("crew-fit", "How it fits the crew", inner)


def render_agent_reference(agent: Agent) -> str:
    tools = ", ".join(f"<code>{esc(tool)}</code>" for tool in agent.tools)
    inner = f"""        <dl class="agent-ref">
          <dt>Name</dt>
          <dd><code>{esc(agent.name)}</code></dd>
          <dt>Description</dt>
          <dd>{esc(agent.description)}</dd>
          <dt>Tools</dt>
          <dd>{tools}</dd>
        </dl>"""
    return _agent_section("reference", "Reference", inner)


def render_agent_source(agent: Agent, ctx: PageContext) -> str:
    blob = ctx.repo_blob_base + agent.source_path
    return f"""    <section class="section order-source" id="source" aria-labelledby="source-title">
      <div class="container container--prose">
        <div class="section__head">
          <h2 class="section__title" id="source-title">Where this lives</h2>
        </div>
        <p>This page is generated from <code>{esc(agent.source_path)}</code>. The installer copies it to <code>~/.claude/agents/{esc(agent.slug)}.md</code> for every project, or <code>.claude/agents/{esc(agent.slug)}.md</code> inside a single repo.</p>
        <a class="btn btn--secondary" href="{link(blob)}">
          {GITHUB_ICON}
          <span>View {esc(agent.source_path)} on GitHub</span>
        </a>
      </div>
    </section>"""


def render_agent_siblings(agent: Agent, all_agents: tuple) -> str:
    items = []
    for other in all_agents:
        name = f"<code>{esc(other.name)}</code>"
        if other.slug == agent.slug:
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
    return f"""    <section class="section" id="other-agents" aria-labelledby="other-agents-title">
      <div class="container container--prose">
        <div class="section__head">
          <h2 class="section__title" id="other-agents-title">Other agents</h2>
        </div>
        <nav class="order-siblings" aria-label="Other agents">
          <ul class="order-siblings__list" role="list">
{listing}
          </ul>
        </nav>
        {render_agent_back_link()}
      </div>
    </section>"""


def render_agent_page(agent: Agent, all_agents: tuple, ctx: PageContext) -> str:
    sections = [
        render_agent_hero(agent),
        render_agent_what(agent),
        render_agent_scenarios(agent),
        render_agent_checks(agent),
        render_agent_crew_fit(agent),
        render_agent_reference(agent),
        render_agent_source(agent, ctx),
        render_agent_siblings(agent, all_agents),
    ]
    body = "\n\n".join(sections)
    return f"""<!doctype html>
<html lang="en">
{render_agent_head(agent, ctx)}
<body class="page--doc">
  <a class="skip-link" href="#main">Skip to content</a>

{render_header()}

  <main class="main" id="main" tabindex="-1">

{body}

  </main>

{render_footer()}
</body>
</html>
"""


# ---------------------------------------------------------------------------
# TOOL PAGES — the same shape as agent pages (hero + authored sections +
# reference + source + siblings), mirroring the crew/commands treatment.
# ---------------------------------------------------------------------------

def canonical_tool_url(tool: Tool, ctx: PageContext) -> str:
    return f"{ctx.site_url}tools/{tool.slug}/"


def tool_page_title(tool: Tool) -> str:
    return f"{tool.name} — {tool.tagline}"


def _tool_copy_prose(texts: tuple, prefix: str) -> str:
    return render_prose(tuple(Para(lineno=0, text=text) for text in texts), TOOL_COPY_SRC, prefix)


def render_tool_jsonld(tool: Tool, ctx: PageContext) -> str:
    payload = {
        "@context": "https://schema.org",
        "@type": "TechArticle",
        "name": tool.name,
        "description": tool.description,
        "url": canonical_tool_url(tool, ctx),
        "isPartOf": {"@type": "CollectionPage", "url": ctx.site_url + "#tools"},
    }
    return _jsonld_script(payload)


def render_tool_head(tool: Tool, ctx: PageContext) -> str:
    return _head(
        full_title=tool_page_title(tool) + " · Shipmates",
        social_title=tool_page_title(tool),
        description=truncate_words(tool.description, MAX_META_DESCRIPTION),
        url=canonical_tool_url(tool, ctx),
        jsonld=render_tool_jsonld(tool, ctx),
        ctx=ctx,
    )


def render_tool_back_link() -> str:
    return _back_link("../../#tools", "All tools")


def render_tool_hero(tool: Tool) -> str:
    return f"""    <section class="section" aria-labelledby="order-title">
      <div class="container container--prose">
        {render_tool_back_link()}
        <div class="order-detail">
          <p class="section__eyebrow"><span aria-hidden="true">\U0001f9f0</span> Tool</p>
          <h1 class="order-detail__title" id="order-title"><code>{esc(tool.name)}</code></h1>
          <p class="order-detail__tagline">{esc(tool.tagline)}</p>
          <p class="order-detail__desc">{esc(tool.description)}</p>
        </div>
      </div>
    </section>"""


def render_tool_what(tool: Tool) -> str:
    return _agent_section("what", "What it does", _tool_copy_prose(tool.what, "        "))


def render_tool_usage(tool: Tool) -> str:
    return _agent_section("usage", "How the crew reach for it", _tool_copy_prose(tool.usage, "        "))


def render_tool_examples(tool: Tool) -> str:
    """A gallery of example outputs. An animated GIF is wrapped in a `<picture>`
    whose reduced-motion source swaps the animation for its static final frame
    (WCAG 2.2.2, the a11y pattern the homepage hero uses); a static PNG or SVG is
    a plain `<img>`. Each example is a different run of the tool."""
    if not tool.examples:
        return ""
    animated = any(ex.poster for ex in tool.examples)
    figures = []
    for ex in tool.examples:
        if ex.poster:
            media = (
                f"""            <picture>
              <source media="(prefers-reduced-motion: reduce)" srcset="{link(ex.poster)}">
              <img class="tool-example__img" src="{link(ex.src)}" width="{ex.width}" height="{ex.height}" alt="{esc(ex.alt)}" loading="lazy" decoding="async">
            </picture>"""
            )
        else:
            media = (
                f'            <img class="tool-example__img" src="{link(ex.src)}" '
                f'width="{ex.width}" height="{ex.height}" alt="{esc(ex.alt)}" '
                'loading="lazy" decoding="async">'
            )
        figures.append(
            '          <figure class="tool-example">\n'
            f"{media}\n"
            f'            <figcaption class="tool-example__caption">{esc(ex.caption)}</figcaption>\n'
            "          </figure>"
        )
    gallery = "\n".join(figures)
    reduced = (
        " If you&#x27;ve set <code>prefers-reduced-motion</code>, any animation shows its final "
        "frame instead."
        if animated else ""
    )
    intro = (
        f"        <p>A few things <code>{esc(tool.name)}</code> can produce, each from a small "
        f"spec of its own.{reduced}</p>\n"
    )
    inner = intro + f'        <div class="tool-gallery">\n{gallery}\n        </div>'
    return _agent_section("examples", "Examples", inner)


def render_tool_sample(tool: Tool) -> str:
    """A plain-text input→output sample for a non-visual tool (scrub, fixtures),
    shown as a code block. Skipped when the tool has none."""
    if not tool.sample:
        return ""
    inner = (
        '        <p>What it does to a real input:</p>\n'
        f'        <pre class="order-code" tabindex="0"><code>{esc(tool.sample)}</code></pre>'
    )
    return _agent_section("sample", "In practice", inner)


def render_tool_reference(tool: Tool) -> str:
    bundled = ", ".join(f"<code>{esc(name)}</code>" for name in tool.bundled) or "—"
    inner = f"""        <dl class="agent-ref">
          <dt>Name</dt>
          <dd><code>{esc(tool.name)}</code></dd>
          <dt>Description</dt>
          <dd>{esc(tool.description)}</dd>
          <dt>Bundled files</dt>
          <dd>{bundled}</dd>
          <dt>Install</dt>
          <dd><code>shipmates install --harness &lt;name&gt; --with-tools {esc(tool.slug)}</code></dd>
        </dl>"""
    return _agent_section("reference", "Reference", inner)


def render_tool_source(tool: Tool, ctx: PageContext) -> str:
    blob = ctx.repo_blob_base + tool.source_path
    return f"""    <section class="section order-source" id="source" aria-labelledby="source-title">
      <div class="container container--prose">
        <div class="section__head">
          <h2 class="section__title" id="source-title">Where this lives</h2>
        </div>
        <p>This page is generated from <code>{esc(tool.source_path)}</code>. Opt in at install with <code>--with-tools {esc(tool.slug)}</code> and the installer copies the tool — instructions and bundled files — into the harness's skills tree.</p>
        <a class="btn btn--secondary" href="{link(blob)}">
          {GITHUB_ICON}
          <span>View {esc(tool.source_path)} on GitHub</span>
        </a>
      </div>
    </section>"""


def render_tool_siblings(tool: Tool, all_tools: tuple) -> str:
    items = []
    for other in all_tools:
        name = f"<code>{esc(other.name)}</code>"
        if other.slug == tool.slug:
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
    return f"""    <section class="section" id="other-tools" aria-labelledby="other-tools-title">
      <div class="container container--prose">
        <div class="section__head">
          <h2 class="section__title" id="other-tools-title">Other tools</h2>
        </div>
        <nav class="order-siblings" aria-label="Other tools">
          <ul class="order-siblings__list" role="list">
{listing}
          </ul>
        </nav>
        {render_tool_back_link()}
      </div>
    </section>"""


def render_tool_page(tool: Tool, all_tools: tuple, ctx: PageContext) -> str:
    sections = [
        render_tool_hero(tool),
        render_tool_what(tool),
        render_tool_examples(tool),
        render_tool_sample(tool),
        render_tool_usage(tool),
        render_tool_reference(tool),
        render_tool_source(tool, ctx),
        render_tool_siblings(tool, all_tools),
    ]
    sections = [s for s in sections if s]
    body = "\n\n".join(sections)
    return f"""<!doctype html>
<html lang="en">
{render_tool_head(tool, ctx)}
<body class="page--doc">
  <a class="skip-link" href="#main">Skip to content</a>

{render_header()}

  <main class="main" id="main" tabindex="-1">

{body}

  </main>

{render_footer()}
</body>
</html>
"""


def render_sitemap(cmds: tuple, agents: tuple, ctx: PageContext, docs: tuple = (), tools: tuple = ()) -> str:
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
    for agent in agents:
        entries.append(
            "  <url>\n"
            f"    <loc>{esc(canonical_agent_url(agent, ctx))}</loc>\n"
            f"    <lastmod>{esc(ctx.lastmod)}</lastmod>\n"
            "    <changefreq>monthly</changefreq>\n"
            "    <priority>0.7</priority>\n"
            "  </url>"
        )
    for slug in docs:
        entries.append(
            "  <url>\n"
            f"    <loc>{esc(ctx.site_url)}docs/{slug}/</loc>\n"
            f"    <lastmod>{esc(ctx.lastmod)}</lastmod>\n"
            "    <changefreq>monthly</changefreq>\n"
            "    <priority>0.7</priority>\n"
            "  </url>"
        )
    for tool in tools:
        entries.append(
            "  <url>\n"
            f"    <loc>{esc(canonical_tool_url(tool, ctx))}</loc>\n"
            f"    <lastmod>{esc(ctx.lastmod)}</lastmod>\n"
            "    <changefreq>monthly</changefreq>\n"
            "    <priority>0.7</priority>\n"
            "  </url>"
        )
    # Docs hub itself (docs/index.html) — always included when any docs leaf is found.
    if docs:
        entries.append(
            "  <url>\n"
            f"    <loc>{esc(ctx.site_url)}docs/</loc>\n"
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
# Layer 5 — EMIT  (paths + bytes)
# No markup is built here. build_site() is pure; write_all() is the only writer.
# ---------------------------------------------------------------------------

SITE_DIR = "site"

#: The harness whose rendered payload the site documents. The detail pages show
#: real invocation syntax and real tool names, so they have to come from one
#: target's output rather than the harness-neutral source.
SITE_TARGET = "claude-code"


def page_path(slug: str) -> str:
    return f"{SITE_DIR}/commands/{slug}/index.html"


def agent_page_path(slug: str) -> str:
    return f"{SITE_DIR}/agents/{slug}/index.html"


def tool_page_path(slug: str) -> str:
    return f"{SITE_DIR}/tools/{slug}/index.html"


SITEMAP_PATH = f"{SITE_DIR}/sitemap.xml"


def build_site(cmds: tuple, agents: tuple, ctx: PageContext, docs: tuple = (), tools: tuple = ()) -> dict:
    """Repo-relative posix path -> full file text. PURE: no I/O, no clock, no cwd.

    Every output is materialised in memory before anything is written, so a parse
    failure in any command, agent, or tool leaves the tree completely untouched.
    """
    files = {page_path(cmd.slug): render_page(cmd, cmds, ctx) for cmd in cmds}
    files.update(
        {agent_page_path(agent.slug): render_agent_page(agent, agents, ctx) for agent in agents}
    )
    files.update(
        {tool_page_path(tool.slug): render_tool_page(tool, tools, ctx) for tool in tools}
    )
    files[SITEMAP_PATH] = render_sitemap(cmds, agents, ctx, docs, tools)
    return files


def expected_paths(cmds: tuple, agents: tuple, tools: tuple = ()) -> frozenset:
    return frozenset(
        [page_path(cmd.slug) for cmd in cmds]
        + [agent_page_path(agent.slug) for agent in agents]
        + [tool_page_path(tool.slug) for tool in tools]
    )


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
        for subtree in ("commands", "agents", "tools")
        for path in sorted(site.glob(f"{subtree}/*/index.html"))
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
# Layer 6 — CLI
# The only layer that prints or exits. Root comes from __file__, never from cwd.
# ---------------------------------------------------------------------------

ROOT = Path(__file__).resolve().parents[1]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

import subprocess

REGENERATE_HINT = "run: python3 tools/gen_command_pages.py && git add site/"


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(
        description=(
            "Generate the per-command detail pages under site/commands/, the per-agent "
            "detail pages under site/agents/, and site/sitemap.xml from skills/*/SKILL.md "
            "and agents/*.md."
        )
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
      with contextlib.ExitStack() as stack:
        # Generate from the RENDERED payload, not the neutral source. The site
        # documents what a user installs; the neutral dialect (`{{issue}}`,
        # `agent-files/*.md`, semantic capability names) exists only inside the
        # exporter and is not valid in any harness. Reading the source published
        # those placeholders live.
        payload = stack.enter_context(tempfile.TemporaryDirectory())
        res = subprocess.run(
            ["cargo", "run", "--", "build", "--target", SITE_TARGET, "--out", str(payload)],
            cwd=root,
            capture_output=True,
            text=True,
        )
        if res.returncode != 0:
            print(f"error: could not build the {SITE_TARGET} payload for the site: {res.stderr}", file=sys.stderr)
            return res.returncode
        rendered = Path(payload) / "harnesses" / SITE_TARGET / ".claude"
        agents = load_agents(rendered / "agents", rendered / "skills")
        cmds = load_skills(rendered / "skills", tuple(agent.name for agent in agents))
        # Tools come from the repo `toolbox/` source, not the rendered payload —
        # a plain `build` excludes opt-in tools, so they aren't in `rendered`.
        tools = load_tools(root / "toolbox")
        # Discover hand-authored docs pages for the sitemap.
        docs = tuple(
            slug for slug in DOCS_SLUGS
            if (root / SITE_DIR / "docs" / slug / "index.html").is_file()
        )
        files = build_site(cmds, agents, ctx, docs, tools)
    except SourceError as err:
        print(f"error: {err}", file=sys.stderr)
        return 1

    if args.check:
        report = check_all(files, root) + [
            f"unexpected generated file: {path} "
            "(renamed or removed a command, agent, or tool? delete it and rerun)"
            for path in find_orphans(root, expected_paths(cmds, agents, tools))
        ]
        if report:
            for line in report:
                print(line)
            print(REGENERATE_HINT)
            return 1
        print(
            f"up to date: {len(files)} generated files, "
            f"{len(cmds)} skills, {len(agents)} agents, {len(tools)} tools"
        )
        return 0

    written = write_all(files, root)
    for path in find_orphans(root, expected_paths(cmds, agents, tools)):
        print(f"warning: unexpected generated file: {path} (renamed or removed a command, agent, or tool?)")
    if written:
        for path in written:
            print(f"wrote {path}")
    print(
        f"{len(written)} of {len(files)} files updated "
        f"({len(cmds)} skills, {len(agents)} agents, {len(tools)} tools)"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
