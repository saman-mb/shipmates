#!/usr/bin/env python3
"""svgflow — deprecated alias for the `diagram` tool.

`svgflow` has become `diagram` (ADR 0001): flow is now a *kind* under a general
diagram tool that also renders PNG and animated GIF, adds a `sequence` kind, and
routes by intent. This shim exists for one release so nothing that already reaches
for `svgflow.py` breaks: it prints a one-line deprecation notice to stderr and
forwards every argument, unchanged, to `diagram.py`, which lives in the sibling
`diagram/` tool directory. The default kind is `flow`, so an existing svgflow spec
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
    layout (toolbox/diagram/diagram.py) mirrors that. Fall back gracefully."""
    here = os.path.dirname(os.path.abspath(__file__))
    candidates = (
        os.path.join(os.path.dirname(here), "diagram", "diagram.py"),  # ../diagram/diagram.py
        os.path.join(here, "diagram.py"),                              # colocated
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
