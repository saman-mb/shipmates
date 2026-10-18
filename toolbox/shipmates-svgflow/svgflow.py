#!/usr/bin/env python3
"""svgflow — deprecated alias for the `diagram` tool.

`svgflow` has become `diagram` (ADR 0001): flow is now a *kind* under a general
diagram tool that also renders PNG and animated GIF, adds a `sequence` kind, and
routes by intent. This shim exists for one release so nothing that already reaches
for `svgflow.py` breaks: it prints a one-line deprecation notice to stderr and
forwards every argument, unchanged to `diagram.py`, which lives in the sibling
`shipmates-diagram` tool directory (or beside this file, on a harness that lays
tools out flat). The default kind is `flow`, so an existing svgflow spec
renders byte-for-byte the same through here as it does through `diagram`.

Prefer `diagram` directly:
    python3 diagram.py --spec spec.json --out flow.svg
"""
import os
import runpy
import sys


def _diagram_path():
    """Locate the sibling diagram.py. Tools install one-per-directory, so the
    installed `diagram` tool sits next to the installed `svgflow` tool; the repo
    layout (toolbox/shipmates-diagram/diagram.py) mirrors that. Harnesses that
    lay tools out flat (opencode's `.opencode/tools/`) keep the two scripts in one
    directory instead, so the colocated candidate is not dead weight — and the
    unnamespaced `diagram/` directory is kept for installs written before the
    `shipmates-*` rename. Fall back gracefully."""
    here = os.path.dirname(os.path.abspath(__file__))
    parent = os.path.dirname(here)
    candidates = (
        os.path.join(parent, "shipmates-diagram", "diagram.py"),  # ../shipmates-diagram/diagram.py
        os.path.join(parent, "diagram", "diagram.py"),            # ../diagram/diagram.py (pre-rename)
        os.path.join(here, "diagram.py"),                         # colocated
    )
    for path in candidates:
        if os.path.isfile(path):
            return path
    return None


def main():
    sys.stderr.write(
        "svgflow: deprecated — svgflow is now `diagram` (flow is a kind of it, "
        "and it also renders PNG/GIF and a sequence kind). Forwarding to "
        "diagram.py; switch to `python3 diagram.py …`.\n")
    target = _diagram_path()
    if target is None:
        sys.exit("svgflow: could not find diagram.py to forward to — install the "
                 "`diagram` tool (it replaces svgflow).")
    # Re-exec diagram.py as __main__ with the same argv so behaviour and exit
    # code are identical to calling diagram directly.
    sys.argv = [target] + sys.argv[1:]
    runpy.run_path(target, run_name="__main__")


if __name__ == "__main__":
    main()
