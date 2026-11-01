#!/usr/bin/env bash
# Fail a pull request that changes released content but leaves the version alone.
#
# Why this gate exists. The release workflow publishes whichever version in
# Cargo.toml has no GitHub release yet, so a change to the crew, the commands,
# the toolbox or the pi extension that lands *without* a version bump merges,
# builds, and goes green — and publishes nothing. Nothing is wrong with CI in
# that moment, which is the problem: the work sits on `main` where no captain
# can install it, and the green tick says otherwise. This makes the silent no-op
# loud at the only point it can still be fixed cheaply, and names the fix.
#
# Behaviour worth stating:
#   - It fails **open** on its own tooling failures (a `gh` or network error is
#     not the author's mistake), and records that in the step summary rather than
#     hiding it — the same rule resolve-release.sh applies to the build gate.
#   - The override is a `no-release` label, for the rare case where the bump
#     arrives in a later commit of a stacked branch. An override that leaves a
#     trace beats one that greps a magic phrase out of a PR description.
#   - The path list is shared with the release trigger through release-paths.sh,
#     so the gate and the trigger cannot drift into different definitions of
#     "release-affecting".
#
# Environment: PR_NUMBER, BASE_SHA (both from the pull_request event), plus the
# runner's GH_TOKEN. Run with --self-test to exercise the decision table with no
# network at all.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=release-paths.sh
# SC1091 ("not following") is expected and harmless here: this gate runs
# `shellcheck .github/scripts/*.sh` without -x, and release-paths.sh is already
# one of those inputs, so it is linted on its own either way.
# shellcheck disable=SC1091
. "$SCRIPT_DIR/release-paths.sh"

OVERRIDE_LABEL="no-release"

# Decide one pull request. Echoes "<verdict>|<detail>" where verdict is
# pass (nothing to complain about), fail (author must act), or skip (this gate
# could not see enough to judge, so it does not block).
evaluate() {
  local head_version="$1" base_version="$2" labeled="$3" changed="$4"
  local hits="" file

  while IFS= read -r file; do
    [ -n "$file" ] || continue
    if is_release_path "$file"; then
      hits+="$file"$'\n'
    fi
  done <<< "$changed"

  if [ -z "$hits" ]; then
    echo "pass|no released content changed"
    return 0
  fi
  if [ "$labeled" = "true" ]; then
    echo "pass|labeled ${OVERRIDE_LABEL}"
    return 0
  fi
  if [ -z "$base_version" ]; then
    # Could not read the base version: never block a PR for our own blindness.
    echo "skip|base version unreadable"
    return 0
  fi
  if [ "$head_version" != "$base_version" ]; then
    echo "pass|version bumped ${base_version} -> ${head_version}"
    return 0
  fi

  echo "fail|${head_version}|$(printf '%s' "$hits" | sed '/^$/d' | head -5 | tr '\n' ' ')"
}

verdict_of() { evaluate "$@" | cut -d'|' -f1; }

summary() {
  [ -n "${GITHUB_STEP_SUMMARY:-}" ] || return 0
  printf '%s\n' "$1" >> "$GITHUB_STEP_SUMMARY"
}

self_test() {
  local fails=0 n=0

  check() {
    local desc="$1" expected="$2" got="$3"
    n=$((n + 1))
    if [ "$expected" = "$got" ]; then
      echo "  ok   $desc"
    else
      echo "  FAIL $desc — expected verdict '$expected', got '$got'"
      fails=$((fails + 1))
    fi
  }

  local crew_only docs_only both_github packaging ext
  crew_only=$'crew/sdet.md'
  docs_only=$'README.md\nsite/docs/install/index.html'
  both_github=$'.github/workflows/pages.yml\n.github/scripts/lint-shell.sh'
  ext=$'src/adapters/pi_crew/index.ts\ntests/payload-digests/pi.sha256'
  packaging=$'packaging/pi-shipmates/README.md'

  check "a crew change with no version bump fails" \
    fail "$(verdict_of 0.19.0 0.19.0 false "$crew_only")"
  check "the same crew change passes once the version moves" \
    pass "$(verdict_of 0.19.1 0.19.0 false "$crew_only")"
  check "the pi extension counts as released content" \
    fail "$(verdict_of 0.19.0 0.19.0 false "$ext")"
  check "docs-only work needs no bump" \
    pass "$(verdict_of 0.19.0 0.19.0 false "$docs_only")"
  check "CI-only work needs no bump" \
    pass "$(verdict_of 0.19.0 0.19.0 false "$both_github")"
  check "the no-release label overrides a payload change" \
    pass "$(verdict_of 0.19.0 0.19.0 true "$crew_only")"
  check "an unreadable base never blocks the author" \
    skip "$(verdict_of 0.19.0 '' false "$crew_only")"
  check "the npm packaging README is outside this gate (see PR note)" \
    pass "$(verdict_of 0.19.0 0.19.0 false "$packaging")"

  # The shared list must keep answering for the paths the release trigger uses,
  # or the gate and the trigger quietly stop meaning the same thing.
  check "src/ is release-affecting" 0 "$(is_release_path src/main.rs && echo 0 || echo 1)"
  check "toolbox/ is release-affecting" 0 "$(is_release_path toolbox/shipmates-gh/tool.md && echo 0 || echo 1)"
  check "site/ is not release-affecting" 1 "$(is_release_path site/index.html && echo 0 || echo 1)"

  echo ""
  if [ "$fails" -ne 0 ]; then
    echo "release-bump gate self-test: ${fails} of ${n} case(s) failed"
    return 1
  fi
  echo "release-bump gate self-test: all ${n} cases passed"
}

main() {
  if [ "${1:-}" = "--self-test" ]; then
    self_test
    return $?
  fi

  if [ "${GITHUB_EVENT_NAME:-}" != "pull_request" ]; then
    echo "not a pull_request event — the version-bump gate does not apply"
    return 0
  fi
  : "${PR_NUMBER:?PR_NUMBER is required}"
  : "${BASE_SHA:?BASE_SHA is required}"

  local changed labels head_version base_version result verdict
  if ! changed="$(gh pr diff "$PR_NUMBER" --name-only 2>&1)"; then
    echo "::warning::release-bump gate skipped — 'gh pr diff' failed: $(printf '%s' "$changed" | head -1)"
    summary "### Release-bump gate skipped
The gate could not list this pull request's changed files, so it did not block.
That is deliberate (a tooling error is not the author's mistake), but it means
**nobody checked whether this change needed a version bump**."
    return 0
  fi

  labels="$(gh pr view "$PR_NUMBER" --json labels --jq "[.labels[].name] | any(. == \"${OVERRIDE_LABEL}\")" 2>/dev/null || echo false)"
  head_version="$(sed -n 's/^version = "\([^"]*\)".*/\1/p' Cargo.toml | head -1)"
  base_version="$(gh api -H 'Accept: application/vnd.github.raw+json' \
    "repos/{owner}/{repo}/contents/Cargo.toml?ref=${BASE_SHA}" 2>/dev/null \
    | sed -n 's/^version = "\([^"]*\)".*/\1/p' | head -1)"

  result="$(evaluate "$head_version" "$base_version" "$labels" "$changed")"
  verdict="$(printf '%s' "$result" | cut -d'|' -f1)"

  case "$verdict" in
    pass)
      printf '%s\n' "$(printf '%s' "$result" | cut -d'|' -f2)"
      return 0
      ;;
    skip)
      echo "::warning::release-bump gate skipped — $(printf '%s' "$result" | cut -d'|' -f2)"
      return 0
      ;;
    fail)
      local version files
      version="$(printf '%s' "$result" | cut -d'|' -f2)"
      files="$(printf '%s' "$result" | cut -d'|' -f3)"
      {
        echo "::error::This pull request changes released content but leaves the version at ${version}."
        echo "  Changed: ${files}"
        echo "  The release workflow publishes a version only when it is new, so as it stands"
        echo "  this merges, goes green, and ships nothing."
        echo "  Fix: bump Cargo.toml to the next version and add its CHANGELOG.md entry in this"
        echo "  branch — or add the '${OVERRIDE_LABEL}' label if the bump arrives separately."
      }
      summary "### Version bump required
Released content changed while \`Cargo.toml\` stayed at \`${version}\`:

\`\`\`
${files}
\`\`\`

The release workflow publishes a version only when no release for it exists yet,
so this change would merge, pass every check, and publish nothing. Bump
\`Cargo.toml\` and add its \`CHANGELOG.md\` entry here, or add the
\`**${OVERRIDE_LABEL}**\` label if the bump is landing in a separate branch."
      return 1
      ;;
    *)
      echo "::warning::release-bump gate skipped — unexpected verdict '${verdict}'"
      return 0
      ;;
  esac
}

main "$@"
