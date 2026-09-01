"""Tests for toolbox/shipmates-gh/gh.py validation (no live gh subprocess)."""
import contextlib
import io
import json
import subprocess
import sys
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "toolbox" / "shipmates-gh"))

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


class FakeGh:
    """Records every argv gh would have been spawned with."""

    def __init__(self, view_payload=None):
        self.view_payload = view_payload if view_payload is not None else {}
        self.calls = []

    def __call__(self, args, input_text=None):
        self.calls.append(list(args))
        stdout = json.dumps(self.view_payload) if args[:2] == ["issue", "view"] else ""
        return subprocess.CompletedProcess(["gh", *args], 0, stdout, "")

    def edit_calls(self):
        return [call for call in self.calls if call[:2] == ["issue", "edit"]]


def connection(*children, total=None):
    """gh's real `subIssues` JSON: a GraphQL connection, not a flat list."""
    nodes = list(children)
    return {"nodes": nodes, "totalCount": len(nodes) if total is None else total}


class GhSubIssueTests(unittest.TestCase):
    def fake_gh(self, view_payload=None):
        fake = FakeGh(view_payload)
        original = gh_tool.run_gh
        gh_tool.run_gh = fake
        self.addCleanup(setattr, gh_tool, "run_gh", original)
        return fake

    def test_sub_issue_numbers_validated(self):
        for spec in (
            {"op": "issue.sub_issue_add", "number": 0, "sub_issue_number": 384},
            {"op": "issue.sub_issue_add", "number": "abc", "sub_issue_number": 384},
            {"op": "issue.sub_issue_add", "number": 383, "sub_issue_number": 0},
            {"op": "issue.sub_issue_add", "number": 383, "sub_issue_number": "abc"},
            {"op": "issue.sub_issue_add", "number": 383},
            {"op": "issue.sub_issue_remove", "number": 383, "sub_issue_number": "abc"},
            {"op": "issue.sub_issue_list", "number": "abc"},
        ):
            with self.subTest(spec=spec):
                self.fake_gh()
                with self.assertRaises(gh_tool.GhError):
                    gh_tool.execute(spec)

    def test_sub_issue_add_rejects_self_parent(self):
        self.fake_gh()
        with self.assertRaises(gh_tool.GhError):
            gh_tool.execute(
                {"op": "issue.sub_issue_add", "number": 383, "sub_issue_number": 383}
            )

    def test_sub_issue_add_edits_parent_with_issue_number(self):
        fake = self.fake_gh({"number": 383, "subIssues": connection()})
        payload = gh_tool.execute(
            {"op": "issue.sub_issue_add", "number": 383, "sub_issue_number": 384}
        )
        self.assertTrue(payload["result"]["attached"])
        self.assertEqual(
            fake.edit_calls(), [["issue", "edit", "383", "--add-sub-issue", "384"]]
        )
        # The child is identified by its issue number — never a GraphQL node id
        # or a fabricated REST database id, and never through `gh api`.
        flat = " ".join(arg for call in fake.calls for arg in call)
        self.assertNotIn("api", flat)
        self.assertNotIn("sub_issue_id", flat)

    def test_sub_issue_add_passes_repo_flag(self):
        fake = self.fake_gh({"number": 383, "subIssues": connection()})
        gh_tool.execute(
            {
                "op": "issue.sub_issue_add",
                "number": 383,
                "sub_issue_number": 384,
                "repo": "owner/name",
            }
        )
        self.assertEqual(
            fake.edit_calls(),
            [["issue", "edit", "383", "--repo", "owner/name", "--add-sub-issue", "384"]],
        )

    def test_sub_issue_add_is_idempotent(self):
        fake = self.fake_gh(
            {
                "number": 383,
                "subIssues": connection(
                    {"number": 384, "title": "story", "state": "OPEN"}
                ),
                "subIssuesSummary": {"total": 1, "completed": 0},
            }
        )
        result = gh_tool.execute(
            {"op": "issue.sub_issue_add", "number": 383, "sub_issue_number": 384}
        )["result"]
        self.assertFalse(result["attached"])
        self.assertEqual(result["reason"], "already-child")
        self.assertEqual(fake.edit_calls(), [])

    def test_sub_issue_add_replace_parent_edits_child(self):
        fake = self.fake_gh({"number": 383, "subIssues": connection()})
        result = gh_tool.execute(
            {
                "op": "issue.sub_issue_add",
                "number": 383,
                "sub_issue_number": 384,
                "replace_parent": True,
            }
        )["result"]
        self.assertEqual(result["mode"], "parent")
        self.assertEqual(
            fake.edit_calls(), [["issue", "edit", "384", "--parent", "383"]]
        )

    def test_sub_issue_add_rejects_non_boolean_replace_parent(self):
        self.fake_gh({"number": 383, "subIssues": connection()})
        with self.assertRaises(gh_tool.GhError):
            gh_tool.execute(
                {
                    "op": "issue.sub_issue_add",
                    "number": 383,
                    "sub_issue_number": 384,
                    "replace_parent": "yes",
                }
            )

    def test_sub_issue_list_requests_summary_fields(self):
        fake = self.fake_gh(
            {
                "number": 383,
                "title": "epic",
                "state": "OPEN",
                "subIssues": connection({"number": 384}, {"number": 385}),
                "subIssuesSummary": {"total": 2, "completed": 1},
            }
        )
        result = gh_tool.execute({"op": "issue.sub_issue_list", "number": 383})["result"]
        self.assertEqual(result["numbers"], [384, 385])
        self.assertEqual(result["subIssues"], [{"number": 384}, {"number": 385}])
        self.assertEqual(result["subIssuesSummary"], {"total": 2, "completed": 1})
        view = fake.calls[0]
        self.assertEqual(view[:3], ["issue", "view", "383"])
        requested = view[view.index("--json") + 1]
        self.assertIn("subIssues", requested)
        self.assertIn("subIssuesSummary", requested)

    def test_sub_issue_list_empty_connection_is_zero_children(self):
        fake = self.fake_gh(
            {
                "number": 383,
                "subIssues": {"nodes": [], "totalCount": 0},
                "subIssuesSummary": {"total": 0, "completed": 0, "percentCompleted": 0},
            }
        )
        result = gh_tool.execute({"op": "issue.sub_issue_list", "number": 383})["result"]
        self.assertEqual(result["numbers"], [])
        self.assertEqual(result["subIssues"], [])
        self.assertEqual(fake.edit_calls(), [])

    def test_sub_issue_children_unwraps_gh_connection(self):
        self.assertEqual(
            gh_tool.sub_issue_children(
                {"nodes": [{"number": 384}], "totalCount": 1}
            ),
            [{"number": 384}],
        )
        self.assertEqual(
            gh_tool.sub_issue_children({"nodes": [], "totalCount": 0}),
            [],
        )

    def test_sub_issue_remove_edits_parent(self):
        fake = self.fake_gh()
        result = gh_tool.execute(
            {"op": "issue.sub_issue_remove", "number": 383, "sub_issue_number": 384}
        )["result"]
        self.assertTrue(result["removed"])
        self.assertEqual(
            fake.edit_calls(), [["issue", "edit", "383", "--remove-sub-issue", "384"]]
        )

    def test_list_ops_includes_sub_issue_ops(self):
        buf = io.StringIO()
        with contextlib.redirect_stdout(buf):
            self.assertEqual(gh_tool.main(["--list-ops"]), 0)
        operations = json.loads(buf.getvalue())["operations"]
        for op in ("issue.sub_issue_add", "issue.sub_issue_list", "issue.sub_issue_remove"):
            self.assertIn(op, operations)


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
