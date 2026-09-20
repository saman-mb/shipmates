---
name: shipmates-consolidate-issues
description: Shipmates: Review every open issue against git history, close what's already done or stale, migrate the legacy ones, slim the backlog, and bundle the survivors into themed groups so similar work ships together efficiently. Read-only by default — it reports; changes happen only on an explicit apply.
argument-hint: [scope filter such as label/area] [apply] — no args: the whole open backlog, report-only
allowed-tools: Bash, Read, Write, Edit, Agent, Grep, Glob, WebSearch, WebFetch
disable-model-invocation: true
---
# /shipmates-consolidate-issues — inventory → cross-check history → close → migrate → bundle
<!-- shipmates:command-preamble -->

Turn a drifting issue tracker back into a plan. The whole open-issue set is pulled, every issue is
checked against the project's git history and merged PRs to see what is **already done**, the done
and stale ones are closed, legacy-shaped issues are migrated to the tracker's current conventions,
and the survivors are **bundled by theme** into groups each big enough to hand to `/shipmates-issue` in
one pass — so the backlog shrinks and the work that remains ships in coherent chunks instead of as
an endless one-issue-at-a-time trickle.

The scope and the go-ahead to change anything come from the Runtime input section at the end of this workflow.

---

## Config

- `MODE` = `report` (default) — **read-only**: inventory, cross-check, triage, bundle. It changes
  nothing on the tracker. `apply` — do the work, only with an explicit request: close the
  evidence-backed ones, migrate the legacy ones, add the bundle labels. When ambiguous, default to
  `report` and state which mode you ran.
- `SCOPE` = optional narrowing of the triage — a label or comma-separated labels, an area, an owner,
  or an age/activity filter. Empty means the entire open-issue set.
- `TRAILER` = the session's required session/author trailer line, read from context; never invent
  one. Every comment or edit you write carries it.
- `REVIEWER` = `product-manager` — the only role this workflow spawns; everything else is the
  orchestrator reading the tracker and the repo directly.

---

## Stage 0 — Inventory  (orchestrator)

1. Pull the **entire open-issue set**: `gh issue list --state open --limit 1000` with
   `number,title,labels,milestone,assignees,createdAt,updatedAt` and the body for each. Also pull
   `gh label list` and the project's `README` / `AGENTS.md` (conventions, quality bar, where work is
   tracked) so triage uses the project's own standards.
2. Apply `SCOPE` if set; otherwise take the full set.
3. Snapshot the baseline count and, per issue, its age, last activity, labels, and milestone. These
   numbers feed the report; record them before anything is touched.

## Stage 1 — Cross-check against git history  (orchestrator, evidence first)

For **every** issue in scope, gather the evidence before any verdict:

- **Merged work**: `git log <base> --oneline` plus `gh pr list --state merged` and `gh pr list
  --state closed` — search titles, descriptions, and linked references for the issue's subject and
  keywords. A merged PR whose body references the issue is the strongest signal.
- **Open work**: is an open PR or a live branch already addressing it? `gh pr list --state open`
  plus `git branch -a` — inspect branch names and PR titles/descriptions for the subject.
- **Freshness**: last-comment and last-update dates, plus the issue's label/milestone state.

Record, per issue, the **evidence string** (the refs, PRs, commits, or activity you found). Nothing
is judged on title-matching alone; every verdict names its evidence.

## Stage 2 — Triage verdicts  (orchestrator deterministic + ONE `product-manager` pass)

First the orchestrator applies the **hard, mechanical rules** (no judgment needed):

- `DONE` — evidence shows the change shipped (a merged PR/commit implements it). Close with a
  comment naming the implementing PR/commit.
- `STALE` — no activity and no signal it's still wanted: older than the project's stale horizon,
  no assignee, no linked PR/branch, and the subject is fully superseded by something that shipped.
  Don't guess at these alone — send them to the `product-manager` review pass.

Then spawn ONE `product-manager` with the remaining borderline set: the `DONE`-adjacent, the
`STALE`-candidates, and the clearly duplicate-looking pairs. It returns **structured verdicts only**
(no `gh` calls): per issue a verdict of `close` / `keep` / `migrate` with a one-line reason, plus a
dedupe map (issue → canonical issue) only where the overlap is unmistakable. It never closes
anything itself.

The orchestrator merges the two: mechanical rules are final; the subagent's judgment calls are
adopted as its recommendation, and every recommendation the user would want to veto is surfaced
prominently in the report.

## Stage 3 — Migrate the legacy  (orchestrator)

`MIGRATE` applies to issues whose **shape** is obsolete even though the work is still wanted:
- wrong tracker conventions (missing labels/structure the project now requires, body not in the
  current template, no acceptance criteria where the project mandates them),
- issues that should be an epic with stories, or a story inside an existing epic (`Part of #…`),
- issues blocked by a fixed/unneeded parent (remove the stale `Blocked by` or dependency note).

Migrating means rewriting to the project's current shape — never inventing new scope. If a
migration would silently change the meaning, keep it and flag it for the user instead.

**When a migration creates a parent and its children, attach the graph with `gh`.** An issue that
should become an epic with stories (or a story that should join an existing epic) is only migrated
when the parent/child relationship exists as a GitHub fact — a body mention is a mention, and an epic
checklist is display, not linkage. After creating each epic and story with `gh issue create` and
validating every captured number against `^[0-9]+$`, attach every child explicitly:

```bash
gh issue edit <epic> --add-sub-issue <story>
```

Several children may be attached in one call as a comma-separated list. Read the parent's current
children first (`gh issue view <epic> --json subIssues`; child numbers live at
`subIssues.nodes[].number` — `subIssues` is a connection object, not a list) and skip any already
attached, so a re-run adds nothing twice. Then verify by re-fetching `gh issue view <epic> --json
subIssues,subIssuesSummary` and confirming the child numbers are exactly the stories created — a
missing child means the attach did not land: retry it **once**, re-verify, then report the gap. If
the host has no sub-issue mechanic (the flag is rejected), fall back to the epic checklist plus
`Part of #<epic>` back-references and say in the report that the graph is missing — never fake it.
`/ship-plan-epics` Stage 3 owns the full filing sequence; mirror it here.

## Stage 4 — Bundle the survivors  (ONE `product-manager`, parallel by area)

Group every `keep` issue into **bundles**: coherent themes, each sized for a single `/shipmates-issue`
run. Spawn one `product-manager` per **area** (from the project's existing area labels) so the
theming runs in parallel; give each its area's issues, the repo context, and the rule that bundles
are **thematic + dependency-ordered + individually shippable** — never a grab-bag of unrelated
tickets. Each returns structured bundle proposals: `name`, `theme` (one line), `issues` (numbers),
`order` (build sequence within the bundle), and a suggested label. Orphan issues that fit no theme
stay as singletons — don't force them in.

The orchestrator dedupes the proposals, resolves cross-area overlaps (an issue can belong to only
one bundle), and assigns each bundle a `bundle:<name>` label.

## Stage 5 — Apply (only in `apply` mode) and report

If `MODE=report`: print the full plan and STOP — the verdict table, the dedupe map, the bundle tree
with its labels, and the proposed closes/migrations. **Change nothing.**

If `MODE=apply`, execute in this order, re-verifying each before acting:
1. **Labels** — create any missing `bundle:<name>` labels.
2. **Migrate** — rewrite the flagged legacy issues to the current shape (with the `TRAILER`).
3. **Close** — close each `DONE` and `STALE` issue with a comment naming its evidence; on a dedupe,
   comment the canonical issue and cross-link the pair.
4. **Tag bundles** — apply each bundle's label to its issues and note the bundle in a comment, so
   `/shipmates-issue` can be run per bundle.

Then verify (re-fetch and grep; don't assume): every planned close happened, every migration landed,
every bundle label is on exactly its issue set. Report the before/after: how many issues were in
scope, how many closed (with the count that were already done), how many migrated, and the bundle
tree ready to hand to `/shipmates-issue` — one bundle at a time.

---

## Parameters

| Name | Required | Token | Values | Default | Help |
|------|----------|-------|--------|---------|------|
| scope | no | label/area filter | label, area, owner, or age filter | whole open backlog | Narrow which open issues to triage. |
| apply | no | `apply` | `apply` | report | When present, execute closes/migrations/labels; otherwise `MODE=report` only. |

## Runtime input

Read `## Parameters` first. `$ARGUMENTS` is the captain-supplied or post-intake token string; parse
Tokens/Values from that table (required then optionals). If intake ran, treat the restated
invocation as authoritative for this run.

Empty means the whole open-issue set in `MODE=report` — every open issue, no scope filter. Otherwise
tokens are usually a scope filter and/or the word `apply`; parse in prose and treat anything unstated
as `MODE=report`. When in doubt, report only — closing other people's issues is a decision the captain
makes, not the default.

### Guardrails
- **Evidence or nothing.** An issue is `DONE` only with a merged PR/commit to point at; it is `STALE`
  only after the `product-manager` pass. Every close comment names the evidence.
- **The orchestrator owns all `gh`/`git`.** `product-manager` subagents only return structured
  verdicts and bundles — they never close, migrate, or edit issues themselves.
- **Never lose intent.** Migrating preserves the issue's substance; a migration that would change
  meaning is reported, not performed. Dedupe only on unmistakable overlap, and always cross-link.
- **Don't over-close or over-bundle.** A `STALE` verdict with any live signal goes to the user.
  Bundles are thematic and shippable — never arbitrary groupings to make the count look better.
- **Respect `MODE`.** In `report` mode, the tracker is never modified, not even a label.
- If a role doesn't resolve to its installed role file, fall back to a general-purpose agent with the
  product-manager brief inlined, and note the fallback.
