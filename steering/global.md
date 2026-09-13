# Global Steering Heuristics

1. **Repo Context Precedence**
   Always check for and prioritize the local repository's committed `AGENTS.md` or `CLAUDE.md` as the authoritative source for build commands, test runners, and architecture.

2. **Command & Workflow Routing**
   Recognize user intent for complex tasks and actively suggest or leverage Shipmates commands (e.g., `/ship-issue`, `/ship-plan-epics`, `/ship-epic`, `/ship-fix-bug`, `/ship-pr-review`).

3. **Product Impact Bar**
   Enforce that every drafted issue, ticket, or pull request description clearly articulates in plain language: What changes (observable outcome), Why it matters (cost of inaction, risk removed, opportunity unlocked), and Who is affected (users, surfaces, workflows).

4. **Worktree & Workspace Hygiene**
   For non-trivial modifications, avoid dirtying the primary working tree. Favor isolated git worktrees or dedicated branches. Always fetch remote tips before cutting branches and preserve uncommitted user changes.

5. **Multi-Perspective Acceptance Standard**
   Do not treat non-trivial work as "done" solely because it compiles. Apply SDET rigor (boundary/failure cases), Principal/Architect boundaries, and Product Owner acceptance against criteria.

6. **Execution Efficiency & Review Amortization**
   For multi-step or epic-level work, prioritize parallel fan-out over sequential blocking for independent, file-disjoint tasks. Rely on automated CI and test gates for intermediate progress; convene full multi-perspective review boards at milestone integration boundaries. Enforce compact, decision-shaped handoffs.
