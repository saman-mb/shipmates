#!/usr/bin/env bash
#
# Shipmates enforcement — FSM state model + run-<issue>.json read/write library.
#
# This is brick 1 of the hook-enforced ship loop (issue #2): it owns the phase
# state machine and the on-disk `.shipmates/run-<issue>.json` document, and
# validates every phase transition before it commits. It does NOT yet wire the
# state into `/ship-issue` — that is a later story. It is a referee, not a jail:
# it cannot stop a rogue process from hand-editing the JSON. Its integrity
# contribution is fail-closed reads (jq-parse + schema_version check) and a hard
# "never a partial/corrupt file" guarantee via atomic temp-then-rename writes.
#
# Dual-mode:
#   source enforcement/lib/state.sh   # defines shipmates_state_* functions only
#   bash   enforcement/lib/state.sh <op> [args...]   # CLI dispatcher
#
# State dir defaults to $PWD/.shipmates; override with SHIPMATES_DIR (tests do).
#
# Frozen exit codes (downstream branches on these — do NOT change):
#   0 success · 1 illegal transition · 2 usage/bad arg · 3 missing dep (jq)
#   4 state file missing · 5 malformed / unsupported schema_version / IO error
# All errors go to stderr, prefixed "shipmates:". Normal output goes to stdout.

# ---------------------------------------------------------------------------
# Internal helpers (prefixed _shipmates_; not part of the public ABI).
# ---------------------------------------------------------------------------

_shipmates_err() {
  printf 'shipmates: %s\n' "$*" >&2
}

_shipmates_require_jq() {
  if ! command -v jq >/dev/null 2>&1; then
    _shipmates_err "jq is required but was not found on PATH"
    return 3
  fi
  return 0
}

# Issue ids are used to build file paths, so they are the primary injection /
# path-traversal surface. Accept only bare positive decimals (no leading zero).
_shipmates_validate_issue() {
  local issue="${1:-}"
  if [[ "$issue" =~ ^[1-9][0-9]*$ ]]; then
    return 0
  fi
  _shipmates_err "invalid issue id"
  return 2
}

_shipmates_dir() {
  printf '%s' "${SHIPMATES_DIR:-$PWD/.shipmates}"
}

_shipmates_file() {
  # $1 = validated issue id
  printf '%s/run-%s.json' "$(_shipmates_dir)" "$1"
}

_shipmates_now() {
  date -u +%Y-%m-%dT%H:%M:%SZ
}

# JSON-encode a bash string via jq (safe quoting) — used to build null-able
# fields without ever interpolating a value into a jq program.
_shipmates_json_string() {
  jq -n --arg s "$1" '$s'
}

# ---- phase model (§1) -------------------------------------------------------
# Encoded as case statements so the module pollutes no caller variables.

_shipmates_state_is_phase() {
  case "$1" in
    INIT|PLANNED|BUILT|PUSHED|CI_GREEN|ACCEPTED|DELIVERED|ESCALATED) return 0 ;;
    *) return 1 ;;
  esac
}

_shipmates_state_is_terminal() {
  case "$1" in
    DELIVERED|ESCALATED) return 0 ;;
    *) return 1 ;;
  esac
}

# The legal non-identity edges, verbatim from the spec's transition table.
_shipmates_state_is_edge() {
  case "$1>$2" in
    INIT'>'PLANNED|PLANNED'>'BUILT|BUILT'>'PUSHED|PUSHED'>'CI_GREEN|CI_GREEN'>'ACCEPTED|ACCEPTED'>'DELIVERED) return 0 ;;
    PUSHED'>'BUILT|CI_GREEN'>'BUILT) return 0 ;;  # fix/retry loop
    INIT'>'ESCALATED|PLANNED'>'ESCALATED|BUILT'>'ESCALATED|PUSHED'>'ESCALATED|CI_GREEN'>'ESCALATED|ACCEPTED'>'ESCALATED) return 0 ;;
    *) return 1 ;;
  esac
}

# ---- concurrency guard (§3.4) ----------------------------------------------
# mkdir is atomic on POSIX (flock is not portable to macOS). Best-effort: bounds
# the wait, steals an assumed-stale lock, and always releases via the callers.

_shipmates_lock_acquire() {
  local issue="$1" dir lock waited=0
  dir="$(_shipmates_dir)"
  lock="$dir/.lock-${issue}"
  mkdir -p "$dir" 2>/dev/null || { _shipmates_err "cannot create state dir: $dir"; return 5; }
  while ! mkdir "$lock" 2>/dev/null; do
    waited=$((waited + 1))
    if [[ $waited -gt 50 ]]; then          # ~5s ceiling
      rmdir "$lock" 2>/dev/null || true    # assume stale, steal once
      if mkdir "$lock" 2>/dev/null; then break; fi
      _shipmates_err "could not acquire lock for issue $issue: $lock"
      return 5
    fi
    sleep 0.1
  done
  _SHIPMATES_LOCK="$lock"
  return 0
}

_shipmates_lock_release() {
  if [[ -n "${_SHIPMATES_LOCK:-}" ]]; then
    rmdir "$_SHIPMATES_LOCK" 2>/dev/null || true
    _SHIPMATES_LOCK=""
  fi
}

# ---- fail-closed read + atomic replace (§3.4) ------------------------------

# Ensure FILE exists, parses as JSON, and is schema_version 1. 0 / 4 / 5.
_shipmates_check_file() {
  local file="$1" sv
  [[ -f "$file" ]] || { _shipmates_err "state file not found: $file"; return 4; }
  jq empty "$file" >/dev/null 2>&1 || { _shipmates_err "malformed state file: $file"; return 5; }
  sv="$(jq -r '.schema_version // empty' "$file" 2>/dev/null)" \
    || { _shipmates_err "cannot read schema_version: $file"; return 5; }
  if [[ "$sv" != "1" ]]; then
    _shipmates_err "unsupported schema_version: ${sv:-<missing>} (expected 1) in $file"
    return 5
  fi
  return 0
}

# Atomically produce FILE from `jq <argv...>` (argv must emit the new doc on
# stdout). Builds a temp IN THE SAME DIR, validates it with `jq empty`, then
# renames it over FILE. The temp is always cleaned up; on any failure FILE is
# left exactly as it was. Assumes the per-issue lock is already held.
# Returns 0 on success, 5 on any IO / build failure.
_shipmates_replace() {
  local issue="$1" file="$2"; shift 2
  local dir; dir="$(_shipmates_dir)"
  (
    set -uo pipefail
    tmp="$(mktemp "$dir/.run-${issue}.XXXXXX.tmp")" || exit 5
    trap 'rm -f "$tmp"' EXIT
    jq "$@" >"$tmp" 2>/dev/null || exit 5
    jq empty "$tmp" >/dev/null 2>&1 || exit 5
    mv -f "$tmp" "$file" 2>/dev/null || exit 5
  )
}

# ---------------------------------------------------------------------------
# Public API (§3). Every function is a pure `return`, never `exit`, so sourcing
# is safe. The CLI dispatcher below turns those returns into process exits.
# ---------------------------------------------------------------------------

# shipmates_state_assert_transition FROM TO — pure predicate, no I/O.
shipmates_state_assert_transition() {
  if [[ $# -ne 2 ]]; then
    _shipmates_err "usage: assert_transition FROM TO"
    return 2
  fi
  local from="$1" to="$2"
  _shipmates_state_is_phase "$from" || { _shipmates_err "unknown phase: $from"; return 2; }
  _shipmates_state_is_phase "$to"   || { _shipmates_err "unknown phase: $to";   return 2; }
  if [[ "$from" == "$to" ]]; then
    # Identity is legal only for non-terminal phases (field-only writes).
    if _shipmates_state_is_terminal "$from"; then
      _shipmates_err "illegal transition: $from -> $to"
      return 1
    fi
    return 0
  fi
  if _shipmates_state_is_edge "$from" "$to"; then
    return 0
  fi
  _shipmates_err "illegal transition: $from -> $to"
  return 1
}

# shipmates_state_init ISSUE [--branch B] [--worktree W] [--base BASE]
#                            [--merge-mode M] [--max-fix-rounds N]
# Idempotent create-if-absent. An existing valid file is left untouched; a
# malformed one is never overwritten (fail closed).
shipmates_state_init() {
  local issue="${1:-}"
  _shipmates_validate_issue "$issue" || return 2
  _shipmates_require_jq || return 3
  shift || true

  local branch_json=null worktree_json=null base_json=null
  local merge_mode="manual" max_fix_rounds=3
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --branch)         [[ $# -ge 2 ]] || { _shipmates_err "--branch needs a value"; return 2; };         branch_json="$(_shipmates_json_string "$2")"   || return 5; shift 2 ;;
      --worktree)       [[ $# -ge 2 ]] || { _shipmates_err "--worktree needs a value"; return 2; };       worktree_json="$(_shipmates_json_string "$2")" || return 5; shift 2 ;;
      --base)           [[ $# -ge 2 ]] || { _shipmates_err "--base needs a value"; return 2; };           base_json="$(_shipmates_json_string "$2")"     || return 5; shift 2 ;;
      --merge-mode)     [[ $# -ge 2 ]] || { _shipmates_err "--merge-mode needs a value"; return 2; };     merge_mode="$2"; shift 2 ;;
      --max-fix-rounds) [[ $# -ge 2 ]] || { _shipmates_err "--max-fix-rounds needs a value"; return 2; }; max_fix_rounds="$2"; shift 2 ;;
      *) _shipmates_err "unknown init option: $1"; return 2 ;;
    esac
  done
  case "$merge_mode" in manual|auto) ;; *) _shipmates_err "merge_mode must be manual|auto: $merge_mode"; return 2 ;; esac
  [[ "$max_fix_rounds" =~ ^[1-9][0-9]*$ ]] || { _shipmates_err "max_fix_rounds must be a positive integer: $max_fix_rounds"; return 2; }

  local file now rc=0
  file="$(_shipmates_file "$issue")"

  _shipmates_lock_acquire "$issue" || return $?
  if [[ -f "$file" ]] && jq empty "$file" >/dev/null 2>&1; then
    _shipmates_lock_release
    cat "$file"
    return 0
  fi
  if [[ -f "$file" ]]; then
    _shipmates_lock_release
    _shipmates_err "refusing to overwrite malformed state file: $file"
    return 5
  fi
  now="$(_shipmates_now)" || { _shipmates_lock_release; _shipmates_err "cannot read clock"; return 5; }
  _shipmates_replace "$issue" "$file" \
    -n \
    --argjson schema_version 1 \
    --argjson issue "$issue" \
    --argjson branch "$branch_json" \
    --argjson worktree "$worktree_json" \
    --argjson base_branch "$base_json" \
    --argjson max_fix_rounds "$max_fix_rounds" \
    --arg merge_mode "$merge_mode" \
    --arg now "$now" \
    '{
       schema_version: $schema_version,
       issue: $issue,
       phase: "INIT",
       pr: null,
       branch: $branch,
       worktree: $worktree,
       base_branch: $base_branch,
       ci: { status: "unknown", run_url: null, sha: null, checked_at: null },
       reviewed_sha: null,
       verdicts: {},
       fix_rounds: 0,
       max_fix_rounds: $max_fix_rounds,
       merge_mode: $merge_mode,
       created_at: $now,
       updated_at: $now
     }' || rc=$?
  _shipmates_lock_release
  if [[ $rc -ne 0 ]]; then
    _shipmates_err "init failed for issue $issue; state left unchanged"
    return "$rc"
  fi
  cat "$file"
}

# shipmates_state_load ISSUE [JQ_FILTER] — centralized fail-closed reader.
shipmates_state_load() {
  local issue="${1:-}"
  _shipmates_validate_issue "$issue" || return 2
  _shipmates_require_jq || return 3
  local filter="${2:-.}"
  local file; file="$(_shipmates_file "$issue")"
  _shipmates_check_file "$file" || return $?
  jq "$filter" "$file" || { _shipmates_err "jq filter failed for issue $issue"; return 5; }
}

# shipmates_state_status ISSUE [--human] — machine projection or one-liner.
shipmates_state_status() {
  local issue="" human=0
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --human) human=1; shift ;;
      --*) _shipmates_err "unknown status option: $1"; return 2 ;;
      *) if [[ -z "$issue" ]]; then issue="$1"; shift; else _shipmates_err "unexpected argument: $1"; return 2; fi ;;
    esac
  done
  _shipmates_validate_issue "$issue" || return 2
  _shipmates_require_jq || return 3
  local file; file="$(_shipmates_file "$issue")"
  _shipmates_check_file "$file" || return $?
  if [[ $human -eq 1 ]]; then
    jq -r '"issue \(.issue): \(.phase) [ci=\(.ci.status)] pr=\(.pr // "-") reviewed=\(.reviewed_sha // "-") fixes=\(.fix_rounds)/\(.max_fix_rounds) merge=\(.merge_mode)"' "$file" \
      || { _shipmates_err "status projection failed for issue $issue"; return 5; }
  else
    jq '{phase, ci, pr, branch, worktree, reviewed_sha, fix_rounds, max_fix_rounds, merge_mode}' "$file" \
      || { _shipmates_err "status projection failed for issue $issue"; return 5; }
  fi
}

# shipmates_state_write ISSUE [--to PHASE] [FIELD VALUE]...
# Transition-validated, atomic read-modify-write. Values are NEVER interpolated
# into the jq program: each whitelisted FIELD maps to a fixed jq path and its
# VALUE is passed via --arg / --argjson.
shipmates_state_write() {
  local issue="${1:-}"
  _shipmates_validate_issue "$issue" || return 2
  _shipmates_require_jq || return 3
  shift || true

  local to=""
  local -a fields=()
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --to) [[ $# -ge 2 ]] || { _shipmates_err "--to needs a phase"; return 2; }; to="$2"; shift 2 ;;
      --*)  _shipmates_err "unknown write option: $1"; return 2 ;;
      *)    [[ $# -ge 2 ]] || { _shipmates_err "field '$1' needs a value"; return 2; }; fields+=("$1" "$2"); shift 2 ;;
    esac
  done
  if [[ -n "$to" ]]; then
    _shipmates_state_is_phase "$to" || { _shipmates_err "unknown phase: $to"; return 2; }
  fi

  local now; now="$(_shipmates_now)" || { _shipmates_err "cannot read clock"; return 5; }
  local -a jq_args=(--arg now "$now")
  local filter='.updated_at = $now'
  if [[ -n "$to" ]]; then
    jq_args+=(--arg to "$to")
    filter="$filter | .phase = \$to"
  fi

  # Fixed whitelist: FIELD -> jq path. Unknown field is a usage error.
  local stamp_ci=0 idx=0 n=${#fields[@]}
  while (( idx < n )); do
    local fname="${fields[idx]}" fval="${fields[idx + 1]}" argn="f$idx" path=""
    case "$fname" in
      pr)             path=".pr";             [[ "$fval" =~ ^[0-9]+$ ]]    || { _shipmates_err "pr must be a non-negative integer: $fval"; return 2; };            jq_args+=(--argjson "$argn" "$fval") ;;
      fix_rounds)     path=".fix_rounds";     [[ "$fval" =~ ^[0-9]+$ ]]    || { _shipmates_err "fix_rounds must be a non-negative integer: $fval"; return 2; };    jq_args+=(--argjson "$argn" "$fval") ;;
      max_fix_rounds) path=".max_fix_rounds"; [[ "$fval" =~ ^[1-9][0-9]*$ ]] || { _shipmates_err "max_fix_rounds must be a positive integer: $fval"; return 2; }; jq_args+=(--argjson "$argn" "$fval") ;;
      branch)         path=".branch";         jq_args+=(--arg "$argn" "$fval") ;;
      base_branch)    path=".base_branch";    jq_args+=(--arg "$argn" "$fval") ;;
      worktree)       path=".worktree";       jq_args+=(--arg "$argn" "$fval") ;;
      reviewed_sha)   path=".reviewed_sha";   jq_args+=(--arg "$argn" "$fval") ;;
      merge_mode)     path=".merge_mode";     case "$fval" in manual|auto) ;; *) _shipmates_err "merge_mode must be manual|auto: $fval"; return 2 ;; esac; jq_args+=(--arg "$argn" "$fval") ;;
      ci_status)      path=".ci.status";      case "$fval" in unknown|pending|green|red) ;; *) _shipmates_err "ci_status must be unknown|pending|green|red: $fval"; return 2 ;; esac; jq_args+=(--arg "$argn" "$fval"); stamp_ci=1 ;;
      ci_run_url)     path=".ci.run_url";     jq_args+=(--arg "$argn" "$fval"); stamp_ci=1 ;;
      ci_sha)         path=".ci.sha";         jq_args+=(--arg "$argn" "$fval"); stamp_ci=1 ;;
      *) _shipmates_err "unknown field: $fname"; return 2 ;;
    esac
    filter="$filter | $path = \$$argn"
    idx=$((idx + 2))
  done
  if (( stamp_ci )); then
    filter="$filter | .ci.checked_at = \$now"
  fi

  local file; file="$(_shipmates_file "$issue")"
  _shipmates_check_file "$file" || return $?

  _shipmates_lock_acquire "$issue" || return $?
  local rc=0 cur_phase target
  cur_phase="$(jq -r '.phase // empty' "$file" 2>/dev/null)" || rc=5
  if [[ $rc -eq 0 ]]; then
    target="${to:-$cur_phase}"
    if shipmates_state_assert_transition "$cur_phase" "$target"; then
      _shipmates_replace "$issue" "$file" "${jq_args[@]}" "$filter" "$file" || rc=$?
    else
      rc=$?
    fi
  fi
  _shipmates_lock_release
  if [[ $rc -ne 0 ]]; then
    [[ $rc -eq 5 ]] && _shipmates_err "write failed for issue $issue; state left unchanged"
    return "$rc"
  fi
  cat "$file"
}

# shipmates_state_record_verdict ISSUE ROLE VERDICT [SHA]
# Identity write (must be at a non-terminal phase). ROLE is passed as a jq
# variable key, so it can never be interpolated into the program.
shipmates_state_record_verdict() {
  local issue="${1:-}" role="${2:-}" verdict="${3:-}" sha="${4:-}"
  _shipmates_validate_issue "$issue" || return 2
  _shipmates_require_jq || return 3
  [[ -n "$role" ]]    || { _shipmates_err "record_verdict requires ROLE"; return 2; }
  [[ -n "$verdict" ]] || { _shipmates_err "record_verdict requires VERDICT"; return 2; }
  case "$verdict" in
    ACCEPT|ACCEPT-WITH-NITS|REJECT|PASS|FAIL) ;;
    *) _shipmates_err "invalid verdict: $verdict"; return 2 ;;
  esac

  local file; file="$(_shipmates_file "$issue")"
  _shipmates_check_file "$file" || return $?
  local now; now="$(_shipmates_now)" || { _shipmates_err "cannot read clock"; return 5; }
  local sha_json=null
  if [[ -n "$sha" ]]; then sha_json="$(_shipmates_json_string "$sha")" || return 5; fi

  _shipmates_lock_acquire "$issue" || return $?
  local rc=0 cur_phase
  cur_phase="$(jq -r '.phase // empty' "$file" 2>/dev/null)" || rc=5
  if [[ $rc -eq 0 ]]; then
    if shipmates_state_assert_transition "$cur_phase" "$cur_phase"; then
      _shipmates_replace "$issue" "$file" \
        --arg role "$role" --arg verdict "$verdict" --argjson sha "$sha_json" --arg now "$now" \
        '.verdicts[$role] = { verdict: $verdict, sha: $sha, at: $now } | .updated_at = $now' \
        "$file" || rc=$?
    else
      rc=$?
    fi
  fi
  _shipmates_lock_release
  if [[ $rc -ne 0 ]]; then
    [[ $rc -eq 5 ]] && _shipmates_err "record_verdict failed for issue $issue; state left unchanged"
    return "$rc"
  fi
  cat "$file"
}

# ---------------------------------------------------------------------------
# CLI dispatcher (only when executed, never when sourced).
# ---------------------------------------------------------------------------

_shipmates_usage() {
  cat >&2 <<'USAGE'
usage: state.sh <op> [args...]
  init ISSUE [--branch B] [--worktree W] [--base BASE] [--merge-mode M] [--max-fix-rounds N]
  load ISSUE [JQ_FILTER]
  write ISSUE [--to PHASE] [FIELD VALUE]...
  status ISSUE [--human]
  record-verdict ISSUE ROLE VERDICT [SHA]
  assert-transition FROM TO
USAGE
}

_shipmates_dispatch() {
  local op="${1:-}"
  [[ $# -gt 0 ]] && shift
  case "$op" in
    init)              shipmates_state_init "$@" ;;
    load)              shipmates_state_load "$@" ;;
    write)             shipmates_state_write "$@" ;;
    status)            shipmates_state_status "$@" ;;
    record-verdict)    shipmates_state_record_verdict "$@" ;;
    assert-transition) shipmates_state_assert_transition "$@" ;;
    ""|-h|--help|help) _shipmates_usage; return 2 ;;
    *) _shipmates_err "unknown op: $op"; _shipmates_usage; return 2 ;;
  esac
}

if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
  set -uo pipefail
  _shipmates_dispatch "$@"
  exit $?
fi
