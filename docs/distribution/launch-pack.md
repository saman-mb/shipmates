# Shipmates Launch Distribution Pack

Pre-formatted, copy-paste ready promotional copy for launch channels.

---

## 1. Hacker News (Show HN)

### Title
Show HN: Shipmates – A crew of 12 specialist AI subagents for Claude Code

### URL
https://github.com/saman-mb/shipmates

### First Comment (Maker Post)
Hey HN! I built Shipmates (MIT): a crew of 12 domain-neutral specialist AI subagents (architect, senior-engineer, sdet, security-engineer, sre, etc.) and 13 command workflows that drive GitHub issues from open to a reviewed, CI-green pull request autonomously.

Why build this?
Single-turn prompts work well for single functions, but real engineering tickets need a disciplined lifecycle: planning, isolated worktree builds, test coverage, CI verification, and adversarial review.

How it works:
1. `/ship-issue <number>` classifies the ticket into a complexity tier (Simple, Medium, High).
2. Sets up an isolated git worktree so your active working copy is never dirtied or clobbered.
3. Builders implement the changes; SDET builds regression tests.
4. Active CI Gate: Polls your GitHub Actions CI and loops on fixes until the commit is green.
5. Review Board: Spawns specialist subagents to evaluate security, performance, architecture, and SRE rollback safety.
6. Opens a clean PR with a concise ledger of decisions.

Multi-harness support:
Runtime-verified on Claude Code today, with native build adapters for OpenCode, Antigravity CLI, Codex CLI, Cursor, GitHub Copilot, and Windsurf.

Written in Rust as a single binary:
`brew install saman-mb/tap/shipmates` or `cargo install shipmates`

Website: https://saman-mb.github.io/shipmates/
GitHub: https://github.com/saman-mb/shipmates

Would love your thoughts, feedback, and questions!

---

## 2. Reddit (r/ClaudeAI, r/commandline, r/ChatGPTCoding)

### Title
I built Shipmates: Give Claude Code a crew of 12 specialist subagents to ship GitHub tickets autonomously

### Body
Hey everyone,

I wanted to share **Shipmates** (open source / MIT), an agentic orchestration layer designed to give Claude Code a crew of 12 domain-neutral specialist subagents and 13 command workflows.

Its flagship command `/ship-issue <issue-number>` does the entire ticket lifecycle:
- Evaluates complexity (Simple tasks skip overhead; High tasks get the full specialist board)
- Builds inside an isolated git worktree
- Writes regression tests
- Waits for your GitHub Actions CI to go green
- Convenes an adversarial review board (Security, SRE, Performance)
- Delivers a reviewed PR ready to merge

Everything is domain-neutral — agents read your repo's README/AGENTS.md (or CLAUDE.md) rather than assuming a specific framework.

Install:
`brew install saman-mb/tap/shipmates`
`shipmates install --harness claude-code`

Repo: https://github.com/saman-mb/shipmates
Site: https://saman-mb.github.io/shipmates/

Let me know what you think or if you'd like to see more command workflows!

---

## 3. X / Twitter Launch Thread

### Tweet 1 (Hook + Tagline)
Announcing Shipmates 🚢 — Give your AI a crew.

An open-source (MIT) crew of 12 specialist subagents and 13 command workflows that drive a GitHub issue from open to a reviewed, CI-green pull request autonomously.

Built for Claude Code today.

https://github.com/saman-mb/shipmates

🧵👇

### Tweet 2 (The Problem)
Single prompts don't build software.

Real tickets require an architectural plan, isolated git branches, flaky test detection, CI polling, and adversarial peer review before merging.

Shipmates structures Claude into specialized roles: Architect, SDET, Security, SRE, Product.

### Tweet 3 (The /ship-issue Flow)
Run `/ship-issue 42`:
1. Complexity evaluation (Simple / Medium / High)
2. Builds in an isolated git worktree
3. SDET generates regression tests
4. CI Gate: polls GitHub Actions until green
5. Review Board: Security & SRE audit the diff
6. Delivers a PR ready to merge

### Tweet 4 (Multi-Harness)
Shipmates is built in Rust with native compilation adapters for 7 harnesses:
• Claude Code
• OpenCode
• Antigravity CLI
• Codex CLI
• Cursor
• GitHub Copilot
• Windsurf

### Tweet 5 (Get Started)
Try it in under 60 seconds:

```bash
brew install saman-mb/tap/shipmates
shipmates install --harness claude-code
```

Star the repo & explore the interactive docs:
https://saman-mb.github.io/shipmates/

---

## 4. Product Hunt

### Tagline
Give your AI coding harness an autonomous crew of specialist subagents

### Short Pitch
Shipmates provides 12 domain-neutral subagents and 13 command workflows for Claude Code and 6 other harnesses. Its flagship, `/ship-issue`, drives a GitHub issue to a reviewed, CI-green pull request autonomously using isolated git worktrees and automated review boards.

### Maker Comment
Hey Product Hunt! 👋

Coding assistants are great at generating snippets, but managing end-to-end features still requires lots of context-switching: branching, running tests, fixing CI, and reviewing security edge cases.

We built Shipmates to automate that whole loop. With one command (`/ship-issue <id>`), a crew of specialist subagents breaks down the task, writes code and tests in an isolated worktree, verifies CI, convenes a security/SRE review board, and delivers a clean PR.

It's 100% open source (MIT) and available via Homebrew, Cargo, or binary installer.

Check it out and let us know what workflows you'd like to see next!
