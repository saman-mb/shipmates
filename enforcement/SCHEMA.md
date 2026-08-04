# Shipmates enforcement — FSM state model & `run-<issue>.json` schema

This documents brick 1 of the hook-enforced ship loop: the phase state machine
and the on-disk run document owned by [`lib/state.sh`](lib/state.sh).

> **Honest scope.** This is the state model and a transition-validated read/write
> library — **not** the enforced end-to-end loop. `/ship-issue` is not yet wired
> to this state, and hooks do not yet block anything. The library is a *referee,
> not a jail*: it cannot stop a rogue process from hand-editing the JSON. Its
> integrity contribution is **fail-closed reads** (jq-parse + `schema_version`
> check) and a **hard "never a partial/corrupt file"** guarantee via atomic
> temp-then-rename writes. Wiring it into the loop is a later story.

## Dependency

- **`jq`** (tested with 1.6). Every read/write op requires it; if it is missing
  the library exits `3`. Otherwise only `bash` + coreutils
  (`date`, `mktemp`, `mkdir`, `mv`, `rm`, `sleep`) are used.

## Phases (8)

| Phase       | Meaning                                        | Terminal |
|-------------|------------------------------------------------|----------|
| `INIT`      | Run created, nothing planned yet               | no       |
| `PLANNED`   | Plan accepted                                  | no       |
| `BUILT`     | Change built locally                           | no       |
| `PUSHED`    | Branch/PR pushed                               | no       |
| `CI_GREEN`  | CI reported green for the pushed SHA           | no       |
| `ACCEPTED`  | Reviewers accepted                             | no       |
| `DELIVERED` | Merged / delivered                             | **yes**  |
| `ESCALATED` | Handed to a human (blocked / out of budget)    | **yes**  |

A **terminal** phase has no outgoing edges *at all* — not even the identity
`X -> X`. It is frozen.

## Legal transitions

```
INIT      -> PLANNED
PLANNED   -> BUILT
BUILT     -> PUSHED
PUSHED    -> CI_GREEN
CI_GREEN  -> ACCEPTED
ACCEPTED  -> DELIVERED

# fix / retry loop — a fix returns to BUILT and re-earns push + CI:
PUSHED    -> BUILT
CI_GREEN  -> BUILT

# escalation is reachable from every non-terminal phase:
INIT | PLANNED | BUILT | PUSHED | CI_GREEN | ACCEPTED -> ESCALATED
```

**Identity rule.** `X -> X` is legal **iff `X` is non-terminal**. Identity writes
exist for field-only updates that do not change phase — e.g. a red CI poll that
stays `PUSHED` with `ci.status = red`, recording a verdict while at `CI_GREEN`,
or stamping `reviewed_sha`. Terminal phases reject even identity.

Anything not listed above (and any identity on `DELIVERED`/`ESCALATED`) is
**illegal** and rejected with exit `1`, no write performed.

Notes on the model: `CI_GREEN` is **not sticky** — a red poll stays `PUSHED` with
`ci.status = red`; any fix goes back to `BUILT` and must re-earn green. A fix
always returns to `BUILT`.

## `run-<issue>.json` schema (`schema_version = 1`)

Stored at `${SHIPMATES_DIR:-$PWD/.shipmates}/run-<issue>.json`. All fields are
required unless noted; nullable where stated. Timestamps are ISO-8601 UTC
(`date -u +%Y-%m-%dT%H:%M:%SZ`) and `updated_at` is refreshed on every write.

| Field            | Type                                   | Default     | Notes |
|------------------|----------------------------------------|-------------|-------|
| `schema_version` | int                                    | `1`         | readers accept only `1` (fail-closed on higher) |
| `issue`          | int                                    | —           | matches the filename |
| `phase`          | enum (8 phases)                        | `"INIT"`    | |
| `pr`             | int \| null                            | `null`      | |
| `branch`         | string \| null                         | `null`      | |
| `worktree`       | string \| null                         | `null`      | |
| `base_branch`    | string \| null                         | `null`      | additive/optional — readers must not depend on presence |
| `ci`             | object                                 | see below   | `{status, run_url, sha, checked_at}` |
| `ci.status`      | enum `unknown`\|`pending`\|`green`\|`red` | `"unknown"` | |
| `ci.run_url`     | string \| null                         | `null`      | |
| `ci.sha`         | string \| null                         | `null`      | |
| `ci.checked_at`  | string \| null                         | `null`      | stamped on any `ci_*` write |
| `reviewed_sha`   | string \| null                         | `null`      | |
| `verdicts`       | object (role -> `{verdict, sha, at}`)  | `{}`        | `verdict ∈ ACCEPT \| ACCEPT-WITH-NITS \| REJECT \| PASS \| FAIL` |
| `fix_rounds`     | int ≥ 0                                | `0`         | |
| `max_fix_rounds` | int ≥ 1                                | `3`         | |
| `merge_mode`     | enum `manual`\|`auto`                  | `"manual"`  | |
| `created_at`     | string                                 | now         | additive/optional |
| `updated_at`     | string                                 | now         | additive/optional; set on every write |

New optional fields and new legal edges are additive and safe. The transition
table, the field names/types, and the function-name + exit-code ABI are frozen.

### Example

```json
{
  "schema_version": 1,
  "issue": 42,
  "phase": "CI_GREEN",
  "pr": 128,
  "branch": "issue-42-fsm-state",
  "worktree": "/home/you/shipmates--issue-42",
  "base_branch": "main",
  "ci": {
    "status": "green",
    "run_url": "https://github.com/you/shipmates/actions/runs/1",
    "sha": "1a2b3c4",
    "checked_at": "2026-07-25T10:00:05Z"
  },
  "reviewed_sha": "1a2b3c4",
  "verdicts": {
    "product-manager": { "verdict": "ACCEPT", "sha": "1a2b3c4", "at": "2026-07-25T10:01:00Z" },
    "sdet":            { "verdict": "PASS",   "sha": "1a2b3c4", "at": "2026-07-25T10:01:30Z" }
  },
  "fix_rounds": 1,
  "max_fix_rounds": 3,
  "merge_mode": "manual",
  "created_at": "2026-07-25T09:40:00Z",
  "updated_at": "2026-07-25T10:01:30Z"
}
```

## Exit codes (frozen ABI)

Downstream code branches on these; they do not change. All errors go to stderr,
prefixed `shipmates:`; normal output goes to stdout.

| Code | Meaning |
|------|---------|
| `0`  | success |
| `1`  | illegal transition |
| `2`  | usage / bad argument (invalid issue id, unknown phase/op/field) |
| `3`  | missing dependency (`jq`) |
| `4`  | state file missing |
| `5`  | malformed file / unsupported `schema_version` / IO error |

## CLI

`lib/state.sh` is dual-mode. Sourced, it defines `shipmates_state_*` functions
only (safe to re-source, no side effects). Executed, it is a dispatcher:

```
state.sh init ISSUE [--branch B] [--worktree W] [--base BASE] [--merge-mode M] [--max-fix-rounds N]
state.sh load ISSUE [JQ_FILTER]
state.sh write ISSUE [--to PHASE] [FIELD VALUE]...
state.sh status ISSUE [--human]
state.sh record-verdict ISSUE ROLE VERDICT [SHA]
state.sh assert-transition FROM TO
```

`write` accepts a fixed FIELD whitelist: `pr`, `branch`, `base_branch`,
`worktree`, `reviewed_sha`, `merge_mode`, `max_fix_rounds`, `fix_rounds`,
`ci_status`, `ci_run_url`, `ci_sha`. Values are always passed to `jq` via
`--arg`/`--argjson` — never interpolated into a filter — and issue ids must match
`^[1-9][0-9]*$`.

## Safety guarantees

- **Injection-safe:** issue ids are validated against `^[1-9][0-9]*$`; every
  value reaches `jq` through `--arg`/`--argjson` (never string-interpolated);
  all expansions are quoted.
- **Atomic writes:** a new document is built into a temp file *in the same dir*,
  validated with `jq empty`, then `mv -f` over the target. On any failure the
  original is left untouched and no `.run-*.tmp` residue remains.
- **Fail-closed reads:** a missing file is `4`; a file that does not parse or
  whose `schema_version` is not `1` is `5` — and is never overwritten.
- **Concurrency:** a per-issue `mkdir`-based lock (portable; not `flock`) serial-
  izes writers best-effort. The hard guarantee is the atomic rename, which keeps
  the file integral regardless of the lock.
