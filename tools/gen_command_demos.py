#!/usr/bin/env python3
"""Generate site/assets/command-<slug>.gif — one illustrative terminal per
command, depicting the actual stage sequence that command runs.

Honest by construction, like gen_demo_gif.py: each reel shows the *real* stages
the workflow performs (parsed and kept in lockstep with the source SKILL.md's
own stage headings — see the assertion in build_artifacts), with short generic
labels. No fabricated counts, no invented file names. The example invocation
uses a placeholder argument, exactly as the /ship-issue demo uses "142".

`/ship-issue` is intentionally absent: its command page reuses the flagship
site/assets/demo.gif, so there is no second, near-identical asset to keep in
sync.

Two artifacts per command, from one set of frames:
  site/assets/command-<slug>.gif        — canonical animation (command page)
  site/assets/command-<slug>-poster.png — final frame, prefers-reduced-motion

Regenerate:            python3 tools/gen_command_demos.py
Check for drift (CI):  python3 tools/gen_command_demos.py --check
"""
import argparse
import sys
from pathlib import Path

import demo_terminal as dt

ROOT = Path(__file__).resolve().parents[1]

W = 820
PALETTE = 96
ACCENTS = [dt.BLUE, dt.PURPLE, dt.ORANGE, dt.CYAN, dt.CORAL, dt.GOLD, dt.SAGE]

# Per command: an example invocation argument, the faithful short-label stage
# sequence (label, one-line detail), and a green closing line. The labels track
# the source stages one-for-one, in order — build_artifacts() asserts the count
# against the parsed SKILL.md so a stage added or removed upstream fails the
# build instead of silently drifting.
COMMANDS = {
    "ship-epic": {
        "arg": "42",
        "stages": [
            ("INTAKE", "parse epic checklist"),
            ("BRANCH", "epic integration line + PR"),
            ("GRAPH", "dependency order + gate map"),
            ("PLAN", "one architect -> shipping units"),
            ("LOOP", "delegate /ship-issue per unit"),
            ("TICK", "checklist after each unit"),
            ("CLOSE", "epic PR green — captain merge"),
        ],
        "closer": "Epic delivered — N stories in U runs, one epic PR. ⚓",
    },
    "fix-bug": {
        "arg": "142",
        "stages": [
            ("REPRODUCE", "a failing test that pins the bug"),
            ("ISOLATE", "throwaway worktree"),
            ("ROOT-CAUSE", "the real cause, not the symptom"),
            ("FIX", "minimal & scoped"),
            ("PROVE", "the test flips red -> green"),
            ("REVIEW", "board checks the diff"),
            ("DELIVER", "PR — reviewed, CI-green"),
        ],
        "closer": "Bug fixed — proven by a test, reviewed, CI-green. ⚓",
    },
    "plan-epics": {
        "arg": "briefs/q3.md",
        "stages": [
            ("INTAKE", "read the brief + repo context"),
            ("SCOPE", "carve the work into epics"),
            ("AUTHOR", "product-manager writes each epic's stories"),
            ("CREATE", "epics + labelled issues on GitHub"),
            ("VERIFY", "every story linked & tracked"),
        ],
        "closer": "Backlog ready — epics + linked stories on GitHub. ⚓",
    },
    "consolidate-issues": {
        "arg": "area:* apply",
        "stages": [
            ("INVENTORY", "every open issue, scoped"),
            ("CROSS-CHECK", "git history — what already shipped"),
            ("TRIAGE", "close / keep / dedupe, evidence-backed"),
            ("MIGRATE", "legacy issues to the current shape"),
            ("BUNDLE", "survivors into themed groups"),
            ("APPLY", "labels, closes, before/after report"),
        ],
        "closer": "Backlog slimmed — the survivors ship as bundles. ⚓",
    },
    "harden": {
        "arg": "the auth flow",
        "stages": [
            ("SCOPE", "map the attack surface"),
            ("THREAT-MODEL", "find & rank the risks"),
            ("TRIAGE", "blockers vs accepted risk"),
            ("ISOLATE", "throwaway worktree"),
            ("REMEDIATE", "fix every blocker"),
            ("RE-REVIEW", "each finding fixed or noted"),
            ("REPORT", "prioritised, with a PR"),
        ],
        "closer": "Hardened — every blocker fixed or signed off. ⚓",
    },
    "spike": {
        "arg": "which queue for jobs",
        "stages": [
            ("FRAME", "the open question"),
            ("PROTOTYPE", "throwaway builds, in parallel"),
            ("JUDGE", "score against the constraints"),
            ("ISOLATE", "worktree for the ADR"),
            ("RECOMMEND", "a decision, as an ADR"),
            ("DELIVER", "ADR committed"),
            ("REPORT", "the call, and why"),
        ],
        "closer": "Decision made — captured as an ADR. ⚓",
    },
    "migrate": {
        "arg": "moment.js -> date-fns",
        "stages": [
            ("DISCOVER", "every call site"),
            ("PLAN", "the transform"),
            ("ISOLATE", "throwaway worktree"),
            ("TRANSFORM", "each batch, verified"),
            ("SWEEP", "no old pattern left"),
            ("REVIEW", "board checks the diff"),
            ("REPORT", "what moved"),
        ],
        "closer": "Migrated — every call site moved, swept clean. ⚓",
    },
    "document": {
        "arg": "the public API",
        "stages": [
            ("SCOPE", "audience + doc type"),
            ("ISOLATE", "throwaway worktree"),
            ("DRAFT", "from the actual code"),
            ("FRESH-READER", "a new reader follows the steps"),
            ("FIX DRIFT", "loop until it completes"),
            ("DELIVER", "docs that actually work"),
        ],
        "closer": "Docs that work — a fresh reader can follow them. ⚓",
    },
    "release": {
        "arg": "v1.4.0",
        "stages": [
            ("SCOPE", "what merged since last tag"),
            ("CHANGELOG", "assembled from real merges"),
            ("VERSION", "bump"),
            ("CI GATE", "green at the release commit"),
            ("PRE-FLIGHT", "SRE: rollback + migration safety"),
            ("TAG", "and, opt-in, publish"),
            ("REPORT", "what shipped"),
        ],
        "closer": "Released — CI-green at the tag. ⚓",
    },
    "polish": {
        "arg": "the dashboard",
        "stages": [
            ("ISOLATE", "throwaway worktree"),
            ("SEE IT", "wire a way to view the output"),
            ("BASELINE", "first render"),
            ("LOOP", "produce -> critique -> fix"),
            ("REPORT", "until the specialist signs off"),
        ],
        "closer": "Shipped — the specialist signed off. ⚓",
    },
    "pr-review": {
        "arg": "128",
        "stages": [
            ("CLASSIFY", "size & risk of the PR"),
            ("CI STATE", "read it, don't fix it"),
            ("BOARD", "specialists review in parallel"),
            ("CONSOLIDATE", "one ranked verdict"),
            ("DELIVER", "accept / block, with reasons"),
        ],
        "closer": "Reviewed — one ranked verdict, with reasons. ⚓",
    },
    "onboard": {
        "arg": "",
        "stages": [
            ("SURVEY", "repo shape + mode"),
            ("ISOLATE", "throwaway worktree"),
            ("RECON", "how the code really works"),
            ("DRAFT", "the onboarding guide"),
            ("VERIFY", "a fresh agent's questions answered"),
            ("DELIVER", "a guide that lands"),
        ],
        "closer": "Onboarded — a guide that answers real questions. ⚓",
    },
    "refactor": {
        "arg": "the order service",
        "stages": [
            ("SCOPE", "what & why"),
            ("CHARACTERIZE", "tests that pin behaviour"),
            ("TARGET", "the shape to reach"),
            ("ISOLATE", "throwaway worktree"),
            ("TRANSFORM", "restructure, not rewrite"),
            ("EQUIVALENCE", "behaviour unchanged"),
            ("REVIEW", "board checks the diff"),
            ("DELIVER", "PR — reviewed, CI-green"),
        ],
        "closer": "Refactored — behaviour proven unchanged. ⚓",
    },
}


def _height(n_stages):
    # command line + blank + n stages + blank + closer, plus top offset & pad.
    n_lines = n_stages + 4
    h = dt.Y0 + n_lines * dt.LINE_H + 26
    return h + (h % 2)  # keep even


def build_one(slug, spec):
    n = len(spec["stages"])
    H = _height(n)
    term = dt.Terminal(W, H, f"shipmates — /{slug}")
    reel = dt.Reel(term)
    prompt = [("$ ", dt.PROMPT, True)]

    invocation = f"/{slug} {spec['arg']}".strip()
    reel.type_command(prompt, invocation, hold_blinks=1)
    reel.blank()
    for i, (label, detail) in enumerate(spec["stages"]):
        reel.stage(label, ACCENTS[i % len(ACCENTS)], detail, detail, cycles=2)
    reel.blank()
    reel.reveal([("✓ ", dt.GREEN, True), (spec["closer"], dt.GREEN, False)], dur=260)
    reel.hold(300, times=3)

    gif_bytes, poster_bytes, frame_count = dt.encode(reel, W, H, PALETTE)
    return gif_bytes, poster_bytes, frame_count, n


def _source_stage_count(slug):
    """Parse the rendered SKILL.md the same way the site does, so the reel's
    stage count is checked against the real source, not a hand-typed number."""
    import gen_command_pages as g
    import subprocess
    import tempfile
    with tempfile.TemporaryDirectory() as payload:
        res = subprocess.run(
            ["cargo", "run", "--", "build", "--target", "claude-code", "--out", payload],
            cwd=ROOT, capture_output=True, text=True,
        )
        if res.returncode != 0:
            sys.exit(f"gen_command_demos: could not build payload: {res.stderr}")
        rendered = Path(payload) / "harnesses" / "claude-code" / ".claude"
        agents = g.load_agents(rendered / "agents", rendered / "skills")
        cmds = g.load_skills(rendered / "skills", tuple(a.name for a in agents))
    return {c.slug: len(c.stages) for c in cmds}


def build_artifacts(verify_sources=True):
    """Return ({relative posix path: bytes}, {slug: frame_count}).

    When verify_sources is set (default; skip only when cargo is unavailable and
    you have already verified counts), the reel's stage count is asserted equal
    to the parsed source's, so an upstream stage change fails the build.
    """
    if verify_sources:
        counts = _source_stage_count("")
        for slug, spec in COMMANDS.items():
            want, got = counts.get(slug), len(spec["stages"])
            if want != got:
                sys.exit(
                    f"gen_command_demos: /{slug} has {want} stages in its SKILL.md "
                    f"but {got} in this generator. Update COMMANDS[{slug!r}] to match, "
                    f"then regenerate."
                )
        # ship-issue reuses the flagship demo.gif; every other command must be here.
        missing = set(counts) - set(COMMANDS) - {"ship-issue"}
        if missing:
            sys.exit(f"gen_command_demos: no reel authored for: {', '.join(sorted(missing))}")

    files, frames = {}, {}
    for slug, spec in COMMANDS.items():
        gif_bytes, poster_bytes, frame_count, _ = build_one(slug, spec)
        files[f"site/assets/command-{slug}.gif"] = gif_bytes
        files[f"site/assets/command-{slug}-poster.png"] = poster_bytes
        frames[slug] = frame_count
    return files, frames


REGENERATE_HINT = "run: python3 tools/gen_command_demos.py && git add site/assets/command-*.gif site/assets/command-*-poster.png"


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(
        description="Generate site/assets/command-<slug>.gif + posters, one per command."
    )
    parser.add_argument("--check", action="store_true",
                        help="report drift against the committed artifacts and exit 1; write nothing")
    parser.add_argument("--no-verify-sources", action="store_true",
                        help="skip the cargo-backed stage-count assertion (offline)")
    parser.add_argument("--root", default=str(ROOT), metavar="PATH",
                        help="repository root (default: the repo this script lives in)")
    args = parser.parse_args(argv)

    root = Path(args.root).resolve()
    files, frames = build_artifacts(verify_sources=not args.no_verify_sources)

    if args.check:
        report = dt.check_all(files, root)
        if report:
            for line in report:
                print(line)
            print(REGENERATE_HINT)
            return 1
        print(f"up to date: {len(files)} artifacts across {len(COMMANDS)} commands")
        return 0

    written = dt.write_all(files, root)
    for rel in sorted(files):
        if rel in written:
            print(f"wrote {rel}")
    print(f"{len(written)} of {len(files)} artifacts updated across {len(COMMANDS)} commands")
    return 0


if __name__ == "__main__":
    sys.exit(main())
