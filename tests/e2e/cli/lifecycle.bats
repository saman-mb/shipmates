#!/usr/bin/env bats
# update / uninstall / identity migration — the lifecycle a captain runs after
# upgrading the binary.

bats_load_library bats-support
bats_load_library bats-assert
load helpers

@test "update without a receipt fails" {
  run "$SHIPMATES_BIN" update --harness claude-code --dir "$SANDBOX"
  assert_failure
}

@test "update restores a removed payload file" {
  install_claude_code "$SANDBOX"
  rm "$SANDBOX/.claude/agents/architect.md"

  run "$SHIPMATES_BIN" update --harness claude-code --dir "$SANDBOX" --no-migrate
  assert_success
  [ -f "$SANDBOX/.claude/agents/architect.md" ]
}

@test "update --with-tools none removes the live tools the receipt claimed" {
  run "$SHIPMATES_BIN" install --harness claude-code --dir "$SANDBOX" --with-tools all
  assert_success
  [ "$(tool_dirs "$SANDBOX")" -eq 11 ]

  run "$SHIPMATES_BIN" update --harness claude-code --dir "$SANDBOX" --with-tools none
  assert_success
  # The removed payload is backed up as a sidecar (the undo a captain can ask
  # for), but no live tool file remains.
  [ "$(find "$SANDBOX/.claude/skills" -path '*shipmates-*' -type f ! -name '*.bak-*' | wc -l | tr -d ' ')" -eq 0 ]
  [ "$(find "$SANDBOX/.claude/skills" -mindepth 2 -maxdepth 2 -name 'SKILL.md' ! -name '*.bak-*' | wc -l | tr -d ' ')" -eq 15 ]
}

@test "uninstall removes receipt-owned files and keeps the captain's own" {
  install_claude_code "$SANDBOX"
  printf 'mine\n' > "$SANDBOX/.claude/agents/notes.md"

  run "$SHIPMATES_BIN" uninstall --harness claude-code --dir "$SANDBOX"
  assert_success
  [ -f "$SANDBOX/.claude/agents/notes.md" ]
  [ "$(cat "$SANDBOX/.claude/agents/notes.md")" = "mine" ]
  [ ! -e "$SANDBOX/.claude/skills/ship-issue" ]
  [ "$(receipt_files "$SANDBOX")" -eq 0 ]
}

@test "uninstall without a receipt fails" {
  run "$SHIPMATES_BIN" uninstall --harness claude-code --dir "$SANDBOX"
  assert_failure
}

@test "update migrates the previous shipmates- prefix in a claimed tree" {
  install_claude_code "$SANDBOX"
  local receipt="$SANDBOX/.shipmates/receipts/claude-code.json"

  mv "$SANDBOX/.claude/skills/ship-harden" "$SANDBOX/.claude/skills/shipmates-harden"
  jq --arg old ".claude/skills/ship-harden/" --arg new ".claude/skills/shipmates-harden/" \
    '(.files[] | select(.path | startswith($old)) | .path) |= sub($old; $new) | .files |= sort_by(.path)' \
    "$receipt" > "$receipt.tmp"
  mv "$receipt.tmp" "$receipt"

  run "$SHIPMATES_BIN" update --harness claude-code --dir "$SANDBOX"
  assert_success

  [ ! -e "$SANDBOX/.claude/skills/shipmates-harden" ]
  [ -f "$SANDBOX/.claude/skills/ship-harden/SKILL.md" ]
  run jq -e '[.files[].path] | index(".claude/skills/ship-harden/SKILL.md") != null' "$receipt"
  assert_success
  run bash -c "find '$SANDBOX/.shipmates-backup' -name 'SKILL.md' | head -1"
  assert_success
  [ -n "$output" ]
}
