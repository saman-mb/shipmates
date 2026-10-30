# Implementation Plan: Harness-native bootstrap for shared Shipmates principles

## Context
- Issue: https://github.com/saman-mb/shipmates/issues/609
- Capability matrix: `tools/harness_matrix.json` `bootstrap_surface_schema` / per-harness `bootstrap_surface`.
- Worktree: `/Users/saman/dev/shipmates/.shipmates/worktrees/issue-606`
- Base branch: `main` (tracking `origin/main`)

## Goal
Replace per-command/per-crew inlining of shared Shipmates principles with a single canonical bootstrap file that is installed through each harness's native instructions or session-start injection surface.

## Design decisions
1. **Canonical bootstrap source**: create `steering/bootstrap.md`. It contains the shared sections currently expanded from `docs/COST.md` markers:
   - Six cost principles
   - Cost discipline + argument intake + model routing
   - Acceptance board
   - Epic integration board
   - Subagent preamble
   - Why-merge-PR
   `docs/COST.md` remains the public documentation and keeps the same markers so existing links do not break; `steering/bootstrap.md` is the installable copy.
2. **Marker in commands/crew**: replace the existing expansion markers with a single `<!-- shipmates:bootstrap -->` marker. Adapters decide whether to inline the bootstrap (for harnesses with no auto-loaded instructions) or render a short pointer (when the bootstrap is loaded natively).
3. **Delivery per harness** (from matrix):
   - Every harness has a documented auto-loaded instructions/rules file, so the canonical delivery channel is that file. This is uniform, testable, and does not require inventing plugin manifests.
   - `claude-code`, `codex`, `opencode`, `antigravity`, `pi`, `grok-build`, `devin`: write the bootstrap to the harness's user-scope instructions file through the existing `install_global_steering` path.
   - `cursor`: write to `.cursor/rules/shipmates-bootstrap.mdc` (alwaysApply).
   - `github-copilot`: project-scope `.github/instructions/shipmates.instructions.md` because no user-scope instructions file is documented.
   - Package-level session-start injection is recorded in the matrix but out of scope for this refactor; the instructions-file channel is the single source of truth for now.
4. **Render layer**: set every dialect's `bootstrap_mode` to `Pointer`. Rendered commands replace `<!-- shipmates:bootstrap -->` with a short pointer to the project instructions file. Keep `docs/COST.md` markers in place for documentation; `steering/bootstrap.md` is the installable copy.
5. **Steering layer**: use the existing user-scope steering path for all harnesses except Copilot. The Copilot project-scope path is already implemented in Slice A; wire it into the adapter's steering target.
6. **Validation**: add integration test `test_bootstrap_installed_for_every_target` and update digest fixtures.

## Slices

### Slice A — Core bootstrap + render + steering
**Owned paths**:
- `steering/bootstrap.md` (new)
- `src/adapters/render.rs`
- `src/steering.rs`
- `tests/integration.rs` (new test, no digest updates yet)

**Done when**:
- `steering/bootstrap.md` exists and contains the shared sections.
- `render::Dialect` has `bootstrap_mode` and `render_body` handles `<!-- shipmates:bootstrap -->`.
- `src/steering.rs` supports project-scope steering (Copilot) and can render `steering/bootstrap.md` content into any instructions file.
- `cargo test --test integration test_matrix_agents_flag` still passes.

**Out of scope**: adapter changes, command/crew trimming, digests, version bump.

### Slice B — Per-harness bootstrap delivery
**Owned paths**:
- `src/adapters/claude_code.rs`
- `src/adapters/codex.rs`
- `src/adapters/antigravity.rs`
- `src/adapters/cursor.rs`
- `src/adapters/devin.rs`
- `src/adapters/pi.rs`
- `src/adapters/opencode.rs`
- `src/adapters/github_copilot.rs`
- `src/adapters/grok_build.rs`
- `src/adapters/mod.rs` (if new `Adapter` methods needed)
- `tools/capability_registry.json` (if new paths need mapping)

**Done when**:
- Every adapter dialect sets `bootstrap_mode: Pointer`.
- `src/catalog.rs` exposes `load_bootstrap()` reading `steering/bootstrap.md`.
- `build.rs` embeds `steering/bootstrap.md`.
- `src/main.rs` installs the bootstrap content into the harness-native instructions/rules file at install time (user-scope for most, project-scope for Copilot).
- `cargo run -- build --target <target>` succeeds for all 9 targets and includes the bootstrap steering file in each payload.

**Out of scope**: plugin/extension session-start injection, editing canonical commands/crew, digest regeneration.

### Slice C — Trim duplication and close the loop
**Owned paths**:
- `commands/*.md` (trim shared markers to `<!-- shipmates:bootstrap -->`)
- `crew/*.md` (same)
- `Cargo.toml`, `Cargo.lock`
- `CHANGELOG.md`
- `tests/payload-digests/*.sha256`
- `tests/integration.rs` (finalize new test)
- `site/commands/<name>/index.html` if generated pages change (regenerate with `tools/gen_command_pages.py`)

**Done when**:
- All commands/crew use the single bootstrap marker.
- `cargo test --all-targets` green.
- `cargo run -- check --target <target>` green for all 9 targets.
- Version bumped and CHANGELOG entry added.
- Payload digests regenerated.

**Out of scope**: changing the bootstrap content itself, adapter internals.

## Shared rules for every implementer
- Work in `/Users/saman/dev/shipmates/.shipmates/worktrees/issue-606`.
- Do not touch `.shipmates/worktrees/issue-602` or PR #604.
- Do not use local models; inherit the session model.
- Keep prose domain-neutral; no issue numbers or harness-specific tool IDs in `commands/` or `crew/`.
- Run `cargo test --test integration test_matrix_agents_flag` after any adapter/render change.
- Use `git status` and `git diff --name-only` to report changed files.
