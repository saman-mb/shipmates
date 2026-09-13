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

# SHA-256 of a file, lowercase hex. python3 hashlib keeps the helper portable.
sha256_of() {
  python3 -c 'import hashlib, sys; print(hashlib.sha256(open(sys.argv[1], "rb").read()).hexdigest())' "$1"
}

# Rewrite an installed shared `.agents` skill to stale bytes and update its
# sha256 in every receipt that claims the path — the attributable-staleness
# fixture for the shared-tree update: each harness recorded the bytes now on
# disk, so a sibling's update may advance them. Receipt file lists stay sorted
# by path.
stale_shared_skill() {
  local dir="$1" skill="$2"
  local rel=".agents/skills/$skill/SKILL.md"
  printf '%s\n' '---' "name: $skill" 'description: stale shared generation' '---' 'stale shared body' \
    > "$dir/$rel"
  local stale
  stale="$(sha256_of "$dir/$rel")"
  local receipt
  for receipt in "$dir"/.shipmates/receipts/*.json; do
    jq --arg rel "$rel" --arg sha "$stale" \
      '(.files[] | select(.path == $rel) | .sha256) = $sha | .files |= sort_by(.path)' \
      "$receipt" > "$receipt.tmp"
    mv "$receipt.tmp" "$receipt"
  done
}

# Rewrite an installed claude-code tree into an older name generation: move one
# skill directory and rewrite the receipt so it claims the old path (the
# receipt writer requires its files sorted by path, so re-sort after the move).
# `previous` is the older identity: `shipmates-<verb>` or the bare verb.
make_previous_generation() {
  local dir="$1" current="$2" previous="$3"
  local receipt="$dir/.shipmates/receipts/claude-code.json"
  mv "$dir/.claude/skills/$current" "$dir/.claude/skills/$previous"
  jq --arg old ".claude/skills/$current/" --arg new ".claude/skills/$previous/" \
    '(.files[] | select(.path | startswith($old)) | .path) |= sub($old; $new) | .files |= sort_by(.path)' \
    "$receipt" > "$receipt.tmp"
  mv "$receipt.tmp" "$receipt"
}
