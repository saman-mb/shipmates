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

@test "doctor points at removed tool files instead of a clean no-tools state" {
  run "$SHIPMATES_BIN" install --harness opencode --dir "$SANDBOX" --with-tools all
  assert_success
  # Intentional --with-tools none prunes bak husks (#418). Plant a sidecar so
  # doctor still has evidence of a prior removal (#412).
  printf 'prior\n' > "$SANDBOX/.opencode/tools/shipmates-termgif.ts.bak-1788191317-3827013-0"
  run "$SHIPMATES_BIN" install --harness opencode --dir "$SANDBOX" --with-tools none
  assert_success
  [ "$(find "$SANDBOX/.opencode/tools" -type f ! -name '*.bak-*' | wc -l | tr -d ' ')" -eq 0 ]
  [ "$(find "$SANDBOX/.opencode/tools" -name '*.bak-*' | wc -l | tr -d ' ')" -eq 0 ]
  printf 'prior\n' > "$SANDBOX/.opencode/tools/shipmates-termgif.ts.bak-1788191317-3827013-0"

  run "$SHIPMATES_BIN" doctor --harness opencode --dir "$SANDBOX"
  refute_output --partial "no optional tools installed"
  assert_output --partial "shipmates-termgif"
  assert_output --partial "--with-tools"
}

@test "doctor fails when a receipt-claimed tool file is missing" {
  run "$SHIPMATES_BIN" install --harness opencode --dir "$SANDBOX" --with-tools all
  assert_success
  rm "$SANDBOX/.opencode/tools/shipmates-termgif.ts"

  run "$SHIPMATES_BIN" doctor --harness opencode --dir "$SANDBOX"
  assert_failure
  assert_output --partial "shipmates-termgif"
  assert_output --partial "missing"
}

@test "--no-migrate without --fix is rejected" {
  install_claude_code "$SANDBOX"
  run "$SHIPMATES_BIN" doctor --harness claude-code --dir "$SANDBOX" --no-migrate
  assert_failure
}

@test "doctor --fix --no-migrate skips the rename sweep a plain --fix performs" {
  install_claude_code "$SANDBOX"
  make_previous_generation "$SANDBOX" shipmates-harden ship-harden

  # The sweep is skipped on purpose, so doctor restores the payload but still
  # reports the leftover identity as a problem (non-zero), never greenwashing it.
  run "$SHIPMATES_BIN" doctor --harness claude-code --dir "$SANDBOX" --fix --no-migrate
  assert_failure
  assert_output --partial "leftover superseded"
  [ -d "$SANDBOX/.claude/skills/ship-harden" ]

  run "$SHIPMATES_BIN" doctor --harness claude-code --dir "$SANDBOX" --fix
  assert_success
  [ ! -e "$SANDBOX/.claude/skills/ship-harden" ]
  [ -f "$SANDBOX/.claude/skills/shipmates-harden/SKILL.md" ]

  run "$SHIPMATES_BIN" doctor --harness claude-code --dir "$SANDBOX"
  assert_success
}

@test "doctor --from-cwd diagnoses against the checkout" {
  install_claude_code "$SANDBOX"
  cd "$REPO_ROOT"
  run "$SHIPMATES_BIN" doctor --harness claude-code --dir "$SANDBOX" --from-cwd
  assert_success
}

@test "doctor --local and --global diagnose that root" {
  run "$SHIPMATES_BIN" install --harness claude-code --local --with-tools none
  assert_success
  run "$SHIPMATES_BIN" doctor --harness claude-code --local
  assert_success

  run "$SHIPMATES_BIN" install --harness claude-code --global --with-tools none
  assert_success
  run "$SHIPMATES_BIN" doctor --harness claude-code --global
  assert_success
}
