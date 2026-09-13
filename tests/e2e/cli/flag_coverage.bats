#!/usr/bin/env bats
# Drift guard: every flag a subcommand documents must be exercised on that
# subcommand — a documented flag that no test passes to its own command fails
# here until it is covered. (`--help` is excluded: it is exercised globally.)

bats_load_library bats-support
bats_load_library bats-assert
load helpers

@test "every documented subcommand flag is exercised on that subcommand" {
  local suite_dir
  suite_dir="$(dirname "$BATS_TEST_FILENAME")"
  local missing=()

  local cmd flags flag invocations
  for cmd in install update uninstall doctor build check; do
    run "$SHIPMATES_BIN" help "$cmd"
    assert_success
    flags="$(printf '%s\n' "$output" | awk '/^Options:/{found=1;next} found' \
      | grep -oE -- '--[a-z][a-z-]+' | sort -u)"
    invocations="$(grep -hF "\$SHIPMATES_BIN\" $cmd" "$suite_dir"/*.bats || true)"
    for flag in $flags; do
      [ "$flag" = "--help" ] && continue
      if ! printf '%s\n' "$invocations" | grep -qF -- "$flag"; then
        missing+=("$cmd $flag")
      fi
    done
  done

  if [ "${#missing[@]}" -gt 0 ]; then
    printf 'unexercised flags (documented but never passed on their own subcommand):\n%s\n' \
      "$(printf '  %s\n' "${missing[@]}")"
    return 1
  fi
}
