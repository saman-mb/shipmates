# Canonical crew and command sources

`canonical/` is the authoritative harness-neutral content, and the only tree a contributor
edits. `agents/`, `skills/` and `tests/golden/claude-code/` are all **generated** from it by
`tools/export.py`; `agents/` and `skills/` stay in the repository because the site generator
and the skills validator read them, and CI proves they still match the export. An edit to a
generated mirror ships nothing and fails the export check with a `compatibility drift:` line.

`canonical/manifest.json` declares every root the exporter reads or gates against, plus the
enabled targets and their status. After any canonical edit, regenerate:

```bash
python3 tools/export.py build --target claude-code --update
```

Only `claude-code` has committed generated mirrors, so it is the only target `--update`
regenerates. The opencode payload is built at install time into a temp directory and is never
committed; check it with `python3 tools/export.py build --target opencode --out /tmp/oc`.

## Crew schema

`canonical/crew/<role>.md` uses frontmatter with `name`, `description`, `capabilities`,
`writes`, and `source`. `source` is provenance metadata only. The body is the complete
authoritative persona; changing it changes every export.

Capabilities are deliberately semantic: `read`, `edit`, `bash`, `web`, and `agent`.
Adapters translate them through `tools/capability_registry.json`; they must not invent
per-harness tool names.

Role scopes refine least privilege without putting target tool names in canonical content:
`web-scopes` distinguishes search from fetch, `read-scopes` distinguishes file read/search/glob,
and `tool-order` preserves an adapter's established permission ordering. Each adapter maps these
scopes onto its own target through the `scopes` map in `tools/capability_registry.json` — the
Claude adapter to its exact tool list, the opencode adapter to its permission keys. So
`art-director`, which declares `web-scopes: search`, gets `WebSearch` (and not `WebFetch`) on
Claude Code and `websearch: allow` (and not `webfetch`) on opencode, from one neutral declaration.

## Command schema

`canonical/commands/<workflow>.md` uses frontmatter with target-neutral narrative metadata,
declared `arguments`, `loop_max`, a JSON `stages` list, and the invocation template
`@{{role}}({{named-argument}})`. The body is the complete authoritative workflow narrative.

Each stage carries `order`, `stage`, `roles`, `gate`, and `max_loops`. `roles` is a **list**
because a stage can fan out — `/pr-review`'s board runs a `product-manager` and an `sdet` on
every PR — and a singular field forced that to be written as two sequential stages, which is
not what happens.

**Stages describe agent dispatch, not narrative structure.** A stage the narrative runs on the
orchestrator (intake, consolidate, tag, report) is not listed: inventing a role for it produces
a table that reads as authoritative and is fiction. The exporter enforces this from the other
side — every role a stage declares must appear in the narrative it ships beside, so the table
cannot drift away from the workflow while the export stays green.

Roles, arguments, invocation references, loop bounds, and `$`-positional placeholders are all
validated before any write.

## Naming

The twelve workflows are **commands** (`/ship-issue`, `/fix-bug`), stored as **skills** on
disk under Claude Code. An *order* is what a single subagent is told to do *within* a command —
never the workflow and never the set of twelve. See
[`../docs/BRAND.md`](../docs/BRAND.md#naming-register).

The register is load-bearing on opencode, which has a `commands/` directory and a `skills/`
directory that are not the same thing. The twelve go to `.opencode/commands/` — see below.

## Targets

`AGENTS.md` is project guidance in the neutral model. Each adapter maps it to its target's
project-instructions filename; this exporter emits crew and command payloads only. Target
status is declarative — `canonical/manifest.json` lists each target in `targets` with a
`target_status`, and the exporter refuses anything that is not `implemented` on that basis
rather than by a hardcoded name.

Two targets are `implemented`: `claude-code` and `opencode`. The remaining six (`cursor`,
`codex`, `github-copilot`, `gemini`, `windsurf`, `zed`) have no adapter and are refused.

### opencode

Two decisions in that adapter are worth knowing before you change it.

**Commands, not skills.** opencode supports both, and the twelve go to
`.opencode/commands/<name>.md` — flat files, `/`-invoked only. Its *skills* are model-invoked:
the model loads one on demand through a native `skill` tool, and `disable-model-invocation` is
not among the frontmatter keys a `SKILL.md` recognises there, so declaring it would be silently
dropped rather than rejected. The twelve create worktrees, push branches and open pull requests,
so shipping them as skills would let the model start one on its own. `commands/` keeps
user-invoked-only a structural property instead of a key the target ignores.

**Deny-first permissions.** opencode's defaults are permissive — effectively `"*": "allow"` — so
listing the tools a role needs would grant nothing extra and restrict nothing. Every generated
agent therefore emits a `"*": deny` catch-all first and its specific allows after; opencode
resolves permissions last-match-wins, so that ordering is what makes least privilege hold. The
net posture is slightly stronger than Claude Code's allowlist: a tool denied by a wildcard rule
is hidden from the model rather than refused when it is called.

Agents land at `.opencode/agents/<name>.md` (plural directory) with `mode: subagent`. opencode
is the only non-Claude target that receives subagents, because it is the only one with a
documented native subagent directory.

**Not runtime-verified.** The format was checked against opencode's own parsing source and its
first-party docs. Nothing here has been exercised against a running opencode — agent resolution,
argument passing and an end-to-end `/ship-issue` are tracked in
[#31](https://github.com/saman-mb/shipmates/issues/31) and
[#32](https://github.com/saman-mb/shipmates/issues/32).
