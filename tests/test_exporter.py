#!/usr/bin/env python3
"""Focused regression tests for the canonical exporter foundation."""

from __future__ import annotations

import subprocess
import sys
import shutil
import tempfile
import unittest
from pathlib import Path

from tools import export as exporter
from tools.adapters.claude_code import ClaudeCodeAdapter
from tools.capability_registry import load_registry


ROOT = Path(__file__).resolve().parents[1]


class ExporterTests(unittest.TestCase):
    def temp_repo(self) -> tuple[tempfile.TemporaryDirectory, Path]:
        temporary = tempfile.TemporaryDirectory()
        destination = Path(temporary.name) / "repo"
        shutil.copytree(
            ROOT,
            destination,
            ignore=shutil.ignore_patterns(".git", "__pycache__"),
        )
        return temporary, destination.resolve()

    def test_canonical_catalog_covers_crew_and_orders(self) -> None:
        roles, orders = exporter.load_catalog(ROOT)
        self.assertEqual(12, len(roles))
        self.assertEqual(12, len(orders))
        self.assertIn("ship-issue", {order.name for order in orders})
        self.assertTrue(all(order.stages for order in orders))
        self.assertTrue(
            all(
                stage["order"] == index
                for order in orders
                for index, stage in enumerate(order.stages, 1)
            )
        )

    def test_registry_maps_semantic_capabilities(self) -> None:
        registry = load_registry(ROOT / "tools/capability_registry.json")
        claude = registry["claude-code"]
        self.assertEqual(
            ("Read", "Grep", "Glob", "Write", "Edit", "Bash", "WebSearch", "WebFetch", "Agent"),
            ClaudeCodeAdapter(claude).map_tools(("read", "edit", "bash", "web", "agent")),
        )
        self.assertEqual(
            ("Read", "Grep", "Glob", "Write", "Edit", "Bash"),
            ClaudeCodeAdapter(claude).map_tools(("read", "edit", "bash")),
        )

    def test_neutral_tokens_render_to_claude_dialect(self) -> None:
        registry = load_registry(ROOT / "tools/capability_registry.json")
        adapter = ClaudeCodeAdapter(registry["claude-code"])
        self.assertEqual("plan #42", adapter.render_args("plan {{issue}}", {"issue": "#42"}))
        self.assertIn("subagent_type: architect", adapter.render_invocation("architect", "#42"))

    def test_canonical_bodies_are_neutral(self) -> None:
        for path in (ROOT / "canonical/crew").glob("*.md"):
            body = path.read_text(encoding="utf-8").split("---", 2)[2]
            self.assertNotRegex(body, r"CLAUDE|\.claude|subagent_type|Claude-Session")
        for path in (ROOT / "canonical/orders").glob("*.md"):
            body = path.read_text(encoding="utf-8").split("---", 2)[2]
            self.assertNotRegex(body, r"CLAUDE|\.claude|subagent_type|Claude-Session")

    def test_exported_permissions_follow_canonical_metadata(self) -> None:
        roles, orders = exporter.load_catalog(ROOT)
        files = ClaudeCodeAdapter(load_registry(ROOT / "tools/capability_registry.json")["claude-code"]).build(
            ROOT, roles, orders
        )
        self.assertIn("tools: Read, Grep, Glob, Bash", files["harnesses/claude-code/agents/architect.md"])
        self.assertIn("tools: Read, Write, Edit, Bash, Grep, Glob", files["harnesses/claude-code/agents/senior-engineer.md"])
        self.assertIn("tools: Read, Bash, WebSearch", files["harnesses/claude-code/agents/art-director.md"])
        self.assertIn("tools: Read, Grep, Glob, Bash, WebSearch, WebFetch", files["harnesses/claude-code/agents/product-manager.md"])
        self.assertNotIn("Write", files["harnesses/claude-code/agents/architect.md"])

    def test_generated_claude_payload_matches_compatibility_sources(self) -> None:
        roles, orders = exporter.load_catalog(ROOT)
        files = ClaudeCodeAdapter(load_registry(ROOT / "tools/capability_registry.json")["claude-code"]).build(
            ROOT, roles, orders
        )
        for relative, content in files.items():
            compatibility = ROOT / relative.removeprefix("harnesses/claude-code/")
            self.assertEqual(compatibility.read_text(encoding="utf-8"), content, relative)

    def test_claude_golden_is_current(self) -> None:
        self.assertEqual(0, exporter.run(ROOT, "claude-code", check=True))

    def test_canonical_edits_change_export_not_compatibility_edits(self) -> None:
        temporary, root = self.temp_repo()
        self.addCleanup(temporary.cleanup)
        canonical_role = root / "canonical/crew/architect.md"
        canonical_role.write_text(
            canonical_role.read_text(encoding="utf-8") + "\nCANONICAL ROLE MARKER\n",
            encoding="utf-8",
        )
        compatibility_role = root / "agents/architect.md"
        compatibility_role.write_text(
            compatibility_role.read_text(encoding="utf-8") + "\nCOMPATIBILITY MARKER\n",
            encoding="utf-8",
        )
        self.assertEqual(0, exporter.run(root, "claude-code", check=False))
        output = (root / "harnesses/claude-code/agents/architect.md").read_text(encoding="utf-8")
        self.assertIn("CANONICAL ROLE MARKER", output)
        self.assertNotIn("COMPATIBILITY MARKER", output)

        compatibility_role.unlink()
        self.assertEqual(0, exporter.run(root, "claude-code", check=False))

        order = root / "canonical/orders/ship-issue.md"
        order.write_text(order.read_text(encoding="utf-8") + "\nCANONICAL ORDER MARKER\n", encoding="utf-8")
        self.assertEqual(0, exporter.run(root, "claude-code", check=False))
        self.assertIn(
            "CANONICAL ORDER MARKER",
            (root / "harnesses/claude-code/skills/ship-issue/SKILL.md").read_text(encoding="utf-8"),
        )

    def test_generated_tree_drift_is_detected(self) -> None:
        temporary, root = self.temp_repo()
        self.addCleanup(temporary.cleanup)
        roles, orders = exporter.load_catalog(root)
        files = ClaudeCodeAdapter(load_registry(root / "tools/capability_registry.json")["claude-code"]).build(
            root, roles, orders
        )
        generated = root / "harnesses/claude-code/agents/architect.md"
        generated.write_text(generated.read_text(encoding="utf-8") + "drift\n", encoding="utf-8")
        self.assertIn("drift: harnesses/claude-code/agents/architect.md", exporter.check_files(root, files, "claude-code"))

    def test_invalid_catalog_fails_before_writes(self) -> None:
        cases = (
            ('"role":"product-manager"', '"role":"missing-role"'),
            ("arguments: issue, guidance", "arguments: issue, 1bad"),
            ("invocation: @{{role}}({{issue}})", "invocation: @{{role}}({{missing}})"),
            ('"max_loops":3', '"max_loops":4'),
        )
        for needle, replacement in cases:
            with self.subTest(replacement=replacement):
                temporary, root = self.temp_repo()
                order = root / "canonical/orders/ship-issue.md"
                text = order.read_text(encoding="utf-8")
                self.assertIn(needle, text)
                order.write_text(text.replace(needle, replacement, 1), encoding="utf-8")
                sentinel = root / "harnesses/claude-code/sentinel.txt"
                sentinel.write_text("untouched\n", encoding="utf-8")
                before = {
                    path: path.read_bytes()
                    for path in (root / "harnesses/claude-code").rglob("*")
                    if path.is_file()
                }
                self.assertNotEqual(0, exporter.run(root, "claude-code", check=False))
                after = {
                    path: path.read_bytes()
                    for path in (root / "harnesses/claude-code").rglob("*")
                    if path.is_file()
                }
                self.assertEqual(before, after)
                temporary.cleanup()

    def test_opencode_is_explicitly_refused(self) -> None:
        result = subprocess.run(
            [sys.executable, "tools/export.py", "build", "--target", "opencode"],
            cwd=ROOT,
            text=True,
            capture_output=True,
        )
        self.assertNotEqual(0, result.returncode)
        self.assertIn("opencode", result.stderr)
        self.assertIn("not implemented", result.stderr)

    def test_installer_consumes_generated_claude_payload(self) -> None:
        temporary, root = self.temp_repo()
        self.addCleanup(temporary.cleanup)
        generated = root / "harnesses/claude-code/skills/ship-issue/SKILL.md"
        canonical = root / "canonical/orders/ship-issue.md"
        canonical.write_text(canonical.read_text(encoding="utf-8") + "\nGENERATED PAYLOAD MARKER\n", encoding="utf-8")
        self.assertEqual(0, exporter.run(root, "claude-code", check=False))
        target = Path(temporary.name) / "installed"
        result = subprocess.run(
            ["bash", str(root / "install.sh"), "--dir", str(target)],
            cwd=root,
            text=True,
            capture_output=True,
        )
        self.assertEqual(0, result.returncode, result.stderr)
        installed = target / "skills/ship-issue/SKILL.md"
        self.assertIn("GENERATED PAYLOAD MARKER", installed.read_text(encoding="utf-8"))

    def test_installer_refuses_stale_opencode_payload(self) -> None:
        temporary, root = self.temp_repo()
        self.addCleanup(temporary.cleanup)
        stale = root / "harnesses/opencode/skills/stale"
        stale.mkdir(parents=True)
        (stale / "SKILL.md").write_text("stale\n", encoding="utf-8")
        target = Path(temporary.name) / "installed"
        result = subprocess.run(
            ["bash", str(root / "install.sh"), "--harness", "opencode", "--dir", str(target)],
            cwd=root,
            text=True,
            capture_output=True,
        )
        self.assertNotEqual(0, result.returncode)
        self.assertIn("no payload for 'opencode'", result.stderr)
        self.assertFalse(target.exists())

    def test_installer_refuses_payload_stale_against_canonical(self) -> None:
        temporary, root = self.temp_repo()
        self.addCleanup(temporary.cleanup)
        canonical = root / "canonical/crew/architect.md"
        canonical.write_text(canonical.read_text(encoding="utf-8") + "\nSTALE CANONICAL CHANGE\n", encoding="utf-8")
        target = Path(temporary.name) / "installed"
        result = subprocess.run(
            ["bash", str(root / "install.sh"), "--dir", str(target)],
            cwd=root,
            text=True,
            capture_output=True,
        )
        self.assertNotEqual(0, result.returncode)
        self.assertFalse(target.exists())

    def test_cli_build_check(self) -> None:
        result = subprocess.run(
            [sys.executable, "tools/export.py", "build", "--target", "claude-code", "--check"],
            cwd=ROOT,
            text=True,
            capture_output=True,
        )
        self.assertEqual(0, result.returncode, result.stderr)
        self.assertIn("up to date", result.stdout)


if __name__ == "__main__":
    unittest.main()
