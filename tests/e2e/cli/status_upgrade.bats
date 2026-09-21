#!/usr/bin/env bats
# status / upgrade: index-backed status reporting and the three-way version
# check plus the upgrade/audit/file-bugs path. Every test is hermetic: throwaway
# HOME (helpers.bash), a stubbed SHIPMATES_CURL for the release feed, and a fake
# `gh` for the filing path — no real network, no real HOME.

bats_load_library bats-support
bats_load_library bats-assert
load helpers

# --- test-local helpers -----------------------------------------------------

# The version number this binary reports, without the leading "shipmates ".
shipmates_version() {
  "$SHIPMATES_BIN" --version | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' | head -1
}

# Write an executable stub that prints the given release-feed JSON to stdout.
write_release_feed() {
  local script="$1" body="$2"
  cat > "$script" <<EOF
#!/usr/bin/env bash
printf '%s\n' '$body'
EOF
  chmod +x "$script"
}

# Write an executable stub curl that fails noisily (both streams), standing in
# for an offline release check.
write_failing_curl() {
  local script="$1"
  cat > "$script" <<'EOF'
#!/usr/bin/env bash
echo "GARBAGE ON STDOUT"
echo "garbage on stderr" >&2
exit 1
EOF
  chmod +x "$script"
}

# A fake `gh` that logs its argv (plus any --body-file contents) to <dir>/gh.log
# and answers `issue list` / `issue create` / `issue comment`. When
# <dir>/match-mode exists, `issue list` returns an open issue whose body carries
# the fingerprint being searched for, so the next run comments instead of filing.
write_fake_gh() {
  local dir="$1"
  cat > "$dir/gh" <<'EOF'
#!/usr/bin/env bash
LOG="$(dirname "$0")/gh.log"
echo "ARGS: $*" >> "$LOG"
prev=""
for a in "$@"; do
  if [ "$prev" = "--body-file" ]; then
    echo "BODY:" >> "$LOG"
    cat "$a" >> "$LOG"
    echo >> "$LOG"
  fi
  prev="$a"
done
if [ "$1" = "issue" ] && [ "$2" = "list" ]; then
  if [ -f "$(dirname "$0")/match-mode" ]; then
    fp=""
    prev=""
    for a in "$@"; do
      if [ "$prev" = "--search" ]; then fp="${a% in:body}"; fi
      prev="$a"
    done
    printf '[{"number":42,"url":"https://example.invalid/issues/42","body":"<!-- shipmates-fingerprint: %s -->"}]' "$fp"
  else
    printf '[]'
  fi
fi
if [ "$1" = "issue" ] && [ "$2" = "create" ]; then
  printf 'https://example.invalid/issues/1'
fi
if [ "$1" = "issue" ] && [ "$2" = "comment" ]; then
  printf 'https://example.invalid/issues/42#issuecomment-1'
fi
exit 0
EOF
  chmod +x "$dir/gh"
}

# --- status -----------------------------------------------------------------

@test "status --json on a fresh HOME reports an empty, valid report" {
  run "$SHIPMATES_BIN" status --json
  assert_success
  local json="$output"
  run jq -e '.installs == [] and .unmanaged == [] and .pruned == []' <<< "$json"
  assert_success
  run jq -r '.shipmates_version' <<< "$json"
  assert_success
  [ "$output" = "$(shipmates_version)" ]
}

@test "status --json lists a registered install and creates the index" {
  install_claude_code "$SANDBOX/app"
  run "$SHIPMATES_BIN" status --json
  assert_success
  run jq -e --arg v "$(shipmates_version)" \
    '.installs | length == 1
      and .[0].harness == "claude-code"
      and .[0].managed == true
      and .[0].receipt_version == $v
      and .[0].state == "ok"
      and .[0].drift_count == 0' <<< "$output"
  assert_success
  [ -f "$HOME/.shipmates/installs.json" ]
}

@test "status lists two roots and prunes a deleted one without erroring" {
  install_claude_code "$SANDBOX/a"
  install_claude_code "$SANDBOX/b"

  run "$SHIPMATES_BIN" status --json
  assert_success
  local json="$output"
  run jq -e '.installs | length == 2' <<< "$json"
  assert_success

  rm -rf "$SANDBOX/a"
  run "$SHIPMATES_BIN" status --json
  assert_success
  json="$output"
  run jq -e '.installs | length == 1' <<< "$json"
  assert_success
  run jq -e '.pruned | length == 1 and .[0].reason == "missing"' <<< "$json"
  assert_success
}

@test "status --dir on a missing root reports it unmanaged (no-receipt)" {
  run "$SHIPMATES_BIN" status --dir "$SANDBOX/nope" --json
  assert_success
  run jq -e '.unmanaged | length == 1 and .[0].reason == "no-receipt"' <<< "$output"
  assert_success
}

# --- upgrade --check --------------------------------------------------------

@test "upgrade --check reports an available upgrade (exit 10)" {
  local feed="$BATS_TEST_TMPDIR/feed-newer"
  write_release_feed "$feed" '[{"draft":false,"prerelease":false,"tag_name":"v99.0.0","html_url":"https://example.invalid"}]'
  export SHIPMATES_CURL="$feed"

  run "$SHIPMATES_BIN" upgrade --check --json
  assert_failure 10
  run jq -e '.upgrade_available == true and .latest_release == "99.0.0"' <<< "$output"
  assert_success
}

@test "upgrade --check on a newer running version does not downgrade (exit 0)" {
  local feed="$BATS_TEST_TMPDIR/feed-older"
  write_release_feed "$feed" '[{"draft":false,"prerelease":false,"tag_name":"v0.1.0","html_url":"https://example.invalid"}]'
  export SHIPMATES_CURL="$feed"

  run "$SHIPMATES_BIN" upgrade --check --json
  assert_success
  run jq -e '.running_is_newer == true and .upgrade_available == false and .latest_release == "0.1.0"' <<< "$output"
  assert_success
}

@test "upgrade --check skips prereleases unless --pre" {
  local feed="$BATS_TEST_TMPDIR/feed-pre"
  write_release_feed "$feed" '[{"draft":false,"prerelease":true,"tag_name":"v99.0.0-rc.1","html_url":"https://example.invalid"}]'
  export SHIPMATES_CURL="$feed"

  run "$SHIPMATES_BIN" upgrade --check --json
  assert_failure 11
  run jq -e '.unknown == true and .latest_release == null' <<< "$output"
  assert_success

  run "$SHIPMATES_BIN" upgrade --check --pre --json
  assert_failure 10
  run jq -e '.upgrade_available == true and .latest_release == "99.0.0-rc.1" and .pre == true' <<< "$output"
  assert_success
}

@test "upgrade --check offline reports unknown (exit 11) with clean JSON" {
  local feed="$BATS_TEST_TMPDIR/failing-curl"
  write_failing_curl "$feed"
  export SHIPMATES_CURL="$feed"

  run "$SHIPMATES_BIN" upgrade --check --json
  assert_failure 11
  run jq -e '.unknown == true and .latest_release == null' <<< "$output"
  assert_success
}

# --- upgrade (self / usage / corruption / filing) ---------------------------

@test "upgrade --self --dry-run never executes and names the channel" {
  local feed="$BATS_TEST_TMPDIR/feed-newer"
  write_release_feed "$feed" '[{"draft":false,"prerelease":false,"tag_name":"v99.0.0","html_url":"https://example.invalid"}]'
  export SHIPMATES_CURL="$feed"

  run "$SHIPMATES_BIN" upgrade --self --dry-run
  assert_success
  # Source refuses ("refusing to self-upgrade a source build"), Cargo refuses
  # ("cannot self-upgrade a cargo install"), Brew/CargoDist print "would run:
  # <command>", Unknown names "channel". Any of those proves it printed instead
  # of executing.
  assert_output --regexp 'source|cargo|brew|channel'
}

@test "upgrade --check conflicts with --fix, --file-bugs, and --self (exit 2)" {
  run "$SHIPMATES_BIN" upgrade --check --fix
  assert_failure 2
  run "$SHIPMATES_BIN" upgrade --check --file-bugs
  assert_failure 2
  run "$SHIPMATES_BIN" upgrade --check --self
  assert_failure 2
}

@test "upgrade --check tolerates a corrupt receipt without erroring" {
  install_claude_code "$SANDBOX/app"
  printf 'not valid json {{{\n' > "$SANDBOX/app/.shipmates/receipts/claude-code.json"

  local feed="$BATS_TEST_TMPDIR/feed-older"
  write_release_feed "$feed" '[{"draft":false,"prerelease":false,"tag_name":"v0.1.0","html_url":"https://example.invalid"}]'
  export SHIPMATES_CURL="$feed"

  run "$SHIPMATES_BIN" upgrade --check --json --dir "$SANDBOX/app"
  assert_success
  run jq -e '.installs[0].state == "corrupt-receipt"' <<< "$output"
  assert_success
}

@test "upgrade --file-bugs files one deduped issue then comments on a match" {
  local bindir="$BATS_TEST_TMPDIR/bin"
  mkdir -p "$bindir"
  write_fake_gh "$bindir"

  local feed="$BATS_TEST_TMPDIR/feed-older"
  write_release_feed "$feed" '[{"draft":false,"prerelease":false,"tag_name":"v0.1.0","html_url":"https://example.invalid"}]'

  # A real, refresh-surviving finding: shipmates-svgflow is a deprecated
  # forwarder whose `--help` exits 1 when the `diagram` tool it forwards to is
  # not installed, so the Full-depth tool smoke fails even after refresh.
  run "$SHIPMATES_BIN" install --harness claude-code --dir "$SANDBOX/app" --with-tools svgflow
  assert_success

  export SHIPMATES_CURL="$feed"
  export PATH="$bindir:$PATH"

  run "$SHIPMATES_BIN" upgrade --file-bugs --dir "$SANDBOX/app" --json
  # The run filed the findings, so exit 5 (bugs filed) — a handled finding
  # outranks the bare finding code 4; only a failed refresh (3) outranks it.
  assert_failure 5

  run grep -c 'issue create' "$bindir/gh.log"
  assert_success
  assert_output "1"

  run grep -qF '<!-- shipmates-fingerprint:' "$bindir/gh.log"
  assert_success
  if grep -qF "$HOME" "$bindir/gh.log"; then
    fail "gh body leaked the home path"
  fi

  # A second run whose fake `gh issue list` returns the matching body comments
  # instead of creating a second issue.
  touch "$bindir/match-mode"
  run "$SHIPMATES_BIN" upgrade --file-bugs --dir "$SANDBOX/app" --json
  # Commenting on the existing fingerprint match also counts as handled → 5.
  assert_failure 5

  run grep -c 'issue create' "$bindir/gh.log"
  assert_success
  assert_output "1"
  run grep -c 'issue comment' "$bindir/gh.log"
  assert_success
  assert_output "1"
}

# --- refresh path ----------------------------------------------------------

@test "upgrade --json refreshes a multi-harness root without prompting (no hang)" {
  install_claude_code "$SANDBOX/app"
  run "$SHIPMATES_BIN" install --harness opencode --dir "$SANDBOX/app" --with-tools none
  assert_success

  local feed="$BATS_TEST_TMPDIR/feed-older"
  write_release_feed "$feed" '[{"draft":false,"prerelease":false,"tag_name":"v0.1.0","html_url":"https://example.invalid"}]'
  export SHIPMATES_CURL="$feed"

  run "$SHIPMATES_BIN" upgrade --json --dir "$SANDBOX/app"
  assert_success
  local json="$output"
  run jq -e '.installs | length == 2' <<< "$json"
  assert_success
  run jq -e '.installs | map(.harness) | sort == ["claude-code", "opencode"]' <<< "$json"
  assert_success
}

@test "upgrade --dry-run reports pruned roots without persisting them" {
  install_claude_code "$SANDBOX/app"
  local index="$HOME/.shipmates/installs.json"
  [ -f "$index" ]
  rm -rf "$SANDBOX/app"

  local feed="$BATS_TEST_TMPDIR/feed-older"
  write_release_feed "$feed" '[{"draft":false,"prerelease":false,"tag_name":"v0.1.0","html_url":"https://example.invalid"}]'
  export SHIPMATES_CURL="$feed"

  run "$SHIPMATES_BIN" upgrade --dry-run --json
  assert_success
  run jq -e '.pruned | length == 1 and .[0].reason == "missing"' <<< "$output"
  assert_success
  # The dead record survives on disk: --dry-run reports, never persists.
  run jq -e '.records | length == 1' "$index"
  assert_success
}

@test "upgrade --json reports and continues on a read-only root" {
  install_claude_code "$SANDBOX/app"
  chmod 0555 "$SANDBOX/app"

  # Where 0555 is a no-op (root user, or a filesystem that ignores modes) the
  # assertion below cannot hold, so skip instead of failing on the environment.
  if touch "$SANDBOX/app/.write-probe" 2>/dev/null; then
    rm -f "$SANDBOX/app/.write-probe"
    skip "chmod is a no-op on this platform/path"
  fi

  local feed="$BATS_TEST_TMPDIR/feed-older"
  write_release_feed "$feed" '[{"draft":false,"prerelease":false,"tag_name":"v0.1.0","html_url":"https://example.invalid"}]'
  export SHIPMATES_CURL="$feed"

  run "$SHIPMATES_BIN" upgrade --json --dir "$SANDBOX/app"
  # Never a hard abort (exit 1): the refresh either succeeds (0) or its failure
  # is recorded as a finding and the run continues (3).
  [ "$status" -eq 0 ] || [ "$status" -eq 3 ]
  run jq -e '.installs | length >= 1' <<< "$output"
  assert_success

  # Restore write permission so the throwaway sandbox can be cleaned up.
  chmod 0755 "$SANDBOX/app"
}
