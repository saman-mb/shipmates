#!/usr/bin/env python3
"""Regression tests for the harness roster audit (#164).

`validate_harness_roster.py` is the gate; these tests prove the gate actually
bites. A validator that only ever passes is indistinguishable from no validator,
and this one guards the two claims most likely to rot silently: that the roster's
`shipped` rows are the targets the binary really installs, and that every shared
tree claim carries the literal path string it was read from.
"""

from __future__ import annotations

import importlib.util
import json
import shutil
import tempfile
import unittest
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent


def _load_validator():
    spec = importlib.util.spec_from_file_location(
        "validate_harness_roster", REPO_ROOT / "tools" / "validate_harness_roster.py"
    )
    assert spec and spec.loader
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class HarnessRosterTests(unittest.TestCase):
    def setUp(self) -> None:
        self.validator = _load_validator()
        self.temp = Path(tempfile.mkdtemp(prefix="roster-test-"))
        self.addCleanup(shutil.rmtree, self.temp, ignore_errors=True)
        for name in ("harness_roster.json", "manifest.json", "harness_watch.json"):
            shutil.copy(REPO_ROOT / "tools" / name, self.temp / name)
        self.roster_path = self.temp / "harness_roster.json"
        self.roster = json.loads(self.roster_path.read_text(encoding="utf-8"))

    def run_validator(self) -> tuple[int, list[str]]:
        self.validator.errors = []
        self.roster_path.write_text(json.dumps(self.roster, indent=2) + "\n", encoding="utf-8")
        self.validator.ROSTER = self.roster_path
        self.validator.MANIFEST = self.temp / "manifest.json"
        self.validator.WATCH = self.temp / "harness_watch.json"
        code = self.validator.main()
        return code, list(self.validator.errors)

    def row(self, ident: str) -> dict:
        for row in self.roster["harnesses"]:
            if row["id"] == ident:
                return row
        raise AssertionError(f"no roster row {ident!r}")

    def test_the_committed_roster_validates(self) -> None:
        code, errors = self.run_validator()
        self.assertEqual(code, 0, errors)

    def test_a_shipped_row_with_no_target_fails(self) -> None:
        self.row("devin")["id"] = "devin-phantom"
        code, errors = self.run_validator()
        self.assertEqual(code, 1)
        self.assertTrue(
            any("does not install" in error for error in errors),
            errors,
        )

    def test_a_target_with_no_shipped_row_fails(self) -> None:
        self.roster["harnesses"] = [
            row for row in self.roster["harnesses"] if row["id"] != "grok-build"
        ]
        code, errors = self.run_validator()
        self.assertEqual(code, 1)
        self.assertTrue(any("no `shipped` roster row" in error for error in errors), errors)

    def test_a_shared_tree_claim_without_the_path_string_fails(self) -> None:
        shared = [row for row in self.roster["harnesses"] if row["tree"] == "shared"]
        self.assertTrue(shared, "the roster should record shared-tree clients")
        shared[0]["evidence"]["quote"] = "the docs say skills work"
        code, errors = self.run_validator()
        self.assertEqual(code, 1)
        self.assertTrue(any(".agents/skills" in error for error in errors), errors)

    def test_a_deferred_verdict_without_a_reason_fails(self) -> None:
        watched = [row for row in self.roster["harnesses"] if row["verdict"] == "watch"]
        self.assertTrue(watched, "the roster should record undecided clients")
        watched[0]["reason"] = ""
        code, errors = self.run_validator()
        self.assertEqual(code, 1)
        self.assertTrue(any("needs a reason" in error for error in errors), errors)

    def test_an_undated_row_fails(self) -> None:
        self.row("junie")["evidence"]["checked_on"] = "recently"
        code, errors = self.run_validator()
        self.assertEqual(code, 1)
        self.assertTrue(any("YYYY-MM-DD" in error for error in errors), errors)

    def test_the_census_must_reconcile_with_the_source_count(self) -> None:
        self.roster["source"]["clients_listed"] = 47
        code, errors = self.run_validator()
        self.assertEqual(code, 1)
        self.assertTrue(any("no longer reconciles" in error for error in errors), errors)
