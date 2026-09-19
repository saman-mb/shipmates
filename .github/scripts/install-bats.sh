#!/usr/bin/env bash
# Install bats-core and assertion libraries (pinned versions).
# Environment: BATS_LIB_PATH, RUNNER_TEMP, GITHUB_PATH, GITHUB_ENV
set -euo pipefail

mkdir -p "$BATS_LIB_PATH" "$RUNNER_TEMP/bats-bin"
git clone --depth 1 --branch v1.14.0 https://github.com/bats-core/bats-core "$RUNNER_TEMP/bats-core"
git clone --depth 1 --branch v0.3.0 https://github.com/bats-core/bats-support "$BATS_LIB_PATH/bats-support"
git clone --depth 1 --branch v2.2.4 https://github.com/bats-core/bats-assert "$BATS_LIB_PATH/bats-assert"
ln -s "$RUNNER_TEMP/bats-core/bin/bats" "$RUNNER_TEMP/bats-bin/bats"
echo "$RUNNER_TEMP/bats-bin" >> "$GITHUB_PATH"
echo "BATS_LIB_PATH=$BATS_LIB_PATH" >> "$GITHUB_ENV"
