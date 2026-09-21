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
