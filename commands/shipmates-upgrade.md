---
name: shipmates-upgrade
description: Shipmates: Check for a newer Shipmates release, then refresh, audit, and repair every known install — reporting only when asked, filing upstream bugs only when asked.
argument-hint: [report] [pre] [file-bugs]
allowed-tools: Bash, Read, Write, Grep, Glob
disable-model-invocation: true
---
# /shipmates-upgrade — check for a newer release, refresh and audit every install

<!-- shipmates:bootstrap -->
Check the installed `shipmates` binary for a newer release, then survey every known install of the
crew and commands, refresh them from the new binary, audit each one, and repair drift. Invoked with no
tokens it does exactly that — the command is named for the work. `report` is the explicit read-only
pass: survey, show the plan, change nothing. Filing shipmates-attributable findings upstream is the
innermost opt-in (`file-bugs`), so the default upgrades and repairs, and never files.

The installed CLI does the real work — the release check, the self-upgrade, the refresh, the audit, and
the upstream filing — and this command runs it and reports. It never shells out to `gh` itself and never
reimplements the upgrade in prose: the CLI is the source of truth for what changed, and its `--json`
output is what this command reads back.

This is the second **meta command**, alongside `/shipmates-report-bug`: unlike domain-neutral crew roles,
it may name Shipmates, harnesses, and the upstream repo `saman-mb/shipmates` explicitly. It upgrades and
repairs *installs of Shipmates*; it does **not** fix upstream code. For a bug in the captain's own repo,
run `/shipmates-fix-bug`; for a bug in Shipmates itself, file a report with `/shipmates-report-bug`.

**Composition target.** Mid-run use from inside another gated command Reads **this installed file** and
executes these stages in-session — Skill may be unavailable because `disable-model-invocation` is on.
Frontmatter stays gated; behaviour matches a standalone run with the same arguments.

---

## Config (override only if the captain asks)

- `MODE` = `apply` (default) — run the upgrade and repair steps. `report` — survey and show the plan,
  changing nothing; it is the explicit safe token. Infer from the runtime tokens; ambiguous → `apply`,
  and state which mode ran.
- `PRE` = `no` (default) — the release check considers stable releases only. `yes` (the `pre` token)
  includes prereleases.
- `FILE_BUGS` = `no` (default) — print the findings and the exact filing command, but file nothing.
  `yes` (the `file-bugs` token, honoured in `MODE=apply`) lets the CLI file or comment upstream.
- `UPSTREAM_REPO` = `saman-mb/shipmates` (fixed) — the only repo the filing step may target.
- **The CLI owns the mutating steps.** `shipmates upgrade --self`, `--fix`, and `--file-bugs` are the
  only things that change state; this command never runs a raw `gh` or `curl` of its own and never
  reimplements an upgrade in shell.
- **No secrets in outputs.** The CLI sanitises home paths and never prints credentials; this command
  redacts anything secret-shaped that happens to appear in JSON before it repeats it.

---

## Stage 0 — Intake  (orchestrator)

1. Parse the runtime input into `MODE`, `PRE`, and `FILE_BUGS` from the Parameters table. Default
   `MODE=apply`, `PRE=no`, `FILE_BUGS=no`; state the resolved values on the first line of the report.
2. Verify the binary and its surface before relying on either. Run `shipmates upgrade --help` and
   `shipmates status --help`. If either subcommand or one of its flags is missing, take Stage 6's
   version-skew path and say which path ran.
3. If the `shipmates` binary itself is absent, **stop** and give install guidance — the Homebrew /
   Cargo / cargo-dist installer lines from the project's own README — never an upgrade.

## Stage 1 — Survey  (orchestrator, read-only)

Run the two read-only reporters and read their JSON:

```bash
shipmates upgrade --check --json
shipmates status --json
```

Present one compact table: latest release, running version, whether an upgrade is available (or that the
release check came back **unknown** — offline, no `curl` on the `PATH`, or a blocked fetch — in which
case say so and continue; never hard-fail on the network), the detected channel, and one row per
install: root, harness, receipt version, state, and drift count. `status` is read-only and never
mutates the index; `--check` is a read-only three-way version report.

## Stage 2 — Binary upgrade  (orchestrator)

If a newer release exists, show the exact channel command the CLI reports for this install (brew,
cargo-dist, or cargo). In `MODE=apply`, run:

```bash
shipmates upgrade --self
```

The CLI executes only for the brew and cargo-dist channels. A cargo install, a source checkout, or an
unknown channel is refused: the CLI prints the manual command (`cargo install shipmates --locked`,
`git pull` and rebuild, or the package manager's own update command) and continues to Stage 3.
`--dry-run` prints what would change without executing anything. Degrade, never break.

## Stage 3 — Refresh + audit  (orchestrator)

In `MODE=apply`, run:

```bash
shipmates upgrade --fix --json
```

Read the JSON back and report what was refreshed, what was repaired, and what remains. A failing root
must not abort the others: the CLI records the failure for that root's installs and continues, and this
command reports the per-root outcome rather than stopping at the first red.

## Stage 4 — Findings + filing  (orchestrator)

Read the findings from the JSON — each is `class`, `harness`, `root`, `detail`, `fingerprint`. Only
shipmates-attributable failures appear there; user-caused drift a `--fix` already repaired is not a
finding, so never present it as one. In `MODE=apply` **with the `file-bugs` token**, run:

```bash
shipmates upgrade --file-bugs
```

The CLI dedupes by fingerprint, sanitises home paths, filters to labels that exist, and files or
comments upstream — this command reports the issue URLs it returns. Without the token, print the
findings list and the exact command a captain would run to file them. **Never file silently**: filing is
opt-in once — the `file-bugs` token — and applying no longer implies it.

## Stage 5 — Report  (orchestrator)

One concise summary: what upgraded (or the manual path taken), what was repaired, findings with their
disposition (filed / commented / skipped / unfiled), any issue URLs, and the reminder that a running
harness loads skills and agents at session start — the captain restarts the harness before invoking a
refreshed command, or the new payload is not yet in the session.

## Stage 6 — Version skew  (orchestrator, fallback)

If the installed CLI predates this command's flags (Stage 0's `--help` does not list `status` or
`upgrade`), fall back to the pre-`status`/`upgrade` path and **say which path ran**. `MODE` governs the
fallback exactly as it governs the main path, so `report` changes nothing here either:

```bash
# MODE=apply (the default) — refresh, then repair drift
shipmates update --harness all
shipmates doctor --fix
# MODE=report — report only: never run the update, and run doctor without --fix
```

The release check degrades to "unknown" here, and filing is not available — report both plainly, and
point at `/shipmates-report-bug` for a manual upstream report instead.

---

### Guardrails

- **User-invoked meta command.** As one of the two meta commands it may name Shipmates, harnesses, and
  `saman-mb/shipmates`; the `disable-model-invocation` gate keeps the decision to upgrade, repair, or
  file with the captain.
- **Never downgrade.** An install whose receipt version is already ahead of the running binary is a
  finding to report, never a reason to move it back.
- **Never self-upgrade a source checkout.** `--self` is for brew / cargo-dist only; a source checkout or
  unknown channel gets the manual path, never an attempted self-upgrade.
- **Never file silently.** Filing is opt-in through the `file-bugs` token and nothing else, and the
  command always prints the list and the exact command before any filing runs.
- **No secrets in outputs.** Home paths are sanitised and credential-shaped strings redacted before
  anything is repeated to the captain.
- **This command does not fix upstream code.** It upgrades and repairs installs. A bug in the captain's
  repo goes to `/shipmates-fix-bug`; a bug in Shipmates itself goes to `/shipmates-report-bug`.

## Parameters

| Name | Required | Token | Values | Default | Mutates | Help |
|------|----------|-------|--------|---------|---------|------|
| report | no | `report` | `report` | apply (`MODE=apply`) | yes | Report only: survey and show the plan without upgrading, repairing, or filing. The command upgrades by default; this is the safe token. |
| pre | no | `pre` | `pre` | omit (stable only) | — | Include prereleases in the release check. |
| file-bugs | no | `file-bugs` | `file-bugs` | omit | — | Allow filing or updating upstream issues for shipmates-attributable findings; needs `MODE=apply`, which is the default. |

## Runtime input

Read `## Parameters` first. `$ARGUMENTS` is the captain-supplied or post-intake token string; parse
Tokens/Values from that table (required then optionals). If intake ran, treat the restated invocation as
authoritative for this run.

The three tokens are optional and order-independent. `report` sets `MODE=report` — survey and show the
plan, never upgrade, repair, or file. Without it the run is `MODE=apply`: bring the binary and every
known install up to date, then repair drift. `pre` includes prereleases in the release check (stable
only otherwise). `file-bugs` allows filing or updating upstream issues for shipmates-attributable
findings; it is honoured in `MODE=apply` and ignored with a note under `report`. An empty invocation is
a full upgrade-and-repair run.
