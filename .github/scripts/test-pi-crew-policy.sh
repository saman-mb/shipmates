#!/usr/bin/env bash
# Policy decisions for the pi crew switch. No network. No model id.
set -euo pipefail
cd "$(dirname "$0")/../.."
node --test src/adapters/pi_crew/policy.test.mjs
