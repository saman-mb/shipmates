#!/usr/bin/env bash
# Install cargo-dist from its release artifact, verifying the published checksum
# before trusting the binary — never pipe a download straight into a shell.
# Environment: DIST_VERSION, DIST_TARGET, HOME, GITHUB_PATH
#
# DIST_TARGET is the host the dist binary runs on (matrix.host), not the
# targets it cross-compiles. Unix archives are tar.xz with
# cargo-dist-$target/dist; Windows (v0.32.0) is a zip with dist.exe at the root.
set -euo pipefail

: "${DIST_VERSION:?DIST_VERSION is required}"
: "${DIST_TARGET:?DIST_TARGET is required}"
: "${HOME:?HOME is required}"

hash_sha256() {
  local artifact="$1"
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$artifact" | awk '{print $1}'
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$artifact" | awk '{print $1}'
  elif command -v python3 >/dev/null 2>&1; then
    python3 -c 'import hashlib, sys
h = hashlib.sha256()
with open(sys.argv[1], "rb") as fh:
    for chunk in iter(lambda: fh.read(1024 * 1024), b""):
        h.update(chunk)
print(h.hexdigest())' "$artifact"
  elif command -v python >/dev/null 2>&1; then
    python -c 'import hashlib, sys
h = hashlib.sha256()
with open(sys.argv[1], "rb") as fh:
    for chunk in iter(lambda: fh.read(1024 * 1024), b""):
        h.update(chunk)
print(h.hexdigest())' "$artifact"
  else
    echo "error: need sha256sum, shasum -a 256, or python3 to verify checksums" >&2
    exit 1
  fi
}

verify_sha256() {
  local artifact="$1"
  local sumfile="$2"
  local expected actual
  expected="$(awk '{print $1; exit}' "$sumfile" | tr 'A-F' 'a-f')"
  if ! printf '%s' "$expected" | grep -Eq '^[0-9a-f]{64}$'; then
    echo "error: ${sumfile} does not start with a sha256 hex digest" >&2
    exit 1
  fi
  actual="$(hash_sha256 "$artifact" | tr 'A-F' 'a-f')"
  if [ "$actual" != "$expected" ]; then
    echo "error: checksum mismatch for ${artifact}" >&2
    echo "expected: ${expected}" >&2
    echo "actual:   ${actual}" >&2
    exit 1
  fi
}

install_bin() {
  local src="$1"
  local dest="$2"
  mkdir -p "$(dirname "$dest")"
  if command -v install >/dev/null 2>&1; then
    install -m755 "$src" "$dest"
  else
    cp "$src" "$dest"
    chmod 755 "$dest"
  fi
}

extract_zip() {
  local zipfile="$1"
  if command -v unzip >/dev/null 2>&1; then
    unzip -o "$zipfile"
  elif command -v python3 >/dev/null 2>&1; then
    python3 -c 'import sys, zipfile; zipfile.ZipFile(sys.argv[1]).extractall()' "$zipfile"
  elif command -v python >/dev/null 2>&1; then
    python -c 'import sys, zipfile; zipfile.ZipFile(sys.argv[1]).extractall()' "$zipfile"
  else
    echo "error: need unzip or python to extract ${zipfile}" >&2
    exit 1
  fi
}

is_windows=0
# Host triples such as x86_64-pc-windows-msvc contain "windows".
case "$DIST_TARGET" in
  *windows*) is_windows=1 ;;
esac

base="https://github.com/axodotdev/cargo-dist/releases/download/v${DIST_VERSION}"
work="$(mktemp -d "${TMPDIR:-/tmp}/dist-XXXXXX")"
cleanup() { rm -rf "$work"; }
trap cleanup EXIT
cd "$work"

if [ "$is_windows" -eq 1 ]; then
  asset="cargo-dist-${DIST_TARGET}.zip"
else
  asset="cargo-dist-${DIST_TARGET}.tar.xz"
fi

curl --proto '=https' --tlsv1.2 -LsSf -o "$asset" "${base}/${asset}"
curl --proto '=https' --tlsv1.2 -LsSf -o "${asset}.sha256" "${base}/${asset}.sha256"
verify_sha256 "$asset" "${asset}.sha256"

if [ "$is_windows" -eq 1 ]; then
  extract_zip "$asset"
  if [ ! -f dist.exe ]; then
    echo "error: expected dist.exe at zip root of ${asset}" >&2
    exit 1
  fi
  install_bin dist.exe "$HOME/.cargo/bin/dist.exe"
else
  tar -xJf "$asset"
  src="cargo-dist-${DIST_TARGET}/dist"
  if [ ! -f "$src" ]; then
    echo "error: expected ${src} in ${asset}" >&2
    exit 1
  fi
  install_bin "$src" "$HOME/.cargo/bin/dist"
fi

if [ -n "${GITHUB_PATH:-}" ]; then
  echo "$HOME/.cargo/bin" >> "$GITHUB_PATH"
fi
