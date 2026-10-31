#!/usr/bin/env bash
# Assemble the pi-shipmates npm package from the pi payload this binary renders.
#
# The package is not a second renderer: every file in it comes out of the same
# adapter path `shipmates install --harness pi` uses. This script only repacks —
# it relocates steering to a prompt resource, adds the package manifest, and
# refuses to emit an artifact it cannot prove is complete.
#
# Environment:
#   VERSION        required, the npm version to publish (release tag without the "v")
#   OUT_DIR        required, directory the package root is written to
#   SRC_ROOT       repo root holding commands/, crew/, toolbox/   (default: .)
#   SHIPMATES_BIN  built binary to render with  (default: ./target/release/shipmates)
#   PKG_NAME       npm package name  (default: pi-shipmates)
set -euo pipefail

: "${VERSION:?VERSION is required}"
: "${OUT_DIR:?OUT_DIR is required}"
VERSION="${VERSION#v}"
SRC_ROOT="$(cd "${SRC_ROOT:-.}" && pwd)"
SHIPMATES_BIN="${SHIPMATES_BIN:-./target/release/shipmates}"
PKG_NAME="${PKG_NAME:-pi-shipmates}"

if [ ! -x "$SHIPMATES_BIN" ]; then
  echo "::error::$SHIPMATES_BIN is not an executable binary; build it first (cargo build --release --bin shipmates)"
  exit 1
fi

BUILT="$OUT_DIR/.built"
rm -rf "$BUILT"
mkdir -p "$BUILT" "$OUT_DIR/docs"

# One invocation yields the whole pi payload: crew, commands, toolbox, broker and
# steering. `build` writes under harnesses/<target>/; a package root does not nest.
"$SHIPMATES_BIN" build --target pi --root "$SRC_ROOT" --out "$BUILT" --with-tools all
if [ -d "$BUILT/harnesses/pi" ]; then
  ROOT="$BUILT/harnesses/pi"
else
  ROOT="$BUILT"
fi
rm -rf "$OUT_DIR/.pi"
cp -R "$ROOT/.pi" "$OUT_DIR/.pi"

# A pi package carries four resource types — extensions, skills, prompts, themes — and none of them
# fits contributor steering. It is reference documentation, not a reusable prompt: declaring it under
# `pi.prompts` would put `/contributor-steering` in the captain's slash menu and pour repository
# conventions into a prompt. It ships as plain, undeclared package content that the README points at.
# The install receipt and the `.shipmates/` tree are machine state and never enter the tarball.
STEERING="$ROOT/.shipmates/contributor-steering.md"
if [ -f "$STEERING" ]; then
  cp "$STEERING" "$OUT_DIR/docs/contributor-steering.md"
fi
cp "$SRC_ROOT/LICENSE" "$OUT_DIR/LICENSE"
cp "$SRC_ROOT/packaging/pi-shipmates/README.md" "$OUT_DIR/README.md"

# The broker's runtime pins live in one place — the adapter's own manifest — so
# the package never disagrees with the extension it ships.
node "$SRC_ROOT/packaging/pi-shipmates/write-manifest.mjs" \
  "$OUT_DIR/package.json" "$PKG_NAME" "$VERSION" "$SRC_ROOT/src/adapters/pi_crew/package.json"

# Expected counts are derived from the sources, never hard-coded, so this gate
# cannot rot when the crew or the toolbox grows.
want_agents=$(find "$SRC_ROOT/crew" -maxdepth 1 -name '*.md' | wc -l | tr -d ' ')
want_skills=$(find "$SRC_ROOT/commands" -maxdepth 1 -name '*.md' | wc -l | tr -d ' ')
want_tools=$(find "$SRC_ROOT/toolbox" -mindepth 1 -maxdepth 1 -type d | wc -l | tr -d ' ')
got_agents=$(find "$OUT_DIR/.pi/agents" -name '*.md' | wc -l | tr -d ' ')
got_skills=$(find "$OUT_DIR/.pi/skills" -name 'SKILL.md' | wc -l | tr -d ' ')
got_ext=$(find "$OUT_DIR/.pi/extensions" -name 'index.ts' | wc -l | tr -d ' ')
# The scratch render dir has done its job; leaving it behind would put machine
# state next to the package root and give the checks below something to lie about.
rm -rf "$BUILT"

echo "package: agents=$got_agents/$want_agents skills=$got_skills/$((want_skills + want_tools)) broker=$got_ext/1"
[ "$got_agents" -eq "$want_agents" ] || { echo "::error::crew agents incomplete"; exit 1; }
[ "$got_skills" -eq "$((want_skills + want_tools))" ] || { echo "::error::skills incomplete — commands and toolbox must both be packaged"; exit 1; }
[ "$got_ext" -eq 1 ] || { echo "::error::exactly one broker extension file must ship"; exit 1; }

for junk in __pycache__ .venv node_modules receipts .shipmates .built; do
  if [ -n "$(find "$OUT_DIR" -name "$junk" -print -quit)" ]; then
    echo "::error::$junk must not be packaged"; exit 1
  fi
done

# Final proof that npm itself accepts the manifest and the files allowlist, and
# that what npm would actually ship is a deliverable with no machine state in it.
cd "$OUT_DIR" && npm pack --dry-run --json | node "$SRC_ROOT/packaging/pi-shipmates/verify-tarball.mjs"
echo "packaged $PKG_NAME@$VERSION into $OUT_DIR"
