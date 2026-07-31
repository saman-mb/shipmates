# Canonical crew and order sources

`canonical/` is authoritative harness-neutral content. `agents/` and `skills/` are retained
as compatibility sources for site/docs and legacy tooling; exporters never read their bodies.
`canonical/manifest.json` records enabled targets and target status.

## Crew schema

`canonical/crew/<role>.md` uses frontmatter with `name`, `description`, `capabilities`,
`writes`, and `source`. `source` is provenance/compatibility metadata only. The body is
the complete authoritative persona; changing it changes every export.

Capabilities are deliberately semantic: `read`, `edit`, `bash`, `web`, and `agent`.
Adapters translate them through `tools/capability_registry.json`; they must not invent
per-harness tool names.

Role scopes refine least privilege without putting target tool names in canonical content:
`web-scopes` distinguishes search from fetch, `read-scopes` distinguishes file read/search/glob,
and `tool-order` preserves an adapter's established permission ordering. The Claude adapter maps
these scopes to its exact compatibility tool list.

## Order schema

`canonical/orders/<workflow>.md` uses frontmatter with target-neutral narrative metadata,
declared `arguments`, `loop_max`, a JSON `stages` list, and the invocation template
`@{{role}}({{named-argument}})`. Stages carry `role`, `gate`, and `max_loops`; roles,
arguments, invocation references, and loop bounds are validated before any write. The
body is the complete authoritative workflow narrative.

`AGENTS.md` is project guidance in the neutral model. Each adapter maps it to its target's
project-instructions filename; this exporter emits crew and order payloads only. `opencode`
remains registered as not implemented and is refused explicitly by `tools/export.py`.
