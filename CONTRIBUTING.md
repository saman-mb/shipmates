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

## Adding a command

1. Create `commands/<name>.md` with a `description` and `argument-hint` frontmatter.
2. Prefer invoking the shared agents by `subagent_type` over inlining personas.
3. Anything with side effects (merging, publishing) should be **opt-in**, not the default — be a
   good guest on other people's repos.

## Testing your change

Install into a throwaway scope and try it on a real repo:

```bash
./install.sh --project /tmp/some-test-repo
```

Then run the command in Claude Code and confirm the agents resolve (no "falling back to
general-purpose" notes in the report).

## PRs

Keep changes focused, explain the intent, and make sure `./install.sh` still installs cleanly.
