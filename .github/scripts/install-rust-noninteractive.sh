#!/usr/bin/env bash
# Install Rust non-interactively if not already present (used in container builds).
set -euo pipefail

if ! command -v cargo > /dev/null 2>&1; then
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
  echo "$HOME/.cargo/bin" >> "$GITHUB_PATH"
fi
