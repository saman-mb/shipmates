#!/usr/bin/env bash
# The exact commands that rendered the badge SVGs in this directory.
# Documentation only — badge.py is pure stdlib, no network, deterministic:
# re-running these reproduces the committed SVGs byte-for-byte.
set -euo pipefail

python3 ../../../../toolbox/badge/badge.py \
  --label build --message passing --color green \
  --out build.svg

python3 ../../../../toolbox/badge/badge.py \
  --label version --message v0.1.3 --color blue \
  --out version.svg

python3 ../../../../toolbox/badge/badge.py \
  --label coverage --message 98% --color brightgreen \
  --out coverage.svg
