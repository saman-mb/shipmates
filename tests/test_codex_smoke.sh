#!/usr/bin/env bash
#
# Codex payload golden/layout-install smoke check.
#
# Default path is deterministic and runs in CI: build the Codex payload,
# verify its committed golden digest, check native file layout, and diagnose
# the install. It does not invoke Codex. Set CODEX_SMOKE=1 to additionally
# run one read-only `harden` skill through an installed Codex CLI. That path
# needs locally authenticated `codex` binary and is not a CI runtime gate.
#
#   bash tests/test_codex_smoke.sh
#   CODEX_SMOKE=1 bash tests/test_codex_smoke.sh

set -euo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

if [[ -n "${CODEX_PROJECT:-}" ]]; then
  PROJECT="$CODEX_PROJECT"
  test -d "$PROJECT"
else
  PROJECT="$WORK/project"
  mkdir -p "$PROJECT"
  printf '# Codex smoke sandbox\n\n' > "$PROJECT/README.md"
  printf '# Project instructions\n\n' > "$PROJECT/AGENTS.md"
fi

(
  cd "$REPO"
  cargo run --quiet -- check --target codex
  cargo run --quiet -- install --harness codex --dir "$PROJECT" --with-tools none
  cargo run --quiet -- doctor --harness codex --dir "$PROJECT"
)

test -f "$PROJECT/.agents/skills/ship-issue/SKILL.md"
test -f "$PROJECT/.codex/agents/sdet.toml"
test ! -d "$PROJECT/.codex/skills"
test "$(ls -1 "$PROJECT/.agents/skills" | wc -l | tr -d ' ')" -eq 15
test "$(ls -1 "$PROJECT/.codex/agents"/*.toml | wc -l | tr -d ' ')" -eq 13

for role in \
  architect art-director data-scientist devops-engineer performance-engineer \
  product-manager sdet security-engineer senior-engineer site-reliability-engineer \
  technical-writer ux-ui-designer
do
  test -f "$PROJECT/.codex/agents/$role.toml"
done

if [[ "${CODEX_SMOKE:-0}" != "1" ]]; then
  printf 'Codex golden/layout-install smoke passed; no Codex runtime invoked.\n'
  exit 0
fi

CODEX_BIN="${CODEX_BIN:-codex}"
if ! command -v "$CODEX_BIN" >/dev/null 2>&1; then
  printf 'Codex runtime smoke skipped: %s not found.\n' "$CODEX_BIN"
  exit 0
fi

# Keep the documented invocation shape parser-checked without requiring an
# authenticated session: this option belongs before the `exec` subcommand.
"$CODEX_BIN" --ask-for-approval never exec --help >/dev/null

LAST_MESSAGE="$WORK/codex-last-message.txt"
python3 - "$CODEX_BIN" "$PROJECT" "$LAST_MESSAGE" <<'PY'
import subprocess
import sys

binary, project, last_message = sys.argv[1:]
command = [
    # `--ask-for-approval` is a root-level Codex option and must precede
    # `exec`; `codex exec --ask-for-approval` is rejected by Codex CLI.
    binary,
    "--ask-for-approval", "never",
    "exec",
    "--ephemeral",
    "--sandbox", "read-only",
    "--skip-git-repo-check",
    "--cd", project,
    "--output-last-message", last_message,
    (
        "Use the installed harden skill in its default report mode on README.md and AGENTS.md "
        "only. Resolve the installed sdet subagent from .codex/agents/sdet.toml and delegate one "
        "read-only inspection to sdet. Do not invoke other roles, edit files, create commits, "
        "access the network, or run destructive commands. After the report completes, make the "
        "final line exactly CODEX_SMOKE_PASS."
    ),
]
try:
    completed = subprocess.run(command, check=False, timeout=120)
except subprocess.TimeoutExpired:
    raise SystemExit("Codex runtime smoke timed out after 120 seconds")
if completed.returncode:
    raise SystemExit(f"Codex runtime smoke exited {completed.returncode}")
PY

python3 - "$LAST_MESSAGE" <<'PY'
from pathlib import Path
import sys

message = Path(sys.argv[1]).read_text()
if not message.rstrip().endswith("CODEX_SMOKE_PASS"):
    raise SystemExit("Codex runtime smoke did not return CODEX_SMOKE_PASS")
PY

printf 'Optional Codex runtime smoke passed: installed crew resolved and read-only harden skill completed.\n'
