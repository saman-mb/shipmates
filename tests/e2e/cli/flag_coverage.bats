#!/usr/bin/env bats
# Drift guard: every flag a subcommand documents must be exercised somewhere in
# this suite. A new flag with no test fails here until it is covered.

bats_load_library bats-support
bats_load_library bats-assert
load helpers

@test "every documented subcommand flag is exercised by the suite" {
  local suite_dir
  suite_dir="$(dirname "$BATS_TEST_FILENAME")"
  local missing=()
  local cmd flags flag

  for cmd in install update uninstall doctor build check; do
    run "$SHIPMATES_BIN" help "$cmd"
    assert_success
    flags="$(printf '%s\n' "$output" | grep -oE -- '--[a-z][a-z-]+' | sort -u)"
    for flag in $flags; do
      if ! grep -rq -- "$flag" "$suite_dir" --include='*.bats' --exclude='flag_coverage.bats'; then
        missing+=("$cmd $flag")
      fi
    done
  done

  if [ "${#missing[@]}" -gt 0 ]; then
    printf 'unexercised flags:\n%s\n' "$(printf '  %s\n' "${missing[@]}")"
    return 1
  fi
}
