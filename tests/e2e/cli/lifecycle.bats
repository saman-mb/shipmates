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
  # Intentional tool drop must not leave bak sidecars or empty husk dirs —
  # those would look like an interrupted update to doctor forever (#418).
  [ "$(tool_files "$SANDBOX")" -eq 0 ]
  [ "$(find "$SANDBOX/.claude/skills" -path '*shipmates-*' -name '*.bak-*' | wc -l | tr -d ' ')" -eq 0 ]
  [ "$(find "$SANDBOX/.claude/skills" -mindepth 1 -maxdepth 1 -type d -name 'shipmates-*' | wc -l | tr -d ' ')" -eq 0 ]
  [ "$(find "$SANDBOX/.claude/skills" -mindepth 2 -maxdepth 2 -name 'SKILL.md' ! -name '*.bak-*' | wc -l | tr -d ' ')" -eq 17 ]
}

@test "update without --with-tools keeps opencode native tools and their receipt claims" {
  run "$SHIPMATES_BIN" install --harness opencode --dir "$SANDBOX" --with-tools all
  assert_success
  local before
  before="$(find "$SANDBOX/.opencode/tools" -type f ! -name '*.bak-*' | wc -l | tr -d ' ')"
  [ "$before" -ge 11 ]

  run "$SHIPMATES_BIN" update --harness opencode --dir "$SANDBOX"
  assert_success
  refute_output --partial "Removed dropped file"
  [ "$(find "$SANDBOX/.opencode/tools" -type f ! -name '*.bak-*' | wc -l | tr -d ' ')" -eq "$before" ]
  [ "$(find "$SANDBOX/.opencode/tools" -name '*.bak-*' | wc -l | tr -d ' ')" -eq 0 ]
  run jq -e '[.files[].path] | index(".opencode/tools/shipmates-termgif.ts") != null' "$SANDBOX/.shipmates/receipts/opencode.json"
  assert_success
}

@test "update advances a drifted shared .agents skill across sibling receipts" {
  local harness
  for harness in codex antigravity github-copilot; do
    run "$SHIPMATES_BIN" install --harness "$harness" --dir "$SANDBOX" --with-tools all
    assert_success
  done
  stale_shared_skill "$SANDBOX" shipmates-issue

  for harness in codex antigravity github-copilot; do
    run "$SHIPMATES_BIN" update --harness "$harness" --dir "$SANDBOX"
    assert_success
    refute_output --partial "shared-managed file left untouched"
    run jq -e '[.files[].path] | index(".agents/skills/shipmates-issue/SKILL.md") != null' "$SANDBOX/.shipmates/receipts/$harness.json"
    assert_success
  done

  run "$SHIPMATES_BIN" install --harness codex --dir "$BATS_TEST_TMPDIR/fresh" --with-tools all
  assert_success
  cmp "$SANDBOX/.agents/skills/shipmates-issue/SKILL.md" "$BATS_TEST_TMPDIR/fresh/.agents/skills/shipmates-issue/SKILL.md"
}

@test "update --harness all refreshes every receipt non-interactively" {
  install_claude_code "$SANDBOX"
  run "$SHIPMATES_BIN" install --harness opencode --dir "$SANDBOX" --with-tools all
  assert_success
  local before
  before="$(find "$SANDBOX/.opencode/tools" -type f ! -name '*.bak-*' | wc -l | tr -d ' ')"

  run "$SHIPMATES_BIN" update --harness all --dir "$SANDBOX"
  assert_success
  assert_output --partial "claude-code"
  assert_output --partial "opencode"
  refute_output --partial "Removed dropped file"
  [ "$(find "$SANDBOX/.opencode/tools" -type f ! -name '*.bak-*' | wc -l | tr -d ' ')" -eq "$before" ]
}

@test "uninstall removes receipt-owned files and keeps the captain's own" {
  install_claude_code "$SANDBOX"
  printf 'mine\n' > "$SANDBOX/.claude/agents/notes.md"

  run "$SHIPMATES_BIN" uninstall --harness claude-code --dir "$SANDBOX"
  assert_success
  [ -f "$SANDBOX/.claude/agents/notes.md" ]
  [ "$(cat "$SANDBOX/.claude/agents/notes.md")" = "mine" ]
  [ ! -e "$SANDBOX/.claude/skills/shipmates-issue" ]
  [ "$(receipt_files "$SANDBOX")" -eq 0 ]
}

@test "uninstall without a receipt fails" {
  run "$SHIPMATES_BIN" uninstall --harness claude-code --dir "$SANDBOX"
  assert_failure
}

@test "update migrates the previous ship- prefix in a claimed tree" {
  install_claude_code "$SANDBOX"
  make_previous_generation "$SANDBOX" shipmates-harden ship-harden

  run "$SHIPMATES_BIN" update --harness claude-code --dir "$SANDBOX"
  assert_success

  [ ! -e "$SANDBOX/.claude/skills/ship-harden" ]
  [ -f "$SANDBOX/.claude/skills/shipmates-harden/SKILL.md" ]
  run jq -e '[.files[].path] | index(".claude/skills/shipmates-harden/SKILL.md") != null' "$SANDBOX/.shipmates/receipts/claude-code.json"
  assert_success
  run bash -c "find '$SANDBOX/.shipmates-backup' -name 'SKILL.md' | head -1"
  assert_success
  [ -n "$output" ]
}

@test "update migrates the pre-prefix bare verb in a claimed tree" {
  install_claude_code "$SANDBOX"
  make_previous_generation "$SANDBOX" shipmates-harden harden

  run "$SHIPMATES_BIN" update --harness claude-code --dir "$SANDBOX"
  assert_success

  [ ! -e "$SANDBOX/.claude/skills/harden" ]
  [ -f "$SANDBOX/.claude/skills/shipmates-harden/SKILL.md" ]
}

# Same rename sweep as the two claude-code tests above, but for every other
# supported harness — proves the migration isn't claude-code-only: an old
# identity on disk is deleted (never left alongside the new one) and the
# current shipmates- name is installed, for both retired generations.
@test "update migrates a previous name generation in a claimed tree, for every other harness" {
  local harness dir old new
  for harness in opencode antigravity codex cursor github-copilot pi windsurf; do
    for old in ship-harden harden; do
      dir="$BATS_TEST_TMPDIR/rename-$harness-$old"
      run "$SHIPMATES_BIN" install --harness "$harness" --dir "$dir" --with-tools none
      assert_success

      make_previous_generation "$dir" shipmates-harden "$old" "$harness"
      new="$(harness_skill_path "$harness" shipmates-harden)"

      run "$SHIPMATES_BIN" update --harness "$harness" --dir "$dir"
      assert_success

      [ ! -e "$dir/$(harness_skill_path "$harness" "$old")" ]
      if [ "$harness" = opencode ]; then
        [ -f "$dir/$new" ]
      else
        [ -f "$dir/$new/SKILL.md" ]
      fi
      run jq -e --arg new "$new" '[.files[].path] | any(startswith($new))' "$dir/.shipmates/receipts/$harness.json"
      assert_success
    done
  done
}

@test "update --from-cwd refreshes from the checkout" {
  install_claude_code "$SANDBOX"
  cd "$REPO_ROOT"
  run "$SHIPMATES_BIN" update --harness claude-code --dir "$SANDBOX" --from-cwd
  assert_success
}

@test "update --local and --global refresh the receipt in that root" {
  run "$SHIPMATES_BIN" install --harness claude-code --local --with-tools none
  assert_success
  run "$SHIPMATES_BIN" update --local
  assert_success

  run "$SHIPMATES_BIN" install --harness claude-code --global --with-tools none
  assert_success
  run "$SHIPMATES_BIN" update --global
  assert_success
}

@test "uninstall --local and --global remove that root's receipt-owned files" {
  run "$SHIPMATES_BIN" install --harness claude-code --local --with-tools none
  assert_success
  run "$SHIPMATES_BIN" uninstall --harness claude-code --local
  assert_success
  [ ! -e "$SANDBOX/.claude/skills/shipmates-issue" ]

  run "$SHIPMATES_BIN" install --harness claude-code --global --with-tools none
  assert_success
  run "$SHIPMATES_BIN" uninstall --harness claude-code --global
  assert_success
  [ ! -e "$HOME/.claude/skills/shipmates-issue" ]
}

@test "uninstall --from-cwd removes the install and uninstall without --harness is ambiguous" {
  install_claude_code "$SANDBOX"
  run "$SHIPMATES_BIN" install --harness opencode --dir "$SANDBOX" --with-tools none
  assert_success

  run "$SHIPMATES_BIN" uninstall --dir "$SANDBOX"
  assert_failure

  cd "$REPO_ROOT"
  run "$SHIPMATES_BIN" uninstall --harness claude-code --dir "$SANDBOX" --from-cwd
  assert_success
  [ ! -e "$SANDBOX/.claude/skills/shipmates-issue" ]
}

@test "an installed tool runs from its installed location" {
  run "$SHIPMATES_BIN" install --harness claude-code --dir "$SANDBOX" --with-tools scrub
  assert_success
  local tool="$SANDBOX/.claude/skills/shipmates-scrub/scrub.py"
  printf 'api_key=abc123xyz\nmail dev@example.com\n' > "$BATS_TEST_TMPDIR/in.txt"

  run python3 "$tool" --in "$BATS_TEST_TMPDIR/in.txt"
  assert_success
  assert_output --partial "[REDACTED_TOKEN]"
  assert_output --partial "[REDACTED_EMAIL]"
}
