# Shipmates

A crew of Claude Code subagents and command workflows that drive a GitHub issue from open to a reviewed, CI-green pull request — autonomously.

## What it is

Shipmates is an open-source (MIT) toolkit for [Claude Code](https://claude.com/claude-code): **12 domain-neutral specialist subagents** and **12 command workflows**. Its flagship, `/ship-issue`, takes a GitHub issue all the way to a reviewed, CI-green pull request on its own — it plans the work, builds it in an isolated git worktree, waits for CI to go green, convenes an adversarial review board, loops on the fixes within bounds, and hands you a PR to merge.

## Who it's for

Developers using Claude Code who want to hand off whole *tickets* — not just single prompts — to a crew of specialist agents. The agents are domain-neutral: they hold the work to the standard in **your** repo's `README` / `CLAUDE.md`, so the same crew works on a game engine, a web app, or a CLI, in any language.

## The crew (12)

`architect` · `senior-engineer` · `sdet` · `security-engineer` · `site-reliability-engineer` · `performance-engineer` · `devops-engineer` · `product-manager` · `ux-ui-designer` · `art-director` · `technical-writer` · `data-scientist` — each defined in `agents/<role>.md`.

## The orders (12)

`/ship-issue` · `/fix-bug` · `/plan-epics` · `/harden` · `/spike` · `/migrate` · `/document` · `/release` · `/polish` · `/review` · `/onboard` · `/refactor` — each defined in `skills/<name>/SKILL.md`.

## Install

```bash
curl -fsSL https://raw.githubusercontent.com/saman-mb/shipmates/main/install.sh | bash
```

Global (`~/.claude`) by default; add `--project /path/to/repo` to scope it to one repo.

## Working in this repository

If you are an AI agent (or a human) contributing to Shipmates itself:

- **The one hard rule: keep agent roles domain-neutral.** No role may name-drop a stack, framework, or product — each enforces whatever *the user's* `README` / `CLAUDE.md` says. That neutrality is what lets the crew sail on anyone's project; a PR that hardcodes a domain will be rejected.
- **Layout:** `agents/*.md` are the subagent personas (frontmatter `name`/`description`/`tools` + a system-prompt body). `skills/<name>/SKILL.md` are the workflows, in the [Agent Skills](https://agentskills.io) format: frontmatter `name`, `description`, `argument-hint`, `allowed-tools` (that canonical order) + a body. **`name` MUST equal the parent directory name** — that is the standard's rule, and the skill will not resolve without it. `argument-hint` and `allowed-tools` are **vendor extensions, not part of the Agent Skills standard** (`allowed-tools` does appear there, but as an experimental, space-separated key; ours stay comma-separated because that is what Claude Code parses). `install.sh` drops both trees into a `.claude/` directory. `site/` is the GitHub Pages landing site (validated by `.github/scripts/validate_site.py`). `site/commands/<name>/index.html` are **generated** from `skills/<name>/SKILL.md` by `tools/gen_command_pages.py` and committed — never hand-edit them; run `python3 tools/gen_command_pages.py` and commit the result. CI enforces this with `--check`.
- **`$ARGUMENTS` is the only argument placeholder in a `SKILL.md` body.** A skill has no positional arguments: indexed substitution is not in the Agent Skills standard, and its index base has changed between Claude Code versions, so a workflow written against one base silently reads the wrong argument on another. Never write a `$` followed by a digit in prose — parse `$ARGUMENTS` instead ("the first word of `$ARGUMENTS` is the issue number"). The one exception is **inside a fenced code block**, where `$` + digit is a shell field reference (`awk`, `sed`) and has nothing to do with arguments. `python3 tools/validate_skills.py` enforces exactly that split.
- **Name by register — never bulk-substitute one noun for the other.** The on-disk artifact and the portable standard is a **skill**; in the Shipmates product domain the same thing is a **command** you give the crew, and the set of them is **the orders**. Use *skill* in tech-leading copy — install paths, layout and reference sections, frontmatter docs, contributor instructions, anything about the file on disk or cross-harness portability. Use *command* / *orders* in brand-leading copy — the intro, the crew-and-orders framing, taglines, section headings that sell the metaphor, example usage. Two retired spellings: don't prefix "command" with "slash-" (Anthropic's pre-skills label, and Claude-Code-only — plain "command" is the product noun we want), and write **subagent** as one word, never hyphenated or spaced, matching the Claude Code, Cursor and OpenCode docs. "The crew" and "shipmates" stay as brand terms.
- **Don't wholesale-rewrite `skills/ship-issue/SKILL.md`** — make scoped edits; it encodes the flagship's gated state machine.
- See [`CONTRIBUTING.md`](CONTRIBUTING.md) for the full contribution guide.

## Scope & honesty

- **Claude Code only today.** Running the crew on opencode / Cursor / Copilot / Codex is on the roadmap, not shipped.
- **Gates are orchestrated by the command, not hook-enforced yet.** The worktree / green-CI / fresh-reviewer gates are driven by the workflow prompt; a code-enforced (hook-backed) state machine is planned, not in the current release.
- **Not affiliated with Anthropic.** "Claude" and "Claude Code" are trademarks of Anthropic.

## Links

- **Website:** https://saman-mb.github.io/shipmates/
- **Repository:** https://github.com/saman-mb/shipmates
- **License:** [MIT](LICENSE)
