#!/usr/bin/env bash
# End-to-end CI attestation proof with fake gh responses.

set -uo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN="$REPO/target/debug/shipmates"
SHIM="$REPO/enforcement/hooks/claude-code/fsm-gate.sh"
(cd "$REPO" && cargo build --quiet) || exit 1

WORK="$(mktemp -d)"
BINDIR="$(mktemp -d)"
trap 'rm -rf "$WORK" "$BINDIR"' EXIT

GIT="$WORK/repo"
mkdir -p "$GIT"
git -C "$GIT" init -q
git -C "$GIT" config user.email test@example.invalid
git -C "$GIT" config user.name test
git -C "$GIT" commit -q --allow-empty -m init
git -C "$GIT" branch -m feat/issue-42-ci
SHA="$(git -C "$GIT" rev-parse HEAD)"

cat > "$BINDIR/gh" <<'GH'
#!/usr/bin/env bash
case "$*" in
  *"pr view"*) printf '{"headRefOid":"%s"}\n' "$FAKE_SHA" ;;
  *"pr checks"*) printf '[{"bucket":"pass"},{"bucket":"skipping"}]\n' ;;
  *) exit 1 ;;
esac
GH
chmod +x "$BINDIR/gh"
ln -s "$BIN" "$BINDIR/shipmates"
export FAKE_SHA="$SHA"
export PATH="$BINDIR:$PATH"

"$BIN" state init --dir "$GIT" --run 42 --command ship-issue >/dev/null
for phase in isolate build verify review deliver; do
    "$BIN" state advance --dir "$GIT" --run 42 --to "$phase" >/dev/null || exit 1
done
"$BIN" state ci-attest --dir "$GIT" --run 42 --pr 249 >/dev/null || exit 1

payload="$(python3 -c 'import json,sys; print(json.dumps({"tool_name":"Bash","tool_input":{"command":"gh pr merge 249 --squash"},"cwd":sys.argv[1]}))' "$GIT")"
out="$(printf '%s' "$payload" | SHIPMATES_NATIVE_HOOK=1 bash "$SHIM")"
[ -z "$out" ] || { printf 'fresh attestation should allow merge: %s\n' "$out"; exit 1; }

FAKE_SHA="stale-remote"
export FAKE_SHA
out="$(printf '%s' "$payload" | SHIPMATES_NATIVE_HOOK=1 bash "$SHIM")"
decision="$(printf '%s' "$out" | python3 -c 'import json,sys; print(json.load(sys.stdin)["hookSpecificOutput"]["permissionDecision"])')"
[ "$decision" = deny ] || { printf 'remote head change should deny merge: %s\n' "$out"; exit 1; }

printf 'CI attestation gate: 2 passed\n'
