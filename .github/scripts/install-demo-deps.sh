#!/usr/bin/env bash
# Install pinned Pillow and DejaVu Sans Mono for deterministic demo GIF rendering.
set -euo pipefail

sudo apt-get update -y
sudo apt-get install -y fonts-dejavu-core=2.37-8
pip install "Pillow==12.3.0"
