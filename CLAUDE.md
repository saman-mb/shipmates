# Shipmates — contributor instructions

`AGENTS.md` at the repository root is the **single source of truth** for how to work in
this repo. Claude Code reads `CLAUDE.md` and does not read `AGENTS.md`
([anthropics/claude-code#34235](https://github.com/anthropics/claude-code/issues/34235)),
so this file imports it.

**Edit `AGENTS.md` — never this file.** This is a pointer, not a second copy, and
`cargo test` fails if it stops resolving.

@AGENTS.md
