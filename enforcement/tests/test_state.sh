#!/usr/bin/env bash
#
# Dependency-free unit tests for enforcement/lib/state.sh (no bats — bash + jq +
# coreutils only). Each case runs in an isolated SHIPMATES_DIR scratch dir that
# is cleaned up on exit. Prints PASS/FAIL per assertion plus a summary, and
# exits non-zero if any assertion fails — this is the CI gate (spec §5/§6).
#
#   bash enforcement/tests/test_state.sh

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
LIB="$HERE/../lib/state.sh"
[[ -f "$LIB" ]] || { echo "cannot find state.sh at $LIB" >&2; exit 2; }

BASHBIN="$(command -v bash)"
ORIG_PATH="$PATH"

PASS=0
FAIL=0
CLEAN_DIRS=()

cleanup() {
  local d
  for d in "${CLEAN_DIRS[@]:-}"; do
    [[ -n "$d" ]] && rm -rf "$d"
  done
}
trap cleanup EXIT

# ---- harness --------------------------------------------------------------

ok()  { PASS=$((PASS + 1)); printf '  PASS  %s\n' "$1"; }
bad() { FAIL=$((FAIL + 1)); printf '  FAIL  %s\n' "$1"; }

banner() { printf '\nCase %s: %s\n' "$1" "$2"; }

new_dir() {
  SHIPMATES_DIR="$(mktemp -d)"
  export SHIPMATES_DIR
  CLEAN_DIRS+=("$SHIPMATES_DIR")
}

# Run the CLI, capturing OUT / ERR / STATUS.
run_state() {
  local errf; errf="$(mktemp)"
  OUT="$(bash "$LIB" "$@" 2>"$errf")"; STATUS=$?
  ERR="$(cat "$errf")"; rm -f "$errf"
}

expect_ok() {  # DESC OP ARGS...
  local desc="$1"; shift
  run_state "$@"
  if [[ "$STATUS" -eq 0 ]]; then ok "$desc"; else bad "$desc (exit $STATUS; stderr: $ERR)"; fi
}

expect_exit() {  # CODE DESC OP ARGS...
  local code="$1" desc="$2"; shift 2
  run_state "$@"
  if [[ "$STATUS" -eq "$code" ]]; then ok "$desc"; else bad "$desc (want exit $code, got $STATUS; stderr: $ERR)"; fi
}

expect_eq() {  # DESC ACTUAL EXPECTED
  if [[ "$2" == "$3" ]]; then ok "$1"; else bad "$1 (want '$3', got '$2')"; fi
}

expect_stderr_match() {  # DESC NEEDLE  (inspects last captured $ERR)
  if [[ "$ERR" == *"$2"* ]]; then ok "$1"; else bad "$1 (stderr lacked '$2': $ERR)"; fi
}

# ---- state helpers used by the cases --------------------------------------

phase_of() { jq -r '.phase' "$SHIPMATES_DIR/run-$1.json"; }

# reach ISSUE PHASE — init the issue then walk the canonical forward path to
# PHASE (a main-line phase). Returns non-zero if any step is rejected.
reach() {
  local issue="$1" target="$2" p
  bash "$LIB" init "$issue" >/dev/null 2>&1 || return 1
  [[ "$target" == "INIT" ]] && return 0
  for p in PLANNED BUILT PUSHED CI_GREEN ACCEPTED DELIVERED; do
    bash "$LIB" write "$issue" --to "$p" >/dev/null 2>&1 || return 1
    [[ "$p" == "$target" ]] && return 0
  done
  return 1
}

# ---------------------------------------------------------------------------
# Cases
# ---------------------------------------------------------------------------

case01() {
  banner 1 "init creates a valid file at INIT with defaults"
  new_dir
  expect_ok "init 1 exits 0" init 1
  local f="$SHIPMATES_DIR/run-1.json"
  [[ -f "$f" ]] && ok "run-1.json exists" || bad "run-1.json exists"
  jq empty "$f" >/dev/null 2>&1 && ok "jq empty passes" || bad "jq empty passes"
  expect_eq "schema_version == 1"      "$(jq -r '.schema_version' "$f")" "1"
  expect_eq "phase == INIT"            "$(jq -r '.phase' "$f")"          "INIT"
  expect_eq "pr defaults null"         "$(jq -r '.pr' "$f")"             "null"
  expect_eq "fix_rounds defaults 0"    "$(jq -r '.fix_rounds' "$f")"     "0"
  expect_eq "max_fix_rounds defaults 3" "$(jq -r '.max_fix_rounds' "$f")" "3"
  expect_eq "merge_mode defaults manual" "$(jq -r '.merge_mode' "$f")"   "manual"
  expect_eq "ci.status defaults unknown" "$(jq -r '.ci.status' "$f")"    "unknown"
  expect_eq "verdicts defaults {}"     "$(jq -c '.verdicts' "$f")"       "{}"
}

case02() {
  banner 2 "init is idempotent — never clobbers progress"
  new_dir
  bash "$LIB" init 1 >/dev/null
  bash "$LIB" write 1 --to PLANNED >/dev/null
  bash "$LIB" write 1 --to BUILT >/dev/null
  expect_ok "re-init exits 0" init 1
  expect_eq "phase still BUILT after re-init" "$(phase_of 1)" "BUILT"
}

case03() {
  banner 3 "every legal forward edge INIT..DELIVERED"
  new_dir
  expect_ok "init 1" init 1
  local edge
  for edge in PLANNED BUILT PUSHED CI_GREEN ACCEPTED DELIVERED; do
    expect_ok "forward -> $edge exits 0" write 1 --to "$edge"
  done
  expect_eq "final phase DELIVERED" "$(phase_of 1)" "DELIVERED"
}

case04() {
  banner 4 "fix/retry edges PUSHED->BUILT and CI_GREEN->BUILT"
  new_dir
  reach 1 PUSHED
  expect_ok "PUSHED -> BUILT" write 1 --to BUILT
  expect_ok "BUILT -> PUSHED" write 1 --to PUSHED
  expect_ok "PUSHED -> CI_GREEN" write 1 --to CI_GREEN
  expect_ok "CI_GREEN -> BUILT" write 1 --to BUILT
}

case05() {
  banner 5 "escalation reachable from every non-terminal phase"
  new_dir
  local ph i=1
  for ph in INIT PLANNED BUILT PUSHED CI_GREEN ACCEPTED; do
    reach "$i" "$ph"
    expect_ok "$ph -> ESCALATED" write "$i" --to ESCALATED
    i=$((i + 1))
  done
}

check_illegal() {  # ISSUE FROM TO
  local issue="$1" from="$2" to="$3"
  reach "$issue" "$from"
  expect_exit 1 "$from -> $to is illegal (exit 1)" write "$issue" --to "$to"
  expect_stderr_match "$from -> $to reports 'illegal transition'" "illegal transition"
}

case06() {
  banner 6 "illegal transitions exit 1 with a clear message"
  new_dir
  check_illegal 1 INIT     BUILT
  check_illegal 2 PLANNED  CI_GREEN
  check_illegal 3 BUILT    ACCEPTED
  check_illegal 4 PUSHED   ACCEPTED
  check_illegal 5 CI_GREEN DELIVERED
  check_illegal 6 ACCEPTED CI_GREEN
}

case07() {
  banner 7 "terminal phases are frozen (no outgoing, no identity)"
  new_dir
  reach 1 DELIVERED
  expect_exit 1 "DELIVERED -> DELIVERED frozen" write 1 --to DELIVERED
  expect_exit 1 "DELIVERED -> PLANNED frozen"   write 1 --to PLANNED
  bash "$LIB" init 2 >/dev/null
  bash "$LIB" write 2 --to ESCALATED >/dev/null
  expect_exit 1 "ESCALATED -> ESCALATED frozen" write 2 --to ESCALATED
  expect_exit 1 "ESCALATED -> PLANNED frozen"   write 2 --to PLANNED
}

case08() {
  banner 8 "identity writes allowed on a non-terminal phase"
  new_dir
  reach 1 CI_GREEN
  expect_ok "write ci_status green (identity)" write 1 ci_status green
  expect_eq "stays CI_GREEN after ci write" "$(phase_of 1)" "CI_GREEN"
  expect_ok "record-verdict at CI_GREEN" record-verdict 1 product-manager ACCEPT
  expect_eq "stays CI_GREEN after verdict" "$(phase_of 1)" "CI_GREEN"
  expect_eq "verdict recorded" \
    "$(jq -r '.verdicts["product-manager"].verdict' "$SHIPMATES_DIR/run-1.json")" "ACCEPT"
}

case09() {
  banner 9 "atomic write — a mid-write failure leaves no partial file / residue"
  new_dir
  bash "$LIB" init 1 >/dev/null
  local before; before="$(cat "$SHIPMATES_DIR/run-1.json")"

  # Shadow `mv` with a stub that always fails, simulating a crash at commit.
  local fakebin; fakebin="$(mktemp -d)"; CLEAN_DIRS+=("$fakebin")
  printf '#!/usr/bin/env bash\nexit 1\n' > "$fakebin/mv"
  chmod +x "$fakebin/mv"

  local errf; errf="$(mktemp)"
  PATH="$fakebin:$ORIG_PATH" bash "$LIB" write 1 reviewed_sha shouldnotpersist \
    >/dev/null 2>"$errf"; STATUS=$?
  ERR="$(cat "$errf")"; rm -f "$errf"

  [[ "$STATUS" -ne 0 ]] && ok "commit failure surfaced (exit $STATUS)" \
    || bad "commit failure surfaced (got exit 0)"
  jq empty "$SHIPMATES_DIR/run-1.json" >/dev/null 2>&1 && ok "target still parses" \
    || bad "target still parses"
  expect_eq "target equals prior content" "$(cat "$SHIPMATES_DIR/run-1.json")" "$before"
  local resid; resid="$(find "$SHIPMATES_DIR" -maxdepth 1 -name '.run-*.tmp' | wc -l | tr -d ' ')"
  expect_eq "no .run-*.tmp residue left behind" "$resid" "0"
}

case10() {
  banner 10 "malformed file — reads/writes fail 5 and never overwrite it"
  new_dir
  printf 'not json\n' > "$SHIPMATES_DIR/run-1.json"
  expect_exit 5 "load malformed -> 5" load 1
  expect_stderr_match "malformed message" "malformed"
  expect_exit 5 "status malformed -> 5" status 1
  expect_exit 5 "write malformed -> 5" write 1 --to PLANNED
  expect_eq "malformed file untouched" "$(cat "$SHIPMATES_DIR/run-1.json")" "not json"
}

case11() {
  banner 11 "missing file — reads/writes fail 4 with a clear message"
  new_dir
  expect_exit 4 "load missing -> 4" load 1
  expect_stderr_match "missing message" "not found"
  expect_exit 4 "status missing -> 4" status 1
  expect_exit 4 "write missing -> 4" write 1 --to PLANNED
}

case12() {
  banner 12 "invalid / injection issue ids are rejected with exit 2"
  new_dir
  local bad
  for bad in "../../etc/passwd" "2; rm -rf /" '$(echo pwned)' "2 3" "" "0" "007"; do
    expect_exit 2 "reject id [$bad]" load "$bad"
  done
  local cnt; cnt="$(find "$SHIPMATES_DIR" -type f 2>/dev/null | wc -l | tr -d ' ')"
  expect_eq "nothing created for rejected ids" "$cnt" "0"
}

case13() {
  banner 13 "unsupported schema_version fails closed with exit 5"
  new_dir
  printf '{"schema_version":999,"issue":1,"phase":"INIT"}\n' > "$SHIPMATES_DIR/run-1.json"
  expect_exit 5 "schema_version 999 -> 5" load 1
  expect_stderr_match "unsupported schema message" "schema_version"
}

case14() {
  banner 14 "missing jq dependency -> exit 3"
  new_dir
  local fakebin t p
  fakebin="$(mktemp -d)"; CLEAN_DIRS+=("$fakebin")
  # Symlink the coreutils the lib may use, but deliberately NOT jq.
  for t in bash sh env cat date mkdir mktemp mv rm sleep dirname grep chmod ln cp find; do
    p="$(command -v "$t" 2>/dev/null)" && ln -s "$p" "$fakebin/$t" 2>/dev/null
  done
  local errf; errf="$(mktemp)"
  PATH="$fakebin" "$BASHBIN" "$LIB" init 1 >/dev/null 2>"$errf"; STATUS=$?
  ERR="$(cat "$errf")"; rm -f "$errf"
  expect_eq "missing jq -> exit 3" "$STATUS" "3"
  expect_stderr_match "reports jq is required" "jq is required"
}

# ---------------------------------------------------------------------------

case01; case02; case03; case04; case05; case06; case07
case08; case09; case10; case11; case12; case13; case14

printf '\n----------------------------------------\n'
printf '%d passed, %d failed\n' "$PASS" "$FAIL"
[[ "$FAIL" -eq 0 ]] || exit 1
exit 0
