#!/usr/bin/env sh
# The exact commands that render the three example sparklines on the tool page.
# Each is just sparkline.py driving a different series, colour, and shape —
# nothing to do with shipmates itself. Run from this directory:
#
#   sh sparklines.sh
#
# The renderer lives at toolbox/sparkline/sparkline.py.
set -eu
SPARK="../../../../toolbox/sparkline/sparkline.py"

# A latency history that improves over a release — p95 falls from 182ms to 88ms.
python3 "$SPARK" \
  --data "182,168,174,150,133,121,118,96,88" \
  --label "p95 latency (ms)" \
  --color coral \
  --out latency.svg

# Throughput climbing as autoscaling kicks in — 420 to 845 req/s.
python3 "$SPARK" \
  --data "420,455,505,540,612,700,762,845" \
  --label "throughput (req/s)" \
  --color teal \
  --out throughput.svg

# An error-rate spike during an incident, then recovery — a lone peak at 2.1%.
python3 "$SPARK" \
  --data "0.4,0.3,0.5,2.1,1.8,0.6,0.3,0.2" \
  --label "error rate (%)" \
  --color gold \
  --out error-rate.svg
