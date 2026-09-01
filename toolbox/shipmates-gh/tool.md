---
name: shipmates-gh
description: Shipmates: Structured GitHub CLI wrapper for shipmates workflows — issues, PRs, CI checks, labels, releases, and run logs with validated inputs and JSON results. Reach for this instead of hand-rolling gh shell when fetching issues, opening PRs, polling CI, posting body-file comments, merging with match-head-commit, or searching for duplicate bugs. Requires the GitHub CLI (gh) installed and authenticated. Never a slash command; use it implicitly when the intent calls for one.
---

# gh

A tool the crew uses on its own. Shipmates commands orchestrate GitHub constantly —
issue intake, PR create, CI poll, epic checklist edits, review posts, merge gates.
This wraps **`gh`** with a JSON spec so agents get validated inputs, `--body-file`
hygiene, and parseable JSON back without improvising shell.

## Prerequisite

The [GitHub CLI](https://cli.github.com/) must be installed and logged in:

```bash
gh auth status
```

The tool fails fast with a clear message if `gh` is missing or unauthenticated.

## Run it

The wrapper `gh.py` sits next to this file. Pass one JSON **spec** on stdin (or via
`--spec spec.json`). It prints a JSON **result** on stdout; diagnostics go to stderr.

```bash
echo '{"op":"repo.view"}' | python3 gh.py
python3 gh.py --spec call.json
```

Exit codes: `0` success; `2` usage/validation error; `3` gh command failed.

## Spec shape

Every call requires `"op"`. Other fields depend on the operation.

| Field | Used for |
| --- | --- |
| `op` | Operation name (see table below) |
| `repo` | `owner/name` — defaults to current repo when omitted |
| `number` | Issue or PR number — the **parent** for sub-issue ops |
| `sub_issue_number` | Child issue number (`issue.sub_issue_add` / `issue.sub_issue_remove`) |
| `replace_parent` | Reassign a child that already has a different parent (default `false`) |
| `query` | Search string (`issue.search`) |
| `title` | Issue/PR title |
| `body_file` | Path to body text (**required** for create/edit/comment/review — never inline untrusted bodies) |
| `labels` | String array |
| `state` | `open`, `closed`, `merged`, `all` |
| `limit` | List cap (default 100, max 1000) |
| `base`, `head` | PR base/head branches |
| `fields` | Override `gh --json` field list |
| `head_sha` | PR merge `match-head-commit` binding |
| `squash`, `delete_branch` | PR merge options (default true) |
| `event` | PR review: `approve`, `request-changes`, `comment` |
| `run_id` | Workflow run id |
| `tag`, `notes_file` | Release create |
| `interval_secs`, `timeout_secs` | `pr.checks_poll` (defaults 15, 3600) |
| `log_lines` | Truncate failed log (`run.log_failed`, default 60) |

## Operations

These mirror what shipmates commands use today.

| op | Purpose |
| --- | --- |
| `auth.status` | Preflight authentication |
| `repo.view` | Default branch, nameWithOwner, url |
| `issue.view` | Fetch one issue |
| `issue.list` | List issues (`state`, `labels`, `limit`) |
| `issue.search` | Search issues (dedupe before filing) |
| `issue.create` | Create issue (`title`, `body_file`, `labels`) |
| `issue.edit` | Edit issue body (`body_file`) |
| `issue.comment` | Comment on issue (`body_file`) |
| `issue.close` | Close issue |
| `issue.sub_issue_add` | Attach a child issue to a parent (`sub_issue_number`, optional `replace_parent`) |
| `issue.sub_issue_list` | Children + completion summary for a parent |
| `issue.sub_issue_remove` | Detach a child from its parent (`sub_issue_number`) |
| `pr.view` | Fetch PR metadata |
| `pr.view_current` | PR for current branch |
| `pr.diff` | PR diff text |
| `pr.create` | Open PR (`base`, `head`, `title`, `body_file`) |
| `pr.checks` | CI check rollup for a PR |
| `pr.checks_poll` | Poll until checks complete or timeout |
| `pr.comment` | PR comment (`body_file`) |
| `pr.review` | Submit review (`event`, `body_file`) |
| `pr.merge` | Squash merge with optional `head_sha` |
| `pr.list` | List PRs (`state`, `base`, `head`, `limit`) |
| `label.list` | Repo labels |
| `label.create` | Create label (`name`, `color`, `description`) |
| `release.list` | Recent releases |
| `release.create` | Tag + release (`tag`, `notes_file`) |
| `run.view` | Workflow run status |
| `run.log_failed` | Failed job log excerpt |

## Examples

**Default branch (ship-issue / ship-epic config):**

```json
{"op": "repo.view"}
```

**Fetch epic issue:**

```json
{"op": "issue.view", "number": 305, "repo": "saman-mb/shipmates"}
```

**Create issue with body file (report-bug / file upstream):**

```json
{
  "op": "issue.create",
  "repo": "saman-mb/shipmates",
  "title": "/ship-epic stops mid-loop",
  "body_file": "/tmp/issue-body.md",
  "labels": ["bug"]
}
```

**Attach a story to its parent epic (plan-epics Stage 3):**

```json
{"op": "issue.sub_issue_add", "number": 305, "sub_issue_number": 306}
```

Both fields are issue **numbers**. The op lists the parent's children first, so a
re-run on an already-attached story returns `{"attached": false, "reason":
"already-child"}` without a write. `gh issue view --json subIssues` is a
connection (`{nodes, totalCount}`); the op unwraps `nodes` into `subIssues` (a
list of children) plus `numbers`.

**Read the parent's sub-issue graph (plan-epics Stage 4, ship-epic Stage 0):**

```json
{"op": "issue.sub_issue_list", "number": 305}
```

**Poll CI until done (ship-issue Stage 4.5):**

```json
{"op": "pr.checks_poll", "number": 306}
```

**Merge bound to PR head (ship-issue Stage 8 auto):**

```json
{
  "op": "pr.merge",
  "number": 306,
  "head_sha": "abc123…"
}
```

**Search open duplicates:**

```json
{
  "op": "issue.search",
  "repo": "saman-mb/shipmates",
  "query": "repo:saman-mb/shipmates is:issue is:open ship-epic checklist",
  "limit": 10
}
```

## Honesty

This is a thin wrapper around **`gh`**, not a second GitHub API client. Behaviour,
rate limits, and auth scopes follow the CLI. Multi-line bodies must use
`body_file` — the tool rejects large inline `body` strings to match shipmates
shell-safety rules. Validate issue/PR numbers and repo slugs before subprocess
invocation; do not pass raw user tokens to `gh` on the command line.

**Sub-issues are a hosted-GitHub feature.** The three sub-issue ops need a host
whose API exposes the parent/child graph — github.com does; a GitHub Enterprise
Server release older than the feature may not, and an installed `gh` predating
its `--add-sub-issue` / `--remove-sub-issue` flags cannot drive it either. The
ops surface the CLI's own error rather than emulating the relationship; a caller
that needs to work on both should treat a failure as "this host has no sub-issue
graph" and fall back to whatever in-body linking it already writes.
