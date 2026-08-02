#!/usr/bin/env python3
"""Regression tests for the site generator's source of truth.

`gen_command_pages.py --check` compares generated HTML against committed HTML,
so it gates *staleness*, not *correctness* — it stayed green throughout the
period the site published harness-neutral exporter tokens (#158). Reverting the
fix and regenerating would make it green again with wrong content.

These tests assert the property `--check` cannot: that the published pages carry
the dialect a user actually installs.
"""

from __future__ import annotations

import shutil
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from tools import export as exporter  # noqa: E402

# gen_command_pages uses @dataclass(slots=True) and so needs Python 3.10+. That
# is fine — it is a CI-and-maintainer tool, not part of the installer, which
# must run on the 3.9 the repo declares as its floor. Imported lazily so the
# content assertions below still run on 3.9, where they are just file reads.
GENERATOR_MIN = (3, 10)


#: Tokens that exist only inside the exporter. None is valid in any harness, so
#: any occurrence on a published page means the generator read the neutral
#: source instead of a rendered payload.
NEUTRAL_TOKENS = (
    "agent-files/",
    "TARGET.md",
    "@role(",
    "Harness-Session",
)
# Deliberately NOT listed: "general-purpose". Canonical uses it, the Claude
# adapter keeps it (it is Claude Code's built-in generic subagent), and only the
# opencode adapter rewrites it to "general". On a Claude-rendered site it is
# correct output, not a leak.

#: Argument placeholders are per-command, so they are checked by shape.
NEUTRAL_ARGUMENT_RE = r"\{\{[a-z][a-z0-9_-]*\}\}"


class SiteGenerationTests(unittest.TestCase):
    def published_pages(self) -> list[Path]:
        pages = sorted((ROOT / "site/commands").rglob("index.html"))
        pages += sorted((ROOT / "site/agents").rglob("index.html"))
        self.assertTrue(pages, "no generated pages found")
        return pages

    def test_published_pages_carry_no_neutral_dialect(self) -> None:
        """The regression from #158, asserted directly on the committed site."""
        for page in self.published_pages():
            text = page.read_text(encoding="utf-8")
            for token in NEUTRAL_TOKENS:
                with self.subTest(page=page.name, token=token):
                    self.assertNotIn(
                        token,
                        text,
                        f"{page.relative_to(ROOT)} contains the neutral token {token!r} — "
                        "the site must be generated from a rendered payload, not from "
                        "crew/ + commands/",
                    )

    def test_published_pages_carry_no_neutral_argument_placeholders(self) -> None:
        for page in self.published_pages():
            with self.subTest(page=page.name):
                self.assertNotRegex(
                    page.read_text(encoding="utf-8"),
                    NEUTRAL_ARGUMENT_RE,
                    f"{page.relative_to(ROOT)} contains a `{{{{name}}}}` placeholder — "
                    "the rendered payload uses $ARGUMENTS",
                )

    def test_command_pages_carry_the_rendered_dialect(self) -> None:
        """Positive control: absence of neutral tokens must not mean absence of content."""
        migrate = (ROOT / "site/commands/migrate/index.html").read_text(encoding="utf-8")
        self.assertIn("ARGUMENTS", migrate)
        self.assertIn(".claude/agents/*.md", migrate)

    def test_agent_pages_list_harness_tool_names(self) -> None:
        """Crew pages must show the harness's tool names, not semantic capabilities.

        `parse_agent` falls back to `capabilities` when a rendered `tools` key is
        absent, which is what published "read, bash" instead of the real list.
        """
        architect = (ROOT / "site/agents/architect/index.html").read_text(encoding="utf-8")
        for tool in ("Read", "Grep", "Glob", "Bash"):
            self.assertIn(tool, architect)
        self.assertNotIn("read, bash", architect)

    @unittest.skipIf(
        sys.version_info < GENERATOR_MIN,
        "gen_command_pages requires Python 3.10+ (dataclass slots)",
    )
    def test_loaders_accept_both_payload_layouts(self) -> None:
        """Authored commands are flat; Claude's rendered payload is nested.

        `parse_skill` took the slug from `path.stem`, which reads every nested
        `<slug>/SKILL.md` as "SKILL" and failed the name-matches-directory gate,
        so the generator wrote nothing and left the stale pages in place.
        """
        from tools import gen_command_pages as generator

        out = tempfile.mkdtemp()
        self.addCleanup(shutil.rmtree, out, True)
        self.assertEqual(0, exporter.run(ROOT, "claude-code", check=False, out_dir=Path(out)))
        rendered = Path(out) / "harnesses/claude-code"

        nested = rendered / "skills"
        agents = generator.load_agents(rendered / "agents", nested)
        self.assertEqual(12, len(agents))
        commands = generator.load_skills(nested, tuple(a.name for a in agents))
        self.assertEqual(12, len(commands))
        self.assertIn("ship-issue", {c.slug for c in commands})

        flat = generator.load_skills(ROOT / "commands", tuple(a.name for a in agents))
        self.assertEqual({c.slug for c in commands}, {c.slug for c in flat})


if __name__ == "__main__":
    unittest.main()
