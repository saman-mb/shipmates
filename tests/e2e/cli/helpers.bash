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

# Tools and commands both install as `shipmates-*` skill dirs, so a name
# pattern can't tell them apart — count against the toolbox source directory
# names instead of guessing from the installed name.
tool_dirs() {
  local dir="$1" name count=0
  for name in "$REPO_ROOT"/toolbox/*/; do
    name="$(basename "$name")"
    [ -d "$dir/.claude/skills/$name" ] && count=$((count + 1))
  done
  echo "$count"
}

# Live (non-backup) files under installed tool directories only — same
# tools-vs-commands ambiguity as tool_dirs, so walk the same toolbox list.
tool_files() {
  local dir="$1" name count=0
  for name in "$REPO_ROOT"/toolbox/*/; do
    name="$(basename "$name")"
    count=$((count + $(find "$dir/.claude/skills/$name" -type f ! -name '*.bak-*' 2>/dev/null | wc -l | tr -d ' ')))
  done
  echo "$count"
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

# Root-relative path to an installed command skill for `name` under
# `harness`'s own tree. A directory holding `SKILL.md` everywhere except
# opencode, which ships one flat command file with no wrapper directory;
# codex, antigravity, github-copilot and pi share one open `.agents/skills/`
# tree rather than each carrying a private copy.
harness_skill_path() {
  local harness="$1" name="$2"
  case "$harness" in
    claude-code) echo ".claude/skills/$name" ;;
    opencode) echo ".opencode/commands/$name.md" ;;
    cursor) echo ".cursor/skills/$name" ;;
    windsurf) echo ".windsurf/skills/$name" ;;
    codex | antigravity | github-copilot | pi) echo ".agents/skills/$name" ;;
    *)
      echo "harness_skill_path: unknown harness '$harness'" >&2
      return 1
      ;;
  esac
}

# Rewrite an installed tree into an older name generation: move one command
# skill (a directory everywhere but opencode, which is one file) and rewrite
# that harness's receipt so it claims the old path (the receipt writer
# requires its files sorted by path, so re-sort after the move). `previous`
# is the older identity: `shipmates-<verb>` or the bare verb. `harness`
# defaults to claude-code.
make_previous_generation() {
  local dir="$1" current="$2" previous="$3" harness="${4:-claude-code}"
  local receipt="$dir/.shipmates/receipts/$harness.json"
  local cur_path new_path
  cur_path="$(harness_skill_path "$harness" "$current")"
  new_path="$(harness_skill_path "$harness" "$previous")"
  mv "$dir/$cur_path" "$dir/$new_path"
  if [ "$harness" = opencode ]; then
    jq --arg old "$cur_path" --arg new "$new_path" \
      '(.files[] | select(.path == $old) | .path) = $new | .files |= sort_by(.path)' \
      "$receipt" > "$receipt.tmp"
  else
    jq --arg old "$cur_path/" --arg new "$new_path/" \
      '(.files[] | select(.path | startswith($old)) | .path) |= sub($old; $new) | .files |= sort_by(.path)' \
      "$receipt" > "$receipt.tmp"
  fi
  mv "$receipt.tmp" "$receipt"
}
