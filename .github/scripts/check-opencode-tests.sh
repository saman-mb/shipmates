#!/usr/bin/env bash
# Run the opencode golden-payload test and the embedded install-fidelity sweep.
# The fidelity check is the all-targets test: it covers opencode too, with the
# nine-target set equality, per-format identity and format-guard assertions.
set -euo pipefail

cargo test --test integration -- --list | python3 -c '
import sys
expected = {
    "test_opencode_cli_build_matches_golden_payload",
    "test_keyless_installed_tree_fidelity_all_targets",
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
cargo test --test integration test_keyless_installed_tree_fidelity_all_targets -- --exact --nocapture
