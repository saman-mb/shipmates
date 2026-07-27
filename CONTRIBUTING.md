# Contributing to Shipmates

Thanks for wanting to add to the crew! New agents and commands are welcome.

## The one hard rule: agents stay domain-neutral

An agent role in `agents/` must **not** mention any specific language, framework, product, or
project. It describes *how the role thinks and works*; the standard it enforces comes from the
target repo's `README` / `CLAUDE.md` at run time. If you find yourself writing "Godot", "React",
"our game", or a specific style guide into a role, move that into the repo that uses it instead.

Good: *"Hold the work to whatever visual bar the project states for itself."*
Not good: *"Match our SC2-style HD dark-chrome UI."*

## Adding an agent

1. Create `agents/<name>.md` (lowercase-and-hyphens name).
2. Frontmatter: `name`, `description` (when Claude should delegate to it), and `tools` (least
   privilege — a reviewer usually needs `Read, Grep, Glob, Bash`, not `Write`/`Edit`).
3. Body = the system prompt: the role's focus, how it reviews/builds, and the verdict format it
   returns (`ACCEPT` / `ACCEPT-WITH-NITS` / `REJECT`, or `PASS` / `FAIL`).
4. If a command should invoke it, wire it into that command's stages by `subagent_type`.

## Adding a skill (workflow)

A workflow ships as a **skill** on disk and is invoked as a **command** in Claude Code. Work the
checklist top to bottom:

1. Create `skills/<name>/SKILL.md` (lowercase-and-hyphens `<name>`), with frontmatter in this
   canonical order: `name`, `description`, `argument-hint`, `allowed-tools`. **`name` must be
   exactly the directory name** — that is the [Agent Skills](https://agentskills.io) standard's
   rule, and the skill will not resolve without it.
2. `argument-hint` and `allowed-tools` are vendor extensions on top of the standard, kept
   comma-separated because that is what Claude Code parses.
3. Make the first heading after the frontmatter `# /<name> — <tagline>` — the page generator parses
   that line for the command's title, and fails on anything else.
4. Take input from `$ARGUMENTS` only, parsed in prose — a skill has no positional arguments. A `$`
   followed by a digit is rejected outside a fenced code block; inside a fence it is read as a shell
   field reference and allowed.
5. Prefer invoking the shared agents by `subagent_type` over inlining personas.
6. Anything with side effects (merging, publishing) should be **opt-in**, not the default — be a
   good guest on other people's repos.
7. Add `<name>` to `SLUGS` in `tools/gen_command_pages.py`.
8. Run `python3 tools/gen_command_pages.py` and commit the regenerated `site/commands/**` and
   `site/sitemap.xml` — never hand-edit those. CI fails if they drift from the skill sources.
9. Add a matching card to the `#orders` grid in `site/index.html`, linking to `commands/<name>/`.
10. Both validators must exit 0 before you open the PR: `python3 tools/validate_skills.py` and
    `python3 .github/scripts/validate_site.py`.

## Testing your change

Install into a throwaway scope and try it on a real repo:

```bash
./install.sh --project /tmp/some-test-repo
```

Then run the command in Claude Code and confirm the agents resolve (no "falling back to
general-purpose" notes in the report).

## PRs

Keep changes focused, explain the intent, and make sure `./install.sh` still installs cleanly.
