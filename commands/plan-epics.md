---
name: plan-epics
description: Shipmates: Turn a brief (or several) into a tracked backlog — GitHub epics + linked, labelled user stories, with a context-selected planning panel authoring and reviewing in parallel.
argument-hint: <brief text | path to a brief file | several briefs> [area/label hints] [dry-run] [optional role hints — e.g. "also involve architect and UX"]
allowed-tools: Bash, Read, Write, Agent, Grep, Glob, WebSearch, WebFetch
disable-model-invocation: true
---
# /plan-epics — briefs → GitHub epics + user stories
<!-- shipmates:command-preamble -->

Turn the supplied brief(s) into a tracked backlog: one or more **epics** (labelled `epic`),
each broken into **user stories** (labelled `user-story` + area tags), created as GitHub issues and
cross-linked. A **planning panel** — always including `product-manager`, plus up to two
context-derived specialists when the brief implicates them — scopes epics, authors stories, and
reviews slices for domain fit. When the work spans **multiple epics, one `product-manager` subagent
authors each epic's stories in parallel** — one PM per epic.

The briefs come from the Runtime input section at the end of this workflow.

---

## Config (defaults — override only if the repo clearly needs it)

- `REPO` = current repo (`gh repo view --json nameWithOwner -q .nameWithOwner`).
- `EPIC_LABEL` = `epic`; `STORY_LABEL` = `user-story` (create if missing).
- `AREA_LABELS` = derived from the repo's existing `gh label list` (prefer the `area:*` family). Create
  a new area label only when a story clearly needs one that doesn't exist — and say which you created.
- Issue-body trailer: `{{session-key}}: ...` on every epic and story (read from the harness/session
  context; do not invent).
- `DRY_RUN` = on if the caller says "dry run" / "preview": print the full plan and create NOTHING.
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
   If `DRY_RUN`, print the panel and reasons here — they appear again in the Stage 3 dry-run output.

   **Panel selection.** Always include **`product-manager`** — backlog authorship is still the core job.

**Explicit overrides win.** If the caller names roles in the brief or runtime guidance (e.g. "also
involve architect and UX designer"), use those roles — resolve each via the crew roster / fuzzy-match
rules below; do not drop a named role because of the cap.

Otherwise, derive additions from the **brief + repo context** using the same domain signals
`/ship-issue` and `/pr-review` use (but read from the brief, not a diff). Each signal is independent;
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

Resolve every role name against `{{agents-glob}}`; on miss, fall back to `{{general-purpose}}` with
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

Other non-PM panel members whose expertise is story-level (UX, art, data, perf, etc.) defer to
Stage 2 — they do not scope epics.

## Stage 2 — Author each epic's stories  (agents: `product-manager` × N, then panel review)

**Author (parallel).** Spawn **one `product-manager` subagent per epic, in a single message** so
they run concurrently. Give each: its epic (title/why/scope), the full repo context, the OTHER
epics' titles (for cross-epic dependencies), the existing-issues list (to dedupe), and `<panel>` so
it knows which specialists will review. Each returns its epic's **user stories** as structured data
(no `gh` calls) — per story:
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

## Stage 3 — Create the issues  (orchestrator, deterministic)

If `DRY_RUN`: print **`<panel>` and `<panel-reasons>`**, then the epic→story tree (titles, criteria,
labels, order), then the **attachment plan** — which stories would become sub-issues of which epic —
and STOP. Create nothing and attach nothing.

Otherwise, in this order (numbers must exist before they're referenced):
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

- **Verify the sub-issue graph** — re-fetch each epic (`gh issue view <epic> --json
  subIssues,subIssuesSummary`) and confirm its children (`subIssues.nodes[].number`) are exactly the
  stories you created for it: the count matches the story count and every captured story number
  appears. A missing child means step 4 didn't land — retry it **once** for that story, then
  re-verify. If it is still missing, report that host/graph gap — do not loop. Report the parent's
  completion summary (children total and done).
- Verify every story carries `Part of #<epic>` matching its epic, and every epic's checklist lists all
  its stories (re-fetch and grep; don't assume).
- Report **`<panel>` and `<panel-reasons>`** first, then a tree: each **epic** (title + link) → its
  **stories** (title + link), with counts, **sub-issue attachment counts per epic**, any labels
  created, a recommended **build order**, and anything skipped as an existing duplicate. Say plainly
  when a host rejected the sub-issue attachments.

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
- Respect `DRY_RUN` — when set, create nothing and attach nothing; still print the selected panel and
  the attachment plan.
- Every epic and story must be individually valuable and traceable (`Part of #`), so the backlog is
  ready to hand to `/ship-epic <epic#>` or `/ship-issue` one story at a time.
- If a role doesn't resolve to an `{{agents-glob}}`, fall back to `{{general-purpose}}` with the
  role brief inlined, and note the fallback.

## Runtime input

`$ARGUMENTS` contains one or more briefs. A brief may be inline text, a path to a file or directory,
or several briefs separated by `---` or numbering. Optional role hints ("also involve …") are part
of the brief text — they override auto panel selection. If empty, ask for the brief before doing anything.
