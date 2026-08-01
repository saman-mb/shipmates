#!/usr/bin/env python3
"""Focused regression tests for the canonical exporter foundation."""

from __future__ import annotations

import os
import subprocess
import sys
import shutil
import tempfile
import unittest
from pathlib import Path

from tools import export as exporter
from tools.adapter_contract import conformance_report
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

    def temp_out(self) -> tuple[tempfile.TemporaryDirectory, Path]:
        temporary = tempfile.TemporaryDirectory()
        return temporary, Path(temporary.name).resolve()

    def test_canonical_catalog_covers_crew_and_commands(self) -> None:
        roles, commands = exporter.load_catalog(ROOT)
        self.assertEqual(12, len(roles))
        self.assertEqual(12, len(commands))
        self.assertIn("ship-issue", {command.name for command in commands})
        self.assertTrue(all(command.stages for command in commands))
        self.assertTrue(
            all(
                stage["order"] == index
                for command in commands
                for index, stage in enumerate(command.stages, 1)
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
        for path in (ROOT / "canonical/commands").glob("*.md"):
            body = path.read_text(encoding="utf-8").split("---", 2)[2]
            self.assertNotRegex(body, r"CLAUDE|\.claude|subagent_type|Claude-Session")

    def test_exported_permissions_follow_canonical_metadata(self) -> None:
        roles, commands = exporter.load_catalog(ROOT)
        files = ClaudeCodeAdapter(load_registry(ROOT / "tools/capability_registry.json")["claude-code"]).build(
            ROOT, roles, commands
        )
        self.assertIn("tools: Read, Grep, Glob, Bash", files["harnesses/claude-code/agents/architect.md"])
        self.assertIn("tools: Read, Write, Edit, Bash, Grep, Glob", files["harnesses/claude-code/agents/senior-engineer.md"])
        self.assertIn("tools: Read, Bash, WebSearch", files["harnesses/claude-code/agents/art-director.md"])
        self.assertIn("tools: Read, Grep, Glob, Bash, WebSearch, WebFetch", files["harnesses/claude-code/agents/product-manager.md"])
        self.assertNotIn("Write", files["harnesses/claude-code/agents/architect.md"])

    def test_generated_claude_payload_matches_compatibility_sources(self) -> None:
        roles, commands = exporter.load_catalog(ROOT)
        files = ClaudeCodeAdapter(load_registry(ROOT / "tools/capability_registry.json")["claude-code"]).build(
            ROOT, roles, commands
        )
        for relative, content in files.items():
            compatibility = ROOT / relative.removeprefix("harnesses/claude-code/")
            self.assertEqual(compatibility.read_text(encoding="utf-8"), content, relative)

    def test_claude_export_matches_golden(self) -> None:
        """Generated output must match committed golden reference files."""
        out_dir = self.temp_out()[1]
        self.addCleanup(shutil.rmtree, out_dir, True)
        self.assertEqual(0, exporter.run(ROOT, "claude-code", check=False, out_dir=out_dir))
        for path in sorted((out_dir / "harnesses/claude-code").rglob("*")):
            if not path.is_file():
                continue
            relative = str(path.relative_to(out_dir / "harnesses/claude-code"))
            golden = ROOT / "tests/golden/claude-code" / relative
            self.assertTrue(golden.exists(), f"golden file missing: {golden}")
            self.assertEqual(golden.read_text(encoding="utf-8"), path.read_text(encoding="utf-8"), relative)

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
        out_dir = self.temp_out()[1]
        self.addCleanup(shutil.rmtree, out_dir, True)
        self.assertEqual(0, exporter.run(root, "claude-code", check=False, out_dir=out_dir))
        output = (out_dir / "harnesses/claude-code/agents/architect.md").read_text(encoding="utf-8")
        self.assertIn("CANONICAL ROLE MARKER", output)
        self.assertNotIn("COMPATIBILITY MARKER", output)

        compatibility_role.unlink()
        out_dir2 = self.temp_out()[1]
        self.addCleanup(shutil.rmtree, out_dir2, True)
        self.assertEqual(0, exporter.run(root, "claude-code", check=False, out_dir=out_dir2))

        command = root / "canonical/commands/ship-issue.md"
        command.write_text(command.read_text(encoding="utf-8") + "\nCANONICAL COMMAND MARKER\n", encoding="utf-8")
        out_dir3 = self.temp_out()[1]
        self.addCleanup(shutil.rmtree, out_dir3, True)
        self.assertEqual(0, exporter.run(root, "claude-code", check=False, out_dir=out_dir3))
        self.assertIn(
            "CANONICAL COMMAND MARKER",
            (out_dir3 / "harnesses/claude-code/skills/ship-issue/SKILL.md").read_text(encoding="utf-8"),
        )

    def test_generated_tree_drift_is_detected(self) -> None:
        temporary, root = self.temp_repo()
        self.addCleanup(temporary.cleanup)
        roles, commands = exporter.load_catalog(root)
        files = ClaudeCodeAdapter(load_registry(root / "tools/capability_registry.json")["claude-code"]).build(
            root, roles, commands
        )
        exporter.write_files(root, files)
        generated = root / "harnesses/claude-code/agents/architect.md"
        generated.write_text(generated.read_text(encoding="utf-8") + "drift\n", encoding="utf-8")
        self.assertIn("drift: harnesses/claude-code/agents/architect.md", exporter.check_files(root, files, "claude-code"))

    def test_invalid_catalog_fails_before_writes(self) -> None:
        cases = (
            ('"roles":["product-manager"]', '"roles":["missing-role"]'),
            ("arguments: issue, guidance", "arguments: issue, 1bad"),
            ("invocation: @{{role}}({{issue}})", "invocation: @{{role}}({{missing}})"),
            ('"max_loops":3', '"max_loops":4'),
        )
        for needle, replacement in cases:
            with self.subTest(replacement=replacement):
                temporary, root = self.temp_repo()
                command = root / "canonical/commands/ship-issue.md"
                text = command.read_text(encoding="utf-8")
                self.assertIn(needle, text)
                command.write_text(text.replace(needle, replacement, 1), encoding="utf-8")
                out_dir = self.temp_out()[1]
                self.addCleanup(shutil.rmtree, out_dir, True)
                sentinel = out_dir / "harnesses/claude-code/sentinel.txt"
                sentinel.parent.mkdir(parents=True, exist_ok=True)
                sentinel.write_text("untouched\n", encoding="utf-8")
                before = {
                    path: path.read_bytes()
                    for path in (out_dir / "harnesses/claude-code").rglob("*")
                    if path.is_file()
                }
                self.assertNotEqual(0, exporter.run(root, "claude-code", check=False, out_dir=out_dir))
                after = {
                    path: path.read_bytes()
                    for path in (out_dir / "harnesses/claude-code").rglob("*")
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

    def test_installer_generates_claude_payload_at_install_time(self) -> None:
        temporary, root = self.temp_repo()
        self.addCleanup(temporary.cleanup)
        canonical = root / "canonical/commands/ship-issue.md"
        canonical.write_text(canonical.read_text(encoding="utf-8") + "\nGENERATED PAYLOAD MARKER\n", encoding="utf-8")
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

    def test_installer_refuses_unsupported_adapter(self) -> None:
        temporary, root = self.temp_repo()
        self.addCleanup(temporary.cleanup)
        target = Path(temporary.name) / "installed"
        result = subprocess.run(
            ["bash", str(root / "install.sh"), "--harness", "opencode", "--dir", str(target)],
            cwd=root,
            text=True,
            capture_output=True,
        )
        self.assertNotEqual(0, result.returncode)
        self.assertIn("opencode", result.stderr)
        self.assertFalse(target.exists())

    def test_compatibility_source_drift_is_caught(self) -> None:
        """Editing agents/ or skills/ directly must fail, not silently ship nothing.

        These trees stay in the repository for the site generator and the skills
        validator, which makes them a second writable copy of the payload. Without
        this gate a contributor edits the tree the layout docs point at, every
        check passes, and the change never reaches an installed crew.
        """
        for relative in ("agents/architect.md", "skills/spike/SKILL.md"):
            with self.subTest(relative=relative):
                temporary, root = self.temp_repo()
                mirror = root / relative
                mirror.write_text(mirror.read_text(encoding="utf-8") + "\nDRIFT\n", encoding="utf-8")
                self.assertNotEqual(0, exporter.run(root, "claude-code", check=True))
                temporary.cleanup()

    def test_positional_placeholder_is_refused(self) -> None:
        """`$1` in a canonical body must fail the build, not reach a payload.

        Substitution is textual over the whole file, so a fence protects nothing:
        a command invoked with extra words rewrites `$2` mid-snippet.
        """
        temporary, root = self.temp_repo()
        self.addCleanup(temporary.cleanup)
        command = root / "canonical/commands/spike.md"
        command.write_text(
            command.read_text(encoding="utf-8") + "\n```bash\nawk '{print $2}'\n```\n",
            encoding="utf-8",
        )
        out_dir = self.temp_out()[1]
        self.addCleanup(shutil.rmtree, out_dir, True)
        self.assertNotEqual(0, exporter.run(root, "claude-code", check=False, out_dir=out_dir))

    def test_stage_roles_must_appear_in_the_narrative(self) -> None:
        """A stage table naming a role the workflow never dispatches is fiction."""
        temporary, root = self.temp_repo()
        self.addCleanup(temporary.cleanup)
        command = root / "canonical/commands/spike.md"
        text = command.read_text(encoding="utf-8")
        self.assertIn('"roles":["architect"]', text)
        command.write_text(
            text.replace('"roles":["architect"]', '"roles":["art-director"]', 1), encoding="utf-8"
        )
        self.assertNotEqual(0, exporter.run(root, "claude-code", check=True))

    def test_golden_tree_rejects_stray_files(self) -> None:
        """An extra golden file is drift too — a generated-side-only walk misses it."""
        temporary, root = self.temp_repo()
        self.addCleanup(temporary.cleanup)
        (root / "tests/golden/claude-code/agents/stray.md").write_text("stray\n", encoding="utf-8")
        self.assertNotEqual(0, exporter.run(root, "claude-code", check=True))

    def test_update_regenerates_references(self) -> None:
        """--update is the only supported way to refresh the golden tree and mirrors."""
        temporary, root = self.temp_repo()
        self.addCleanup(temporary.cleanup)
        canonical = root / "canonical/crew/architect.md"
        canonical.write_text(
            canonical.read_text(encoding="utf-8") + "\nREGENERATED MARKER\n", encoding="utf-8"
        )
        (root / "tests/golden/claude-code/agents/stray.md").write_text("stray\n", encoding="utf-8")
        self.assertNotEqual(0, exporter.run(root, "claude-code", check=True))
        self.assertEqual(0, exporter.run(root, "claude-code", check=False, update=True))
        self.assertEqual(0, exporter.run(root, "claude-code", check=True))
        self.assertFalse((root / "tests/golden/claude-code/agents/stray.md").exists())
        for tree in ("tests/golden/claude-code/agents", "agents"):
            self.assertIn(
                "REGENERATED MARKER", (root / tree / "architect.md").read_text(encoding="utf-8")
            )

    def test_adapter_conformance_is_enforced(self) -> None:
        """A half-implemented adapter must be refused, not silently registered."""
        registry = load_registry(ROOT / "tools/capability_registry.json")
        adapter = ClaudeCodeAdapter(registry["claude-code"])
        self.assertEqual([], conformance_report(adapter, "claude-code"))
        self.assertIn("does not match target", " ".join(conformance_report(adapter, "opencode")))

        class Partial:
            name = "claude-code"
            capabilities = registry["claude-code"]

        self.assertTrue(conformance_report(Partial(), "claude-code"))

    def test_installer_preflights_python3(self) -> None:
        """A missing python3 must name itself, not read as an adapter problem.

        The exporter runs on every install now, including the plain curl | bash
        path that previously needed only curl and tar.
        """
        temporary, root = self.temp_repo()
        self.addCleanup(temporary.cleanup)
        shim = Path(temporary.name) / "bin"
        shim.mkdir()
        (shim / "python3").write_text("#!/bin/sh\nexit 127\n", encoding="utf-8")
        (shim / "python3").chmod(0o755)
        target = Path(temporary.name) / "installed"
        result = subprocess.run(
            ["bash", str(root / "install.sh"), "--dir", str(target)],
            cwd=root,
            text=True,
            capture_output=True,
            env={**os.environ, "PATH": f"{shim}:{os.environ['PATH']}"},
        )
        self.assertNotEqual(0, result.returncode)
        self.assertIn("python3", result.stderr)
        self.assertFalse(target.exists())

    def test_cli_build_check(self) -> None:
        """--check generates to temp and compares against golden reference files."""
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
