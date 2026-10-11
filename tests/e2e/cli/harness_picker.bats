#!/usr/bin/env bats
# install: the harness picker. Without --harness the real binary prompts on a
# terminal, so this is the only test that covers the prompt path — a non-TTY run
# would silently install claude-code, which is why the chosen harness differs.

bats_load_library bats-support
bats_load_library bats-assert
load helpers

@test "omitting --harness prompts on a PTY and installs the numbered choice" {
  run python3 "$REPO_ROOT/tests/e2e/cli/pty_install.py" --target "$SANDBOX" --harness opencode
  assert_success
  [ -f "$SANDBOX/.opencode/commands/shipmates-ship-issue.md" ]
  [ "$(receipt_files "$SANDBOX")" -eq 1 ]
  run jq -e '.harness == "opencode"' "$SANDBOX/.shipmates/receipts/opencode.json"
  assert_success
}
