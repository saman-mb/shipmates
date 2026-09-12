#!/usr/bin/env python3
"""Regression tests for the site generator's source of truth.

`gen_command_pages.py --check` compares generated HTML against committed HTML,
so it gates *staleness*, not *correctness* — it stayed green throughout the
period the site published harness-neutral exporter tokens (#158). Reverting the
fix and regenerating would make it green again with wrong content.

These tests assert the property `--check` cannot: that published command
pages are a human guide (not a dump of the install skill), and that agent
pages still carry the dialect a user actually installs.
"""

from __future__ import annotations

import importlib.util
import shutil
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

import subprocess


def _load_site_validator():
    """Import .github/scripts/validate_site.py by path.

    Module-level, that script builds the rendered payload it audits, so it is
    loaded lazily — only the test that pins its frontmatter parsing pays for it.
    """
    path = ROOT / ".github/scripts/validate_site.py"
    spec = importlib.util.spec_from_file_location("shipmates_validate_site", path)
    module = importlib.util.module_from_spec(spec)
    # Register before exec: the script's @dataclass classes resolve their
    # module through sys.modules, which importlib only fills in for us after.
    sys.modules[spec.name] = module
    try:
        spec.loader.exec_module(module)
    except BaseException:
        del sys.modules[spec.name]
        raise
    return module


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
UNRESOLVED_EXPORTER_RE = r"\{\{[a-z][a-z0-9_-]*(?::[a-z][a-z0-9_-]*)?\}\}"


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

    def test_published_pages_carry_no_unresolved_exporter_tokens(self) -> None:
        for page in self.published_pages():
            with self.subTest(page=page.name):
                self.assertNotRegex(
                    page.read_text(encoding="utf-8"),
                    UNRESOLVED_EXPORTER_RE,
                    f"{page.relative_to(ROOT)} contains an unresolved exporter token",
                )

    def test_command_pages_are_a_guide_not_the_skill(self) -> None:
        """Positive control: the page names the process and crew, and links the skill."""
        migrate = (ROOT / "site/commands/shipmates-migrate/index.html").read_text(encoding="utf-8")
        self.assertIn('id="process"', migrate)
        self.assertIn("How it works", migrate)
        self.assertIn("senior-engineer", migrate)
        self.assertIn("Also sit when", migrate)
        self.assertIn("commands/shipmates-migrate.md", migrate)
        self.assertNotIn("ARGUMENTS", migrate)

    def test_agent_pages_list_harness_tool_names(self) -> None:
        """Crew pages must show the harness's tool names, not semantic capabilities.

        `parse_agent` falls back to `capabilities` when a rendered `tools` key is
        absent, which is what published "read, bash" instead of the real list.
        """
        architect = (ROOT / "site/agents/architect/index.html").read_text(encoding="utf-8")
        for tool in ("Read", "Grep", "Glob", "Bash"):
            self.assertIn(tool, architect)
        self.assertNotIn("read, bash", architect)

    def test_opencode_quickstart_keeps_runtime_claim_narrow(self) -> None:
        page = (ROOT / "site/docs/harnesses/index.html").read_text(encoding="utf-8")
        self.assertIn('id="opencode-quickstart"', page)
        self.assertIn("install-fidelity checks", page)
        self.assertIn("not opencode runtime behaviour", page)
        self.assertIn("/ship-issue", page)

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
        res = subprocess.run(["cargo", "run", "--", "build", "--target", "claude-code", "--out", out], cwd=ROOT)
        self.assertEqual(0, res.returncode)
        rendered = Path(out) / "harnesses/claude-code/.claude"

        nested = rendered / "skills"
        agents = generator.load_agents(rendered / "agents", nested)
        self.assertEqual(13, len(agents))
        commands = generator.load_skills(nested, tuple(a.name for a in agents))
        self.assertEqual(15, len(commands))
        self.assertIn("ship-issue", {c.slug for c in commands})

        flat = generator.load_skills(ROOT / "commands", tuple(a.name for a in agents))
        self.assertEqual({c.slug for c in commands}, {c.slug for c in flat})

    @unittest.skipIf(
        sys.version_info < GENERATOR_MIN,
        "gen_command_pages requires Python 3.10+ (dataclass slots)",
    )
    def test_quoted_frontmatter_round_trips_to_authored_copy(self) -> None:
        """#407: double-quoted scalars must not publish their quotes.

        The renderer quotes free-text frontmatter so a strict YAML loader (the
        one behind Cursor's slash picker) accepts a description containing
        `: `, but the page must still show the authored string — the quotes are
        YAML syntax, not copy.
        """
        from tools import gen_command_pages as generator

        lines = [
            "---",
            "name: ship-epic",
            'description: "Shipmates: Loop /ship-issue over an epic\'s stories — colon: yes"',
            'argument-hint: "<epic issue> [focus] [max-cycles]"',
            'allowed-tools: "Read, Grep, Glob, Bash"',
            "---",
            "# /ship-epic — test",
            "",
        ]
        fm, _ = generator.split_frontmatter(lines, "commands/ship-epic.md", set())
        self.assertEqual("ship-epic", fm.name)
        self.assertEqual(
            "Shipmates: Loop /ship-issue over an epic's stories — colon: yes",
            fm.description,
        )
        self.assertEqual("<epic issue> [focus] [max-cycles]", fm.argument_hint)
        self.assertEqual(("Read", "Grep", "Glob", "Bash"), fm.allowed_tools)

    @unittest.skipIf(
        sys.version_info < GENERATOR_MIN,
        "gen_command_pages requires Python 3.10+ (dataclass slots)",
    )
    def test_quoted_agent_frontmatter_round_trips_to_authored_copy(self) -> None:
        """The same quoting lands on rendered crew files, which agent pages read."""
        from tools import gen_command_pages as generator

        path = Path(tempfile.mkdtemp()) / "architect.md"
        self.addCleanup(shutil.rmtree, path.parent, True)
        path.write_text(
            "---\n"
            "name: architect\n"
            'description: "Agent copy: colon-space, and a \\"quote\\""\n'
            "tools: Read, Grep\n"
            "---\n"
            "# architect\n",
            encoding="utf-8",
        )
        fm = generator.parse_agent(path)
        self.assertEqual("architect", fm.name)
        self.assertEqual('Agent copy: colon-space, and a "quote"', fm.description)
        self.assertEqual(("Read", "Grep"), fm.tools)

    @unittest.skipIf(
        sys.version_info < GENERATOR_MIN,
        "gen_command_pages requires Python 3.10+ (dataclass slots)",
    )
    def test_unquoted_frontmatter_values_pass_through_unchanged(self) -> None:
        """Authored sources are not quoted; unquoting must leave them byte-identical."""
        from tools import gen_command_pages as generator

        lines = [
            "---",
            "name: ship-issue",
            "description: A plain description with no mapping colon",
            "argument-hint: <issue-number>",
            "allowed-tools: Read, Grep",
            "---",
            "# /ship-issue — test",
            "",
        ]
        fm, _ = generator.split_frontmatter(lines, "commands/ship-issue.md", set())
        self.assertEqual("A plain description with no mapping colon", fm.description)
        self.assertEqual("<issue-number>", fm.argument_hint)
        self.assertEqual(("Read", "Grep"), fm.allowed_tools)

    @unittest.skipIf(
        sys.version_info < GENERATOR_MIN,
        "gen_command_pages requires Python 3.10+ (dataclass slots)",
    )
    def test_yaml_unquote_mirrors_the_renderer_escapes(self) -> None:
        """The helper inverts `yaml_scalar` (src/adapters/render.rs) exactly."""
        from tools import gen_command_pages as generator

        self.assertEqual(
            'quote " backslash \\ slash \n newline \r cr \t tab \x00 nul',
            generator._yaml_unquote(
                '"quote \\" backslash \\\\ slash \\n newline \\r cr \\t tab \\u0000 nul"'
            ),
        )
        self.assertEqual("plain value", generator._yaml_unquote("plain value"))
        self.assertEqual("", generator._yaml_unquote(""))

    @unittest.skipIf(
        sys.version_info < GENERATOR_MIN,
        "gen_command_pages requires Python 3.10+ (dataclass slots)",
    )
    def test_site_validator_unquotes_rendered_agent_frontmatter(self) -> None:
        """#407: the independent site gate must read rendered frontmatter too.

        `check_agent_reference` compares a parsed description against page
        prose. A gate that keeps the renderer's `yaml_scalar` quotes fails every
        crew page; one whose unquoting drifts from the generator's would pass a
        wrong comparison instead.
        """
        from tools import gen_command_pages as generator

        validator = _load_site_validator()
        quoted = '"quote \\" backslash \\\\ slash \\n newline \\r cr \\t tab \\u0000 nul"'
        self.assertEqual(generator._yaml_unquote(quoted), validator._yaml_unquote(quoted))
        self.assertEqual(
            'quote " backslash \\ slash \n newline \r cr \t tab \x00 nul',
            validator._yaml_unquote(quoted),
        )
        self.assertEqual("plain value", validator._yaml_unquote("plain value"))
        self.assertEqual("", validator._yaml_unquote(""))

        path = Path(tempfile.mkdtemp()) / "architect.md"
        self.addCleanup(shutil.rmtree, path.parent, True)
        path.write_text(
            "---\n"
            'name: "architect"\n'
            'description: "Agent copy: colon-space, and a \\"quote\\""\n'
            "---\n",
            encoding="utf-8",
        )
        front = validator.agent_frontmatter(path)
        self.assertEqual("architect", front["name"])
        self.assertEqual('Agent copy: colon-space, and a "quote"', front["description"])

    def test_redirect_stubs_emitted_and_excluded_from_sitemap(self) -> None:
        """Legacy renamed command and tool paths serve a meta-refresh stub and are not indexed."""
        from tools.gen_command_pages import REDIRECTS, SITE_URL, TOOL_REDIRECTS

        sitemap_text = (ROOT / "site/sitemap.xml").read_text(encoding="utf-8")
        for old_slug, target_slug in REDIRECTS.items():
            stub = ROOT / f"site/commands/{old_slug}/index.html"
            self.assertTrue(stub.is_file(), f"missing redirect stub {stub}")
            text = stub.read_text(encoding="utf-8")
            self.assertIn(f'url=../{target_slug}/"', text)
            self.assertIn(f'rel="canonical" href="{SITE_URL}commands/{target_slug}/"', text)
            self.assertIn('name="robots" content="noindex"', text)
            self.assertNotIn(f"{SITE_URL}commands/{old_slug}/", sitemap_text)

        for old_slug, target_slug in TOOL_REDIRECTS.items():
            stub = ROOT / f"site/tools/{old_slug}/index.html"
            self.assertTrue(stub.is_file(), f"missing tool redirect stub {stub}")
            text = stub.read_text(encoding="utf-8")
            self.assertIn(f'url=../{target_slug}/"', text)
            self.assertIn(f'rel="canonical" href="{SITE_URL}tools/{target_slug}/"', text)
            self.assertIn('name="robots" content="noindex"', text)
            self.assertNotIn(f"{SITE_URL}tools/{old_slug}/", sitemap_text)


if __name__ == "__main__":
    unittest.main()
