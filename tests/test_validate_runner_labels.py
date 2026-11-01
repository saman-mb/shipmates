#!/usr/bin/env python3
"""Regression tests for .github/scripts/validate_runner_labels.py.

The gate exists because a release job pinned to a retired runner label is
cancelled a second after it starts, never gets a runner, and silently skips
every job that needs it — so a merged change publishes nothing and the only
symptom is a cancelled run. The regression tests are the two edits that cause
it: a workflow naming the retired label, and the dist runner pin removed or
pointed back at the retired image.
"""

from __future__ import annotations

import importlib.util
import shutil
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]

RETIRED = "macos-14"
SUPPORTED = "macos-15"
PINNED_RUNNERS = f'[dist.github-custom-runners]\naarch64-apple-darwin = "{SUPPORTED}"\n'


def _load_validator():
    path = ROOT / ".github/scripts/validate_runner_labels.py"
    spec = importlib.util.spec_from_file_location(
        "shipmates_validate_runner_labels", path
    )
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    try:
        spec.loader.exec_module(module)
    except BaseException:
        del sys.modules[spec.name]
        raise
    return module


class ValidateRunnerLabelsTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.mod = _load_validator()

    def _root(self, tmp: str, dist_config: str | None = PINNED_RUNNERS) -> Path:
        root = Path(tmp)
        (root / ".github/workflows").mkdir(parents=True)
        if dist_config is not None:
            (root / "dist-workspace.toml").write_text(dist_config, encoding="utf-8")
        return root

    def test_current_tree_passes(self):
        self.assertEqual(self.mod.validate(ROOT), [])

    def test_workflow_literal_retired_label_is_reported(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = self._root(tmp)
            (root / ".github/workflows/release.yml").write_text(
                "jobs:\n  build:\n    runs-on: macos-14\n    steps:\n      - run: echo ok\n",
                encoding="utf-8",
            )
            errors = self.mod.validate(root)
        self.assertTrue(errors, "a retired runner literal must fail validation")
        blob = "\n".join(errors)
        self.assertIn("release.yml", blob)
        self.assertIn("macos-14", blob)
        self.assertRegex(blob, r"release\.yml:\d+")

    def test_dist_pin_back_on_the_retired_image_is_reported(self):
        """The exact #601 follow-up edit: arm target pinned to macos-14."""
        with tempfile.TemporaryDirectory() as tmp:
            root = self._root(
                tmp,
                f'[dist.github-custom-runners]\naarch64-apple-darwin = "{RETIRED}"\n',
            )
            errors = self.mod.validate(root)
        self.assertTrue(errors, "a dist runner pinned to a retired image must fail")
        blob = "\n".join(errors)
        self.assertIn("aarch64-apple-darwin", blob)
        self.assertIn(RETIRED, blob)
        self.assertIn("dist-workspace.toml", blob)

    def test_missing_pin_is_reported(self):
        """cargo-dist defaults this target to the retired image, so no pin is a failure."""
        with tempfile.TemporaryDirectory() as tmp:
            root = self._root(tmp, "[dist]\nci = \"github\"\n")
            errors = self.mod.validate(root)
        self.assertTrue(errors, "an unpinned arm target must fail validation")
        blob = "\n".join(errors)
        self.assertIn("aarch64-apple-darwin", blob)
        self.assertIn("no runner pin", blob)

    def test_missing_dist_config_is_reported(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = self._root(tmp, None)
            errors = self.mod.validate(root)
        self.assertTrue(errors, "a missing dist config leaves the target unpinned")
        self.assertIn("dist-workspace.toml", "\n".join(errors))

    def test_supported_pin_passes(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = self._root(tmp)
            (root / ".github/workflows/release.yml").write_text(
                "jobs:\n  build:\n    runs-on: ${{ matrix.runner }}\n"
                "    steps:\n      - run: echo ok\n",
                encoding="utf-8",
            )
            self.assertEqual(self.mod.validate(root), [])

    def test_comment_mentioning_a_retired_label_passes(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = self._root(tmp)
            (root / ".github/workflows/release.yml").write_text(
                "# macos-14 is retired; the matrix now comes from dist plan\n"
                "jobs:\n  build:\n    runs-on: ${{ matrix.runner }}\n",
                encoding="utf-8",
            )
            self.assertEqual(self.mod.validate(root), [])

    def test_larger_retired_label_is_not_matched_by_prefix(self):
        """`macos-14-xlarge` is its own label; it must be reported by its own name."""
        with tempfile.TemporaryDirectory() as tmp:
            root = self._root(tmp)
            (root / ".github/workflows/release.yml").write_text(
                "jobs:\n  build:\n    runs-on: macos-14-xlarge\n",
                encoding="utf-8",
            )
            errors = self.mod.validate(root)
        self.assertEqual(len(errors), 1, errors)
        self.assertIn("macos-14-xlarge", errors[0])


class RunnerLabelTreeTests(unittest.TestCase):
    """The shipped tree itself must pin the arm target off the retired image."""

    def test_dist_workspace_pins_arm_runner(self):
        text = (ROOT / "dist-workspace.toml").read_text(encoding="utf-8")
        self.assertIn("[dist.github-custom-runners]", text)
        self.assertIn("aarch64-apple-darwin", text)

    def test_validator_is_wired_into_ci(self):
        pages = (ROOT / ".github/workflows/pages.yml").read_text(encoding="utf-8")
        self.assertIn("validate_runner_labels.py", pages)


if __name__ == "__main__":
    unittest.main()
