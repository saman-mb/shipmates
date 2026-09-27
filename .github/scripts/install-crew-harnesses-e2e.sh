#!/usr/bin/env bash
# Codex agents are TOML and Copilot needs the .agent.md double extension.
# Both are easy to regress into a plain <name>.md that installs cleanly
# and is silently never loaded, so assert the format, not just presence.
# Grok Build reads a native .grok tree and keeps the commands'
# `disable-model-invocation` guard, which the shared two-key rendering drops,
# so assert the tree AND the guard. Steering is asserted on the built payload:
# it installs only when the target IS the Shipmates tree (catalog::steering_for_target).
set -euo pipefail

cargo run -- install --harness codex --dir "$RUNNER_TEMP/install-codex"
# skills follow the open Agent Skills standard (.agents/skills), crew stay Codex-native (.codex/agents)
test -f "$RUNNER_TEMP/install-codex/.agents/skills/shipmates-ship-issue/SKILL.md"
test ! -d "$RUNNER_TEMP/install-codex/.codex/skills"
test -f "$RUNNER_TEMP/install-codex/.codex/agents/sdet.toml"
test ! -f "$RUNNER_TEMP/install-codex/.codex/agents/sdet.md"
cargo run -- install --harness github-copilot --dir "$RUNNER_TEMP/install-copilot"
# Copilot skills follow the open standard (.agents/skills); crew stay .github-native
test -f "$RUNNER_TEMP/install-copilot/.agents/skills/shipmates-ship-issue/SKILL.md"
test ! -d "$RUNNER_TEMP/install-copilot/.github/skills"
test -f "$RUNNER_TEMP/install-copilot/.github/agents/sdet.agent.md"
test ! -f "$RUNNER_TEMP/install-copilot/.github/agents/sdet.md"
cargo run -- install --harness grok-build --dir "$RUNNER_TEMP/install-grok"
# Grok Build reads its OWN .grok tree, not the shared .agents one: the shared
# rendering emits only the standard's name/description pair and would drop the
# commands' disable-model-invocation guard.
test -f "$RUNNER_TEMP/install-grok/.grok/skills/shipmates-ship-issue/SKILL.md"
grep -q '^disable-model-invocation: true' "$RUNNER_TEMP/install-grok/.grok/skills/shipmates-ship-issue/SKILL.md"
test -f "$RUNNER_TEMP/install-grok/.grok/agents/sdet.md"
test ! -d "$RUNNER_TEMP/install-grok/.agents/skills"
# Contributor steering lands only when the install TARGET is the Shipmates tree
# itself (catalog::steering_for_target), which a scratch dir never is — true for
# claude-code too — so assert the adapter's declared steering path on the built
# payload, where every target emits it, rather than on the install tree.
cargo run -- build --target grok-build --out "$RUNNER_TEMP/grok-payload"
test -f "$RUNNER_TEMP/grok-payload/harnesses/grok-build/.grok/rules/shipmates-contributor.md"
cargo run -- install --harness devin --dir "$RUNNER_TEMP/install-devin"
# Devin CLI reads its own .devin tree for crew and would otherwise resolve the
# shared .agents one; the guard on the eighteen commands is `triggers: [user]`,
# which the shared two-key rendering cannot express, so assert both.
test -f "$RUNNER_TEMP/install-devin/.devin/skills/shipmates-ship-issue/SKILL.md"
grep -q 'triggers:' "$RUNNER_TEMP/install-devin/.devin/skills/shipmates-ship-issue/SKILL.md"
grep -q -- '- user' "$RUNNER_TEMP/install-devin/.devin/skills/shipmates-ship-issue/SKILL.md"
test -f "$RUNNER_TEMP/install-devin/.devin/agents/sdet.md"
test ! -f "$RUNNER_TEMP/install-devin/.devin/agents/sdet.toml"
test ! -d "$RUNNER_TEMP/install-devin/.agents/skills"
# Legacy: a pre-rename windsurf tree must not be created by a devin install.
test ! -d "$RUNNER_TEMP/install-devin/.windsurf"
cargo run -- build --target devin --out "$RUNNER_TEMP/devin-payload"
test -f "$RUNNER_TEMP/devin-payload/harnesses/devin/.devin/rules/shipmates-contributor.md"
