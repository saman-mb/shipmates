---
name: shipmates-plan-epics
description: Shipmates: Turn a brief (or several) into a tracked backlog — GitHub epics + linked, labelled user stories, with a context-selected planning panel authoring and reviewing in parallel.
argument-hint: <brief text | path to a brief file | several briefs> [area/label hints] [apply] [optional role hints — e.g. "also involve architect and UX"]
allowed-tools: Bash, Read, Write, Agent, Grep, Glob, WebSearch, WebFetch
disable-model-invocation: true
---
# /shipmates-plan-epics — briefs → GitHub epics + user stories

<!-- shipmates:bootstrap -->
Turn the supplied brief(s) into a planned backlog: one or more **epics** (labelled `epic`), each
broken into **user stories** (labelled `user-story` + area tags), cross-linked once filed. A
**planning panel** — always including `product-manager`, plus up to two context-derived specialists
when the brief implicates them — scopes epics, authors stories, and reviews slices for domain fit.
When the work spans **multiple epics, one `product-manager` subagent authors each epic's stories in
parallel** — one PM per epic.

**Planning is the default; filing is opt-in.** The run returns the panel, the epic→story tree and the
attachment plan without touching the tracker. Only an explicit `apply` creates the issues, labels and
parent/child links — a scatter of issues and labels across someone's tracker has no one-command undo,
so it is the caller's decision, never a default.

The briefs come from the Runtime input section at the end of this workflow.

---

## Config (defaults — override only if the repo clearly needs it)

- `REPO` = current repo (`gh repo view --json nameWithOwner -q .nameWithOwner`).
- `EPIC_LABEL` = `epic`; `STORY_LABEL` = `user-story` (create if missing, and only under `APPLY`).
- `AREA_LABELS` = derived from the repo's existing `gh label list` (prefer the `area:*` family). Create
  a new area label only when a story clearly needs one that doesn't exist — and say which you created.
- Issue-body trailer: `{{session-key}}: ...` on every epic and story (read from the harness/session
  context; do not invent).
- `APPLY` = on only if the caller says `apply`. This is the **single** switch for tracker writes: unset
  means plan and report, touching nothing. `dry-run` / `dry run` / `preview` are still accepted and
  simply name that default explicitly, so no second knob decides the same thing.
- `MAX_PANEL_ADDITIONS` = `2` — non-PM specialists added beyond the always-present PM; cost discipline
  says spend seats only where their decision can change the backlog shape.

---

## Stage 0 — Intake & context  (orchestrator)

1. Parse `{{brief}}` into one or more briefs; read any referenced files/dirs.
2. Gather repo context so the backlog fits the project: `README` / `{{project-instructions}}` (domain, conventions,
   quality bar), `gh label list`, and the **existing open issues** (`gh issue list --state open
   --limit 300 --json number,title,labels`) so you can dedupe against work already tracked.
3. Decide the shape: does the brief map to **one** cohesive epic, or **several** independent ones?
   (A brief naming multiple distinct capabilities → multiple epics.)
4. If a brief is too vague to scope responsibly, ask the user ONE round of clarifying questions
   before creating anything — don't invent a backlog from thin air.
5. **Select the planning panel** (see below). Record `<panel>` and `<panel-reasons>` before Stage 1.
   When `APPLY` is unset, also print them here — they appear again in the Stage 3 plan output.

   **Panel selection.** Always include **`product-manager`** — backlog authorship is still the core job.

**Explicit overrides win.** If the caller names roles in the brief or runtime guidance (e.g. "also
involve architect and UX designer"), use those roles — resolve each via the crew roster / fuzzy-match
rules below; do not drop a named role because of the cap.

Otherwise, derive additions from the **brief + repo context** using the same domain signals
`/shipmates-ship-issue` and `/shipmates-pr-review` use (but read from the brief, not a diff). Each signal is independent;
a brief can trip more than one. Add a role only when the brief **genuinely implicates** that
specialty — a neutral backlog-grooming brief (label hygiene, duplicate cleanup, pure bookkeeping)
stays **PM-only**, same as today. Cap auto-selected non-PM additions at `MAX_PANEL_ADDITIONS`; if
more signals fire, keep the two with the highest planning impact and note the rest as out-of-panel.

| Signal (brief implicates…) | Role |
|---|---|
| New subsystems, persisted schema, cross-cutting platform boundaries | `architect` |
| On-screen UI — screens, flows, design system, a11y | `ux-ui-designer` |
| Rendered visual **art** in an art-producing domain (game sprites, brand/motion assets — not app chrome) | `art-director` |
| Data, ML, metrics, analytics, experimentation | `data-scientist` |
| Latency, throughput, profiling, resource limits | `performance-engineer` |
| Build, CI, packaging, shipping, toolchain pins | `devops-engineer` |
| Authn/authz, secrets, crypto, untrusted input | `security-engineer` |
| Reliability, incidents, rollback, SLOs | `site-reliability-engineer` |
| Documented behaviour, public API/CLI surface, user-facing docs | `technical-writer` |

Resolve every role name against the crew roster; on miss, fall back to a general-purpose agent with
that role's brief inlined and note the fallback. Set `<panel-reasons>` to one line per non-PM role
(`role: why the brief needs them`).

## Stage 1 — Scope into epics  (agents: panel)

Spawn **`product-manager`** (always) with all briefs + Stage 0 context. It returns the **epic set**
(writes no issues) — for each epic: a crisp `title`, the `why`/goal (the outcome, not the output),
high-level `scope` and non-goals, `dependencies`/build-order between epics, and suggested `area`
label(s). One brief may yield one epic; that's fine.

If **`architect`** is on `<panel>`, spawn it **in the same message** so it runs in parallel: given
the PM's epic set (or the briefs when the PM returns first — prefer parallel spawn with the same
inputs; the architect validates boundaries, coupling, and cross-epic ordering). Merge architect
amendments into the epic set before Stage 2 — drop or reshape epics the architect flags as
ill-scoped, and carry its notes forward.

**Harness portability — Pi parallel spawn.** On most harnesses, several blocking subagent calls in
one turn run concurrently. On Pi, `pi-subagents` rejects a second blocking call while one is already
in progress. For parallel panel fan-out on Pi, use either (a) one `subagent({ workflow: true, ... })`
call with parallel children, or (b) async launches (`subagent({ async: true, ... })`) followed by
status polling. A rejected panelist is a failed run — never silently shrink the planning panel.

Other non-PM panel members whose expertise is story-level (UX, art, data, perf, etc.) defer to
Stage 2 — they do not scope epics.

## Stage 2 — Author each epic's stories  (agents: `product-manager` × N, then panel review)

**Author (parallel).** Spawn **one `product-manager` subagent per epic, in a single message** so
they run concurrently. Give each: its epic (title/why/scope), the full repo context, the OTHER
epics' titles (for cross-epic dependencies), the existing-issues list (to dedupe), and `<panel>` so
it knows which specialists will review. Each returns its epic's **user stories** as structured data
(no `gh` calls) — per story:

**Harness portability — Pi parallel spawn.** On most harnesses, several blocking subagent calls in
one turn run concurrently. On Pi, `pi-subagents` rejects a second blocking call while one is already
in progress. For parallel author fan-out on Pi, use either (a) one `subagent({ workflow: true, ... })`
call with parallel children, or (b) async launches (`subagent({ async: true, ... })`) followed by
status polling. A rejected author is a failed run — never silently drop an epic's stories.
- `title` (short, outcome-shaped),
- `description` (the user value: "As a … I want … so that …" where it fits),
- **acceptance criteria** — explicit and checkable, Given/When/Then where natural,
- `area` label(s), and dependency order (`blocked_by` other stories where real).

Tell each PM: stories should be **INVEST** (independent, negotiable, valuable, estimable, small,
testable) — one shippable slice each, vertically sliced over horizontal layers, dependency-ordered,
and NOT duplicating an existing open issue. Prefer a handful of well-formed stories over a long shallow
list. To avoid clobbering, each PM returns its data **inline** (no shared temp filenames).

**Panel review (parallel, only when `<panel>` has non-PM members).** After all PMs return, spawn
**each non-PM panel member once, in a single message**, with every epic's draft stories + `<panel-reasons>`.
Each returns review notes and concrete amendments for its domain (UX coherence, art gates, data
validity, perf measurability, etc.). The orchestrator merges amendments into the story set before
Stage 3 — prefer specialist fixes that sharpen acceptance criteria over PM rewrites.

**Harness portability — Pi parallel spawn.** On most harnesses, several blocking subagent calls in
one turn run concurrently. On Pi, `pi-subagents` rejects a second blocking call while one is already
in progress. For parallel review fan-out on Pi, use either (a) one `subagent({ workflow: true, ... })`
call with parallel children, or (b) async launches (`subagent({ async: true, ... })`) followed by
status polling. A rejected reviewer is a failed run — never silently shrink the review panel.

## Stage 3 — File the issues  (orchestrator, deterministic — only under `APPLY`)

**When `APPLY` is unset this stage is the deliverable, not a skipped one.** Print **`<panel>` and
`<panel-reasons>`**, then the epic→story tree (titles, criteria, labels, order), then the
**attachment plan** — which stories would become sub-issues of which epic — and STOP. Create nothing,
attach nothing, and say plainly that the run was plan-only and how to file it.

Under `APPLY`, proceed in this order (numbers must exist before they're referenced):
1. **Labels** — ensure `epic`, `user-story`, and any needed `area:*` exist (`gh label create` the
   missing ones; note which you created). Don't spam the namespace — only labels you'll actually use.
2. **Epics first** — `gh issue create` each epic: body = the why/goal + scope/non-goals + a **story
   checklist placeholder** + cross-epic dependency notes + the trailer + `epic` label. Capture each
   epic's issue number.
3. **Stories** — `gh issue create` each story: body = description + acceptance criteria + **`Part of
   #<epic>`** + `Blocked by #<n>` where known + the trailer + `user-story` and area labels. Capture
   each story number.
4. **Attach every story as a sub-issue of its epic** — a body mention is a mention; the parent/child
   relationship is a separate GitHub fact and only this step creates it. For each epic, using the
   **numbers captured in steps 2–3** (validate each against `^[0-9]+$` first — never interpolate a
   title, body, or any other GitHub-sourced text into the command):
   ```bash
   gh issue edit <epic> --add-sub-issue <story>
   ```
   Several children may be attached in one call as a comma-separated list of numbers.
   **Idempotent** — read the parent's current children first (`gh issue view <epic> --json subIssues`;
   child numbers live at `subIssues.nodes[].number` — `subIssues` is a connection object, not a list)
   and skip any story already attached, so a re-run or a partial earlier run adds nothing twice.
   If the host has no sub-issue graph (the flag is rejected or the field is unknown), note that in the
   report and carry on — steps 3 and 5 still leave the epic traceable.
5. **Backfill the epic checklists** — edit each epic body, replacing the placeholder with `- [ ] #<story>`
   lines for its stories, so the epic tracks its children in readable form and GitHub cross-links them.

## Stage 4 — Verify & report

**Under `APPLY`** (something was filed):

- **Verify the sub-issue graph** — re-fetch each epic (`gh issue view <epic> --json
  subIssues,subIssuesSummary`) and confirm its children (`subIssues.nodes[].number`) are exactly the
  stories you created for it: the count matches the story count and every captured story number
  appears. A missing child means step 4 didn't land — retry it **once** for that story, then
  re-verify. If it is still missing, report that host/graph gap — do not loop. Report the parent's
  completion summary (children total and done).
- Verify every story carries `Part of #<epic>` matching its epic, and every epic's checklist lists all
  its stories (re-fetch and grep; don't assume).
- Report a tree: each **epic** (title + link) → its **stories** (title + link), with counts,
  **sub-issue attachment counts per epic**, any labels created, a recommended **build order**, and
  anything skipped as an existing duplicate. Say plainly when a host rejected the sub-issue
  attachments.

**Without `APPLY`** (nothing was filed — there is no graph to verify, so do not pretend to): report
**`<panel>` and `<panel-reasons>`**, then the same tree without links, the attachment plan, a
recommended **build order**, anything skipped as an existing duplicate, and one closing line naming
`apply` as the token that files it.

---

### Guardrails
- The orchestrator owns all `gh` calls; subagents only return structured data — they never create
  issues themselves.
- **Dedupe.** Never re-file work already tracked in an open issue; note what you skipped and why.
- **Don't over-scope.** A brief becomes the smallest coherent set of epics/stories that delivers it —
  no speculative epics the brief didn't ask for.
- **Panel cost.** Respect `MAX_PANEL_ADDITIONS` for auto-selection; never spawn a specialist whose
  domain the brief does not implicate. Explicit user-named roles override the cap.
- **The sub-issue graph is the relationship source of truth**; the epic's checklist is display and
  progress copy for human readers. Write **both** — never treat a tidy checklist as evidence that the
  parent/child link exists, and never drop the checklist because the graph is populated.
- **One switch decides the tracker.** `APPLY` is the only token that writes; unset means create
  nothing and attach nothing, while still printing the selected panel and the attachment plan.
  `dry-run` / `preview` are accepted and name that same default — they are not a second decision.
- Every epic and story must be individually valuable and traceable (`Part of #`), so the backlog is
  ready to hand to `/shipmates-ship-epic <epic#>` or `/shipmates-ship-issue` one story at a time.
- If a role doesn't resolve to a shipped crew role, fall back to a general-purpose agent with the
  role brief inlined, and note the fallback.

## Parameters

| Name | Required | Token | Values | Default | Mutates | Help |
|------|----------|-------|--------|---------|---------|------|
| brief | yes | free text / path(s) | text  /  path  /  multi | — | — | One or more briefs: inline text, a file or directory path, or several separated by `---` or numbering. |
| area_hints | no | free text | area/label names | — | — | Optional area or label hints for the backlog. |
| apply | no | `apply` | `apply` | plan only | yes | When present, create the epics, stories, labels and sub-issue links; otherwise plan and report without mutating the tracker. |
| role_hints | no | `also involve …` free text | crew role names | — | — | Explicit planning-panel roles; overrides auto selection. |

## Runtime input

Read `## Parameters` first. `$ARGUMENTS` is the captain-supplied or post-intake token string; parse Tokens/Values from that table (required then optionals). If intake ran, treat the restated invocation as authoritative for this run.

A brief may be inline text, a path to a file or directory, or several briefs separated by `---` or numbering. Optional role hints ("also involve …") travel with the brief text and override auto panel selection. `apply` is the single token that files the backlog; omitting it plans and reports only. `dry-run` / `dry run` / `preview` are accepted and name that same default.
