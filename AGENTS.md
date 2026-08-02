# Shipmates

A crew of specialist AI agents and command workflows that drive a GitHub issue from open to a reviewed, CI-green pull request — autonomously. Runs on Claude Code today; builds for seven more harnesses. Only Claude Code is runtime-verified.

## What it is

Shipmates is an open-source (MIT) crew of specialist AI agents: **12 domain-neutral subagents** and **12 command workflows**. It runs on [Claude Code](https://claude.com/product/claude-code) today, and installs for seven more harnesses — opencode, Antigravity CLI, Codex CLI, Cursor, GitHub Copilot, Windsurf and Zed — see [Scope & honesty](#scope--honesty) for what each has and has not been verified against. Its flagship, `/ship-issue`, takes a GitHub issue all the way to a reviewed, CI-green pull request on its own — it plans the work, builds it in an isolated git worktree, waits for CI to go green, convenes an adversarial review board, loops on the fixes within bounds, and hands you a PR to merge.

## Who it's for

Developers who want to hand off whole *tickets* — not just single prompts — to a crew of specialist agents. The agents are domain-neutral: they hold the work to the standard in **your** repo's `README` / `AGENTS.md` (or `CLAUDE.md` on Claude Code), so the same crew works on a game engine, a web app, or a CLI, in any language.

## The crew (12)

`architect` · `senior-engineer` · `sdet` · `security-engineer` · `site-reliability-engineer` · `performance-engineer` · `devops-engineer` · `product-manager` · `ux-ui-designer` · `art-director` · `technical-writer` · `data-scientist` — each defined in `crew/<role>.md`.

## The commands (12)

`/ship-issue` · `/fix-bug` · `/plan-epics` · `/harden` · `/spike` · `/migrate` · `/document` · `/release` · `/polish` · `/pr-review` · `/onboard` · `/refactor` — each defined in `commands/<name>.md`.

## Install

`shipmates` is a single Rust binary. Grab it any way you like:

**macOS / Linux (Homebrew):**
```bash
brew install saman-mb/tap/shipmates
```

**Anywhere (Cargo):**
```bash
cargo install shipmates
```

**Binary Installer (cargo-dist):**
```bash
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/saman-mb/shipmates/releases/download/vX.Y.Z/shipmates-installer.sh | sh
```

Then `shipmates install --harness <name>` drops the harness's own tree (`.claude/`, `.opencode/`,
`.codex/`, …) into the current directory — or `--dir` for a specific project. `shipmates targets`
lists every harness.

## Working in this repository

If you are an AI agent (or a human) contributing to Shipmates itself:

- **The one hard rule: keep agent roles domain-neutral.** No role may name-drop a stack, framework, or product — each enforces whatever *the user's* `README` / `AGENTS.md` (or `CLAUDE.md`) says. That neutrality is what lets the crew sail on anyone's project; a PR that hardcodes a domain will be rejected.
- **`crew/*.md` and `commands/*.md` must read as general heuristics, not a diary of this repo's bugs.** They ship into other people's repositories via the `shipmates` CLI, so a persona or workflow that only makes sense here is dead weight everywhere else it lands. If a defect found on Shipmates' own site (or anywhere else in this repo) prompts a fix to one of these files, extract the project-agnostic principle it teaches and write that — never the incident: no reference to this project, its selectors, its file paths, or its issue numbers.
- **Layout.** `commands/*.md` and `crew/*.md` are the **canonical sources** — harness-neutral, and the only files you edit for crew/command content. Per-harness payloads are compiled by the Rust CLI (`cargo run -- build --target <target>`): each adapter in `src/adapters/` emits the harness's frontmatter shape, and the shared render layer (`src/adapters/render.rs`) rewrites the neutral prose into the harness's real dialect — where its agents live (`.claude/agents/`, `.opencode/agents/`, …), what its session metadata is called, which project-instructions file it reads (`CLAUDE.md`, `AGENTS.md`), and how a role is spawned. Canonical `commands/*.md` frontmatter ships `name`, `description`, `argument-hint`, `allowed-tools`, `disable-model-invocation` — in that order. The Agent Skills standard requires only `name` and `description`, first, in that order; unknown keys are rejected, so a typo fails the gate rather than being silently ignored. **`name` MUST equal the parent directory name** — that is the standard's rule, and the skill will not resolve without it. `argument-hint`, `allowed-tools` and `disable-model-invocation` are **vendor extensions, not part of the Agent Skills standard** (`allowed-tools` does appear there, but as an experimental, space-separated key; ours stay comma-separated because that is what Claude Code parses). A skill-only harness (codex, cursor, github-copilot, windsurf, zed) gets the standard's pair plus the rendered body and nothing else — unknown keys would be rejected by a strict parser. `site/` is the GitHub Pages landing site (validated by `.github/scripts/validate_site.py`). `site/commands/<name>/index.html` are **generated** from the **rendered** Claude Code payload — the site documents what a user installs, and the neutral dialect (`{{issue}}`, `agent-files/*.md`) is not valid in any harness — by `tools/gen_command_pages.py` and committed; never hand-edit them; run `python3 tools/gen_command_pages.py` and commit the result. CI enforces this with `--check`. `site/docs/` are **hand-authored** reference pages (install, harnesses, troubleshooting, architecture) — the validator covers them, and `gen_command_pages.py` discovers them for the sitemap.
- **`$ARGUMENTS` is the only argument placeholder a `commands/*.md` may contain.** A skill has no positional arguments: indexed substitution is not in the Agent Skills standard, and its index base has changed between Claude Code versions, so a workflow written against one base silently reads the wrong argument on another. Parse `$ARGUMENTS` in prose instead ("the first word of `$ARGUMENTS` is the issue number"). **No `$` followed by a digit anywhere in the file — frontmatter and fenced code blocks included.** Substitution is textual over the whole file, so a fence protects nothing: run `/ship-issue 42 focus on retries` and a shell snippet inside a fence that asks for field two is rewritten to ask for the literal word `on`. *(An earlier version of this bullet said fenced blocks were exempt because `$` + digit there is a shell field reference. That was wrong — nothing in the Claude Code docs describes a fence exemption, and the documented way to keep a literal is a backslash escape, which would be pointless if fences were exempt.)* If you genuinely need a literal, escape it — `\$2` — but prefer restructuring so you never do: `cut -f2` rather than an `awk` field reference. `cargo run -- check` enforces this over the whole file, fenced or not.
- **Name by register — never bulk-substitute one noun for another.** Three terms, three jobs. A **skill** is the artifact on disk — `.claude/skills/<name>/SKILL.md`, the [Agent Skills](https://agentskills.io) open-standard shape. A **command** is a whole workflow the captain issues to the crew — `/ship-issue`, `/fix-bug`; the twelve are **the commands**. An **order** is what a single subagent is told to do *within* a command — one specialist's instruction, never the workflow and never the set of twelve. Use *skill* in tech-leading copy — install paths, layout and reference sections, frontmatter docs, contributor instructions, anything about the file on disk or cross-harness portability. Use *command* in brand-leading copy — the intro, the crew-and-commands framing, taglines, section headings that sell the metaphor, example usage. Two retired spellings: don't prefix "command" with "slash-" (Anthropic's pre-skills label, and Claude-Code-only — plain "command" is the product noun we want), and write **subagent** as one word, never hyphenated or spaced, matching the Claude Code, Cursor and OpenCode docs. "The crew" and "shipmates" stay as brand terms. Full statement, with the do/don't examples: [`docs/BRAND.md`](docs/BRAND.md#naming-register).
- **Every command is user-invoked only.** All twelve set `disable-model-invocation: true`. You start one by typing `/ship-issue`; Claude never loads one on its own. These workflows create worktrees, push branches and open pull requests, so the decision to start one stays with the captain. Per the Claude Code docs this does **not** affect direct `/name` invocation (`user-invocable` is the field that would), and it additionally keeps the skill out of subagent preloading and off scheduled-task triggering.
  **The same decision is why the twelve land in `.opencode/commands/`, not `.opencode/skills/`.** opencode has both directories and they mean different things: its *skills* are model-invoked — the model loads one on demand through a native `skill` tool — and `disable-model-invocation` is not one of the frontmatter keys a `SKILL.md` recognises there, so writing it would be silently dropped rather than rejected. `commands/` is `/`-invoked only, which preserves user-invoked-only structurally instead of by a key the target ignores. This makes the naming register load-bearing rather than stylistic: on opencode, *commands* and *skills* are two real directories, so using the words interchangeably is actively wrong.
- **Don't wholesale-rewrite `commands/ship-issue.md`** — make scoped edits; it encodes the flagship's gated state machine.
- **Adding a frontmatter key to a canonical command?** You must add a harness entry per target in `tools/capability_registry.json` where the key maps a tool, and the reference digests must be regenerated: run `cargo run -- build --target <target> --update` for every implemented target and commit `tests/payload-digests/*.sha256` — never edit them by hand. They are regression fixtures: CI proves each freshly built payload matches its committed digest with `cargo run -- check --target <target>`, so a changed rendering rule fails loudly instead of shipping silently. Nothing under `tests/payload-digests/` is ever installed; what a user receives is compiled from `commands/` + `crew/` at install time.
- **Adding a harness?** Implement `src/adapters/<harness>.rs`, register it in `src/adapters/mod.rs` (module + `targets()`), wire it into the `select()` in `src/main.rs`, add the target to `targets` and `target_status` in `tools/manifest.json`, and map its tool names in `tools/capability_registry.json`. Adapters translate the semantic capabilities (`read`, `edit`, `bash`, `web`, `agent`) and the role `*-scopes` refinements — they must never put a target's tool names into canonical content. **Least privilege is the adapter's job, not the target's default.** Claude Code's permission model is an allowlist, so naming the tools a role needs is sufficient. opencode's is not: its shipped defaults are effectively `"*": "allow"`, so an allow-list alone grants nothing, and every generated opencode agent emits a `"*": deny` catch-all *first* and its specific allows after — that target resolves permissions last-match-wins, so the ordering is the mechanism. A harness with no subagent mechanic ships the crew as skills only (`emit_skill_files` in `src/adapters/render.rs`), emitting the Agent Skills standard's `name`/`description` pair and nothing else; a new adapter must establish the equivalent for its own target and say so.
- See [`CONTRIBUTING.md`](CONTRIBUTING.md) for the full contribution guide.

## Scope & honesty

- **Claude Code today; seven more targets build but are not runtime-verified.** Claude Code is the only harness Shipmates has been *run* on. opencode, Antigravity CLI, Codex CLI, Cursor, GitHub Copilot, Windsurf and Zed each have an adapter — `shipmates install --harness <name>` produces their trees, every payload's format was checked against the harness's own parsing source and first-party docs, and each target's digest is gate-checked in CI. opencode and Antigravity receive the full crew + all 12 commands (they have native subagent directories); the other five ship the 12 skills only. None has been **verified against a running harness**: whether agents resolve, whether argument passing behaves, and whether `/ship-issue` completes end to end are open. opencode's are tracked in [#31](https://github.com/saman-mb/shipmates/issues/31) and [#32](https://github.com/saman-mb/shipmates/issues/32). Do not write "tested on <harness>" anywhere until those close. See the roadmap on the [website](https://saman-mb.github.io/shipmates/#next) and in [`README.md`](README.md#-on-the-horizon). The Gemini CLI is retired (shut down June 18, 2026); the Antigravity CLI (`agy`) is its successor and reads `.agents/`, so no `gemini` target is shipped.
- **Gates are orchestrated by the command, not hook-enforced yet.** The worktree / green-CI / fresh-reviewer gates are driven by the workflow prompt; a code-enforced (hook-backed) state machine is planned, not in the current release.
- **Not affiliated with Anthropic.** "Claude" and "Claude Code" are trademarks of Anthropic.

## Links

- **Website:** https://saman-mb.github.io/shipmates/
- **Repository:** https://github.com/saman-mb/shipmates
- **License:** [MIT](LICENSE)
