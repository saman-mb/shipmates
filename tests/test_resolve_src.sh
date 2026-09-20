#!/usr/bin/env bash
#
# Regression test for the `shipmates install` command: it must drop the
# harness's own tree (`.claude/`, `.opencode/`, `.codex/`, …) at the target
# root, not the `harnesses/<target>/` container the build layout uses. The
# harness reads its tree from its own root, so installing the container would
# install nothing any harness loads.
#
# Also gates the documented harness surface: every advertised target installs,
# skill-only targets emit no agent files, and an unknown target is refused.
#
#   bash tests/test_resolve_src.sh
#
# Exit 0 = all passed, 1 = at least one failure.

set -uo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

PASS=0; FAIL=0
ok()  { PASS=$((PASS+1)); printf 'ok   %s\n' "$1"; }
bad() { FAIL=$((FAIL+1)); printf 'FAIL %s\n' "$1"; }

assert() {
  local desc="$1"; shift
  if "$@" >/dev/null 2>&1; then ok "$desc"; else bad "$desc"; fi
}

install_to() { # harness dest  -- runs the local CLI, not a stale installed copy
  local harness="$1" dest="$2"
  ( cd "$REPO" && cargo run --quiet -- install --harness "$harness" --dir "$dest" )
}

# --- claude-code: agents + skills land under .claude/ ---
D="$WORK/claude"
assert "claude-code: install exits 0" install_to claude-code "$D"
assert "claude-code: skill under .claude/skills" test -f "$D/.claude/skills/shipmates-issue/SKILL.md"
assert "claude-code: agent under .claude/agents" test -f "$D/.claude/agents/sdet.md"
assert "claude-code: no harnesses/ container leaks" test ! -d "$D/harnesses"

# --- opencode: commands + agents land under .opencode/ ---
D="$WORK/opencode"
assert "opencode: install exits 0" install_to opencode "$D"
assert "opencode: command under .opencode/commands" test -f "$D/.opencode/commands/shipmates-issue.md"
assert "opencode: agent under .opencode/agents" test -f "$D/.opencode/agents/sdet.md"

# --- antigravity: agents + skills land under .agents/ ---
D="$WORK/antigravity"
assert "antigravity: install exits 0" install_to antigravity "$D"
assert "antigravity: skill under .agents/skills" test -f "$D/.agents/skills/shipmates-issue/SKILL.md"
assert "antigravity: agent is a dir per agent (agent.md)" test -f "$D/.agents/agents/sdet/agent.md"
assert "antigravity: flat <name>.md is NOT emitted" test ! -f "$D/.agents/agents/sdet.md"

# --- crew-bearing targets whose agent format is not Claude's ---
# Codex agents are TOML, not Markdown; Copilot needs the .agent.md double
# extension or the file is not discovered. Both are easy to regress into a
# plain <name>.md that installs cleanly and is silently never loaded.
D="$WORK/codex"
assert "codex: install exits 0" install_to codex "$D"
# Codex reads skills from the open Agent Skills standard (.agents/skills), NOT
# .codex/skills; only its crew are Codex-native (.codex/agents).
assert "codex: skill under .agents/skills" test -f "$D/.agents/skills/shipmates-issue/SKILL.md"
assert "codex: no skills under .codex" test ! -d "$D/.codex/skills"
assert "codex: agent is TOML under .codex/agents" test -f "$D/.codex/agents/sdet.toml"
assert "codex: agent is not markdown" test ! -f "$D/.codex/agents/sdet.md"

# Copilot reads Agent Skills from the open .agents/skills tree; only its crew
# are .github-native (.github/agents/*.agent.md).
D="$WORK/github-copilot"
assert "github-copilot: install exits 0" install_to github-copilot "$D"
assert "github-copilot: skill under .agents/skills" test -f "$D/.agents/skills/shipmates-issue/SKILL.md"
assert "github-copilot: no skills under .github" test ! -d "$D/.github/skills"
assert "github-copilot: agent uses .agent.md" test -f "$D/.github/agents/sdet.agent.md"
assert "github-copilot: bare .md is not emitted" test ! -f "$D/.github/agents/sdet.md"

# --- skill-only targets on the open Agent Skills tree: skills only, no crew ---
# cursor: slash-command discovery requires its first-party .cursor/skills tree
#   (#405); it no longer shares .agents/skills.
# windsurf: keeps its canonical .windsurf/skills (.agents/skills is only a
#   secondary compat scan there — do not move it off its documented path).
for pair in "cursor:.cursor" "windsurf:.windsurf"; do
  harness="${pair%%:*}"
  dirname="${pair##*:}"
  D="$WORK/$harness"
  assert "$harness: install exits 0" install_to "$harness" "$D"
  assert "$harness: skill under $dirname/skills" test -f "$D/$dirname/skills/shipmates-issue/SKILL.md"
  assert "$harness: no agent files emitted" test ! -d "$D/$dirname/agents"
done
assert "cursor: no shared .agents skills tree" test ! -d "$WORK/cursor/.agents/skills"

# pi: crew are pi-native under .pi/agents, skills stay on the shared tree so a
# sibling harness in the same repo is one copy (#513). Global omits skills.
D="$WORK/pi"
assert "pi: install exits 0" install_to "pi" "$D"
assert "pi: skill under .agents/skills" test -f "$D/.agents/skills/shipmates-issue/SKILL.md"
assert "pi: crew under .pi/agents" test -f "$D/.pi/agents/sdet.md"
# The shared crew tree belongs to Antigravity. pi reads it as a legacy location
# and cannot resolve its tool vocabulary, so shipping there would shadow pi's
# own crew with an inert one (#437).
assert "pi: no crew in the shared .agents tree" test ! -d "$D/.agents/agents"
assert "pi: tools are a comma scalar, not a YAML list" grep -q '^tools: read, grep, find' "$D/.pi/agents/sdet.md"

# grok-build: skills, crew and steering are ALL native under .grok/, never the
# shared .agents tree — the shared two-key rendering emits only name/description
# and would drop the commands' disable-model-invocation guard.
D="$WORK/grok-build"
assert "grok-build: install exits 0" install_to "grok-build" "$D"
assert "grok-build: skill under .grok/skills" test -f "$D/.grok/skills/shipmates-issue/SKILL.md"
assert "grok-build: command keeps disable-model-invocation" grep -q '^disable-model-invocation: true' "$D/.grok/skills/shipmates-issue/SKILL.md"
assert "grok-build: crew under .grok/agents" test -f "$D/.grok/agents/sdet.md"
assert "grok-build: no shared .agents skills tree" test ! -d "$D/.agents/skills"
# Contributor steering installs only when the install TARGET is the Shipmates
# tree itself (catalog::steering_for_target), so this scratch target never gets
# it — claude-code's `.claude/rules/` behaves the same way. Assert the adapter's
# declared steering path on the built payload, where every target emits it.
assert "grok-build: steering at .grok/rules in the built payload" bash -c "cd '$REPO' && cargo run --quiet -- build --target grok-build --out '$WORK/grok-payload' && test -f '$WORK/grok-payload/harnesses/grok-build/.grok/rules/shipmates-contributor.md'"

# --- a GLOBAL install must land in each harness's own user-scope tree ---
#
# `--global` (the default) writes into $HOME. For most harnesses the workspace
# path joined to home is already correct; for antigravity and pi it is not, and
# the files landed where nothing reads them. This asserts the relocated layout
# for the crew, the command skills AND the toolbox — the last of which is a
# separate payload and was missed once already.
GHOME="$WORK/global-home"
mkdir -p "$GHOME"
# Invoke the built binary rather than `cargo run`: cargo needs the real $HOME for
# ~/.cargo, so overriding HOME inside a cargo invocation fails before the CLI
# under test ever starts.
( cd "$REPO" && cargo build --quiet ) || true
BIN="$REPO/target/debug/shipmates"
global_install() { # harness
  HOME="$GHOME" "$BIN" install --harness "$1"
}
assert "global pi: exits 0" global_install pi
assert "global pi: crew at ~/.pi/agent/agents" test -f "$GHOME/.pi/agent/agents/sdet.md"
assert "global pi: no command skills at ~/.pi/agent/skills" test ! -d "$GHOME/.pi/agent/skills"
assert "global pi: no toolbox at ~/.pi/agent/skills" test ! -f "$GHOME/.pi/agent/skills/shipmates-badge/SKILL.md"
assert "global pi: crew is NOT a flat <name>.md in the shared tree" test ! -d "$GHOME/.agents/agents"
assert "global pi: writes nothing into the shared .agents tree" test ! -d "$GHOME/.agents"

assert "global antigravity: exits 0" global_install antigravity
assert "global antigravity: crew is a dir per agent" test -f "$GHOME/.gemini/config/agents/sdet/agent.md"
assert "global antigravity: commands under .gemini/config/skills" test -f "$GHOME/.gemini/config/skills/shipmates-issue/SKILL.md"
assert "global antigravity: writes nothing into the shared .agents tree" test ! -d "$GHOME/.agents"

# Both harnesses in one home must be clean — including the toolbox, which doctor
# compares against the relocated paths. Asserting the layout alone missed that:
# the files landed correctly while `doctor` still read the workspace paths and
# reported every installed tool as orphaned.
assert "global pi: doctor is clean" bash -c "HOME='$GHOME' '$BIN' doctor --harness pi | grep -q 'All shipshape'"
assert "global antigravity: doctor is clean" bash -c "HOME='$GHOME' '$BIN' doctor --harness antigravity | grep -q 'All shipshape'"

# codex and github-copilot are the other two shared-tree harnesses, and their
# GLOBAL half does not live on the shared tree either: Codex reads
# `$CODEX_HOME/skills` and Copilot's config dir is `~/.copilot`. Writing their
# global payload to `.agents/skills/` put it where neither one looks — and where
# pi *does*, which is what produced a collision warning per skill.
assert "global codex: exits 0" global_install codex
assert "global codex: crew at ~/.codex/agents" test -f "$GHOME/.codex/agents/sdet.toml"
assert "global codex: skills at ~/.codex/skills, not the shared tree" test -f "$GHOME/.codex/skills/shipmates-issue/SKILL.md"
assert "global codex: writes nothing into the shared .agents tree" test ! -d "$GHOME/.agents"

assert "global github-copilot: exits 0" global_install github-copilot
assert "global github-copilot: crew at ~/.copilot/agents" test -f "$GHOME/.copilot/agents/sdet.agent.md"
assert "global github-copilot: skills at ~/.copilot/skills" test -f "$GHOME/.copilot/skills/shipmates-issue/SKILL.md"
assert "global github-copilot: writes nothing to ~/.github" test ! -d "$GHOME/.github"
assert "global github-copilot: writes nothing into the shared .agents tree" test ! -d "$GHOME/.agents"

assert "global codex: doctor is clean" bash -c "HOME='$GHOME' '$BIN' doctor --harness codex | grep -q 'All shipshape'"
assert "global github-copilot: doctor is clean" bash -c "HOME='$GHOME' '$BIN' doctor --harness github-copilot | grep -q 'All shipshape'"
assert "all four global installs leave the shared .agents tree untouched" test ! -d "$GHOME/.agents"

# --- a symlinked config path is SKIPPED and reported, never fatal (#462) ---
#
# Sharing one skills tree across harnesses is a normal thing to want, and it is
# what a symlink farm (a dotfiles repo, a skills manager) produces. Refusing to
# write *through* one is a containment property; abandoning the other forty files
# because of it is not.
SHOME="$WORK/symlinked-home"
mkdir -p "$SHOME/.agents/skills/shipmates-issue" "$SHOME/.gemini/config/skills"
printf -- '---\nname: shipmates-issue\ndescription: mine\n---\nbody\n' > "$SHOME/.agents/skills/shipmates-issue/SKILL.md"
ln -s "$SHOME/.agents/skills/shipmates-issue" "$SHOME/.gemini/config/skills/shipmates-issue"
assert "symlinked path: install still exits 0" bash -c "HOME='$SHOME' '$BIN' install --harness antigravity --with-tools none"
assert "symlinked path: the rest of the crew still lands" test -f "$SHOME/.gemini/config/agents/sdet/agent.md"
assert "symlinked path: the user's own file is untouched" grep -q 'description: mine' "$SHOME/.gemini/config/skills/shipmates-issue/SKILL.md"
assert "symlinked path: install names what it skipped" bash -c "HOME='$SHOME' '$BIN' install --harness antigravity --with-tools none | grep -q 'sit behind a symlink'"
assert "symlinked path: doctor reports it instead of failing" bash -c "HOME='$SHOME' '$BIN' doctor --harness antigravity | grep -q 'Symlinked paths'"
assert "symlinked path: doctor --fix leaves it alone" bash -c "HOME='$SHOME' '$BIN' doctor --fix --harness antigravity >/dev/null; grep -q 'description: mine' '$SHOME/.gemini/config/skills/shipmates-issue/SKILL.md'"

# --- unknown target is refused, not silently ignored ---
assert "unknown target exits non-zero" bash -c "cd '$REPO' && ! cargo run --quiet -- install --harness nope --dir '$WORK/nope' 2>/dev/null"

# --- embedded sources: a project dir has no crew/ or commands/, so the CLI
# must fall back to the payload compiled into the binary by build.rs ---
EMBED="$WORK/embedded"
mkdir -p "$EMBED"
assert "embedded: install from empty cwd exits 0" bash -c "cd '$EMBED' && cargo run --quiet --manifest-path '$REPO/Cargo.toml' -- install --harness claude-code --dir '$EMBED'"
assert "embedded: skill from embedded payload" test -f "$EMBED/.claude/skills/shipmates-issue/SKILL.md"
assert "embedded: agent from embedded payload" test -f "$EMBED/.claude/agents/sdet.md"
assert "embedded: twenty-eight skills emitted (commands + tools)" test "$(ls "$EMBED/.claude/skills" | wc -l | tr -d ' ')" -eq 28
assert "embedded: thirteen agents emitted" test "$(ls "$EMBED/.claude/agents" | wc -l | tr -d ' ')" -eq 13

# --- summary ---

echo
echo "passed: $PASS, failed: $FAIL"
[ "$FAIL" -eq 0 ]
