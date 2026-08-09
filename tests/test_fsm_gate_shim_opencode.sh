#!/usr/bin/env bash
#
# Structural test for the opencode `tool.execute.before` FSM tool-gate plugin
# (enforcement/hooks/opencode/fsm-gate.ts).
#
# LIMITATION: opencode's deny channel is a throwing `tool.execute.before` plugin
# that runs on opencode's Bun runtime and shells out via Bun's `$`. Neither Bun
# nor opencode's plugin host is available in CI, and node's own runtime has no
# Bun `$`, so this cannot be executed end-to-end the way the bash shims can. Per
# the plan we assert the plugin STRUCTURALLY (its discovery + deny + fail-safe
# shape) rather than fake a runtime pass. A live behavioural test belongs with
# the #217 install-wiring work, on a runner that has opencode.
#
#   bash tests/test_fsm_gate_shim_opencode.sh
#
# Exit 0 = all passed, 1 = at least one failure.

set -uo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PLUGIN="$REPO/enforcement/hooks/opencode/fsm-gate.ts"

PASS=0; FAIL=0
ok()  { PASS=$((PASS+1)); printf 'ok   %s\n' "$1"; }
bad() { FAIL=$((FAIL+1)); printf 'FAIL %s\n' "$1"; }

has() { grep -qF -- "$2" "$1"; }

[ -f "$PLUGIN" ] && ok "plugin file exists" || { bad "plugin file missing: $PLUGIN"; }

# Hook surface: it registers the tool.execute.before hook.
has "$PLUGIN" 'tool.execute.before' && ok "registers tool.execute.before" || bad "missing tool.execute.before hook"

# Only the shell tool is gated.
has "$PLUGIN" 'input.tool !== "bash"' && ok "gates only the bash tool" || bad "does not restrict to the bash tool"

# Deny form: it denies by THROWING an Error.
has "$PLUGIN" 'throw new Error' && ok "denies by throwing an Error" || bad "missing throw-on-deny"

# Discovery: parses a feat/issue-<N> branch and reads .shipmates/run-<N>.json.
has "$PLUGIN" 'issue-(' && ok "discovers run from feat/issue-<N> branch" || bad "missing branch discovery"
has "$PLUGIN" '.shipmates' && ok "reads the .shipmates run file" || bad "missing run-file discovery"

# Engine: shells out to `shipmates state gate` and only denies on exit 1.
has "$PLUGIN" 'shipmates state gate' && ok "shells out to shipmates state gate" || bad "missing engine call"
has "$PLUGIN" 'res.exitCode === 1' && ok "denies only on engine exit 1" || bad "missing exit-1 deny mapping"

# Fail-safe: a catch that returns (allows) on any discovery/engine fault.
has "$PLUGIN" 'catch' && ok "has a fail-safe catch (allow on error)" || bad "missing fail-safe catch"

# Documents the opencode #5894 subagent-bypass gap.
has "$PLUGIN" '5894' && ok "documents the #5894 subagent-bypass gap" || bad "missing #5894 gap note"

printf '\n%d passed, %d failed\n' "$PASS" "$FAIL"
[ "$FAIL" -eq 0 ]
