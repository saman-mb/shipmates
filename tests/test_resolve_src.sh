#!/usr/bin/env bash
#
# Regression test for #81: curl|bash must not install from $PWD when the
# directory looks like a Shipmates checkout.
#
# Covers: piped-from-stdin must download (not trust decoy), real-file from
# true checkout must stay offline, real-file from look-alike dir missing the
# fingerprint must download.
#
#   bash tests/test_resolve_src.sh
#
# Exit 0 = all passed, 1 = at least one failure.

set -uo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
INSTALLER="$REPO/install.sh"

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

export HOME="$WORK/home"
mkdir -p "$HOME"
unset CLAUDE_CONFIG_DIR

PASS=0; FAIL=0
ok()  { PASS=$((PASS+1)); printf 'ok   %s\n' "$1"; }
bad() { FAIL=$((FAIL+1)); printf 'FAIL %s\n' "$1"; }

assert() {
  local desc="$1"; shift
  if "$@" >/dev/null 2>&1; then ok "$desc"; else bad "$desc"; fi
}

# --- curl stub: logs invocation, exits 1 (no network) ---
CURL_STUB="$WORK/bin"
mkdir -p "$CURL_STUB"
cat > "$CURL_STUB/curl" <<'STUB'
#!/usr/bin/env bash
echo "curl $*" >> "$CURL_LOG"
exit 1
STUB
chmod +x "$CURL_STUB/curl"
export CURL_LOG="$WORK/curl.log"
export PATH="$CURL_STUB:$PATH"

# --- decoy directory: looks like Shipmates but isn't ---
DECOY="$WORK/decoy"
mkdir -p "$DECOY/agents" "$DECOY/skills/ship-issue"
echo "decoy agent" > "$DECOY/agents/sdet.md"
echo "decoy skill" > "$DECOY/skills/ship-issue/SKILL.md"
cat > "$DECOY/install.sh" <<'DECOY'
#!/usr/bin/env bash
echo "decoy"
DECOY
chmod +x "$DECOY/install.sh"

# --- Case 1: piped from decoy dir → must download, not trust decoy ---

(
  cd "$DECOY"
  cat "$INSTALLER" | bash -s -- --dir "$WORK/case1" 2>/dev/null
)
assert "piped: curl stub called (download attempted)" grep -q curl "$CURL_LOG"
assert "piped: decoy agent NOT installed" test ! -f "$WORK/case1/agents/sdet.md"
assert "piped: decoy skill NOT installed" test ! -f "$WORK/case1/skills/ship-issue/SKILL.md"
assert "piped: install dir NOT created by decoy" test ! -d "$WORK/case1"

# --- Case 2: positive control — real checkout installs offline ---

rm -f "$CURL_LOG"
# Capture the status: the assertions below check side effects, and a partial or
# late failure that still wrote files would otherwise pass unnoticed.
checkout_rc=0
bash "$INSTALLER" --dir "$WORK/case2" >/dev/null 2>&1 || checkout_rc=$?
assert "checkout: installer exits 0" test "$checkout_rc" -eq 0
assert "checkout: no curl call (offline)" test ! -s "$CURL_LOG"
assert "checkout: skills installed" test -f "$WORK/case2/skills/ship-issue/SKILL.md"
assert "checkout: agents installed" test -f "$WORK/case2/agents/sdet.md"
assert "checkout: manifest written" test -f "$WORK/case2/shipmates/manifest"

# --- Case 3: real-file from look-alike missing fingerprint → download ---

LOOKALIKE="$WORK/lookalike"
mkdir -p "$LOOKALIKE/agents" "$LOOKALIKE/skills"
echo "lookalike" > "$LOOKALIKE/agents/sdet.md"
echo "lookalike" > "$LOOKALIKE/skills/some-other-skill.md"
cp "$INSTALLER" "$LOOKALIKE/install.sh"
chmod +x "$LOOKALIKE/install.sh"

rm -f "$CURL_LOG"
(
  cd "$LOOKALIKE"
  bash "$LOOKALIKE/install.sh" --dir "$WORK/case3" 2>/dev/null
)
assert "lookalike: curl stub called (download attempted)" grep -q curl "$CURL_LOG"
assert "lookalike: nothing installed from lookalike" test ! -d "$WORK/case3"

# --- summary ---

echo
echo "passed: $PASS, failed: $FAIL"
[ "$FAIL" -eq 0 ]
