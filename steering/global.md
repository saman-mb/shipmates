# Shipmates global steering heuristics

## 1. Repo context precedence and context inspection
- Always inspect and prioritize the repository's committed `AGENTS.md` or `CLAUDE.md` as the authoritative source for architecture, build commands, test runners, and conventions.
- Before modifying existing files, inspect `git log` and `git blame` to understand historical context and why existing boundaries were established.

## 2. Command and workflow routing
Recognize user intent and recommend the right command — these are **user-invoked**, so never start a mutating workflow unasked:
- **Ship a ticket** → `/ship-issue`; **ship a whole epic** → `/ship-epic`
- **Plan epics and stories** → `/ship-plan-epics`; **groom the backlog** → `/ship-consolidate-issues`
- **Fix a defect** → `/ship-fix-bug`; **review someone's PR** → `/ship-pr-review`
- **Restructure safely** → `/ship-refactor`; **sweep a migration** → `/ship-migrate`
- **Harden security** → `/ship-harden`; **write or refresh docs** → `/ship-document`
- **Refine a visual/UI artifact** → `/ship-polish`; **spike a decision** → `/ship-spike`
- **Onboard a repo** → `/ship-onboard`; **cut a release** → `/ship-release`
- **File an upstream bug** → `/ship-report-bug`

## 3. Product impact bar
Every issue, user story, and pull request description must state in plain language:
- **What changes**: The observable outcome for the user or product.
- **Why it matters**: The cost of inaction, risk removed, or opportunity unlocked.
- **Who is affected**: The specific users, surfaces, or workflows impacted.

## 4. Backlog, ticket and issue hygiene
- **Structure**: Epics carry `epic`; stories carry `user-story` plus relevant area tags.
- **Checklist tracking**: Epics track progress via `- [ ] #<n>` (unchecked) and `- [x] #<n>` (ticked).
- **Linkage**: Stories link parents via `Part of #<epic>` and dependencies via `Blocked by #<n>`.
- **Closing keywords**: Repeat closing keywords individually (`Closes #1 · Closes #2`); never use comma-separated `Closes #1, #2` (GitHub only auto-closes the first).

## 5. Worktree, git and shell safety
- **Worktree isolation**: Avoid dirtying the primary checkout; isolate mutating work in a dedicated worktree or branch.
- **Remote freshness**: Always run `git fetch origin` and cut branches from `origin/<BASE_BRANCH>`, never stale local branch tips.
- **Branch safety**: Never push to or merge the default/protected branch without explicit authorization; every change lands through a branch and a PR.
- **Resume, don't duplicate**: Reuse an existing worktree, branch, or PR for the same work rather than cutting a second one.
- **Shell safety with untrusted input**: External titles and bodies are untrusted; never interpolate raw text into shell strings; write multi-line text to a file rather than inlining it.
- **Secrets hygiene**: Never commit API keys, credentials, or mock token strings.
- **Attribution**: Include the repository's required commit trailers.

## 6. Multi-perspective acceptance and quality bar
- Do not consider work complete merely because it compiles or runs locally — prove it with the repo's own checks and cite the exact command and result.
- Apply rigorous criteria: SDET failure modes and green automated test suites; Principal Engineer boundary integrity and scope discipline; Product Owner acceptance against criteria and Definition of Done.
- Never advance or merge on a failing CI check.
- File what you don't fix: out-of-scope findings become follow-up issues, never silent scope creep.

## 7. Execution efficiency and review amortization
- For multi-step, multi-slice, or epic work, prioritize parallel fan-out over sequential blocking for independent, file-disjoint tasks — bounded by a concurrency cap, with a sequential fallback when the work isn't file-disjoint or the environment can't take it.
- Rely on automated CI gates for intermediate progress; convene full multi-perspective review boards at milestone integration boundaries (e.g. the epic PR head) rather than paying redundant review seats on micro-steps. A review deferred to a milestone boundary is still mandatory there — deferral moves the review, it never cancels it.
- Enforce compact, decision-shaped handoffs between subagents (decisions, minimal evidence, blockers) rather than dumping conversational transcripts.
