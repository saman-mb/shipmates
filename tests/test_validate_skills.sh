#!/usr/bin/env bash
#
# Regression test for the shell-injection lint in tools/validate_skills.py
# (#82 in ship-issue, #138 in pr-review).
#
# The lint is a negative control, so it can rot silently: a regex "cleanup"
# that stops matching still leaves every suite green. This pins both halves —
# the forms that must be rejected, and the fact that pr-review still posts its
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
# tree is never touched.
mkdir -p "$WORK/tools" "$WORK/commands"
cp "$REPO/tools/validate_skills.py" "$WORK/tools/"

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

for f in commands/pr-review.md; do
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
