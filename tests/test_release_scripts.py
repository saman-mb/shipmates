#!/usr/bin/env python3
"""Regression test for #557: cargo-dist release scripts must expand TAG_FLAG into separate arguments."""

from __future__ import annotations

import os
import subprocess
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


class ReleaseScriptsTagFlagTests(unittest.TestCase):
    def setUp(self):
        self.tmpdir = tempfile.TemporaryDirectory()
        self.bin_dir = Path(self.tmpdir.name) / "bin"
        self.bin_dir.mkdir()
        self.args_log = Path(self.tmpdir.name) / "dist_args.txt"

        mock_dist = self.bin_dir / "dist"
        mock_dist.write_text(
            f"""#!/usr/bin/env bash
for arg in "$@"; do
    echo "$arg" >> "{self.args_log}"
done
echo '{{"upload_files": []}}'
""",
            encoding="utf-8",
        )
        mock_dist.chmod(0o755)

        self.env = os.environ.copy()
        self.env["PATH"] = f"{self.bin_dir}:{self.env.get('PATH', '')}"
        self.env["BUILD_MANIFEST_NAME"] = str(Path(self.tmpdir.name) / "manifest.json")
        self.env["GITHUB_OUTPUT"] = str(Path(self.tmpdir.name) / "github_output.txt")
        Path(self.env["GITHUB_OUTPUT"]).touch()

    def tearDown(self):
        self.tmpdir.cleanup()

    def _read_logged_args(self) -> list[str]:
        if not self.args_log.exists():
            return []
        return self.args_log.read_text(encoding="utf-8").splitlines()

    def test_build_dist_artifacts_splits_tag_flag(self):
        script = ROOT / ".github/scripts/build-dist-artifacts.sh"
        env = self.env.copy()
        env["TAG_FLAG"] = "--tag=v0.14.1 --force-tag"
        env["DIST_ARGS"] = "--artifacts=local --target=x86_64-apple-darwin"

        subprocess.run(["bash", str(script)], env=env, cwd=self.tmpdir.name, check=True)
        args = self._read_logged_args()

        self.assertNotIn(
            "--tag=v0.14.1 --force-tag",
            args,
            "TAG_FLAG must not be passed as a single quoted argument",
        )
        self.assertIn("--tag=v0.14.1", args)
        self.assertIn("--force-tag", args)

    def test_build_global_dist_splits_tag_flag(self):
        script = ROOT / ".github/scripts/build-global-dist.sh"
        env = self.env.copy()
        env["TAG_FLAG"] = "--tag=v0.14.1 --force-tag"

        subprocess.run(["bash", str(script)], env=env, cwd=self.tmpdir.name, check=True)
        args = self._read_logged_args()

        self.assertNotIn(
            "--tag=v0.14.1 --force-tag",
            args,
            "TAG_FLAG must not be passed as a single quoted argument",
        )
        self.assertIn("--tag=v0.14.1", args)
        self.assertIn("--force-tag", args)

    def test_host_release_splits_tag_flag(self):
        script = ROOT / ".github/scripts/host-release.sh"
        env = self.env.copy()
        env["TAG_FLAG"] = "--tag=v0.14.1 --force-tag"

        subprocess.run(["bash", str(script)], env=env, cwd=self.tmpdir.name, check=True)
        args = self._read_logged_args()

        self.assertNotIn(
            "--tag=v0.14.1 --force-tag",
            args,
            "TAG_FLAG must not be passed as a single quoted argument",
        )
        self.assertIn("--tag=v0.14.1", args)
        self.assertIn("--force-tag", args)

    def test_empty_tag_flag_does_not_pass_empty_string_arg(self):
        script = ROOT / ".github/scripts/build-dist-artifacts.sh"
        env = self.env.copy()
        env["TAG_FLAG"] = ""
        env["DIST_ARGS"] = "--artifacts=local"

        subprocess.run(["bash", str(script)], env=env, cwd=self.tmpdir.name, check=True)
        args = self._read_logged_args()

        self.assertNotIn("", args, "Empty TAG_FLAG must not produce an empty string argument")


if __name__ == "__main__":
    unittest.main()
