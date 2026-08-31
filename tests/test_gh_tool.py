"""Tests for toolbox/gh/gh.py validation (no live gh subprocess)."""
import json
import sys
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "toolbox" / "gh"))

import gh as gh_tool  # noqa: E402


class GhValidationTests(unittest.TestCase):
    def test_validate_repo(self):
        self.assertEqual(gh_tool.validate_repo("saman-mb/shipmates"), "saman-mb/shipmates")
        with self.assertRaises(gh_tool.GhError):
            gh_tool.validate_repo("not-a-repo")

    def test_validate_number(self):
        self.assertEqual(gh_tool.validate_number(305), 305)
        self.assertEqual(gh_tool.validate_number("42"), 42)
        with self.assertRaises(gh_tool.GhError):
            gh_tool.validate_number(0)
        with self.assertRaises(gh_tool.GhError):
            gh_tool.validate_number("abc")

    def test_read_body_file_required(self):
        with self.assertRaises(gh_tool.GhError):
            gh_tool.read_body_file({})

    def test_read_body_file_inline_cap(self):
        with self.assertRaises(gh_tool.GhError):
            gh_tool.read_body_file({"body": "x" * 201})

    def test_unknown_op(self):
        with self.assertRaises(gh_tool.GhError):
            gh_tool.execute({"op": "nope"})

    def test_list_ops_json(self):
        proc = gh_tool.main(["--list-ops"])
        self.assertEqual(proc, 0)


class GhSummarizeChecksTests(unittest.TestCase):
    def test_rollup_pass(self):
        summary = gh_tool.summarize_checks(
            [{"name": "CI", "state": "pass"}, {"name": "Lint", "state": "success"}]
        )
        self.assertEqual(summary["rollup"], "pass")

    def test_rollup_pending(self):
        summary = gh_tool.summarize_checks([{"name": "CI", "state": "pending"}])
        self.assertEqual(summary["rollup"], "pending")


if __name__ == "__main__":
    unittest.main()
