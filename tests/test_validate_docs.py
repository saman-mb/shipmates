#!/usr/bin/env python3
"""Regression tests for tools/validate_docs.py.

The validator is a negative control: a regex "cleanup" that stops matching still
leaves the real tree green, so every rejection it promises is pinned here, on a
fixture tree in a temp dir — the validator resolves its root from its own
`__file__`, so a copy in a sandbox lints the sandbox, and the real documents are
never rewritten to prove a point.
"""

from __future__ import annotations

import importlib.util
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]

# A command source that declares every config token the root documents speak in,
# in the two forms the validator reads: a `## Config` bullet and a `## Parameters`
# row. Values match what the real commands declare.
FIXTURE_COMMAND = """# /fixture

A fixture command used by the docs-consistency regression suite.

## Config

- `MODE` = `report` (default) or `edit-in-place` — where the result lands.
- `MERGE_MODE` = `manual` (stop at a reviewed PR; `auto` opt-in).
- `PUBLISH_MODE` = `manual` (prepare and stop).
- `NITS_MODE` = `absorb` — how non-blocking findings are disposed.
- `DRY_RUN` = on if the caller says "dry run".

## Parameters

| Name | Required | Token | Values | Default | Help |
|------|----------|-------|--------|---------|------|
| mode | no | `MODE=edit-in-place` | report / edit-in-place | report | Where the result lands. |
| merge_mode | no | `MERGE_MODE=auto` | manual / auto | manual | Merge when the gates pass. |
| publish_mode | no | `PUBLISH_MODE=auto` | manual / auto | manual | Publish when set. |
| nits_mode | no | `nits absorb` | absorb / file | absorb | How nits are disposed. |
| dry_run | no | `dry-run` | on / off | off | Print the plan and create nothing. |
"""

VALID_README = """# Fixture

Run `/fixture` for the whole loop, or `/shipmates-fixture-issue <n>` on one
issue. Read the `## Config` section first.

Writing into the working tree is opt-in (`MODE=edit-in-place`); so are merging
(`MERGE_MODE=auto`) and publishing (`PUBLISH_MODE=auto`). Ask for the fixes
(`MODE=report`). A dry run (`DRY_RUN`) creates nothing.
"""

VALID_CONTRIBUTING = """# Contributing

Check the name against harness built-ins first — `/review` shadowed Claude Code's
built-in review. Merge (`MERGE_MODE=auto`) and publish (`PUBLISH_MODE=auto`) stay
opt-in, and `/skill:` is Pi's own invocation syntax.
"""

VALID_AGENTS = """# Agent notes

Do not invent a second captain slash, and `/name` in the harness docs is a
placeholder, not a command. The drift check is `tools/harness_watch.py`.
"""


# A second command in the sandbox, so a reference outside the fixture's own name
# is resolvable and a typo in the namespace is not.
OTHER_COMMAND = """# /shipmates-fixture-issue

A second fixture command for the docs-consistency regression suite.
"""


def _load_validator():
    path = ROOT / "tools/validate_docs.py"
    spec = importlib.util.spec_from_file_location("shipmates_validate_docs", path)
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    try:
        spec.loader.exec_module(module)
    except BaseException:
        del sys.modules[spec.name]
        raise
    return module


class ValidateDocsTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.mod = _load_validator()

    def sandbox(self, docs, command=FIXTURE_COMMAND):
        """A temp tree holding a copy of the validator, a commands/ and the docs."""
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        root = Path(tmp.name)
        (root / "tools").mkdir()
        shutil.copy(ROOT / "tools/validate_docs.py", root / "tools/validate_docs.py")
        if command is not None:
            (root / "commands").mkdir()
            (root / "commands" / "fixture.md").write_text(command, encoding="utf-8")
            (root / "commands" / "shipmates-fixture-issue.md").write_text(
                OTHER_COMMAND, encoding="utf-8"
            )
        for name, text in docs.items():
            (root / name).write_text(text, encoding="utf-8")
        return root

    def run_validator(self, root):
        return subprocess.run(
            [sys.executable, str(root / "tools/validate_docs.py")],
            capture_output=True,
            text=True,
        )

    def assert_fails(self, root, *expected):
        result = self.run_validator(root)
        self.assertEqual(result.returncode, 1, f"expected failure, got:\n{result.stdout}")
        self.assertIn("FAIL", result.stdout)
        for fragment in expected:
            self.assertIn(fragment, result.stdout)

    def assert_fails_at(self, root, rel, path_re, *expected):
        """The failure must name the file and the line someone has to edit."""
        result = self.run_validator(root)
        self.assertEqual(result.returncode, 1, f"expected failure, got:\n{result.stdout}")
        self.assertIn("FAIL", result.stdout)
        self.assertRegex(result.stdout, rf"FAIL {path_re}:\d+:")
        for fragment in expected:
            self.assertIn(fragment, result.stdout)

    def run_on_real_tree(self):
        return subprocess.run(
            [sys.executable, str(ROOT / "tools/validate_docs.py")],
            capture_output=True,
            text=True,
        )

    # --- the positive half -------------------------------------------------

    def test_valid_docs_and_commands_pass(self):
        root = self.sandbox(
            {
                "README.md": VALID_README,
                "CONTRIBUTING.md": VALID_CONTRIBUTING,
                "AGENTS.md": VALID_AGENTS,
            }
        )
        result = self.run_validator(root)
        self.assertEqual(result.returncode, 0, result.stdout)
        self.assertIn("checks passed", result.stdout)

    def test_current_tree_passes(self):
        result = self.run_on_real_tree()
        self.assertEqual(result.returncode, 0, result.stdout)

    def test_foreign_and_placeholder_references_pass(self):
        root = self.sandbox(
            {
                "README.md": VALID_README,
                "CONTRIBUTING.md": "See `/review`, `/harness-watch` and `/skill:`.\n",
                "AGENTS.md": "Direct `/name` invocation is unaffected.\n",
            }
        )
        result = self.run_validator(root)
        self.assertEqual(result.returncode, 0, result.stdout)

    def test_paths_urls_and_html_tags_are_not_references(self):
        docs = {
            "README.md": (
                "```\nhttps://github.com/saman-mb/shipmates/blob/main/README.md\n```\n"
                "Layout: `commands/*.md`, `src/adapters/render.rs`, `tools/gen_command_pages.py`,\n"
                "`site/commands/<name>/index.html`, `</p>`, `~/.pi/agent/skills`.\n"
            ),
            "CONTRIBUTING.md": "Not a command: `/tmp/some-test-repo`.\n",
            "AGENTS.md": "The roster is the front door.\n",
        }
        root = self.sandbox(docs)
        result = self.run_validator(root)
        self.assertEqual(result.returncode, 0, result.stdout)
        self.assertIn("README.md: 0 command reference(s)", result.stdout)

    # --- the negative half -------------------------------------------------

    def test_unresolved_config_value_fails_with_file_line(self):
        root = self.sandbox(
            {
                "README.md": VALID_README + "\nSet `MODE=banana` for that.\n",
                "CONTRIBUTING.md": VALID_CONTRIBUTING,
                "AGENTS.md": VALID_AGENTS,
            }
        )
        self.assert_fails_at(
            root, "README.md", r"README\.md", "`MODE=banana`", "edit-in-place, report"
        )

    def test_case_different_config_value_fails_with_file_line(self):
        # `MODE=EDIT` is the same defect as `MODE=banana` but spelled in caps: the
        # value pattern must capture it and resolve it, not read a bare `MODE`
        # token and pass.
        root = self.sandbox(
            {
                "README.md": VALID_README + "\nSet `MODE=EDIT` for that.\n",
                "CONTRIBUTING.md": VALID_CONTRIBUTING,
                "AGENTS.md": VALID_AGENTS,
            }
        )
        self.assert_fails_at(
            root, "README.md", r"README\.md", "`MODE=EDIT`", "edit-in-place, report"
        )

    def test_undeclared_config_token_fails(self):
        # NITS_MODE is a token the documents may speak in, but this fixture
        # command declares nothing for it.
        command = FIXTURE_COMMAND.replace(
            "- `NITS_MODE` = `absorb` — how non-blocking findings are disposed.\n", ""
        ).replace("| nits_mode | no | `nits absorb` | absorb / file | absorb | How nits are disposed. |\n", "")
        root = self.sandbox(
            {
                "README.md": "Report nits with `NITS_MODE=absorb`.\n",
                "CONTRIBUTING.md": VALID_CONTRIBUTING,
                "AGENTS.md": VALID_AGENTS,
            },
            command=command,
        )
        self.assert_fails_at(root, "README.md", r"README\.md", "`NITS_MODE` is declared by no command")

    def test_bare_token_reference_must_be_declared(self):
        command = FIXTURE_COMMAND.replace(
            '- `DRY_RUN` = on if the caller says "dry run".\n', ""
        ).replace("| dry_run | no | `dry-run` | on / off | off | Print the plan and create nothing. |\n", "")
        root = self.sandbox(
            {
                "README.md": "A dry run (`DRY_RUN`) creates nothing.\n",
                "CONTRIBUTING.md": VALID_CONTRIBUTING,
                "AGENTS.md": VALID_AGENTS,
            },
            command=command,
        )
        self.assert_fails_at(root, "README.md", r"README\.md", "`DRY_RUN` is declared by no command")

    def test_unknown_command_reference_fails(self):
        root = self.sandbox(
            {
                "README.md": "Start with `/fixture-command` instead.\n",
                "CONTRIBUTING.md": VALID_CONTRIBUTING,
                "AGENTS.md": VALID_AGENTS,
            }
        )
        self.assert_fails_at(
            root, "README.md", r"README\.md", "`/fixture-command` is not a current command"
        )

    def test_typo_in_a_product_slug_fails(self):
        root = self.sandbox(
            {
                "README.md": "Start with `/shipmates-fixture-issues`.\n",
                "CONTRIBUTING.md": VALID_CONTRIBUTING,
                "AGENTS.md": VALID_AGENTS,
            }
        )
        self.assert_fails_at(
            root,
            "README.md",
            r"README\.md",
            "`/shipmates-fixture-issues` is not a current command",
        )

    def test_retired_slug_fails_as_a_command(self):
        # `document` is a renamed slug kept only as a static-site redirect stub
        # (REDIRECTS in tools/gen_command_pages.py) — it resolves no invocation.
        root = self.sandbox(
            {
                "README.md": VALID_README,
                "CONTRIBUTING.md": "Use `/document` for the docs pass.\n",
                "AGENTS.md": VALID_AGENTS,
            }
        )
        self.assert_fails_at(
            root, "CONTRIBUTING.md", r"CONTRIBUTING\.md", "`/document` is not a current command"
        )

    def test_missing_document_fails(self):
        root = self.sandbox({"README.md": VALID_README})
        self.assert_fails(root, "CONTRIBUTING.md: file not found")

    def test_missing_commands_tree_fails(self):
        root = self.sandbox(
            {
                "README.md": VALID_README,
                "CONTRIBUTING.md": VALID_CONTRIBUTING,
                "AGENTS.md": VALID_AGENTS,
            },
            command=None,
        )
        self.assert_fails(root, "commands/")

    # --- the allowlist's own hygiene ---------------------------------------

    def test_every_foreign_entry_carries_a_reason(self):
        self.assertTrue(self.mod.FOREIGN)
        for name, reason in self.mod.FOREIGN.items():
            self.assertTrue(reason.strip(), f"FOREIGN entry {name!r} has no reason")

    def test_no_foreign_entry_shadows_a_current_command(self):
        current = {p.stem for p in (ROOT / "commands").glob("*.md")}
        shadowed = sorted(set(self.mod.FOREIGN) & current)
        self.assertEqual(
            shadowed,
            [],
            f"a FOREIGN entry would mask a real command: {shadowed}",
        )

    def test_config_token_list_has_no_duplicates(self):
        self.assertEqual(
            sorted(self.mod.CONFIG_TOKENS),
            sorted(set(self.mod.CONFIG_TOKENS)),
        )

    def test_every_config_token_is_declared_by_a_real_command(self):
        # The vocabulary is explicit; the declarations are not. If a command
        # renames or drops one of these tokens, the vocabulary has to be
        # revisited rather than quietly checking nothing.
        declared = self.mod.declared_values()
        for token in self.mod.CONFIG_TOKENS:
            self.assertTrue(
                declared[token],
                f"{token} is in CONFIG_TOKENS but no commands/*.md declares it",
            )


if __name__ == "__main__":
    unittest.main()
