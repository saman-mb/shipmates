#!/usr/bin/env bash
#
# Regression test for the shell-injection lint in tools/validate_skills.py
# (#82 in ship-issue, #138 in ship-pr-review).
#
# The lint is a negative control, so it can rot silently: a regex "cleanup"
# that stops matching still leaves every suite green. This pins both halves —
# the forms that must be rejected, and the fact that ship-pr-review still posts its
# review through --body-file.
#
#   bash tests/test_validate_skills.sh
#
# Exit 0 = all passed, 1 = at least one failure.

set -uo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

PASS=0; FAIL=0
ok()  { PASS=$((PASS+1)); printf 'ok   %s\n' "$1"; }
bad() { FAIL=$((FAIL+1)); printf 'FAIL %s\n' "$1"; }

# The validator resolves the repo root from its own __file__, so a copy in a
# sandbox lints the sandbox's commands/ — no --root flag needed, and the real
# tree is never touched. crew/ comes along because the spawn-binding check reads
# the roster from it; without it that check would degrade to a no-op here and
# the negative control below would pass vacuously.
mkdir -p "$WORK/tools" "$WORK/commands" "$WORK/crew"
cp "$REPO/tools/validate_skills.py" "$WORK/tools/"
cp "$REPO"/crew/*.md "$WORK/crew/"

# Run the validator over a fixture command built from the body on stdin.
# Prints nothing; returns the validator's exit status.
lint_body() {
  {
    printf -- '---\nname: fixture\ndescription: A fixture command used by the lint regression suite.\n---\n\n# /fixture\n\n'
    cat
  } > "$WORK/commands/fixture.md"
  python3 "$WORK/tools/validate_skills.py" >/dev/null 2>&1
}

# $1 = description, stdin = SKILL.md body that must be REJECTED.
rejects() {
  if lint_body; then bad "rejects: $1"; else ok "rejects: $1"; fi
}

# $1 = description, stdin = SKILL.md body that must be ACCEPTED.
accepts() {
  if lint_body; then ok "accepts: $1"; else bad "accepts: $1"; fi
}

# --- forms that must be rejected ---

rejects 'double-quoted --body (the original #138 line)' <<'MD'
```bash
gh pr review <PR#> --comment --body "<consolidated findings>"
```
MD

rejects 'single-quoted --body' <<'MD'
```bash
gh pr review <PR#> --comment --body '<consolidated findings>'
```
MD

rejects 'unquoted --body $VAR (word-splits on attacker text)' <<'MD'
```bash
gh pr review <PR#> --comment --body $FINDINGS
```
MD

rejects '--body= with an equals sign' <<'MD'
```bash
gh pr review <PR#> --comment --body="$FINDINGS"
```
MD

rejects "gh's short -b spelling" <<'MD'
```bash
gh pr comment <PR#> -b "$FINDINGS"
```
MD

rejects '--body in an ```sh fence' <<'MD'
```sh
gh pr review <PR#> --comment --body "$FINDINGS"
```
MD

rejects '--body in a bare ``` fence' <<'MD'
```
gh pr review <PR#> --comment --body "$FINDINGS"
```
MD

rejects '--body after a nested fence inside a heredoc' <<'MD'
````bash
cat <<'INNER'
```
not a real fence
```
INNER
gh pr review <PR#> --comment --body "$FINDINGS"
````
MD

rejects '--body split across a backslash continuation' <<'MD'
```bash
gh pr review <PR#> --comment \
  --body "$FINDINGS"
```
MD

rejects '--body-file with a command substitution as the path' <<'MD'
```bash
gh pr review <PR#> --comment --body-file "$(gh pr view <PR#> --json title -q .title)"
```
MD

# --- spawn binding: a fan-out command must declare its crew role (#488) ---
# The defect this guards is silent: a command fans work out to "workers", names no
# role, and the harness resolves nothing — every finding comes from a general-purpose
# agent while the payload, digests and CI all look healthy. The check reads
# DECLARATIONS, not sentences: an earlier prose-scanning version was defeated four
# times in review (a file-wide anchor, a one-word insert in an unrelated stage, the
# substring `architect` inside `architectural`, and a negative mention satisfying it).
# These cases pin the structural behaviour, including each of those defeats.
rejects "fan-out with no role declared anywhere" <<'EOF'
## Stage 1 — Census

- `MAX_CONCURRENT_WORKERS` = `5`.

Split the survey across workers.
EOF
rejects "role named in prose but never declared" <<'EOF'
## Stage 1 — Census

- `MAX_CONCURRENT_WORKERS` = `5`.

Spawn a `security-engineer` per class.
EOF
rejects "role name hidden inside a longer word" <<'EOF'
## Stage 1 — Audit the architecture

- `MAX_CONCURRENT_WORKERS` = `5`.

Spawn workers across the layers.
EOF
rejects "a negative mention of a composed command" <<'EOF'
## Stage 1 — Census

- `MAX_CONCURRENT_WORKERS` = `5`.

Run units concurrently. Do not compose `/ship-issue` yourself.
EOF
rejects "annotation on an unrelated stage" <<'EOF'
## Stage 0 — Plan  (agent: `architect`)

Nothing here.

## Stage 1 — Census

- `MAX_CONCURRENT_WORKERS` = `5`.

Spawn workers.
EOF
accepts "fan-out stage annotated with its role" <<'EOF'
## Stage 1 — Census  (agent: `sdet`)

- `MAX_CONCURRENT_WORKERS` = `5`.

Spawn workers.
EOF
accepts "role declared through a Config binding" <<'EOF'
## Config

- `BUILDER` = `senior-engineer`. `MAX_CONCURRENT_WORKERS` = `5`.

Spawn builders.
EOF
accepts "fan-out stage declaring a composed command" <<'EOF'
## Stage 2 — Loop  (composes: /ship-issue, one per unit)

- `MAX_CONCURRENT_WORKERS` = `5`.
EOF
accepts "no fan-out at all needs no declaration" <<'EOF'
This command runs its analysis itself and spawns nothing.
EOF

# --- forms that must be accepted ---

accepts '--body-file with a quoted variable path (the sanctioned form)' <<'MD'
```bash
REVIEW_BODY_FILE=$(mktemp)
gh pr review <PR#> --comment --body-file "$REVIEW_BODY_FILE"
```
MD

accepts "git worktree's unrelated -b <BRANCH> flag" <<'MD'
```bash
git -C <repo> worktree add <WORKTREE_DIR> -b <BRANCH> HEAD
```
MD

accepts 'a flag that merely starts with --body' <<'MD'
```bash
sometool --bodyguard "on"
```
MD

accepts 'a non-shell fence that happens to contain --body' <<'MD'
```json
{"flag": "--body \"x\""}
```
MD

# --- positive control: the real tree, and the fix #138 actually shipped ---

real_rc=0
python3 "$REPO/tools/validate_skills.py" >/dev/null 2>&1 || real_rc=$?
if [ "$real_rc" -eq 0 ]; then ok "real commands/ passes the lint"; else bad "real commands/ passes the lint"; fi

for f in commands/ship-pr-review.md; do
  if grep -q -- '--body-file "\$REVIEW_BODY_FILE"' "$REPO/$f"; then
    ok "$f still posts via --body-file (#138 fix present)"
  else
    bad "$f still posts via --body-file (#138 fix present)"
  fi
done

# --- summary ---

echo
echo "passed: $PASS, failed: $FAIL"
[ "$FAIL" -eq 0 ]
