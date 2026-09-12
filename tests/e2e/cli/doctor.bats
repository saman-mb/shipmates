#!/usr/bin/env bats
# doctor: health exit codes, drift detection, read-only default, --fix repair.

bats_load_library bats-support
bats_load_library bats-assert
load helpers

@test "a fresh install is healthy" {
  install_claude_code "$SANDBOX"
  run "$SHIPMATES_BIN" doctor --harness claude-code --dir "$SANDBOX"
  assert_success
}

@test "doctor refuses a directory with no install" {
  run "$SHIPMATES_BIN" doctor --harness claude-code --dir "$SANDBOX"
  assert_failure
}

@test "drift is reported without writing, and --fix restores the payload" {
  install_claude_code "$SANDBOX"
  printf 'tampered\n' > "$SANDBOX/.claude/agents/architect.md"

  run "$SHIPMATES_BIN" doctor --harness claude-code --dir "$SANDBOX"
  assert_success
  assert_output --partial "differ"
  [ "$(cat "$SANDBOX/.claude/agents/architect.md")" = "tampered" ]

  run "$SHIPMATES_BIN" doctor --harness claude-code --dir "$SANDBOX" --fix
  assert_success
  [ "$(cat "$SANDBOX/.claude/agents/architect.md")" != "tampered" ]

  install_claude_code "$SANDBOX/fresh"
  cmp "$SANDBOX/fresh/.claude/agents/architect.md" "$SANDBOX/.claude/agents/architect.md"

  run "$SHIPMATES_BIN" doctor --harness claude-code --dir "$SANDBOX"
  assert_success
}

@test "--no-migrate without --fix is rejected" {
  install_claude_code "$SANDBOX"
  run "$SHIPMATES_BIN" doctor --harness claude-code --dir "$SANDBOX" --no-migrate
  assert_failure
}
