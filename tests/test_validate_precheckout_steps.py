#!/usr/bin/env python3
"""Regression tests for .github/scripts/validate_precheckout_steps.py.

The gate exists because the release job's longpaths step was extracted into a
script that only exists *after* checkout (#534), so the regression test is that
exact edit: reintroduce it and the validator must name the file and the job.
"""

from __future__ import annotations

import importlib.util
import shutil
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]

INLINE_LONGPATHS = (
    "      - name: enable windows longpaths\n"
    "        run: |\n"
    "          git config --global core.longpaths true\n"
)
EXTRACTED_LONGPATHS = (
    "      - name: enable windows longpaths\n"
    "        run: bash .github/scripts/enable-windows-longpaths.sh\n"
)


def _load_validator():
    path = ROOT / ".github/scripts/validate_precheckout_steps.py"
    spec = importlib.util.spec_from_file_location(
        "shipmates_validate_precheckout_steps", path
    )
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    try:
        spec.loader.exec_module(module)
    except BaseException:
        del sys.modules[spec.name]
        raise
    return module


def _fixture_workflow(body: str) -> str:
    return (
        "name: Fixture\n"
        "on: [push]\n"
        "jobs:\n"
        "  build:\n"
        "    runs-on: ubuntu-latest\n"
        "    steps:\n"
        f"{body}"
    )


class ValidatePrecheckoutStepsTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.mod = _load_validator()

    def _validate_fixture(self, workflow: str, name: str = "fixture.yml") -> list[str]:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            workflows = root / ".github/workflows"
            workflows.mkdir(parents=True)
            (workflows / name).write_text(workflow, encoding="utf-8")
            return self.mod.validate(root)

    def test_current_tree_passes(self):
        self.assertEqual(self.mod.validate(ROOT), [])

    def test_extracted_release_longpaths_step_is_reported(self):
        """The exact #534 edit: a pre-checkout script in build-local-artifacts."""
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            shutil.copytree(ROOT / ".github", root / ".github")
            release = root / ".github/workflows/release.yml"
            text = release.read_text(encoding="utf-8")
            self.assertIn(INLINE_LONGPATHS, text)
            release.write_text(
                text.replace(INLINE_LONGPATHS, EXTRACTED_LONGPATHS, 1), encoding="utf-8"
            )
            errors = self.mod.validate(root)
        self.assertTrue(errors, "extracted longpaths script must fail validation")
        blob = "\n".join(errors)
        self.assertIn("release.yml", blob)
        self.assertIn("build-local-artifacts", blob)
        self.assertIn("enable-windows-longpaths.sh", blob)
        self.assertRegex(blob, r"release\.yml:\d+")

    def test_self_contained_step_before_checkout_passes(self):
        workflow = _fixture_workflow(
            "      - name: enable windows longpaths\n"
            "        run: git config --global core.longpaths true\n"
            "      - uses: actions/checkout@v6\n"
            "      - name: build\n"
            "        run: bash .github/scripts/build.sh\n"
        )
        self.assertEqual(self._validate_fixture(workflow), [])

    def test_repo_script_before_checkout_is_reported(self):
        workflow = _fixture_workflow(
            "      - name: build\n"
            "        run: bash .github/scripts/build.sh\n"
            "      - uses: actions/checkout@v6\n"
        )
        errors = self._validate_fixture(workflow)
        self.assertTrue(errors)
        self.assertIn("build.sh", "\n".join(errors))
        self.assertIn("workspace is empty", "\n".join(errors))

    def test_local_action_before_checkout_is_reported(self):
        workflow = _fixture_workflow(
            "      - uses: ./.github/actions/setup\n"
            "      - uses: actions/checkout@v6\n"
        )
        errors = self._validate_fixture(workflow)
        self.assertTrue(errors)
        self.assertIn("local action", "\n".join(errors))

    def test_escape_hatch_marker_silences_a_step(self):
        workflow = _fixture_workflow(
            "      # pre-checkout-ok: reads a path the runner already provides\n"
            "      - name: build\n"
            "        run: bash .github/scripts/build.sh\n"
            "      - uses: actions/checkout@v6\n"
        )
        self.assertEqual(self._validate_fixture(workflow), [])

    def test_runner_and_variable_paths_are_not_repo_paths(self):
        workflow = _fixture_workflow(
            "      - name: install\n"
            "        run: sudo apt-get install -y shellcheck\n"
            "      - name: warm cache\n"
            "        run: chmod +x ~/.cargo/bin/dist\n"
            "      - name: pin provider\n"
            "        run: /usr/bin/env curl -sS https://example.com/v1/install\n"
            "      - name: matrix packages\n"
            "        run: ${{ matrix.packages_install }}\n"
            "      - uses: actions/checkout@v6\n"
        )
        self.assertEqual(self._validate_fixture(workflow), [])


if __name__ == "__main__":
    unittest.main()
