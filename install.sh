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
# Run from a local clone it copies the files sitting next to it; piped from the
# web there is no local copy to trust, so it always downloads the main branch
# tarball first. Skills are copied whole, so whatever a skill bundles
# (references/, scripts/, assets/) comes along with its SKILL.md.
#
# Every install records a manifest at <target>/shipmates/manifest: one line per
# file with its SHA-256, so later runs can tell Shipmates' files from yours.
# Re-installing skips files that are already identical and upgrades ones only
# we touched, without making backups. A file you wrote or edited is backed up
# to <file>.bak-<timestamp> first, and if its frontmatter `name:` says it is a
# *different* agent or skill than the one replacing it, you get a loud warning.
#
# --uninstall uses the manifest to delete only what Shipmates put there and
# left untouched; anything you modified is left alone. When a file is removed
# and a .bak-<timestamp> exists beside it, your original is restored. Without
# a manifest (installs from before this change) it falls back to the old
# behaviour — deleting only payload-identical files — and says so loudly.
#
# Shipmates used to ship flat commands/<slug>.md files. Install and --uninstall
# both sweep those aside to <file>.bak-<timestamp> so a stale copy can't shadow
# the new skills/<slug>/SKILL.md. Commands you wrote yourself are never touched.
#

set -Eeuo pipefail

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

MANIFEST="$TARGET/shipmates/manifest"

ts="$(date +%Y%m%d%H%M%S)"; installed=0; backed_up=0; removed=0; kept=0; swept=0; unchanged=0; upgraded=0; skipped=0
IDENTITY_CHANGES=""

CLEANUP=""
# Paths from mktemp contain no spaces; word-splitting CLEANUP here is intentional.
on_exit() {
  # shellcheck disable=SC2086
  [ -n "$CLEANUP" ] && rm -rf $CLEANUP
  return 0
}
# Nothing here deletes, so a half-finished run leaves every original on disk —
# say where, because a file we moved aside is no longer under its own name.
on_err() {
  echo >&2
  echo "Shipmates: stopped partway through ${TARGET}. Nothing was deleted — anything" >&2
  echo "moved aside is still there as <file>.bak-${ts}, restore it with mv." >&2
}
trap on_exit EXIT
trap on_err ERR

# --- hashing ------------------------------------------------------------------

# Detect the SHA-256 tool once, before any mutation. sha256sum on Linux,
# `shasum -a 256` on macOS. Install and manifest-driven uninstall both hash;
# the legacy uninstall path compares bytes with cmp and never needs this.
SHA256_BIN=""
need_sha256() {
  [ -n "$SHA256_BIN" ] && return 0
  if command -v sha256sum >/dev/null 2>&1; then SHA256_BIN="sha256sum"
  elif command -v shasum >/dev/null 2>&1; then SHA256_BIN="shasum -a 256"
  else echo "Shipmates: needs 'sha256sum' or 'shasum' to track installs." >&2; exit 1; fi
}
sha256() { $SHA256_BIN "$1" | awk '{print $1}'; }

# --- manifest -----------------------------------------------------------------
#
# Line-based key=value, one record per line, parseable with plain grep/awk —
# no jq. Paths are relative to TARGET so a moved project dir still resolves.
# Schema v1:
#   manifest_version=1                       (required, version gate)
#   version=<git-short-sha|unknown>          (informational)
#   installed_at=<epoch>                     (informational)
#   scope=global|project|explicit            (informational)
#   file=<relpath> sha256=<64-hex> [name=<frontmatter-name>]
# name= appears on agents/*.md and skills/*/SKILL.md entries (files whose
# frontmatter carries an identity); payload files under a skill dir go without.

MANIFEST_STATE=1  # 0 = present and valid, 1 = absent, 2 = present but corrupt
MANIFEST_PARSED=""  # temp file of canonicalized "relpath sha256" pairs, valid state only

# Validate the whole manifest in one awk pass — the single trust boundary —
# and emit canonicalized `relpath sha256` pairs to MANIFEST_PARSED. Every
# consumer (uninstall, orphan sweep, sha lookups) reads THAT file, never the
# raw manifest: two parsers reading one format is how a crafted line passes
# validation while extracting to something else (a traversal path hiding in a
# name= field, say). Corrupt means: unreadable, missing/unknown
# manifest_version, duplicate paths, absolute or ..-traversing paths, paths
# outside agents|skills, unsafe characters in a path or name, bad sha. A
# manifest is a deletion instruction list; anything suspicious must stop us.
manifest_read() {
  MANIFEST_STATE=1
  [ -e "$MANIFEST" ] || return 0
  if [ ! -r "$MANIFEST" ]; then
    echo "Shipmates: manifest exists but is not readable: $MANIFEST" >&2
    MANIFEST_STATE=2; return 0
  fi
  MANIFEST_PARSED="$(mktemp)"; CLEANUP="${CLEANUP:+$CLEANUP }$MANIFEST_PARSED"
  if awk '
    BEGIN { mv=0; bad=0 }
    /^[[:space:]]*$/ { next }
    /^#/ { next }
    $0 == "manifest_version=1" { mv=1; next }
    /^manifest_version=/ { print "unsupported manifest_version line: " $0 > "/dev/stderr"; bad=1; exit }
    /^(version|installed_at|scope)=[^[:space:]]+$/ { next }
    /^file=[^[:space:]]/ {
      path=""; sha=""; nm=""; bad_field=0
      for (i=1; i<=NF; i++) {
        if ($i ~ /^file=/) path=substr($i, 6)
        else if ($i ~ /^sha256=/) sha=substr($i, 8)
        else if ($i ~ /^name=/) nm=substr($i, 6)
        else bad_field=1
      }
      if (bad_field || path == "" || sha == "") { print "malformed record: " $0 > "/dev/stderr"; bad=1; exit }
      if (path ~ /^\// || path ~ /\.\./ || path !~ /^(agents|skills)\//) { print "unsafe path: " path > "/dev/stderr"; bad=1; exit }
      if (path !~ /^[A-Za-z0-9._\/-]+$/) { print "unsafe characters in path: " path > "/dev/stderr"; bad=1; exit }
      if (nm != "" && nm !~ /^[A-Za-z0-9._-]+$/) { print "unsafe characters in name for " path > "/dev/stderr"; bad=1; exit }
      if (length(sha) != 64 || sha !~ /^[0-9a-f]+$/) { print "bad sha256 for " path > "/dev/stderr"; bad=1; exit }
      if (seen[path]++) { print "duplicate path: " path > "/dev/stderr"; bad=1; exit }
      print path " " sha
      next
    }
    { next }  # unknown record types are ignored, for forward compatibility
    END {
      if (bad) exit 1
      if (!mv) { print "missing manifest_version=1" > "/dev/stderr"; exit 1 }
    }
  ' "$MANIFEST" > "$MANIFEST_PARSED"; then
    MANIFEST_STATE=0
  else
    MANIFEST_STATE=2
  fi
}

# sha for a relpath from a valid manifest; empty when not listed. Exact field
# match against the canonicalized parse — never a regex over the raw file.
manifest_sha_of() {
  [ -n "$MANIFEST_PARSED" ] || return 0
  awk -v p="$1" '$1 == p { print $2; exit }' "$MANIFEST_PARSED"
}

# Write the collected NEW_ENTRIES as the new manifest, atomically: temp beside
# the destination, renamed in — a crash can't leave a half-written manifest,
# and the old one survives untouched until the whole copy pass has completed.
manifest_write() {
  local tmp ver
  ver="unknown"
  if [ -n "$SELF_DIR" ]; then
    ver="$(git -C "$SELF_DIR" rev-parse --short HEAD 2>/dev/null || printf unknown)"
  fi
  mkdir -p "$TARGET/shipmates"
  tmp="$MANIFEST.shipmates-tmp.$$"
  {
    echo "# shipmates manifest"
    echo "manifest_version=1"
    echo "version=$ver"
    echo "installed_at=$(date +%s)"
    echo "scope=$SCOPE"
    cat "$NEW_ENTRIES"
  } > "$tmp"
  mv "$tmp" "$MANIFEST"
}

# --- source resolution --------------------------------------------------------
#
# Lazy: only install and the legacy uninstall need the payload. A manifest-
# driven uninstall works entirely from the manifest, offline — no clone, no
# tarball download just to remove files.
#
# "Running from a checkout" has to mean this script is a real file on disk
# whose directory looks like the Shipmates repo. Piped from curl, BASH_SOURCE[0]
# is unset and defaulting it to "." would silently make the *current working
# directory* the payload — so any directory holding a skills/ and an agents/
# could feed arbitrary instructions into ~/.claude. Fingerprint, don't guess.
SRC=""; SELF_DIR=""
resolve_src() {
  [ -n "$SRC" ] && return 0
  local self="${BASH_SOURCE[0]:-}"
  if [ -n "$self" ] && [ -f "$self" ] \
     && SELF_DIR="$(cd "$(dirname "$self")" 2>/dev/null && pwd)" \
     && [ -f "$SELF_DIR/install.sh" ] \
     && [ -d "$SELF_DIR/agents" ] \
     && [ -f "$SELF_DIR/skills/ship-issue/SKILL.md" ]; then
    SRC="$SELF_DIR"
  else
    command -v curl >/dev/null 2>&1 || { echo "Shipmates: 'curl' is required." >&2; exit 1; }
    command -v tar  >/dev/null 2>&1 || { echo "Shipmates: 'tar' is required." >&2; exit 1; }
    echo "${c_dim}Fetching Shipmates…${c_reset}"
    TMP="$(mktemp -d)"; CLEANUP="${CLEANUP:+$CLEANUP }$TMP"
    curl -fsSL "$TARBALL" | tar -xz -C "$TMP" || { echo "Shipmates: download failed." >&2; exit 1; }
    SRC="$(find "$TMP" -maxdepth 1 -mindepth 1 -type d | head -1)"
    [ -n "$SRC" ] && [ -d "$SRC/agents" ] && [ -d "$SRC/skills" ] || { echo "Shipmates: unexpected archive layout." >&2; exit 1; }
  fi
}

echo "${c_bold}Shipmates${c_reset} ${c_dim}→${c_reset} ${c_bold}${TARGET}${c_reset}"
echo

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

# Write beside the destination and rename in: a failed or interrupted copy
# can't leave the user with a half-written file, or with none at all.
copy_atomic() {
  local src="$1" dst="$2" tmp
  mkdir -p "$(dirname "$dst")"
  tmp="$dst.shipmates-tmp.$$"
  cp "$src" "$tmp"
  mv "$tmp" "$dst"
}

# The frontmatter `name:` of an agent or SKILL.md — its identity, which the
# filename only happens to mirror. Empty when absent; never fails.
agent_name() {
  sed -n 's/^name:[[:space:]]*//p' "$1" 2>/dev/null | head -1 || return 0
}

# Record a file in the new manifest: hash of the destination POST-copy, so the
# manifest is always a true statement about what is on disk right now.
NEW_ENTRIES="$(mktemp)"; CLEANUP="${CLEANUP:+$CLEANUP }$NEW_ENTRIES"
record_entry() {
  local rel="$1" sha nm=""
  need_sha256
  sha="$(sha256 "$TARGET/$rel")"
  case "$rel" in
    agents/*.md|skills/*/SKILL.md)
      nm="$(agent_name "$TARGET/$rel")"
      if [ -n "$nm" ]; then
        # Writer enforces the same charset the reader whitelists, or a
        # multi-word/exotic name would produce a manifest we then reject as
        # corrupt — bricking uninstall against our own write.
        if [[ "$nm" =~ ^[A-Za-z0-9._-]+$ ]]; then
          nm=" name=$nm"
        else
          echo "  ${c_yellow}WARNING${c_reset} $rel: name '$nm' has unsafe characters — omitted from manifest" >&2
          nm=""
        fi
      fi
      ;;
  esac
  echo "file=$rel sha256=$sha$nm" >> "$NEW_ENTRIES"
}

# Warn when an overwrite swaps one identity for another (#77): the pre-existing
# file answers to a different `name:` than what replaces it, so anything
# referencing the old name stops resolving — silently, unless we say so here.
check_identity() {
  local src="$1" dst="$2" rel="$3" old_nm new_nm
  case "$rel" in
    agents/*.md|skills/*/SKILL.md) ;;
    *) return 0 ;;
  esac
  old_nm="$(agent_name "$dst")"; new_nm="$(agent_name "$src")"
  if [ -n "$old_nm" ] && [ -n "$new_nm" ] && [ "$old_nm" != "$new_nm" ]; then
    echo "  ${c_yellow}WARNING${c_reset} $rel currently provides '${old_nm}', will be replaced by '${new_nm}'"
    IDENTITY_CHANGES="${IDENTITY_CHANGES}    $rel: '${old_nm}' -> '${new_nm}'\n"
  fi
}

# Install one payload file with ownership-aware decisions:
#   absent                          → install
#   identical                       → skip (no new backup on re-install)
#   differs, but == our manifest    → upgrade in place, no backup (ours, older)
#   differs otherwise               → user's: warn, back up, then install
install_file() {
  local src="$1" dst="$2" rel="$3" src_sha dst_sha old_sha
  if [ ! -e "$dst" ]; then
    copy_atomic "$src" "$dst"
    echo "  ${c_green}installed${c_reset} $rel"; installed=$((installed+1))
    record_entry "$rel"; return
  fi
  need_sha256
  src_sha="$(sha256 "$src")"; dst_sha="$(sha256 "$dst")"
  if [ "$src_sha" = "$dst_sha" ]; then
    echo "  ${c_dim}unchanged${c_reset} $rel"; unchanged=$((unchanged+1))
    record_entry "$rel"; return
  fi
  old_sha=""
  [ "$MANIFEST_STATE" = "0" ] && old_sha="$(manifest_sha_of "$rel")"
  if [ -n "$old_sha" ] && [ "$dst_sha" = "$old_sha" ]; then
    copy_atomic "$src" "$dst"
    echo "  ${c_green}updated${c_reset}   $rel ${c_dim}(our previous version)${c_reset}"; upgraded=$((upgraded+1))
    record_entry "$rel"; return
  fi
  check_identity "$src" "$dst" "$rel"
  echo "  ${c_yellow}modified${c_reset}  $rel ${c_dim}(yours or hand-edited — backed up)${c_reset}"
  stash_file "$dst" "backed up existing"; backed_up=$((backed_up+1))
  copy_atomic "$src" "$dst"
  installed=$((installed+1))
  record_entry "$rel"
}

# Legacy uninstall only: delete payload-identical files, stash the rest.
remove_file() {
  local src="$1" dst="$2"
  [ -e "$dst" ] || return 0
  if cmp -s "$src" "$dst"; then
    rm -f "$dst"
    echo "  ${c_yellow}removed${c_reset}  ${dst#"$TARGET/"}"; removed=$((removed+1))
  else
    stash_file "$dst" "kept your version of"; kept=$((kept+1))
  fi
}

# Restore the newest .bak-* for a just-removed file — but only a backup that
# matches the shape WE create (<file>.bak-<14 digits>[.N]): anything else
# sitting at that glob (planted, misnamed, foreign) is not ours to promote
# onto a live agent/skill path. Never onto an occupied path either: a
# surviving file there is the user's and clobbering it is #77 in reverse.
# Timestamps sort lexicographically (%Y%m%d%H%M%S), so newest = max name.
restore_bak() {
  local dst="$1" b suffix best=""
  for b in "$dst".bak-*; do
    [ -f "$b" ] || continue
    suffix="${b#"$dst".bak-}"
    [[ "$suffix" =~ ^[0-9]{14}(\.[0-9]+)?$ ]] || continue
    [[ "$b" > "$best" ]] && best="$b"
  done
  if [ -n "$best" ] && [ ! -e "$dst" ]; then
    mv -n "$best" "$dst"
    echo "  ${c_green}restored${c_reset}  ${dst#"$TARGET/"} ${c_dim}(from $(basename "$best"))${c_reset}"
  fi
}

# $1 is the dir a file we just removed lived in: rmdir it, then walk up
# rmdir'ing each now-empty ancestor, stopping at (never removing) $2 or the
# first dir still holding something. Called once per removal rather than
# swept over the whole tree after the fact, so a nested bundle (skills/x/
# SKILL.md beside skills/x/scripts/) converges — emptying scripts/ empties
# x/ in turn — without ever touching a dir the sweep didn't vacate, like an
# empty skills/my-wip-skill/ the user made themselves.
prune_empty_dirs() {
  local d="$1" boundary="$2"
  while [ "$d" != "$boundary" ] && [ "${d#"$boundary"/}" != "$d" ]; do
    rmdir "$d" 2>/dev/null || break
    d="$(dirname "$d")"
  done
}

# An upgrade may leave a flat commands/<slug>.md from a previous install sitting
# next to commands the user wrote. Ours shadows the new skill, so move it aside —
# never delete it, it may be hand-edited. Runs on install and on --uninstall: a
# stale flat file would otherwise keep answering /ship-issue after an uninstall.
# Slugs come from the payload when SRC is resolved, else from the manifest.
sweep_legacy_commands() {
  local slug legacy
  local slugs=""
  if [ -n "$SRC" ]; then
    local d
    for d in "$SRC/skills"/*/; do
      [ -d "$d" ] || continue
      slugs="$slugs$(basename "$d")"$'\n'
    done
  elif [ "$MANIFEST_STATE" = "0" ]; then
    slugs="$(awk '$1 ~ /^skills\// { split($1, a, "/"); print a[2] }' "$MANIFEST_PARSED" | sort -u || true)"$'\n'
  else
    return 0
  fi
  # Process substitution, not a pipe: the loop must run in THIS shell or the
  # swept counter and the echoes that depend on it are lost to a subshell.
  while IFS= read -r slug; do
    [ -n "$slug" ] || continue
    legacy="$TARGET/commands/$slug.md"
    [ -f "$legacy" ] || continue
    stash_file "$legacy" "moved legacy"; swept=$((swept+1))
  done < <(printf '%s' "$slugs")
  # Only tidy up a dir we emptied. An untouched commands/ that happens to be
  # empty is the user's, and removing it is none of our business.
  if [ "$swept" -gt 0 ]; then
    rmdir "$TARGET/commands" 2>/dev/null || :
  fi
}

# --- install / uninstall ------------------------------------------------------

if $UNINSTALL; then
  manifest_read
  case "$MANIFEST_STATE" in
    2)
      # A corrupt manifest is an unreadable ownership claim: refuse ALL
      # deletion rather than guess. Falling back to name-based removal here
      # would recreate the exact data loss the manifest exists to prevent.
      echo >&2 "Shipmates: $MANIFEST is present but failed validation (see above)."
      echo >&2 "Refusing to uninstall against a corrupt manifest."
      echo >&2 "Delete it and re-run to force name-based uninstall:  rm '$MANIFEST'"
      exit 1
      ;;
    0)
      need_sha256
      # Iterate the canonicalized parse (relpath + sha per line, both
      # charset-whitelisted by the validator) — never re-parse the raw
      # manifest, or a crafted line can validate as one path and extract as
      # another.
      while read -r rel sha; do
        [ -n "$rel" ] && [ -n "$sha" ] || continue
        dst="$TARGET/$rel"
        if [ ! -e "$dst" ]; then
          echo "  ${c_dim}already gone${c_reset} $rel"; continue
        fi
        if [ "$(sha256 "$dst")" = "$sha" ]; then
          rm -f "$dst"
          echo "  ${c_yellow}removed${c_reset}  $rel"; removed=$((removed+1))
          restore_bak "$dst"
          # Tidy the dir this removal emptied and its now-empty ancestors up
          # to skills/ — rmdir fails on any dir still holding a user's files,
          # which is the correct outcome.
          [ -d "$TARGET/skills" ] && prune_empty_dirs "$(dirname "$dst")" "$TARGET/skills"
        else
          echo "  ${c_yellow}kept${c_reset}     $rel ${c_dim}(modified since install — yours now; rm '$dst' to force)${c_reset}"
          kept=$((kept+1)); skipped=$((skipped+1))
        fi
      done < "$MANIFEST_PARSED"
      rmdir "$TARGET/skills" 2>/dev/null || :
      rmdir "$TARGET/agents" 2>/dev/null || :
      sweep_legacy_commands
      if [ "$skipped" -eq 0 ]; then
        rm -f "$MANIFEST"
        rmdir "$TARGET/shipmates" 2>/dev/null || :
      else
        echo "${c_dim}Manifest kept at $MANIFEST — re-run --uninstall after resolving the kept files.${c_reset}"
      fi
      ;;
    1)
      # Legacy: installs from before the manifest existed. Behaviour unchanged
      # from the old installer — but say plainly what that means.
      resolve_src
      echo "${c_yellow}No manifest at $MANIFEST — falling back to name-based uninstall.${c_reset}" >&2
      baks="$( { find "$TARGET/agents" "$TARGET/skills" "$TARGET/commands" -name '*.bak-*' 2>/dev/null || true; } )"
      if [ -n "$baks" ]; then
        echo "${c_yellow}Files you wrote whose names match Shipmates' will be moved aside, and any" >&2
        echo ".bak-* backups are NOT loadable by Claude Code. Restore them by hand, e.g.:" >&2
        printf '%s\n' "$baks" | while IFS= read -r b; do
          echo "  mv '$b' '${b%.bak-*}'" >&2
        done
      else
        echo "${c_yellow}Files you wrote whose names match Shipmates' will be moved aside, not deleted.${c_reset}" >&2
      fi
      echo >&2
      for f in "$SRC/agents"/*.md; do
        [ -e "$f" ] || continue
        remove_file "$f" "$TARGET/agents/$(basename "$f")"
      done
      for d in "$SRC/skills"/*/; do
        [ -d "$d" ] || continue
        d="${d%/}"; [ -f "$d/SKILL.md" ] || continue
        slug="$(basename "$d")"
        # Walk the payload, not the target: only files this version ships are
        # candidates for removal, so anything you dropped in yourself is
        # invisible to this loop and survives.
        while IFS= read -r f; do
          remove_file "$f" "$TARGET/skills/$slug/${f#"$d/"}"
        done < <(find "$d" -type f | sort)
        while IFS= read -r sub; do
          rmdir "$TARGET/skills/$slug/${sub#"$d/"}" 2>/dev/null || :
        done < <(find "$d" -mindepth 1 -type d | sort -r)
        rmdir "$TARGET/skills/$slug" 2>/dev/null || :
      done
      rmdir "$TARGET/skills" 2>/dev/null || :
      rmdir "$TARGET/agents" 2>/dev/null || :
      sweep_legacy_commands
      ;;
  esac
else
  resolve_src
  need_sha256  # preflight: fail before any mutation on a host with no sha tool
  manifest_read
  if [ "$MANIFEST_STATE" = "2" ]; then
    # Installing never deletes, so a corrupt manifest can't cause data loss
    # here — but it can't prove ownership either, so every overwrite gets a
    # backup. The fresh manifest written at the end replaces the corrupt one.
    echo "${c_yellow}Manifest at $MANIFEST failed validation — treating as a fresh install;${c_reset}" >&2
    echo "${c_yellow}every overwritten file will be backed up this run.${c_reset}" >&2
    MANIFEST_STATE=1
  fi

  for f in "$SRC/agents"/*.md; do
    [ -e "$f" ] || continue
    install_file "$f" "$TARGET/agents/$(basename "$f")" "agents/$(basename "$f")"
  done

  for d in "$SRC/skills"/*/; do
    [ -d "$d" ] || continue
    d="${d%/}"; [ -f "$d/SKILL.md" ] || continue
    slug="$(basename "$d")"
    # The whole skill dir, file by file: the Agent Skills standard lets a skill
    # bundle references/, scripts/ and assets/ beside SKILL.md, and per-file
    # keeps the back-up-before-overwrite rule over every one of them.
    while IFS= read -r f; do
      install_file "$f" "$TARGET/skills/$slug/${f#"$d/"}" "skills/$slug/${f#"$d/"}"
    done < <(find "$d" -type f | sort)
  done

  # Orphan sweep: files the previous install owned that this version no longer
  # ships. Untouched (still matching the old manifest) → remove; modified →
  # leave and warn. Only meaningful with a valid prior manifest. Iterates the
  # canonicalized parse — same single-parser rule as uninstall.
  if [ "$MANIFEST_STATE" = "0" ]; then
    NEW_RELS="$(mktemp)"; CLEANUP="${CLEANUP:+$CLEANUP }$NEW_RELS"
    sed -n 's/^file=\([^[:space:]]*\).*/\1/p' "$NEW_ENTRIES" | sort > "$NEW_RELS"
    while read -r rel old_sha; do
      [ -n "$rel" ] || continue
      grep -qxF "$rel" "$NEW_RELS" && continue
      dst="$TARGET/$rel"
      [ -e "$dst" ] || continue
      if [ -n "$old_sha" ] && [ "$(sha256 "$dst")" = "$old_sha" ]; then
        rm -f "$dst"
        echo "  ${c_yellow}removed${c_reset}  $rel ${c_dim}(no longer shipped)${c_reset}"; removed=$((removed+1))
        # Tidy the dir this removal emptied (e.g. a renamed/dropped skill) and
        # its now-empty ancestors up to skills/ — same rmdir-only rule as
        # uninstall: a dir still holding any file is left alone.
        [ -d "$TARGET/skills" ] && prune_empty_dirs "$(dirname "$dst")" "$TARGET/skills"
      else
        echo "  ${c_yellow}kept${c_reset}     $rel ${c_dim}(no longer shipped, but you modified it)${c_reset}"
      fi
    done < "$MANIFEST_PARSED"
  fi

  sweep_legacy_commands
  manifest_write

  if [ -n "$IDENTITY_CHANGES" ]; then
    echo
    echo "${c_yellow}${c_bold}Identity changes — these files used to provide a different agent/skill:${c_reset}"
    printf '%b' "$IDENTITY_CHANGES"
    echo "${c_yellow}Anything referencing an old name above will no longer resolve.${c_reset}"
  fi
fi

echo
if $UNINSTALL; then
  echo "Uninstalled ${removed} file(s)."
  if [ "$kept" -gt 0 ]; then
    echo "${c_dim}${kept} file(s) were yours or hand-edited: left in place (see above).${c_reset}"
  fi
else
  summary="Installed ${installed} file(s)"
  [ "$upgraded" -gt 0 ] && summary="$summary, upgraded ${upgraded}"
  [ "$unchanged" -gt 0 ] && summary="$summary, ${unchanged} unchanged"
  [ "$backed_up" -gt 0 ] && summary="$summary (${backed_up} existing backed up)"
  echo "$summary."
  echo "${c_dim}Manifest: ${MANIFEST}${c_reset}"
  echo "${c_dim}If a skills/ or agents/ dir was created for the first time, restart Claude Code"
  echo "so it picks them up. Then run ${c_reset}${c_bold}/ship-issue <issue#>${c_reset}${c_dim}.${c_reset}"
fi
if [ "$swept" -gt 0 ]; then
  echo "${c_dim}Swept ${swept} legacy commands/*.md aside (kept as .bak-*) — this only covers ${TARGET}.${c_reset}"
  if [ "$SCOPE" != "global" ]; then
    echo "${c_dim}Re-run without --project/--dir to clear a global install too.${c_reset}"
  fi
fi
