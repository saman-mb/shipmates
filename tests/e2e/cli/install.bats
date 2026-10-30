#!/usr/bin/env bats
# install: layout, receipt, idempotence, tool selection, collision safety, location flags.

bats_load_library bats-support
bats_load_library bats-assert
load helpers

@test "install writes the full claude-code crew and a receipt" {
  install_claude_code "$SANDBOX"
  [ "$(skill_dirs "$SANDBOX")" -eq 19 ]
  [ "$(agent_files "$SANDBOX")" -eq 13 ]
  [ "$(tool_dirs "$SANDBOX")" -eq 0 ]
  [ "$(receipt_files "$SANDBOX")" -eq 1 ]
  run jq -e '.harness == "claude-code"' "$SANDBOX/.shipmates/receipts/claude-code.json"
  assert_success
}

@test "re-install is idempotent" {
  install_claude_code "$SANDBOX"
  cp "$SANDBOX/.claude/skills/shipmates-ship-issue/SKILL.md" "$BATS_TEST_TMPDIR/before.md"
  run "$SHIPMATES_BIN" install --harness claude-code --dir "$SANDBOX" --with-tools none
  assert_success
  cmp "$BATS_TEST_TMPDIR/before.md" "$SANDBOX/.claude/skills/shipmates-ship-issue/SKILL.md"
  [ "$(skill_dirs "$SANDBOX")" -eq 19 ]
}

@test "omitting --with-tools installs every tool; --with-tools all matches" {
  run "$SHIPMATES_BIN" install --harness claude-code --dir "$SANDBOX"
  assert_success
  [ "$(tool_dirs "$SANDBOX")" -eq 11 ]
  [ "$(skill_dirs "$SANDBOX")" -eq 30 ]
  [ -d "$SANDBOX/.claude/skills/shipmates-termgif" ]

  run "$SHIPMATES_BIN" install --harness claude-code --dir "$SANDBOX/all" --with-tools all
  assert_success
  [ "$(tool_dirs "$SANDBOX/all")" -eq 11 ]
}

@test "omitting --harness installs claude-code non-interactively" {
  run "$SHIPMATES_BIN" install --dir "$SANDBOX" --with-tools none
  assert_success
  [ "$(skill_dirs "$SANDBOX")" -eq 19 ]
  [ -f "$SANDBOX/.shipmates/receipts/claude-code.json" ]
}

@test "--no-migrate leaves a superseded generation name in place" {
  install_claude_code "$SANDBOX"
  make_previous_generation "$SANDBOX" shipmates-harden ship-harden

  run "$SHIPMATES_BIN" install --harness claude-code --dir "$SANDBOX" --with-tools none --no-migrate
  assert_success
  [ -d "$SANDBOX/.claude/skills/ship-harden" ]
}

@test "--with-tools selects a subset and accepts a legacy tool name" {
  run "$SHIPMATES_BIN" install --harness claude-code --dir "$SANDBOX" --with-tools termgif,scrub
  assert_success
  [ "$(tool_dirs "$SANDBOX")" -eq 2 ]
  [ -d "$SANDBOX/.claude/skills/shipmates-termgif" ]
  [ -d "$SANDBOX/.claude/skills/shipmates-scrub" ]
  [ ! -d "$SANDBOX/.claude/skills/shipmates-pixelart" ]
}

@test "a foreign file at a payload path fails closed; --force adopts it with a backup" {
  mkdir -p "$SANDBOX/.claude/skills/shipmates-ship-issue"
  printf 'mine\n' > "$SANDBOX/.claude/skills/shipmates-ship-issue/SKILL.md"

  run "$SHIPMATES_BIN" install --harness claude-code --dir "$SANDBOX" --with-tools none
  assert_failure
  [ "$(cat "$SANDBOX/.claude/skills/shipmates-ship-issue/SKILL.md")" = "mine" ]

  run "$SHIPMATES_BIN" install --harness claude-code --dir "$SANDBOX" --with-tools none --force
  assert_success
  [ "$(cat "$SANDBOX/.claude/skills/shipmates-ship-issue/SKILL.md")" != "mine" ]
  find "$SANDBOX" -name '*.bak-*' | grep -q .
}

@test "--local installs into the working directory and --global into HOME" {
  run "$SHIPMATES_BIN" install --harness claude-code --local --with-tools none
  assert_success
  [ -f "$SANDBOX/.claude/skills/shipmates-ship-issue/SKILL.md" ]

  run "$SHIPMATES_BIN" install --harness claude-code --global --with-tools none
  assert_success
  [ -f "$HOME/.claude/skills/shipmates-ship-issue/SKILL.md" ]
}

@test "--global and --local together are rejected" {
  run "$SHIPMATES_BIN" install --harness claude-code --global --local
  assert_failure
}

@test "--harness all installs every harness with one receipt each" {
  run "$SHIPMATES_BIN" install --harness all --dir "$SANDBOX" --with-tools none
  assert_success
  [ "$(receipt_files "$SANDBOX")" -eq 9 ]
}

@test "--from-cwd installs from the contributor checkout" {
  cd "$REPO_ROOT"
  run "$SHIPMATES_BIN" install --harness claude-code --dir "$SANDBOX" --with-tools none --from-cwd
  assert_success
  [ "$(skill_dirs "$SANDBOX")" -eq 19 ]
}

@test "superseded home-scope tree repair is skipped by --no-migrate and retired safely by doctor --fix" {
  mkdir -p "$HOME/.agents/skills/shipmates-badge"
  mkdir -p "$HOME/.agents/skills/shipmates-scrub"
  mkdir -p "$HOME/.agents/skills/ship-harden"

  printf 'external\n' > "$BATS_TEST_TMPDIR/external-badge.py"
  ln -s "$BATS_TEST_TMPDIR/external-badge.py" "$HOME/.agents/skills/shipmates-badge/badge.py"

  cat <<'EOF' > "$HOME/.agents/skills/shipmates-badge/SKILL.md"
---
name: shipmates-badge
description: "Shipmates: Render a shields-style status badge"
---
badge body
EOF

  cat <<'EOF' > "$HOME/.agents/skills/shipmates-scrub/SKILL.md"
---
name: shipmates-scrub
description: "My custom local skill"
---
scrub body
EOF

  cat <<'EOF' > "$HOME/.agents/skills/ship-harden/SKILL.md"
---
name: ship-harden
description: "Shipmates: retired skill"
---
retired body
EOF

  # Step A: install --no-migrate skips the superseded home-tree repair
  run "$SHIPMATES_BIN" install --harness claude-code --global --with-tools none --no-migrate
  assert_success
  assert_output --partial "Skipped the superseded home-tree repair (--no-migrate)"
  [ -f "$HOME/.agents/skills/shipmates-badge/SKILL.md" ]

  # Step B: doctor --fix retires provably-ours manifests and preserves foreign/linked/retired entries
  run "$SHIPMATES_BIN" doctor --harness claude-code --global --fix
  assert_success
  assert_output --partial "Retired 1 superseded home-tree payload file(s)"
  assert_output --partial "Superseded skill tree"
  assert_output --partial "Mostly shipshape — see the warnings above"

  # The provably-ours manifest was retired into a backup
  [ ! -f "$HOME/.agents/skills/shipmates-badge/SKILL.md" ]
  [ -d "$HOME/.shipmates-backup" ]
  find "$HOME/.shipmates-backup" -name "SKILL.md" | grep -q .

  # Symlinked sibling, foreign entry, and retired entry are preserved untouched
  [ -L "$HOME/.agents/skills/shipmates-badge/badge.py" ]
  [ -f "$HOME/.agents/skills/shipmates-scrub/SKILL.md" ]
  [ -f "$HOME/.agents/skills/ship-harden/SKILL.md" ]
}

