#!/usr/bin/env bash
# Run opencode golden payload and embedded install-fidelity tests.
set -euo pipefail

cargo test --test integration -- --list | python3 -c '
import sys
expected = {
    "test_opencode_cli_build_matches_golden_payload",
    "test_opencode_embedded_install_fidelity",
}
found = {
    line.split(":", 1)[0]
    for line in sys.stdin
    if line.rstrip().endswith(": test")
}
missing = expected - found
if missing:
    print("missing tests:", *sorted(missing))
    raise SystemExit(1)
'
cargo test --test integration test_opencode_cli_build_matches_golden_payload -- --exact --nocapture
cargo test --test integration test_opencode_embedded_install_fidelity -- --exact --nocapture
