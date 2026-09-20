#!/usr/bin/env bash
# Enable Git longpaths on Windows runners.
set -euo pipefail

git config --global core.longpaths true
