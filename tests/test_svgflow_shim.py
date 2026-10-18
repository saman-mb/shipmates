"""Regression tests for the `shipmates-svgflow` deprecation shim (#588).

The shim is a thin forwarder: it prints a deprecation notice and re-execs
`diagram.py` with the same argv. It resolves that sibling by relative path, so
what it can find depends entirely on the layout it was installed into. Three
layouts have to keep working — the namespaced one the CLI ships today, the
pre-rename one some installs still carry, and a colocated pair — and the shim
must still degrade honestly when `diagram` is genuinely absent.

Each test builds the layout in a temp directory out of the real tools rather
than monkey-patching the resolver, so the test exercises the same path
resolution a user's install does.
"""
from __future__ import annotations

import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SHIM_SRC = ROOT / "toolbox" / "shipmates-svgflow" / "svgflow.py"
DIAGRAM_SRC = ROOT / "toolbox" / "shipmates-diagram" / "diagram.py"
FLOW_SPEC = ROOT / "site" / "tools" / "shipmates-diagram" / "examples" / "pipeline.json"


def build_layout(tmp: Path, diagram_dir: str | None, *, colocated: bool = False) -> Path:
    """Copy the real shim (and, unless asked not to, the real diagram tool) into
    a throwaway install tree and return the path to run the shim from."""
    if colocated:
        tool_dir = tmp / "tools"
        tool_dir.mkdir(parents=True)
        shutil.copy(DIAGRAM_SRC, tool_dir / "diagram.py")
        shutil.copy(SHIM_SRC, tool_dir / "svgflow.py")
        return tool_dir / "svgflow.py"

    shim_dir = tmp / "shipmates-svgflow"
    shim_dir.mkdir(parents=True)
    shutil.copy(SHIM_SRC, shim_dir / "svgflow.py")
    if diagram_dir is not None:
        diagram_dir_path = tmp / diagram_dir
        diagram_dir_path.mkdir(parents=True)
        shutil.copy(DIAGRAM_SRC, diagram_dir_path / "diagram.py")
    return shim_dir / "svgflow.py"


def run_shim(shim: Path, *args: str) -> subprocess.CompletedProcess:
    """Run from a scratch cwd so the test proves the shim resolves its sibling by
    file location, never by the caller's working directory."""
    return subprocess.run(
        [sys.executable, str(shim), *args],
        capture_output=True,
        text=True,
        cwd=tempfile.gettempdir(),
    )


class SvgflowShimTests(unittest.TestCase):
    """The shim must forward, not just announce that it will."""

    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.tmp = Path(self._tmp.name)
        self.addCleanup(self._tmp.cleanup)

    def assert_forwarded_help(self, shim: Path):
        result = run_shim(shim, "--help")
        self.assertEqual(result.returncode, 0, msg=result.stderr)
        self.assertIn("usage: diagram.py", result.stdout)
        self.assertIn("deprecated", result.stderr)

    def test_forwards_in_namespaced_layout(self):
        """Current install layout: `shipmates-svgflow/` beside `shipmates-diagram/`."""
        shim = build_layout(self.tmp, "shipmates-diagram")
        self.assert_forwarded_help(shim)

    def test_forwards_in_prerename_layout(self):
        """Pre-rename layout, still on disk for installs predating #373."""
        shim = build_layout(self.tmp, "diagram")
        self.assert_forwarded_help(shim)

    def test_forwards_when_colocated(self):
        """Shim and `diagram.py` sharing one directory."""
        shim = build_layout(self.tmp, None, colocated=True)
        self.assert_forwarded_help(shim)

    def test_forwards_a_real_render_not_just_help(self):
        """`--help` proves the resolver; a render proves the forwarding."""
        shim = build_layout(self.tmp, "shipmates-diagram")
        out = self.tmp / "via-shim.svg"
        result = run_shim(shim, "--spec", str(FLOW_SPEC), "--out", str(out))
        self.assertEqual(result.returncode, 0, msg=result.stderr)
        self.assertTrue(out.is_file(), msg=result.stderr)
        self.assertIn("<svg", out.read_text())

        direct = self.tmp / "direct.svg"
        subprocess.run(
            [sys.executable, str(DIAGRAM_SRC), "--spec", str(FLOW_SPEC), "--out", str(direct)],
            capture_output=True,
            text=True,
            check=True,
        )
        self.assertEqual(
            out.read_bytes(),
            direct.read_bytes(),
            "the shim must render byte-for-byte what a direct diagram call produces",
        )

    def test_degrades_loudly_when_diagram_absent(self):
        """No sibling at all: still a clear, actionable failure — never a
        traceback, and never a silent success."""
        shim = build_layout(self.tmp, None)
        result = run_shim(shim, "--help")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("could not find diagram.py", result.stderr)
        self.assertNotIn("Traceback", result.stderr)


if __name__ == "__main__":
    unittest.main()