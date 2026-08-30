# Shipmates contributor steering

Agent-facing conventions for changing **this repository's** canonical resources — crew, commands, tools, and the generated site. This file is installed at `{{project-instructions}}` (fallback: `{{project-instructions-fallback}}`) so every harness reads the same checklists. Human-oriented detail also lives in `AGENTS.md` and `CONTRIBUTING.md`.

When you touch canonical sources, extend the model in `tools/gen_command_pages.py` (or the Rust installer) and **regenerate** — never hand-edit generated pages under `site/commands/`, `site/agents/`, or `site/tools/`.

## New command

- Add `commands/<name>.md` (harness-neutral prose; `$ARGUMENTS` only).
- Register in `SLUGS` and `COMMAND_COPY` in `tools/gen_command_pages.py`.
- Add a reel in `tools/gen_command_demos.py` and commit `site/assets/command-<slug>.gif` + poster (except `/ship-issue`, which reuses `demo.gif`).
- Regenerate command pages; update the homepage commands grid and sitemap.
- Run `cargo run -- build --target <harness> --update` for every target and commit `tests/payload-digests/*.sha256`.

## New tool

- Add `toolbox/<name>/` with exactly `tool.md` + `<name>.py`.
- Register in `TOOLS` and `TOOL_COPY` in `tools/gen_command_pages.py`.
- Tool page **Examples**: terminal tools (`scrub`, `fixtures`, `domaincheck`) need a termgif JSON spec plus GIF and reduced-motion poster under `site/tools/<name>/examples/`; visual tools commit SVG/PNG output. Homepage tool cards use **emoji** — GIFs belong on the detail page, not the grid.
- Update `site/index.html` `#tools` grid: **every** `TOOLS` entry in canonical order (do not drop siblings).
- Update README toolbox table; regenerate tool pages and sitemap.
- Bump `tools/e2e_cli.sh` install file counts when tool payloads change.
- Run payload digest updates per harness.

## New crew role

- Add `crew/<role>.md`; register in `AGENT_COPY` and crew order in `tools/gen_command_pages.py`.
- Regenerate agent pages; update homepage crew grid and sitemap.
- Update payload digests per target.

## Before opening a PR (Shipmates repo)

```bash
python3 tools/gen_command_pages.py --check   # or regenerate and commit
python3 .github/scripts/validate_site.py
python3 tools/gen_command_demos.py --check   # when command demos change
cargo run -- check --target <harness>        # all implemented targets in CI
cargo test
```

When the diff adds or extends `toolbox/<name>/` or `TOOLS`, verify the new tool page matches `scrub` or `fixtures`: an **Examples** gallery with committed assets, plus homepage grid parity.
