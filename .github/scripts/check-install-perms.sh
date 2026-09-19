#!/usr/bin/env bash
# Verify install.sh has executable mode tracked in git.
set -euo pipefail

mode=$(git ls-files -s install.sh | awk '{print $1}')
if [ "$mode" != "100755" ]; then
  echo "install.sh mode is $mode, expected 100755"
  echo "Fix: git update-index --chmod=+x install.sh && git commit"
  exit 1
fi
