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

## Crew schema

`canonical/crew/<role>.md` uses frontmatter with `name`, `description`, `capabilities`,
`writes`, and `source`. `source` is provenance metadata only. The body is the complete
authoritative persona; changing it changes every export.

Capabilities are deliberately semantic: `read`, `edit`, `bash`, `web`, and `agent`.
Adapters translate them through `tools/capability_registry.json`; they must not invent
per-harness tool names.

Role scopes refine least privilege without putting target tool names in canonical content:
`web-scopes` distinguishes search from fetch, `read-scopes` distinguishes file read/search/glob,
and `tool-order` preserves an adapter's established permission ordering. The Claude adapter maps
these scopes to its exact tool list.

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
disk. An *order* is what a single subagent is told to do *within* a command — never the
workflow and never the set of twelve. See [`../docs/BRAND.md`](../docs/BRAND.md#naming-register).

## Targets

`AGENTS.md` is project guidance in the neutral model. Each adapter maps it to its target's
project-instructions filename; this exporter emits crew and command payloads only. Target
status is declarative: `opencode` is registered as `registered-not-implemented` in
`canonical/manifest.json` and refused on that basis, not by a hardcoded name in the exporter.
