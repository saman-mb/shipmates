#!/usr/bin/env python3
"""Generate site/assets/cli-demo.gif — an illustrative terminal of installing the
shipmates CLI and bringing the crew aboard.

Honest by construction: every command shown is a real one and every line of
output matches what the CLI actually prints — `cargo install shipmates`, then
`shipmates install --harness claude-code` printing its real
"Installed harness: … (24 files written)" line (12 crew + 12 commands), then
`ls .claude` showing the two trees it writes.

Writes two artifacts from one set of frames:
  site/assets/cli-demo.gif        — canonical animation (install docs page)
  site/assets/cli-demo-poster.png — final frame, served under prefers-reduced-motion

Regenerate:            python3 tools/gen_cli_demo_gif.py
Check for drift (CI):  python3 tools/gen_cli_demo_gif.py --check
"""
import argparse
import sys
from pathlib import Path

import demo_terminal as dt

ROOT = Path(__file__).resolve().parents[1]

W, H = 900, 512
TITLE = "shipmates — install"

# cargo prints its status verb in bold green, then the detail. Kept honest: the
# version is the real current release and the paths are generic.
CARGO_VERSION = "0.1.3"


def _status(verb, detail):
    """A cargo-style output line: right-aligned bold-green verb, then detail."""
    return [(verb.rjust(12) + " ", dt.GREEN, True), (detail, dt.GREY, False)]


def build_reel():
    term = dt.Terminal(W, H, TITLE)
    reel = dt.Reel(term)
    prompt = [("$ ", dt.PROMPT, True)]

    # 1) cargo install
    reel.type_command(prompt, "cargo install shipmates")
    reel.reveal(_status("Updating", "crates.io index"))
    reel.reveal(_status("Compiling", f"shipmates v{CARGO_VERSION}"))
    reel.reveal(_status("Installed", f"package `shipmates v{CARGO_VERSION}` (executable `shipmates`)"))
    reel.blank()

    # 2) drop the crew into a harness
    reel.type_command(prompt, "shipmates install --harness claude-code")
    reel.reveal([("Installed harness: ", dt.WHITE, False),
                 ("claude-code", dt.BLUE, True),
                 (" (24 files written)", dt.GREY, False)])
    reel.blank()

    # 3) show what landed
    reel.type_command(prompt, "ls .claude")
    reel.reveal([("agents", dt.CYAN, True), ("  ", dt.GREY, False), ("skills", dt.CYAN, True)])
    reel.blank()

    # 4) sign-off
    reel.reveal([("✓ ", dt.GREEN, True),
                 ("Crew aboard — run ", dt.GREEN, False),
                 ("/ship-issue <issue#>", dt.WHITE, True),
                 (" to set sail. ⚓", dt.GREEN, False)])
    reel.hold(300, times=6)
    return reel


def build_artifacts():
    reel = build_reel()
    gif_bytes, poster_bytes, frame_count = dt.encode(reel, W, H)
    return (
        {
            "site/assets/cli-demo.gif": gif_bytes,
            "site/assets/cli-demo-poster.png": poster_bytes,
        },
        frame_count,
    )


REGENERATE_HINT = (
    "run: python3 tools/gen_cli_demo_gif.py && "
    "git add site/assets/cli-demo.gif site/assets/cli-demo-poster.png"
)


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(
        description="Generate site/assets/cli-demo.gif and cli-demo-poster.png."
    )
    parser.add_argument("--check", action="store_true",
                        help="report drift against the committed artifacts and exit 1; write nothing")
    parser.add_argument("--root", default=str(ROOT), metavar="PATH",
                        help="repository root (default: the repo this script lives in)")
    args = parser.parse_args(argv)

    root = Path(args.root).resolve()
    files, frame_count = build_artifacts()

    if args.check:
        report = dt.check_all(files, root)
        if report:
            for line in report:
                print(line)
            print(REGENERATE_HINT)
            return 1
        print(f"up to date: {len(files)} artifacts, {frame_count} encoded GIF frames")
        return 0

    written = dt.write_all(files, root)
    for rel in sorted(files):
        if rel in written:
            print(f"wrote {rel}")
    print(f"{len(written)} of {len(files)} artifacts updated ({frame_count} encoded GIF frames)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
