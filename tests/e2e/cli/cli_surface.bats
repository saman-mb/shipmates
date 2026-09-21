#!/usr/bin/env bats
# CLI surface: version, help, targets, and the failure paths of unknown input.

bats_load_library bats-support
bats_load_library bats-assert
load helpers

@test "--version reports the version in Cargo.toml" {
  run "$SHIPMATES_BIN" --version
  assert_success
  local want
  want="$(grep -m1 '^version' "$REPO_ROOT/Cargo.toml" | cut -d'"' -f2)"
  assert_output --partial "$want"
}

@test "top-level help names every subcommand" {
  run "$SHIPMATES_BIN" --help
  assert_success
  local cmd
  for cmd in install update uninstall doctor targets build check; do
    assert_output --partial "$cmd"
  done
}

@test "every subcommand has a help page that exits zero" {
  local cmd
  for cmd in install update uninstall doctor targets build check; do
    run "$SHIPMATES_BIN" help "$cmd"
    assert_success
  done
}

@test "an unknown subcommand fails with a usage error" {
  run "$SHIPMATES_BIN" frobnicate
  assert_failure
  assert_output --partial "frobnicate"
}

@test "an unknown flag fails with a usage error" {
  run "$SHIPMATES_BIN" targets --bogus
  assert_failure
  assert_output --partial "--bogus"
}

@test "targets lists exactly the harness set this binary supports" {
  run "$SHIPMATES_BIN" targets
  assert_success
  local harness
  for harness in claude-code opencode antigravity codex cursor github-copilot pi grok-build devin; do
    assert_output --partial "$harness"
  done
  [ "$(printf '%s\n' "$output" | wc -w | tr -d ' ')" -eq 9 ]
}

@test "an invalid harness name fails and names the value" {
  run "$SHIPMATES_BIN" install --harness nope --dir "$SANDBOX"
  assert_failure
  assert_output --partial "nope"
}
