#!/usr/bin/env bash
#
# Shipmates installer — copies the skill workflows and agent roles into your
# Claude Code config so `/ship-issue` and the crew of specialist subagents
# become available. Run it straight from the web, no clone required:
#
#   curl -fsSL https://raw.githubusercontent.com/saman-mb/shipmates/main/install.sh | bash
#
#   ...| bash                          # install for all your projects (~/.claude)
#   ...| bash -s -- --project          # install into ./.claude (current repo)
#   ...| bash -s -- --project PATH     # install into PATH/.claude
#   ...| bash -s -- --dir PATH         # install into an explicit .claude dir
#   ...| bash -s -- --uninstall        # remove the files Shipmates installed
#
# Run from a local clone, it copies the files next to it; run via curl, it
# downloads the latest release tarball first. Existing files of the same name
# are backed up to <file>.bak-<timestamp> before being overwritten.
#
# Shipmates used to ship flat commands/<slug>.md files. Install and --uninstall
# both sweep those aside to <file>.bak-<timestamp> so a stale copy can't shadow
# the new skills/<slug>/SKILL.md. Commands you wrote yourself are never touched.
#

set -euo pipefail

REPO="saman-mb/shipmates"
TARBALL="https://github.com/${REPO}/archive/refs/heads/main.tar.gz"

SCOPE="global"; EXPLICIT_DIR=""; PROJECT_PATH=""; UNINSTALL=false

c_bold=$'\033[1m'; c_dim=$'\033[2m'; c_green=$'\033[32m'; c_yellow=$'\033[33m'; c_reset=$'\033[0m'

# Help is the header block above: from line 3 to the first non-comment line.
# Terminator-driven, so editing the header can't desync a hardcoded range.
usage() { awk 'NR>=3 { if ($0 !~ /^#/) exit; sub(/^# ?/, ""); print }' "${BASH_SOURCE[0]:-/dev/null}" 2>/dev/null; exit "${1:-0}"; }

while [ $# -gt 0 ]; do
  case "$1" in
    --project) SCOPE="project"; if [ $# -gt 1 ] && [[ "$2" != --* ]]; then PROJECT_PATH="$2"; shift; fi ;;
    --dir)     SCOPE="explicit"; EXPLICIT_DIR="${2:?--dir needs a path}"; shift ;;
    --uninstall) UNINSTALL=true ;;
    -h|--help) usage 0 ;;
    *) echo "Unknown option: $1" >&2; exit 1 ;;
  esac
  shift
done

# Where to install.
case "$SCOPE" in
  global)   TARGET="${CLAUDE_CONFIG_DIR:-$HOME/.claude}" ;;
  project)  base="${PROJECT_PATH:-.}"; mkdir -p "$base"; TARGET="$(cd "$base" && pwd)/.claude" ;;
  explicit) TARGET="$EXPLICIT_DIR" ;;
esac

# Source of the files: a local checkout if we're running inside one, else download.
SELF_DIR="$(cd "$(dirname "${BASH_SOURCE[0]:-.}")" 2>/dev/null && pwd || true)"
CLEANUP=""
if [ -n "$SELF_DIR" ] && [ -d "$SELF_DIR/skills" ] && [ -d "$SELF_DIR/agents" ]; then
  SRC="$SELF_DIR"
else
  command -v curl >/dev/null 2>&1 || { echo "Shipmates: 'curl' is required." >&2; exit 1; }
  command -v tar  >/dev/null 2>&1 || { echo "Shipmates: 'tar' is required." >&2; exit 1; }
  echo "${c_dim}Fetching Shipmates…${c_reset}"
  TMP="$(mktemp -d)"; CLEANUP="$TMP"; trap '[ -n "$CLEANUP" ] && rm -rf "$CLEANUP"' EXIT
  curl -fsSL "$TARBALL" | tar -xz -C "$TMP" || { echo "Shipmates: download failed." >&2; exit 1; }
  SRC="$(find "$TMP" -maxdepth 1 -mindepth 1 -type d | head -1)"
  [ -n "$SRC" ] && [ -d "$SRC/agents" ] && [ -d "$SRC/skills" ] || { echo "Shipmates: unexpected archive layout." >&2; exit 1; }
fi

echo "${c_bold}Shipmates${c_reset} ${c_dim}→${c_reset} ${c_bold}${TARGET}${c_reset}"
echo

ts="$(date +%Y%m%d%H%M%S)"; installed=0; backed_up=0; removed=0; swept=0

# --- file helpers, shared by the flat agents/ and nested skills/ loops --------

# Echo a backup name nothing occupies yet. Two runs in the same second would
# otherwise resolve to the same .bak-<ts> and silently clobber the first backup.
backup_path() {
  local base="$1.bak-$ts" candidate n=1
  candidate="$base"
  while [ -e "$candidate" ]; do candidate="$base.$n"; n=$((n+1)); done
  printf '%s\n' "$candidate"
}

# Move a file aside rather than destroy it. $2 is the log verb; the caller owns
# its counter, because legacy sweeps are reported apart from overwrite backups.
stash_file() {
  local f="$1" label="$2" bak
  bak="$(backup_path "$f")"
  mv "$f" "$bak"
  echo "  ${c_dim}${label} ${f#"$TARGET/"} → $(basename "$bak")${c_reset}"
}

install_file() {
  local src="$1" dst="$2"
  mkdir -p "$(dirname "$dst")"
  # Byte-identical content is left alone, so re-installing makes no new backups.
  if [ -e "$dst" ] && ! cmp -s "$src" "$dst"; then
    stash_file "$dst" "backed up existing"; backed_up=$((backed_up+1))
  fi
  cp "$src" "$dst"
  echo "  ${c_green}installed${c_reset} ${dst#"$TARGET/"}"; installed=$((installed+1))
}

remove_file() {
  local f="$1"
  [ -e "$f" ] || return 0
  rm -f "$f"
  echo "  ${c_yellow}removed${c_reset}  ${f#"$TARGET/"}"; removed=$((removed+1))
}

# An upgrade may leave a flat commands/<slug>.md from a previous install sitting
# next to commands the user wrote. Ours shadows the new skill, so move it aside —
# never delete it, it may be hand-edited. Runs on install and on --uninstall: a
# stale flat file would otherwise keep answering /ship-issue after an uninstall.
sweep_legacy_commands() {
  local d slug legacy
  for d in "$SRC/skills"/*/; do
    [ -d "$d" ] || continue
    slug="$(basename "$d")"
    legacy="$TARGET/commands/$slug.md"
    [ -f "$legacy" ] || continue
    stash_file "$legacy" "moved legacy"; swept=$((swept+1))
  done
  # Succeeds only once nothing of the user's own is left in there.
  rmdir "$TARGET/commands" 2>/dev/null || :
}

# --- install / uninstall ------------------------------------------------------

if $UNINSTALL; then
  for f in "$SRC/agents"/*.md; do
    [ -e "$f" ] || continue
    remove_file "$TARGET/agents/$(basename "$f")"
  done

  for d in "$SRC/skills"/*/; do
    [ -d "$d" ] || continue
    d="${d%/}"; [ -f "$d/SKILL.md" ] || continue
    remove_file "$TARGET/skills/$(basename "$d")/SKILL.md"
    # rmdir, never rm -rf: the skill dir may also hold references/, scripts/ or
    # assets you added. A non-empty dir makes this rmdir fail, your files
    # survive, and that is the correct outcome.
    rmdir "$TARGET/skills/$(basename "$d")" 2>/dev/null || :
  done

  rmdir "$TARGET/skills" 2>/dev/null || :
  rmdir "$TARGET/agents" 2>/dev/null || :
else
  for f in "$SRC/agents"/*.md; do
    [ -e "$f" ] || continue
    install_file "$f" "$TARGET/agents/$(basename "$f")"
  done

  for d in "$SRC/skills"/*/; do
    [ -d "$d" ] || continue
    d="${d%/}"; [ -f "$d/SKILL.md" ] || continue
    install_file "$d/SKILL.md" "$TARGET/skills/$(basename "$d")/SKILL.md"
  done
fi

sweep_legacy_commands

echo
if $UNINSTALL; then
  echo "Uninstalled ${removed} file(s). Your .bak-* backups (if any) were left in place."
else
  suffix=""; [ "$backed_up" -gt 0 ] && suffix=" (${backed_up} existing backed up)"
  echo "Installed ${installed} file(s)${suffix}."
  echo "${c_dim}If a skills/ or agents/ dir was created for the first time, restart Claude Code"
  echo "so it picks them up. Then run ${c_reset}${c_bold}/ship-issue <issue#>${c_reset}${c_dim}.${c_reset}"
fi
if [ "$swept" -gt 0 ]; then
  echo "${c_dim}Swept ${swept} legacy commands/*.md aside (kept as .bak-*) — this only covers ${TARGET}; re-run without --project to clear a global install too.${c_reset}"
fi
