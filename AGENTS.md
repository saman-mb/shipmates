# Shipmates

A crew of Claude Code sub-agents and slash-command workflows that drive a GitHub issue from open to a reviewed, CI-green pull request — autonomously.

## What it is

Shipmates is an open-source (MIT) toolkit for [Claude Code](https://claude.com/claude-code): **11 domain-neutral specialist sub-agents** and **9 slash-command workflows**. Its flagship, `/ship-issue`, takes a GitHub issue all the way to a reviewed, CI-green pull request on its own — it plans the work, builds it in an isolated git worktree, waits for CI to go green, convenes an adversarial review board, loops on the fixes within bounds, and hands you a PR to merge.

## Who it's for

Developers using Claude Code who want to hand off whole *tickets* — not just single prompts — to a crew of specialist agents. The agents are domain-neutral: they hold the work to the standard in **your** repo's `README` / `CLAUDE.md`, so the same crew works on a game engine, a web app, or a CLI, in any language.

## The crew (11)

`architect` · `senior-engineer` · `sdet` · `security-engineer` · `site-reliability-engineer` · `performance-engineer` · `product-manager` · `ux-ui-designer` · `art-director` · `technical-writer` · `data-scientist` — each defined in `agents/<role>.md`.

## The orders (9)

`/ship-issue` · `/fix-bug` · `/plan-epics` · `/harden` · `/spike` · `/migrate` · `/document` · `/release` · `/polish` — each defined in `commands/<name>.md`.

## Install

```bash
curl -fsSL https://raw.githubusercontent.com/saman-mb/shipmates/main/install.sh | bash
```

Global (`~/.claude`) by default; add `--project /path/to/repo` to scope it to one repo.

## Working in this repository

If you are an AI agent (or a human) contributing to Shipmates itself:

- **The one hard rule: keep agent roles domain-neutral.** No role may name-drop a stack, framework, or product — each enforces whatever *the user's* `README` / `CLAUDE.md` says. That neutrality is what lets the crew sail on anyone's project; a PR that hardcodes a domain will be rejected.
- **Layout:** `agents/*.md` are the sub-agent personas (frontmatter `name`/`description`/`tools` + a system-prompt body). `commands/*.md` are the slash-command workflows (`description`/`argument-hint`/`allowed-tools` + a body using `$ARGUMENTS`/`$1`). `install.sh` drops both into a `.claude/` directory. `site/` is the GitHub Pages landing site (validated by `.github/scripts/validate_site.py`). `site/commands/<name>/index.html` are **generated** from `commands/*.md` by `tools/gen_command_pages.py` and committed — never hand-edit them; run `python3 tools/gen_command_pages.py` and commit the result. CI enforces this with `--check`.
- **Don't wholesale-rewrite `commands/ship-issue.md`** — make scoped edits; it encodes the flagship's gated state machine.
- See [`CONTRIBUTING.md`](CONTRIBUTING.md) for the full contribution guide.

## Scope & honesty

- **Claude Code only today.** Running the crew on opencode / Cursor / Copilot / Codex is on the roadmap, not shipped.
- **Gates are orchestrated by the command, not hook-enforced yet.** The worktree / green-CI / fresh-reviewer gates are driven by the workflow prompt; a code-enforced (hook-backed) state machine is planned, not in the current release.
- **Not affiliated with Anthropic.** "Claude" and "Claude Code" are trademarks of Anthropic.

## Links

- **Website:** https://saman-mb.github.io/shipmates/
- **Repository:** https://github.com/saman-mb/shipmates
- **License:** [MIT](LICENSE)
