#!/usr/bin/env bats
# Contributor CLI: build / check against a copied source root, so the checkout
# is never written to.

bats_load_library bats-support
bats_load_library bats-assert
load helpers

copy_root() {
  local dest="$1"
  mkdir -p "$dest"
  git -C "$REPO_ROOT" archive HEAD | tar -x -C "$dest"
}

@test "build writes the payload under --out" {
  local copy="$BATS_TEST_TMPDIR/root" out="$BATS_TEST_TMPDIR/out"
  copy_root "$copy"
  run "$SHIPMATES_BIN" build --target claude-code --root "$copy" --out "$out"
  assert_success
  [ -f "$out/harnesses/claude-code/.claude/skills/shipmates-ship-issue/SKILL.md" ]
  [ ! -e "$copy/harnesses" ]
}

@test "build --with-tools controls whether the toolbox joins the written tree" {
  local copy="$BATS_TEST_TMPDIR/root" plain="$BATS_TEST_TMPDIR/plain" full="$BATS_TEST_TMPDIR/full"
  local subset="$BATS_TEST_TMPDIR/subset" none="$BATS_TEST_TMPDIR/none"
  copy_root "$copy"

  # Omitted means none — unlike `install`. The default written tree must stay
  # byte-for-byte the committed digest, which is what the digest gate compares.
  run "$SHIPMATES_BIN" build --target pi --root "$copy" --out "$plain"
  assert_success
  [ -f "$plain/harnesses/pi/.pi/skills/shipmates-ship-issue/SKILL.md" ]
  [ ! -e "$plain/harnesses/pi/.pi/skills/shipmates-diagram" ]

  # all ships the toolbox beside the commands — the tree the npm packager packs.
  run "$SHIPMATES_BIN" build --target pi --root "$copy" --out "$full" --with-tools all
  assert_success
  [ -f "$full/harnesses/pi/.pi/skills/shipmates-ship-issue/SKILL.md" ]
  [ -f "$full/harnesses/pi/.pi/skills/shipmates-diagram/SKILL.md" ]
  [ ! -e "$full/harnesses/pi/.shipmates/receipts" ]
  local ext_count
  ext_count="$(find "$full/harnesses/pi/.pi/extensions" -name index.ts | wc -l | tr -d ' ')"
  [ "$ext_count" = "1" ]

  # A named subset ships that tool only, and `none` ships none explicitly.
  run "$SHIPMATES_BIN" build --target pi --root "$copy" --out "$subset" --with-tools scrub
  assert_success
  [ -f "$subset/harnesses/pi/.pi/skills/shipmates-scrub/SKILL.md" ]
  [ ! -e "$subset/harnesses/pi/.pi/skills/shipmates-diagram" ]

  run "$SHIPMATES_BIN" build --target pi --root "$copy" --out "$none" --with-tools none
  assert_success
  [ ! -e "$none/harnesses/pi/.pi/skills/shipmates-diagram" ]
}

@test "check passes on a clean copy and fails on a tampered source" {
  local copy="$BATS_TEST_TMPDIR/root"
  copy_root "$copy"

  run "$SHIPMATES_BIN" check --target claude-code --root "$copy"
  assert_success

  run "$SHIPMATES_BIN" build --target claude-code --root "$copy" --check
  assert_success
  [ ! -e "$copy/harnesses" ]

  printf '\ntampered\n' >> "$copy/commands/shipmates-ship-issue.md"
  run "$SHIPMATES_BIN" check --target claude-code --root "$copy"
  assert_failure
}

@test "build --update refreshes the copied digests" {
  local copy="$BATS_TEST_TMPDIR/root"
  copy_root "$copy"
  printf '\ntampered\n' >> "$copy/commands/shipmates-ship-issue.md"

  run "$SHIPMATES_BIN" build --target claude-code --root "$copy" --update
  assert_success
  run "$SHIPMATES_BIN" check --target claude-code --root "$copy"
  assert_success
}

@test "an invalid target fails" {
  run "$SHIPMATES_BIN" check --target nope --root "$REPO_ROOT"
  assert_failure
}

@test "the suite never writes into the contributor checkout" {
  run git -C "$REPO_ROOT" status --porcelain
  assert_success
  assert_output ""
}
