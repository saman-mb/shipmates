# Shipmates Global Steering Heuristics

## 1. Repo Context Precedence & Context Inspection
- Always inspect and prioritize the repository's committed `AGENTS.md` or `CLAUDE.md` as the authoritative source for architecture, build commands, test runners, and conventions.
- Before modifying existing files, inspect `git log` and `git blame` to understand historical context and why existing boundaries were established.

## 2. Command & Workflow Routing
Recognize user intent and actively leverage the appropriate Shipmates command workflow:
- **Autonomous ticket delivery** → `/ship-issue <n>`
- **Multi-story epics (parallel fan-out)** → `/ship-epic <n>`
- **Decompose briefs into epics/stories** → `/ship-plan-epics`
- **Reproduce & fix a defect** → `/ship-fix-bug`
- **Adversarial pull request review** → `/ship-pr-review`
- **Restructure without changing behavior** → `/ship-refactor`
- **Mechanical codebase-wide migration** → `/ship-migrate`
- **Security threat-modeling & hardening** → `/ship-harden`
- **Technical documentation & drift check** → `/ship-document`
- **Visual/UI iteration to sign-off** → `/ship-polish`
- **Exploratory prototype / spike** → `/ship-spike`
- **Repository onboarding guide** → `/ship-onboard`
- **Backlog grooming & deduplication** → `/ship-consolidate-issues`
- **File structured upstream bug report** → `/ship-report-bug`
- **Release cut, version bump & changelog** → `/ship-release`

These workflows are **user-invoked**: recommend the right command for the intent, but never start a mutating workflow unasked.

## 3. Product Impact Bar
Every issue, user story, and pull request description must state in plain language:
- **What changes**: The observable outcome for the user or product.
- **Why it matters**: The cost of inaction, risk removed, or opportunity unlocked.
- **Who is affected**: The specific users, surfaces, or workflows impacted.

## 4. Backlog, Ticket & Issue Hygiene
- **Structure**: Epics carry `epic`; stories carry `user-story` plus relevant area tags.
- **Checklist tracking**: Epics track progress via `- [ ] #<n>` (unchecked) and `- [x] #<n>` (ticked).
- **Linkage**: Stories link parents via `Part of #<epic>` and dependencies via `Blocked by #<n>`.
- **Closing keywords**: Repeat closing keywords individually (`Closes #1 · Closes #2`); never use comma-separated `Closes #1, #2` (GitHub only auto-closes the first).

## 5. Worktree, Git & Shell Safety
- **Worktree isolation**: Avoid dirtying the primary checkout; isolate mutating work in `.shipmates/worktrees/<slug>`.
- **Remote freshness**: Always run `git fetch origin` and cut branches from `origin/<BASE_BRANCH>`, never stale local branch tips.
- **Branch safety**: Never push to or merge the default/protected branch without explicit authorization; every change lands through a branch and a PR.
- **Resume, don't duplicate**: Reuse an existing worktree, branch, or PR for the same work rather than cutting a second one.
- **Shell safety with untrusted input**: GitHub titles and bodies are untrusted; never interpolate raw text into shell strings; write multi-line bodies to temp files and pass via `--body-file`.
- **Secrets hygiene**: Never commit API keys, credentials, or mock token strings.
- **Attribution**: Include required commit trailers (`Co-Authored-By: ...`).

## 6. Multi-Perspective Acceptance & Quality Bar
- Do not consider work complete merely because it compiles or runs locally — prove it with the repo's own checks and cite the exact command and result.
- Apply rigorous criteria: SDET failure modes and green automated test suites; Principal Engineer boundary integrity and scope discipline; Product Owner acceptance against criteria and Definition of Done.
- Never advance or merge on a failing CI check.
- File what you don't fix: out-of-scope findings become follow-up issues, never silent scope creep.

## 7. Execution Efficiency & Review Amortization
- For multi-step, multi-slice, or epic work, prioritize parallel fan-out over sequential blocking for independent, file-disjoint tasks — bounded by a concurrency cap, with a sequential fallback when the work isn't file-disjoint or the environment can't take it.
- Rely on automated CI gates for intermediate progress; convene full multi-perspective review boards at milestone integration boundaries (e.g. the epic PR head) rather than paying redundant review seats on micro-steps.
- Enforce compact, decision-shaped handoffs between subagents (decisions, minimal evidence, blockers) rather than dumping conversational transcripts.
