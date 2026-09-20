# Contributing to Shipmates

Thanks for wanting to add to the crew! New agents and commands are welcome.

## The one hard rule: agents stay domain-neutral

An agent role in `crew/` must **not** mention any specific language, framework, product, or
project. It describes *how the role thinks and works*; the standard it enforces comes from the
target repo's `README` / `AGENTS.md` (or `CLAUDE.md`) at run time. If you find yourself writing
"Godot", "React", "our game", or a specific style guide into a role, move that into the repo that
uses it instead.

Good: *"Hold the work to whatever visual bar the project states for itself."*
Not good: *"Match our SC2-style HD dark-chrome UI."*

## Adding an agent

1. Create `crew/<name>.md` (lowercase-and-hyphens name).
2. Frontmatter: `name`, `description` (when the harness should delegate to it), and `capabilities`
   (semantic: `read`, `edit`, `bash`, `web`, `agent` — least privilege — a reviewer usually needs
   `read, bash`, not `edit`). Adapters map capabilities to each harness's real tools.
3. Body = the system prompt: the role's focus, how it reviews/builds, and the verdict format it
   returns (`ACCEPT` / `ACCEPT-WITH-NITS` / `REJECT`, or `PASS` / `FAIL`).
4. If a command should invoke it, wire it into that command's stages by `subagent_type`.

## Adding a skill (workflow)

A workflow ships as a **skill** on disk and is invoked as a **command** in the harness. Both names
are correct; which one belongs in a given sentence is set by the
[naming register](docs/BRAND.md#naming-register). Work the checklist top to bottom:

1. Create `commands/<name>.md` (lowercase-and-hyphens `<name>`). **Check the name against
   harness built-ins first** — run `python3 tools/validate_skill_names.py` and pick a name that
   doesn't collide. A collision silently shadows a first-party feature (e.g. `/review` shadowed
   Claude Code's built-in code review; see #102). Only `name` and `description`
   are required, and they must come first, in that order. After them, in any order: the standard's
   optional `license`, `compatibility`, `metadata`, and the Claude Code extensions `argument-hint`,
   `allowed-tools`, `disable-model-invocation`. Unknown keys are rejected, so a typo fails the gate.
   The seventeen ship `name`, `description`, `argument-hint`, `allowed-tools`, `disable-model-invocation`
   — recommended, not required. **`name` must be exactly the directory name** — that is the
   [Agent Skills](https://agentskills.io) standard's rule, and the skill will not resolve without it.
2. `argument-hint`, `allowed-tools` and `disable-model-invocation` are vendor extensions on top of
   the standard, kept comma-separated because that is what Claude Code parses. Set
   `disable-model-invocation: true` unless the workflow is genuinely safe for the model to start
   unprompted — every shipped command sets it, because they create worktrees, push branches and open
   pull requests. It does not affect typing `/<name>` yourself.
3. Make the first heading after the frontmatter `# /<name> — <tagline>` — the page generator parses
   that line for the command's title, and fails on anything else.
4. Take input from `$ARGUMENTS` only, parsed in prose — a skill has no positional arguments. A `$`
   followed by a digit is rejected **anywhere in the file**, frontmatter and fenced code blocks
   included: substitution is textual over the whole file, so a fence protects nothing. (An earlier
   version of this checklist allowed it inside a fence, on the grounds that it reads as a shell field
   reference there. That was wrong: run `/shipmates-issue 42 focus on retries` and a fenced snippet asking
   for field two gets the literal word `on`.) If you genuinely need a literal, escape it as `\$2` —
   but prefer restructuring so you don't, e.g. `cut -f2` rather than an `awk` field reference.
   `cargo run -- check` enforces this over the whole file, fenced or not.
5. Every catalogue command ships interactive argument intake by default: declare a `## Parameters`
   table (Name / Required / Token / Values / Default / Help) immediately above Runtime input —
   every captain-facing knob is its own row with a Default, never a catch-all; shared picker rules
   live in `docs/COST.md`'s command-preamble (always show every row; enums show default plus other
   choices; then OK or type what to change) — do not copy them into the command.
6. Prefer invoking the shared agents by `subagent_type` over inlining personas.
7. **Read-only, or worktree + PR — in-place only on explicit request.** A workflow that changes a
   repo works on its own branch in its own worktree and proposes the result as a pull request; the
   caller's checkout is left as they left it. Writing straight into the working tree is an opt-in
   (`MODE=edit-in-place`), never a default. `/shipmates-release` is the one shipped exception: the release
   commit has to land on the branch being tagged, so it commits, pushes and tags straight in the
   caller's checkout instead of an unmerged side branch — not because a worktree defeats tagging (it
   doesn't; a worktree shares the object database and the remote, so `git tag` behaves the same
   either way), but because the commit being tagged can't sit on a branch that was never merged. If
   your workflow genuinely can't isolate for a comparable structural reason, say so in its Config and
   state why — don't just skip the default quietly. **Irreversible side effects are a separate axis,
   judged by its own test, not this one:** *irreversible = the caller cannot undo it with one command
   in their own repo.* Opening a PR doesn't clear that bar — the caller closes it with one command —
   so it's not in the class. Merging (`MERGE_MODE=auto`), publishing (`PUBLISH_MODE=auto`),
   posting on a third party's pull request (`MODE=post`), and creating issues or labels on someone
   else's tracker all pass it — none undoes with one command in the caller's own repo — so each stays
   opt-in, off unless the caller sets it, with one known deviation: `/shipmates-plan-epics` files issues and
   labels by default today ([#111](https://github.com/saman-mb/shipmates/issues/111), not yet fixed).
   Be a good guest on other people's repos: a branch is a suggestion, a merge is a decision.
   **Which ref to branch from follows the same split: cut from wherever the stage that found the
   work read from.** `/shipmates-harden` locates findings in your checkout, `/shipmates-document` describes what you
   built, `/shipmates-onboard` surveys your files, `/shipmates-polish` critiques your render, and `/shipmates-spike`'s ADR
   belongs on top of the state that provoked it, so those cut from `HEAD`; `/shipmates-issue` and
   `/shipmates-fix-bug` start from an issue and a bug must reproduce against the base branch, and
   `/shipmates-migrate` and `/shipmates-refactor` transform the whole codebase and want a clean mergeable baseline,
   so those cut from `origin/<BASE_BRANCH>`. `HEAD` is not the working tree — uncommitted work
   isn't in it — so a command that cuts from `HEAD` must check `git status --porcelain` first
   and stop or warn, or it ends up surveying one thing and changing another.
8. Add `<name>` to `SLUGS` in `tools/gen_command_pages.py`.
9. Run `python3 tools/gen_command_pages.py` and commit the regenerated `site/commands/**` and
   `site/sitemap.xml` — never hand-edit those. CI fails if they drift from the rendered sources.
10. Add a matching card to the `#commands` grid in `site/index.html`, linking to `commands/<name>/`.
11. Both validators must exit 0 before you open the PR: `cargo run -- check --target <target>` and
    `python3 .github/scripts/validate_site.py`.
12. **Release-affecting changes** (new/changed commands, tools, crew, or install payload) must bump
    `Cargo.toml` and add a `CHANGELOG.md` entry in the **same PR** — not a follow-up. Without the
    bump, merge to `main` does not publish a new release.

## Testing your change

Prompt-cost rules are documented in [`docs/COST.md`](docs/COST.md). Keep command instructions
cache-friendly: stable workflow text first, one runtime `$ARGUMENTS` section last; ask subagents for
compact structured decisions rather than transcripts. The shared preamble markers are expanded by
the renderer, so update the doctrine instead of copying its wording into commands or crew.

### Portability sources

**`commands/` and `crew/` are the only things you edit.** Harness-neutral role and workflow
content lives there authoritatively. Per-harness payloads are compiled by the Rust CLI — the
adapters in `src/adapters/` emit frontmatter, the render layer (`src/adapters/render.rs`) rewrites
the neutral dialect (`agent-files/*.md`, `{{argument}}`, `Harness-Session`) into each harness's
real one. `site/commands/**` is generated from the **rendered** Claude Code payload; the committed
payload digests under `tests/payload-digests/` are regression fixtures, checked by
`cargo run -- check --target <target>`. Editing a generated site page or a digest directly changes
nothing that ships and fails CI.

Keep semantic capabilities in `tools/capability_registry.json`, and regenerate rather than hand-edit:

```bash
cargo run -- check --target claude-code            # what CI runs
cargo run -- build --target claude-code --update   # after a commands/ or crew/ edit
```

Install into a throwaway scope and try it on a real repo:

```bash
shipmates install --harness claude-code --dir /tmp/some-test-repo
```

Then run the command in Claude Code and confirm the agents resolve (no "falling back to
general-purpose" notes in the report).

### Required status checks on `main`

Branch protection and rulesets are repository settings, so no pull request can change them: a
workflow edit proposes the jobs, never the gate that blocks on them. The desired state is committed
in `.github/rulesets/main.json`, next to the workflows it gates.

A maintainer applies it:

```bash
# update the existing `main` ruleset (id 19800250) in place
gh api --method PUT repos/saman-mb/shipmates/rulesets/19800250 --input .github/rulesets/main.json
# or, when no ruleset named main exists yet
gh api --method POST repos/saman-mb/shipmates/rulesets --input .github/rulesets/main.json
```

It requires four checks before a pull request can merge — **Validate site**, **Exporter on minimum
environment**, **CLI e2e**, and **Sandbox install + CLI e2e** — each the job name a PR-triggered
workflow declares, since GitHub matches a required check on that name. It also blocks branch
deletion and force-pushes to `main`. The `Deploy to GitHub Pages` job and the Release workflow's
matrix-expanded jobs are deliberately not required: they either run only on a push to `main` or
rename themselves with the release plan, so requiring one would block every merge.

## PRs

Keep changes focused, explain the intent, and make sure `shipmates install` still installs cleanly.

### Never push to a branch whose PR has already merged

This repo **squash-merges**. Once a PR is merged its branch is a dead end: a commit pushed
afterwards reaches no open PR and never lands on `main`. GitHub surfaces it nowhere, so it is lost
silently — most often to work that continued *while* the PR was being merged.

Check the state before pushing to a branch that already has a PR:

```bash
gh pr view <number> --json state -q .state    # MERGED → cut a new branch instead
```

If it has merged, branch from the current `origin/main` and **cherry-pick** the stranded commit
(`git cherry-pick <sha>`) rather than rebasing the old branch: a squash merge rewrites SHAs, so the
original branch looks like it diverges from `main` even when every change in it already landed.

A stale branch also makes `git diff origin/main` lie — commits merged into `main` while you were
working show up as large deletions of *your own*, which reads like someone reverted your work. Run
`git log --oneline HEAD..origin/main` before concluding anything was lost.
