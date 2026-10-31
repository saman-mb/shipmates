# pi-shipmates

The Shipmates crew, commands and toolbox for [pi](https://pi.dev), installed as one pi package.

```bash
pi install npm:pi-shipmates
```

That is the whole install. No Rust toolchain, no separate binary — the files pi loads come from this
package, and `pi update --extensions` keeps them current.

## What you get

- **13 specialist crew roles** — architect, senior-engineer, principal-engineer, sdet, security-engineer,
  site-reliability-engineer, performance-engineer, devops-engineer, product-manager, ux-ui-designer,
  art-director, technical-writer, data-scientist. Pi resolves them through
  [`pi-subagents`](https://github.com/nicobailon/pi-subagents), which must be installed for the crew to
  be spawnable.
- **Every Shipmates command**, invoked as `/skill:shipmates-ship-issue`, `/skill:shipmates-fix-bug`, and
  so on. Commands are captain-invoked: nothing here runs on its own.
- **The toolbox** — the model-invoked utilities (diagram, badge, sparkline, scrub, fixtures, GitHub CLI
  wrapper and friends) the crew reach for implicitly.
- **The crew resource broker**, which meters local-model capacity so a subagent fan-out cannot exhaust
  VRAM. It starts at Off and does not change your model; run `/skill:shipmates-crew` once to set it.

## The crew holds your standard, not ours

The roles are deliberately domain-neutral: they enforce whatever your repository's `AGENTS.md` or
`CLAUDE.md` says, so the same crew works on a game engine, a web app or a CLI. Point them at a repo with
real instructions and they hold that line.

## Updating and removing

```bash
pi update --extensions      # refresh this package
pi list                     # what is installed
pi remove npm:pi-shipmates  # remove it, and its entry in your settings
```

Because the payload lives inside the package rather than being copied into your home directory, removal
leaves nothing behind.

## Project steering

If you are changing Shipmates itself, the contributor doctrine is in the package at
`docs/contributor-steering.md` — read it before editing `commands/`, `crew/` or `toolbox/`. It is
reference documentation rather than a command, and a package cannot write your project's instructions
file anyway. To have steering written into user or project instruction files, use the `shipmates` CLI,
which owns that merge.

## If you installed Shipmates before

If you previously ran `shipmates install --harness pi`, its files live in your pi user directory, and
pi-subagents ranks those **above** the ones in this package. You would keep getting the older crew with
nothing looking wrong. Check and clear them, then restart pi:

```bash
ls ~/.pi/agent/agents        # shipmates crew from a CLI install?
rm ~/.pi/agent/agents/<the shipmates roles you no longer want>
```

Prefer the CLI's behaviour instead? Then leave those files alone — a package install never deletes them,
and it does not overwrite anything in your home directory.

## Relationship to the CLI

This package and the `shipmates` binary render the **same** payload from the same sources; the publish
job runs the normal installer path and repacks its output. The binary remains the way to install for the
other eight harnesses, to repair or audit an existing install (`shipmates doctor`,
`/skill:shipmates-upgrade`), and to write steering into instruction files.

Source and issue tracker: <https://github.com/saman-mb/shipmates>.
