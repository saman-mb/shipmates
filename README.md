# 🚢 Shipmates

**A crew of specialist AI sub-agents + autonomous workflows for [Claude Code](https://claude.com/claude-code).**

Instead of prompting an agent over and over and grading its work yourself, you hand it a *crew* —
an architect, a senior engineer, a product manager, an SDET, a designer, an artist — and a workflow
that puts them to work: plan → build in an isolated worktree → gate on green CI → run an adversarial
acceptance board → loop on fixes → open a reviewed PR. You stay the captain; the shipmates do the
twenty steps in between.

> The flagship order is **`/ship-issue`** — it takes a GitHub issue from *open* to a *reviewed,
> CI-green pull request*, autonomously. More orders and crew are on the way.

---

## The crew (agents)

Six **domain-neutral** specialist sub-agents. They work on *any* project — the standard they hold
work to comes from **your** repo's `README` / `CLAUDE.md`, not from anything baked into the role.

| Agent | Role |
|---|---|
| `architect` | Structural & schema review — coupling, boundaries, migration safety, does it fit the codebase |
| `senior-engineer` | Builds features to spec, fixes failing tests/CI, addresses review defects |
| `product-manager` | Accepts/rejects work against the acceptance criteria **and** the stated quality bar |
| `sdet` | Runs the real tests/build/validation and reports pass/fail with a severity-tagged defect list |
| `ux-ui-designer` | Specs and reviews on-screen UI — design tokens, responsive layout, focus, accessibility |
| `artist` | Directs and reviews *rendered* visual output — judges the picture, not the source that made it |

## The orders (commands)

| Command | What it does |
|---|---|
| `/ship-issue <n>` | Drive GitHub issue `#n` from open → reviewed, CI-green PR (→ merged, opt-in) with the full crew |

---

## Install

```bash
git clone https://github.com/saman-mb/shipmates.git
cd shipmates
./install.sh            # installs for all your projects (~/.claude)
```

Or scope it to a single repo (checked in, shared with your team):

```bash
./install.sh --project /path/to/your/repo    # installs into <repo>/.claude
```

The installer copies `commands/*.md` and `agents/*.md` into the target `.claude/` directory. Any
existing file of the same name is **backed up** to `<file>.bak-<timestamp>` first, so your own edits
are never lost. Re-run any time to update; `./install.sh --uninstall` removes what it added.

> If this was the first time a `commands/` or `agents/` directory was created in that config,
> restart Claude Code so it picks them up.

## Use it

```
/ship-issue 42
```

Then watch it plan, spin up a worktree, build, wait for CI to go green, convene the acceptance
board, loop on any fixes, and hand you a reviewed PR. By default it **stops at the PR** for you to
merge; set `MERGE_MODE=auto` in the command if you want fully hands-off delivery in a repo where
that's acceptable.

---

## How `/ship-issue` works

It's not a clever prompt — it's a **state machine with gates**:

1. **Plan** — a planner reads the issue + your README/CLAUDE.md and returns a build plan, acceptance
   criteria, a validation plan, and flags for which specialists this story needs.
2. **Design specs** *(conditional)* — for UI / visual / architecture-significant stories, the
   matching specialist writes a spec the builders must implement against.
3. **Isolate** — all work happens in a throwaway `git worktree`; your base branch never breaks.
4. **Build** — parallel `senior-engineer` builders with non-overlapping file ownership.
5. **Self-check → CI gate** — the SDET runs the tests; then CI must go **green** on the pushed PR
   before anything proceeds. If it's red, it reads the logs and fixes, bounded to a few rounds.
6. **Acceptance board** — `product-manager` + `sdet` (+ gated `ux-ui-designer` / `artist` /
   `architect`) review the *pushed PR head* independently and adversarially.
7. **Remediate** — any rejection loops back to a fixer, then re-reviews. Bounded, then escalates.
8. **Deliver** — file the non-blocking nits as follow-up issues, and open (or, opt-in, merge) the PR.

The principles that make the loop hold together: an explicit **state machine** (not a wish),
an **isolated sandbox** (safe autonomy), **objective gates** (green CI beats "looks done"),
**independent reviewers** (a *fresh* agent reviews the PR, not the one that wrote it),
**bounded loops** (retry N, then escalate), and **capture-don't-block** (nits become tickets).

## Scopes & precedence

Claude Code resolves agents/commands from `~/.claude/` (global, all projects) and
`<repo>/.claude/` (project-scoped). A project definition **wins** over a global one of the same
name — so a repo can override or specialise a role without touching the shared copy.

## Requirements

- [Claude Code](https://claude.com/claude-code)
- `git` and the [`gh`](https://cli.github.com/) CLI (authenticated), for the GitHub workflow
- A repo with CI is strongly recommended — the CI gate is what makes autonomous delivery trustworthy.

## Design principles

- **Agents are generic; the project supplies the bar.** No role mentions any specific stack or
  product — it enforces whatever your README/CLAUDE.md states. This is what makes the pool reusable.
- **Reviewers can't grade their own homework.** Acceptance is done by freshly-spawned agents against
  the pushed PR, never by the builder.
- **A loop is only as good as its ground-truth signal.** Tests and CI are solid gates; taste isn't —
  the visual specialists explicitly flag "needs a human visual pass" when they can't render.

## Roadmap

More crew and more orders are coming — think `security-reviewer`, `technical-writer`,
`devops-engineer`, and workflow commands beyond `/ship-issue`. Suggestions and PRs welcome.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). The one hard rule: **keep agent roles domain-neutral** so
they work on anyone's project.

## License

[MIT](LICENSE).
