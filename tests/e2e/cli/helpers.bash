# Shared sandbox setup for the install / CLI e2e suite.
#
# Every test runs the real binary built from this branch against a throwaway
# HOME and target directory, so nothing here can touch the contributor
# checkout. `SHIPMATES_BIN` defaults to the release build CI makes; point it at
# target/debug/shipmates for a fast local loop.

setup() {
  REPO_ROOT="$(cd "$(dirname "$BATS_TEST_FILENAME")/../../.." && pwd)"
  SHIPMATES_BIN="${SHIPMATES_BIN:-$REPO_ROOT/target/release/shipmates}"

  SANDBOX="$BATS_TEST_TMPDIR/sandbox"
  HOME="$BATS_TEST_TMPDIR/home"
  export HOME
  export XDG_CONFIG_HOME="$HOME/.config"
  export XDG_DATA_HOME="$HOME/.local/share"
  export NO_COLOR=1
  export TERM=dumb
  mkdir -p "$SANDBOX" "$HOME"
  cd "$SANDBOX" || return 1
}

# Install claude-code into a fresh sandbox with no tools, then echo its path.
install_claude_code() {
  local dir="$1"
  run "$SHIPMATES_BIN" install --harness claude-code --dir "$dir" --with-tools none
  assert_success
}

skill_dirs() {
  find "$1/.claude/skills" -mindepth 1 -maxdepth 1 -type d 2>/dev/null | wc -l | tr -d ' '
}

tool_dirs() {
  find "$1/.claude/skills" -mindepth 1 -maxdepth 1 -type d -name 'shipmates-*' 2>/dev/null | wc -l | tr -d ' '
}

agent_files() {
  find "$1/.claude/agents" -mindepth 1 -maxdepth 1 -type f -name '*.md' 2>/dev/null | wc -l | tr -d ' '
}

receipt_files() {
  find "$1/.shipmates/receipts" -mindepth 1 -maxdepth 1 -type f -name '*.json' 2>/dev/null | wc -l | tr -d ' '
}
