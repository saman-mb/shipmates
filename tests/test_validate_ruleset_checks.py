#!/usr/bin/env python3
"""Regression tests for .github/scripts/validate_ruleset_checks.py."""

from __future__ import annotations

import importlib.util
import shutil
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def _load_validator():
    path = ROOT / ".github/scripts/validate_ruleset_checks.py"
    spec = importlib.util.spec_from_file_location(
        "shipmates_validate_ruleset_checks", path
    )
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    try:
        spec.loader.exec_module(module)
    except BaseException:
        del sys.modules[spec.name]
        raise
    return module


class ValidateRulesetChecksTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.mod = _load_validator()

    def test_current_tree_passes(self):
        errors = self.mod.validate(ROOT)
        self.assertEqual(errors, [])

    def test_renamed_required_job_fails_with_file_line(self):
        with tempfile.TemporaryDirectory() as tmp:
            tmp_root = Path(tmp)
            github = tmp_root / ".github"
            shutil.copytree(ROOT / ".github/rulesets", github / "rulesets")
            shutil.copytree(ROOT / ".github/workflows", github / "workflows")
            pages = github / "workflows" / "pages.yml"
            text = pages.read_text(encoding="utf-8")
            self.assertIn("name: Validate site\n", text)
            pages.write_text(
                text.replace("name: Validate site\n", "name: Validate site renamed\n", 1),
                encoding="utf-8",
            )
            errors = self.mod.validate(tmp_root)
            self.assertTrue(errors, "renamed required job must fail validation")
            blob = "\n".join(errors)
            self.assertIn("Validate site", blob)
            self.assertRegex(blob, r"main\.json:\d+")
            self.assertRegex(blob, r"pages\.yml:\d+")
            self.assertIn("Validate site renamed", blob)
            self.assertIn("job validate", blob)


if __name__ == "__main__":
    unittest.main()
