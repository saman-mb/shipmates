#!/usr/bin/env bash
#
# Shipmates installer — copies the slash commands and agent roles into your
# Claude Code config so `/ship-issue` and the crew of specialist sub-agents
# become available.
#
#   ./install.sh                 # install for all your projects  (~/.claude)
#   ./install.sh --project       # install into ./.claude (this repo only)
#   ./install.sh --project PATH  # install into PATH/.claude
#   ./install.sh --dir PATH      # install into an explicit .claude dir
#   ./install.sh --uninstall     # remove the files Shipmates installed
#
# Existing files with the same name are backed up to <file>.bak-<timestamp>
# before being overwritten, so your own edits are never silently lost.

set -euo pipefail

SRC_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SUBDIRS=(commands agents)

SCOPE="global"
EXPLICIT_DIR=""
PROJECT_PATH=""
UNINSTALL=false

c_bold=$'\033[1m'; c_dim=$'\033[2m'; c_green=$'\033[32m'; c_yellow=$'\033[33m'; c_reset=$'\033[0m'

usage() {
  sed -n '3,17p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
  exit "${1:-0}"
}

while [ $# -gt 0 ]; do
  case "$1" in
    --project) SCOPE="project"; if [ $# -gt 1 ] && [[ "$2" != --* ]]; then PROJECT_PATH="$2"; shift; fi ;;
    --dir)     SCOPE="explicit"; EXPLICIT_DIR="${2:?--dir needs a path}"; shift ;;
    --uninstall) UNINSTALL=true ;;
    -h|--help) usage 0 ;;
    *) echo "Unknown option: $1" >&2; usage 1 ;;
  esac
  shift
done

case "$SCOPE" in
  global)   TARGET="${CLAUDE_CONFIG_DIR:-$HOME/.claude}" ;;
  project)  TARGET="$(cd "${PROJECT_PATH:-.}" && pwd)/.claude" ;;
  explicit) TARGET="$EXPLICIT_DIR" ;;
esac

echo "${c_bold}Shipmates${c_reset} ${c_dim}→${c_reset} ${c_bold}${TARGET}${c_reset}"
echo

ts="$(date +%Y%m%d%H%M%S)"
installed=0; backed_up=0; removed=0

for sub in "${SUBDIRS[@]}"; do
  src="$SRC_DIR/$sub"
  [ -d "$src" ] || continue
  dst="$TARGET/$sub"

  if $UNINSTALL; then
    for f in "$src"/*.md; do
      [ -e "$f" ] || continue
      name="$(basename "$f")"
      if [ -e "$dst/$name" ]; then
        rm -f "$dst/$name"
        echo "  ${c_yellow}removed${c_reset}  $sub/$name"
        removed=$((removed+1))
      fi
    done
    continue
  fi

  mkdir -p "$dst"
  for f in "$src"/*.md; do
    [ -e "$f" ] || continue
    name="$(basename "$f")"
    if [ -e "$dst/$name" ] && ! cmp -s "$f" "$dst/$name"; then
      mv "$dst/$name" "$dst/$name.bak-$ts"
      echo "  ${c_dim}backed up existing $sub/$name → $name.bak-$ts${c_reset}"
      backed_up=$((backed_up+1))
    fi
    cp "$f" "$dst/$name"
    echo "  ${c_green}installed${c_reset} $sub/$name"
    installed=$((installed+1))
  done
done

echo
if $UNINSTALL; then
  echo "Uninstalled ${removed} file(s). Your .bak-* backups (if any) were left in place."
else
  suffix=""; [ "$backed_up" -gt 0 ] && suffix=" (${backed_up} existing backed up)"
  echo "Installed ${installed} file(s)${suffix}."
  echo "${c_dim}If this was the first time a commands/ or agents/ dir was created, restart"
  echo "Claude Code so it picks them up. Then run ${c_reset}${c_bold}/ship-issue <issue#>${c_reset}${c_dim}.${c_reset}"
fi
