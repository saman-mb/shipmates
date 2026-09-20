#!/usr/bin/env bash
# Install cargo-dist from its release artifact, verifying the published checksum
# before trusting the binary — never pipe a download straight into a shell.
# Environment: DIST_VERSION, DIST_TARGET, HOME, GITHUB_PATH
set -euo pipefail

base="https://github.com/axodotdev/cargo-dist/releases/download/v${DIST_VERSION}"
asset="cargo-dist-${DIST_TARGET}.tar.xz"
curl --proto '=https' --tlsv1.2 -LsSf -o "$asset" "${base}/${asset}"
curl --proto '=https' --tlsv1.2 -LsSf -o "${asset}.sha256" "${base}/${asset}.sha256"
sha256sum -c "${asset}.sha256"
tar -xJf "$asset"
mkdir -p "$HOME/.cargo/bin"
install -m755 "cargo-dist-${DIST_TARGET}/dist" "$HOME/.cargo/bin/dist"
