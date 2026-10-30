#!/usr/bin/env bash
# Policy and extension decisions for the pi crew switch. No network. No model id.
set -euo pipefail
cd "$(dirname "$0")/../.."
node --experimental-strip-types --test \
  src/adapters/pi_crew/policy.test.mjs \
  src/adapters/pi_crew/capacity.test.mjs \
  src/adapters/pi_crew/telemetry.test.mjs \
  src/adapters/pi_crew/broker.test.mjs \
  src/adapters/pi_crew/extension.test.mjs
