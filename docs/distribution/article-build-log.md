---
title: "I gave Claude Code a crew of 12 specialist subagents that ships a GitHub issue to a reviewed PR autonomously"
published: false
description: "How Shipmates orchestrates 12 specialist subagents and 13 command workflows with isolated git worktrees and adversarial review boards to drive issues from open to CI-green PRs."
tags: "ai, claudecode, programming, devops"
canonical_url: "https://saman-mb.github.io/shipmates/"
cover_image: "https://saman-mb.github.io/shipmates/assets/social-preview.png"
---

# I gave Claude Code a crew of 12 specialist subagents that ships a GitHub issue to a reviewed PR autonomously

Single AI prompts are great for generating a function or explaining a bug. But real engineering rarely happens in single prompts: a real ticket requires an architectural sanity check, an isolated branch or worktree, test suite validation, continuous integration gating, and an adversarial peer review before anything gets merged.

That is why we built [**Shipmates**](https://saman-mb.github.io/shipmates/) — an open-source (MIT) crew of **12 domain-neutral specialist subagents** and **13 command workflows** designed to take whole GitHub tickets from open to a reviewed, CI-green pull request on their own.

---

## The Core Metaphor: The Captain and the Crew

Instead of treating your AI coding harness as a single general-purpose prompt loop, Shipmates organizes the process into specialized roles:

```
Captain (/ship-issue 42)
  │
  ├── 1. Intake & Complexity Tiering (Simple / Medium / High)
  ├── 2. Architect & Planner (Decomposition & Design Specs)
  ├── 3. Builders (Parallel execution in an isolated git worktree)
  ├── 4. SDET (Test suite authoring & regression verification)
  ├── 5. CI Gate (Polls GitHub Actions until green at exact SHA)
  ├── 6. Acceptance Review Board (Security, SRE, Performance, Product)
  └── 7. Delivery (PR opened, summary posted, follow-ups logged)
```

The crew consists of 12 specialist agents:
- **`architect`**: System boundaries, invariants, dependency layering, and data migrations.
- **`senior-engineer`**: High-craft implementation and minimal diffs.
- **`sdet`**: Flaky-test detection, regression fixtures, edge cases, and test assertions.
- **`security-engineer`**: Authn/authz, input sanitization, least privilege, and secret exposure audits.
- **`site-reliability-engineer`**: Rollback safety, health probes, graceful degradation, and rate limits.
- **`performance-engineer`**: Algorithmic complexity, memory profiling, query plans, and LCP.
- **`devops-engineer`**: CI/CD pipelines, container workflows, and reproducible environments.
- **`product-manager`**: Scope containment, acceptance criteria verification, and user intent.
- **`ux-ui-designer`**: Accessibility (WCAG), responsive states, visual hierarchy, and component consistency.
- **`art-director`**: Brand aesthetics, palette harmony, typography, and visual assets.
- **`technical-writer`**: Documentation fidelity, API references, changelogs, and clear prose.
- **`data-scientist`**: Statistical metrics, model evaluation, and data pipeline invariants.

---

## The Flagship Workflow: `/ship-issue`

The flagship workflow of Shipmates is `/ship-issue`. When you issue `/ship-issue <number>` in Claude Code (or any supported harness), the orchestrator executes a multi-stage lifecycle:

### 1. Complexity Tiering
Not every change requires a full board of 6 specialists. Stage 0 dynamically classifies incoming tasks into three execution tiers:
- **Simple**: Minor documentation edits, typos, or single-line config fixes. Completed directly by the main agent with zero subagent overhead.
- **Medium**: Single-feature or bug fixes affecting a few files. Spawns a lightweight `senior-engineer` and `sdet` pair, bypassing the full specialist board.
- **High**: Architectural changes, auth/security logic, or multi-module refactors. Executes the full parallel specialist loop and adversarial acceptance board.

### 2. Worktree Isolation
Mutating code directly in your active working copy risks dirty git state, uncommitted work clobbering, and broken dev servers. Shipmates builds every PR in an isolated git worktree (`.shipmates-worktrees/<branch>`), keeping your working copy untouched while the subagents work.

### 3. Hard CI Gate at the Tagged SHA
Shipmates never relies on "it compiled on my machine". The orchestrator commits the change in the worktree, pushes the branch, and actively polls your GitHub Actions CI pipeline. If CI fails, the SDET and senior engineer inspect the failure logs, push fixes, and loop until CI turns green.

### 4. Adversarial Review Board
Once CI passes, Shipmates convenes an adversarial review board consisting of the relevant specialists (`security-engineer`, `site-reliability-engineer`, `performance-engineer`, etc.). Each specialist reviews the diff through their specific lens, outputting clear `APPROVE` or `BLOCK` verdicts with concrete line numbers and remediation steps.

---

## Domain Neutrality: The One Hard Rule

A key design principle of Shipmates is **domain neutrality**. No subagent persona hardcodes a framework, programming language, or cloud vendor. 

Instead, every specialist enforces whatever standard is documented in **your** repository's `README.md` and `AGENTS.md` (or `CLAUDE.md`). The exact same crew works seamlessly on:
- A Rust CLI application
- A Next.js / TypeScript web application
- A Python data engineering pipeline
- A Go microservice backend

---

## Multi-Harness Portability

While Shipmates is runtime-verified on **Claude Code** today, the CLI compiles native adapters for six additional harnesses:
- **Claude Code**: `.claude/skills/` and `.claude/agents/`
- **OpenCode**: `.opencode/commands/`, `.opencode/agents/`, and native TypeScript `.opencode/tools/`
- **Antigravity CLI**: `.agents/skills/` and `.agents/agents/`
- **Codex CLI**: `.agents/skills/` and standalone TOML `.codex/agents/`
- **Cursor**: `.agents/skills/`
- **GitHub Copilot**: `.agents/skills/` and `.github/agents/`
- **Windsurf**: `.windsurf/skills/`

---

## Built-in Developer Toolbox

Shipmates also includes an opt-in suite of 10 self-contained developer tools that subagents can reach for automatically:
- `scrub`: Redacts API keys, credentials, and tokens from logs before committing.
- `domaincheck`: Checks domain availability via RDAP (registry-authoritative, not DNS guesswork).
- `badge`: Generates SVG status badges for READMEs.
- `diagram`: Generates clean architectural diagrams and flowcharts.
- `fixtures`: Creates deterministic mock data.
- `pixelart`: Generates pixel art graphics.
- `social-card`: Produces 1200x630 Open Graph preview banners.
- `sparkline`: Generates lightweight SVG trendlines.
- `svgflow`: Renders SVG workflow state diagrams.
- `termgif`: Converts terminal sessions into optimized demo GIFs.

---

## Getting Started

Shipmates is distributed as a single standalone Rust binary:

```bash
# macOS / Linux (Homebrew)
brew install saman-mb/tap/shipmates

# Cargo
cargo install shipmates

# Binary Installer
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/saman-mb/shipmates/shipmates-releases/latest/download/shipmates-installer.sh | sh
```

To install the crew into your repository:

```bash
cd your-project
shipmates install --harness claude-code
```

Then open Claude Code and run:
```bash
/ship-issue <your-issue-number>
```

---

## Links & Community

- **Website & Documentation**: [https://saman-mb.github.io/shipmates/](https://saman-mb.github.io/shipmates/)
- **GitHub Repository**: [https://github.com/saman-mb/shipmates](https://github.com/saman-mb/shipmates)
- **License**: MIT
