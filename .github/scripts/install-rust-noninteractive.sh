#!/usr/bin/env bash
# Install Rust non-interactively if not already present (used in container builds).
# Version-pinned rustup-init from the rustup archive, checksum-verified — never
# pipe an installer download into a shell.
set -euo pipefail

RUSTUP_VERSION="1.29.1"

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

linux_triple() {
  local sys arch
  sys="$(uname -s)"
  arch="$(uname -m)"
  case "$sys" in
    Linux) ;;
    *)
      echo "error: rustup-init pin is linux-only (container builds); uname -s=${sys}" >&2
      exit 1
      ;;
  esac
  case "$arch" in
    x86_64|amd64) echo "x86_64-unknown-linux-gnu" ;;
    aarch64|arm64) echo "aarch64-unknown-linux-gnu" ;;
    *)
      echo "error: unsupported linux arch for rustup-init: ${arch}" >&2
      exit 1
      ;;
  esac
}

if ! command -v cargo > /dev/null 2>&1; then
  triple="$(linux_triple)"
  base="https://static.rust-lang.org/rustup/archive/${RUSTUP_VERSION}/${triple}"
  work="$(mktemp -d "${TMPDIR:-/tmp}/rustup-XXXXXX")"
  cleanup() { rm -rf "$work"; }
  trap cleanup EXIT
  curl --proto '=https' --tlsv1.2 -sSf -o "$work/rustup-init" "${base}/rustup-init"
  curl --proto '=https' --tlsv1.2 -sSf -o "$work/rustup-init.sha256" "${base}/rustup-init.sha256"
  verify_sha256 "$work/rustup-init" "$work/rustup-init.sha256"
  chmod +x "$work/rustup-init"
  "$work/rustup-init" -y
  echo "$HOME/.cargo/bin" >> "$GITHUB_PATH"
fi
