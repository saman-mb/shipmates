# Contributing to Shipmates

Thanks for wanting to add to the crew! New agents and commands are welcome.

## The one hard rule: agents stay domain-neutral

An agent role in `agents/` must **not** mention any specific language, framework, product, or
project. It describes *how the role thinks and works*; the standard it enforces comes from the
target repo's `README` / `CLAUDE.md` at run time. If you find yourself writing "Godot", "React",
"our game", or a specific style guide into a role, move that into the repo that uses it instead.

Good: *"Hold the work to whatever visual bar the project states for itself."*
Not good: *"Match our SC2-style HD dark-chrome UI."*

## Adding an agent

1. Create `agents/<name>.md` (lowercase-and-hyphens name).
2. Frontmatter: `name`, `description` (when Claude should delegate to it), and `tools` (least
   privilege — a reviewer usually needs `Read, Grep, Glob, Bash`, not `Write`/`Edit`).
3. Body = the system prompt: the role's focus, how it reviews/builds, and the verdict format it
   returns (`ACCEPT` / `ACCEPT-WITH-NITS` / `REJECT`, or `PASS` / `FAIL`).
4. If a command should invoke it, wire it into that command's stages by `subagent_type`.

## Adding a skill (workflow)

A workflow ships as a **skill** on disk and is invoked as a **command** in Claude Code. Both names
are correct; which one belongs in a given sentence is set by the
[naming register](docs/BRAND.md#naming-register). Work the checklist top to bottom:

1. Create `skills/<name>/SKILL.md` (lowercase-and-hyphens `<name>`). **Check the name against
   harness built-ins first** — run `python3 tools/validate_skill_names.py` and pick a name that
   doesn't collide. A collision silently shadows a first-party feature (e.g. `/review` shadowed
   Claude Code's built-in code review; see #102). Only `name` and `description`
   are required, and they must come first, in that order. After them, in any order: the standard's
   optional `license`, `compatibility`, `metadata`, and the Claude Code extensions `argument-hint`,
   `allowed-tools`, `disable-model-invocation`. Unknown keys are rejected, so a typo fails the gate.
   The twelve ship `name`, `description`, `argument-hint`, `allowed-tools`, `disable-model-invocation`
   — recommended, not required. **`name` must be exactly the directory name** — that is the
   [Agent Skills](https://agentskills.io) standard's rule, and the skill will not resolve without it.
2. `argument-hint`, `allowed-tools` and `disable-model-invocation` are vendor extensions on top of
   the standard, kept comma-separated because that is what Claude Code parses. Set
   `disable-model-invocation: true` unless the workflow is genuinely safe for Claude to start
   unprompted — every shipped command sets it, because they create worktrees, push branches and open
   pull requests. It does not affect typing `/<name>` yourself.
3. Make the first heading after the frontmatter `# /<name> — <tagline>` — the page generator parses
   that line for the command's title, and fails on anything else.
4. Take input from `$ARGUMENTS` only, parsed in prose — a skill has no positional arguments. A `$`
   followed by a digit is rejected **anywhere in the file**, frontmatter and fenced code blocks
   included: substitution is textual over the whole file, so a fence protects nothing. (An earlier
   version of this checklist allowed it inside a fence, on the grounds that it reads as a shell field
   reference there. That was wrong: run `/ship-issue 42 focus on retries` and a fenced snippet asking
   for field two gets the literal word `on`.) If you genuinely need a literal, escape it as `\$2` —
   but prefer restructuring so you don't, e.g. `cut -f2` rather than an `awk` field reference.
5. Prefer invoking the shared agents by `subagent_type` over inlining personas.
6. **Read-only, or worktree + PR — in-place only on explicit request.** A workflow that changes a
   repo works on its own branch in its own worktree and proposes the result as a pull request; the
   caller's checkout is left as they left it. Writing straight into the working tree is an opt-in
   (`MODE=edit-in-place`), never a default. `/release` is the one shipped exception: the release
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
   opt-in, off unless the caller sets it, with one known deviation: `/plan-epics` files issues and
   labels by default today ([#111](https://github.com/saman-mb/shipmates/issues/111), not yet fixed).
   Be a good guest on other people's repos: a branch is a suggestion, a merge is a decision.
   **Which ref to branch from follows the same split: cut from wherever the stage that found the
   work read from.** `/harden` locates findings in your checkout, `/document` describes what you
   built, `/onboard` surveys your files, `/polish` critiques your render, and `/spike`'s ADR
   belongs on top of the state that provoked it, so those cut from `HEAD`; `/ship-issue` and
   `/fix-bug` start from an issue and a bug must reproduce against the base branch, and
   `/migrate` and `/refactor` transform the whole codebase and want a clean mergeable baseline,
   so those cut from `origin/<BASE_BRANCH>`. `HEAD` is not the working tree — uncommitted work
   isn't in it — so a command that cuts from `HEAD` must check `git status --porcelain` first
   and stop or warn, or it ends up surveying one thing and changing another.
7. Add `<name>` to `SLUGS` in `tools/gen_command_pages.py`.
8. Run `python3 tools/gen_command_pages.py` and commit the regenerated `site/commands/**` and
   `site/sitemap.xml` — never hand-edit those. CI fails if they drift from the skill sources.
9. Add a matching card to the `#commands` grid in `site/index.html`, linking to `commands/<name>/`.
10. Both validators must exit 0 before you open the PR: `python3 tools/validate_skills.py` and
    `python3 .github/scripts/validate_site.py`.

## Testing your change

### Portability sources

**`canonical/` is the only thing you edit.** Harness-neutral role and workflow content lives
there authoritatively. `agents/`, `skills/` and `tests/golden/claude-code/` are all **generated**
from it — `agents/` and `skills/` stay in the repository only because the site generator and the
skills validator read them. Editing one of them directly changes nothing that ships and fails CI
with a `compatibility drift:` line naming the file.

Keep semantic capabilities in `tools/capability_registry.json`, implement new targets through
`tools/adapter_contract.py` (run `conformance_report` against your adapter before registering it
in `canonical/manifest.json`), and regenerate rather than hand-edit:

```bash
cargo run -- check --target claude-code            # what CI runs
cargo run -- build --target claude-code --update   # after a canonical/ edit
```

Install into a throwaway scope and try it on a real repo:

```bash
shipmates install --project /tmp/some-test-repo
```

Then run the command in Claude Code and confirm the agents resolve (no "falling back to
general-purpose" notes in the report).

## PRs

Keep changes focused, explain the intent, and make sure `shipmates install` still installs cleanly.
