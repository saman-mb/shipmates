#!/usr/bin/env bash
# Lint every extracted workflow shell script.
#
# actionlint shellchecks the inline `run:` blocks it can see, but a script that
# lives in .github/scripts/ is outside its reach — this is that gate. Keep it
# green: a finding here is a real defect, not a style opinion.
set -euo pipefail

if ! command -v shellcheck >/dev/null 2>&1; then
  sudo apt-get update -y
  sudo apt-get install -y shellcheck
fi

shellcheck .github/scripts/*.sh
