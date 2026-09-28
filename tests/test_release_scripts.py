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


class HomebrewFormulaReleaseScriptTests(unittest.TestCase):
    def setUp(self):
        self.tmpdir = tempfile.TemporaryDirectory()
        self.root = Path(self.tmpdir.name)
        self.remote = self.root / "remote.git"
        subprocess.run(
            ["git", "init", "--bare", str(self.remote)],
            check=True,
            capture_output=True,
        )

        self.bin_dir = self.root / "bin"
        self.bin_dir.mkdir()
        mock_brew = self.bin_dir / "brew"
        mock_brew.write_text("#!/usr/bin/env bash\nexit 0\n", encoding="utf-8")
        mock_brew.chmod(0o755)

        self.tap = self.root / "tap"
        subprocess.run(
            ["git", "clone", str(self.remote), str(self.tap)],
            check=True,
            capture_output=True,
        )
        subprocess.run(
            ["git", "-C", str(self.tap), "config", "user.name", "axo bot"],
            check=True,
        )
        subprocess.run(
            ["git", "-C", str(self.tap), "config", "user.email", "admin+bot@axo.dev"],
            check=True,
        )

        formula_dir = self.tap / "Formula"
        formula_dir.mkdir(parents=True)
        (formula_dir / "shipmates.rb").write_text("# old formula\n", encoding="utf-8")
        subprocess.run(["git", "-C", str(self.tap), "add", "."], check=True)
        subprocess.run(["git", "-C", str(self.tap), "commit", "-m", "initial"], check=True)
        subprocess.run(["git", "-C", str(self.tap), "push", "origin", "HEAD"], check=True)

    def tearDown(self):
        self.tmpdir.cleanup()

    def test_commit_homebrew_formula_commits_and_pushes_to_tap(self):
        script = ROOT / ".github/scripts/commit-homebrew-formula.sh"
        (self.tap / "Formula/shipmates.rb").write_text("# formula 0.14.3\n", encoding="utf-8")

        env = os.environ.copy()
        env["PATH"] = f"{self.bin_dir}:{env.get('PATH', '')}"
        env["GITHUB_USER"] = "axo bot"
        env["GITHUB_EMAIL"] = "admin+bot@axo.dev"
        env["PLAN"] = '{"releases": [{"app_version": "0.14.3", "artifacts": ["shipmates.rb"]}]}'

        subprocess.run(
            ["bash", str(script), str(self.tap)],
            env=env,
            cwd=self.tmpdir.name,
            check=True,
        )

        log = subprocess.run(
            ["git", "-C", str(self.tap), "log", "-1", "--format=%s"],
            capture_output=True,
            text=True,
            check=True,
        ).stdout.strip()
        self.assertEqual(log, "shipmates 0.14.3")


if __name__ == "__main__":
    unittest.main()

