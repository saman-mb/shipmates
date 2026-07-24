<p align="center">
  <img src="assets/logo.png" width="200" alt="Shipmates — a pixel-art sailboat sailing into the sunset" />
</p>

# 🚢 Shipmates

[![License: MIT](https://img.shields.io/github/license/saman-mb/shipmates?color=blue)](LICENSE)
[![Made for Claude Code](https://img.shields.io/badge/made%20for-Claude%20Code-D97757?logo=anthropic&logoColor=white)](https://claude.com/claude-code)
[![Crew aboard](https://img.shields.io/badge/crew-6%20specialists-orange)](#-meet-the-crew)
[![PRs welcome](https://img.shields.io/badge/PRs-welcome-brightgreen.svg)](CONTRIBUTING.md)
[![Stars](https://img.shields.io/github/stars/saman-mb/shipmates?style=flat&logo=github)](https://github.com/saman-mb/shipmates/stargazers)
[![Last commit](https://img.shields.io/github/last-commit/saman-mb/shipmates)](https://github.com/saman-mb/shipmates/commits/main)
[![Issues](https://img.shields.io/github/issues/saman-mb/shipmates)](https://github.com/saman-mb/shipmates/issues)

### Stop being your AI's for-loop. Give it a crew. ⚓

You know the drill: prompt, read the reply, prompt again, sigh, prompt again. **You** are the
control loop — the planner, the reviewer, the nagger. Shipmates hands that job to a *crew* of
specialist sub-agents and a workflow that actually finishes things.

One command — **`/ship-issue`** — takes a GitHub issue from *"open"* to a *reviewed, CI-green pull
request*, on its own: it plans the work, builds it in an isolated worktree, waits for CI to go
green, convenes an adversarial review board, loops on the fixes, and hands you a PR to merge.
You stay the captain. The shipmates do the twenty steps in between. 🫡

---

## 🧭 Meet the crew

Six **domain-neutral** specialists. They'll work on *anything* — a game engine, a web app, a CLI —
because the standard they hold your work to comes from **your** repo's `README` / `CLAUDE.md`, not
from anything hardcoded into the role.

| Shipmate | Rank & duty |
|---|---|
| 🏛️ `architect` | Structure & schema — coupling, boundaries, migration safety, "does this actually fit?" |
| 🔧 `senior-engineer` | Builds to spec, fixes red CI, clears review defects |
| 📋 `product-manager` | Accepts or rejects against the acceptance criteria **and** your quality bar |
| 🧪 `sdet` | Runs the real tests/build and reports pass/fail with a proper defect list |
| 🎛️ `ux-ui-designer` | Specs & reviews on-screen UI — tokens, responsive layout, focus, a11y |
| 🎨 `artist` | Directs & reviews *rendered* visuals — judges the picture, not the code that drew it |

## 📜 The orders (commands)

| Command | What it does |
|---|---|
| `/ship-issue <n>` | Drives GitHub issue `#n` from open → reviewed, CI-green PR (→ merged, opt-in), with the whole crew |
| `/plan-epics <brief>` | Turns a brief (or several) into GitHub epics + linked, labelled user stories — a `product-manager` sub-agent authors each epic's stories in parallel |
| `/polish <target>` | Iterates a visual/UI/output artifact to a specialist's sign-off — render → critique → fix, looping until the `artist` / `ux-ui-designer` / `product-manager` is genuinely happy |

*More crew and more orders are on the way.* ⛵

---

## ⚓ Come aboard (install)

One line, no clone required:

```bash
curl -fsSL https://raw.githubusercontent.com/saman-mb/shipmates/main/install.sh | bash
```

That brings the crew aboard for **every** project (`~/.claude`). Scope it to a single repo instead
(checked in, shared with your team):

```bash
curl -fsSL https://raw.githubusercontent.com/saman-mb/shipmates/main/install.sh | bash -s -- --project /path/to/your/repo
```

Prefer to read the script first? Clone the repo and run `./install.sh` (same flags). Either way it
drops `commands/*.md` and `agents/*.md` into your `.claude/`, **backing up** any existing file of the
same name to `<file>.bak-<timestamp>` — your edits are safe. Re-run to update; add `--uninstall` to
remove what it installed.

> 🔁 First time a `commands/` or `agents/` dir got created? Restart Claude Code so it spots them.

## 🚀 Weigh anchor (use it)

```
/ship-issue 42
```

Then go get a coffee ☕. It plans, spins up a worktree, builds, waits for CI to go green, convenes
the board, loops on fixes, and hands you a reviewed PR. By default it **stops at the PR** for you to
merge — set `MERGE_MODE=auto` if you want it fully hands-off in a repo where that's fair game.

---

## 🛠️ How the voyage works

`/ship-issue` isn't a clever prompt — it's a **state machine with gates**:

1. **Plan** 🗺️ — a planner reads the issue + your docs → build plan, acceptance criteria, validation
   plan, and flags for which specialists this story needs.
2. **Design specs** ✏️ *(only if needed)* — for UI / visual / architecture-heavy stories, the right
   specialist writes a spec the builders must build to.
3. **Isolate** 📦 — all work in a throwaway `git worktree`; your base branch never breaks.
4. **Build** 🔨 — parallel `senior-engineer` builders with non-overlapping file ownership.
5. **Self-check → CI gate** 🚦 — the SDET runs the tests; then **CI must go green** on the pushed PR
   before anything moves on. Red? It reads the logs and fixes — bounded to a few rounds.
6. **Acceptance board** ⚖️ — `product-manager` + `sdet` (+ gated `ux-ui-designer` / `artist` /
   `architect`) review the *pushed PR head*, independently and adversarially.
7. **Remediate** 🔁 — any rejection loops back to a fixer, then re-reviews. Bounded, then escalates.
8. **Deliver** 🏁 — files the non-blocking nits as follow-ups, and opens (or, opt-in, merges) the PR.

The tricks that make the loop hold together:

- 🎯 **An explicit state machine, not a wish.** Stages converge; "go fix it" drifts.
- 📦 **An isolated sandbox.** Autonomy is only safe when the blast radius is zero.
- 🚦 **Objective gates over vibes.** Green CI beats "looks done to me."
- 👥 **Reviewers can't grade their own homework.** A *fresh* agent reviews the PR — never the builder.
- ⏱️ **Bounded loops.** Retry N, then tap the human. A loop with no cap just spins.
- 🎟️ **Capture, don't block.** Nits become tickets, not roadblocks.

## 🧭 Examples — putting the crew to work

**Ship a single ticket, hands-off to a reviewed PR:**
```
/ship-issue 142
```
The planner reads issue #142 and your README; a `senior-engineer` builds it in a worktree; CI has to
go green; then a `product-manager` and `sdet` review the pushed PR — a `ux-ui-designer` or `artist`
joins automatically if the story is UI or art. Fixes loop until they pass, and you get a reviewed PR
to merge.

**Ship it fully autonomously (merge included), where that's acceptable:**
```
MERGE_MODE=auto /ship-issue 142      # or just say "auto-merge" in the prompt
```

**Turn a one-line brief into a tracked backlog:**
```
/plan-epics "User accounts: signup, login, password reset, and a profile page"
```
A `product-manager` scopes it into an epic, drafts INVEST user stories with acceptance criteria, and
files them as linked, labelled GitHub issues — ready to hand to `/ship-issue` one at a time.

**Break a big vision into several epics at once (fan-out):**
```
/plan-epics ./docs/product-brief.md
```
When the brief spans multiple epics, one `product-manager` sub-agent per epic drafts its stories in
parallel, then everything is created and cross-linked.

**Preview a backlog without creating anything:**
```
/plan-epics "checkout + payments + order history"  — dry run
```

**Polish a UI screen until it's actually right:**
```
/polish the settings screen
```
The `ux-ui-designer` reviews the *rendered* screen (not the code), lists concrete fixes, a
`senior-engineer` applies them, it re-renders, and the loop repeats until the designer signs off — or
hands you the outstanding notes after a few rounds.

**Polish rendered art the same way:**
```
/polish the title-screen background — reviewer: artist
```
Same loop, but the `artist` judges the actual render — palette, composition, contrast — round after
round until it meets the bar.

**Chain them — scope the work, ship a story, polish its UI:**
```
/plan-epics "settings redesign"     # → creates the epic + stories
/ship-issue 148                     # → builds & reviews one story to a PR
/polish the settings screen         # → iterates the visuals to sign-off
```

## 🗂️ Scopes & precedence

Claude Code loads agents/commands from `~/.claude/` (global, every project) and `<repo>/.claude/`
(that project only). A project definition **wins** over a global one of the same name — so any repo
can override or specialise a crew member without touching the shared copy.

## 🎒 What you'll need

- [Claude Code](https://claude.com/claude-code)
- `git` + an authenticated [`gh`](https://cli.github.com/) CLI, for the GitHub flow
- A repo with CI (strongly recommended — the green-CI gate is what makes autonomy trustworthy)

## 💡 Why it's built this way

- **Agents are generic; your project supplies the bar.** No role name-drops a stack or product — it
  enforces whatever *your* README/CLAUDE.md says. That's what makes the crew reusable everywhere.
- **A loop is only as smart as its ground-truth signal.** Tests and CI are solid gates; taste isn't —
  so the visual specialists flat-out flag *"needs a human visual pass"* when they can't render.

## 🌊 On the horizon

More crew (`security-reviewer`, `technical-writer`, `devops-engineer`…) and more orders beyond
`/ship-issue`. Ideas and PRs very welcome.

## 🤝 Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). The one hard rule: **keep agent roles domain-neutral** so
they sail on anyone's project.

## 📄 License

[MIT](LICENSE) — take it, fork it, crew up. 🚢
