#!/usr/bin/env bash
#
# Shipmates installer — copies the slash commands and agent roles into your
# Claude Code config so `/ship-issue` and the crew of specialist sub-agents
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

set -euo pipefail

REPO="saman-mb/shipmates"
TARBALL="https://github.com/${REPO}/archive/refs/heads/main.tar.gz"
SUBDIRS=(commands agents)

SCOPE="global"; EXPLICIT_DIR=""; PROJECT_PATH=""; UNINSTALL=false

c_bold=$'\033[1m'; c_dim=$'\033[2m'; c_green=$'\033[32m'; c_yellow=$'\033[33m'; c_reset=$'\033[0m'

usage() { sed -n '3,18p' "${BASH_SOURCE[0]:-/dev/null}" 2>/dev/null | sed 's/^# \{0,1\}//'; exit "${1:-0}"; }

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
if [ -n "$SELF_DIR" ] && [ -d "$SELF_DIR/commands" ] && [ -d "$SELF_DIR/agents" ]; then
  SRC="$SELF_DIR"
else
  command -v curl >/dev/null 2>&1 || { echo "Shipmates: 'curl' is required." >&2; exit 1; }
  command -v tar  >/dev/null 2>&1 || { echo "Shipmates: 'tar' is required." >&2; exit 1; }
  echo "${c_dim}Fetching Shipmates…${c_reset}"
  TMP="$(mktemp -d)"; CLEANUP="$TMP"; trap '[ -n "$CLEANUP" ] && rm -rf "$CLEANUP"' EXIT
  curl -fsSL "$TARBALL" | tar -xz -C "$TMP" || { echo "Shipmates: download failed." >&2; exit 1; }
  SRC="$(find "$TMP" -maxdepth 1 -mindepth 1 -type d | head -1)"
  [ -n "$SRC" ] && [ -d "$SRC/agents" ] || { echo "Shipmates: unexpected archive layout." >&2; exit 1; }
fi

echo "${c_bold}Shipmates${c_reset} ${c_dim}→${c_reset} ${c_bold}${TARGET}${c_reset}"
echo

ts="$(date +%Y%m%d%H%M%S)"; installed=0; backed_up=0; removed=0

for sub in "${SUBDIRS[@]}"; do
  [ -d "$SRC/$sub" ] || continue
  dst="$TARGET/$sub"

  if $UNINSTALL; then
    for f in "$SRC/$sub"/*.md; do
      [ -e "$f" ] || continue
      name="$(basename "$f")"
      if [ -e "$dst/$name" ]; then rm -f "$dst/$name"; echo "  ${c_yellow}removed${c_reset}  $sub/$name"; removed=$((removed+1)); fi
    done
    continue
  fi

  mkdir -p "$dst"
  for f in "$SRC/$sub"/*.md; do
    [ -e "$f" ] || continue
    name="$(basename "$f")"
    if [ -e "$dst/$name" ] && ! cmp -s "$f" "$dst/$name"; then
      mv "$dst/$name" "$dst/$name.bak-$ts"
      echo "  ${c_dim}backed up existing $sub/$name → $name.bak-$ts${c_reset}"; backed_up=$((backed_up+1))
    fi
    cp "$f" "$dst/$name"
    echo "  ${c_green}installed${c_reset} $sub/$name"; installed=$((installed+1))
  done
done

echo
if $UNINSTALL; then
  echo "Uninstalled ${removed} file(s). Your .bak-* backups (if any) were left in place."
else
  suffix=""; [ "$backed_up" -gt 0 ] && suffix=" (${backed_up} existing backed up)"
  echo "Installed ${installed} file(s)${suffix}."
  echo "${c_dim}If a commands/ or agents/ dir was created for the first time, restart Claude Code"
  echo "so it picks them up. Then run ${c_reset}${c_bold}/ship-issue <issue#>${c_reset}${c_dim}.${c_reset}"
fi
