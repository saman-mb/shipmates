---
description: Check whether any AI harness has changed how it discovers skills, and update the adapter/config if it has (repo-local dev command — NOT a shipped shipmates resource).
argument-hint: "[harness names, comma-separated] | --offline"
allowed-tools: Bash(python3 tools/harness_watch.py*), Bash(. \"$HOME/.cargo/env\"*), Bash(cargo*), Read, Edit, WebFetch, WebSearch
---

# /harness-watch — is any harness's skill-discovery drifting?

This is a **repo-local** command for developing *shipmates itself*. It is **not**
one of the twelve shipped commands and must never be added to `commands/` (that
catalog installs into users' harnesses). It lives only in `.claude/commands/`.

Shipmates maps each harness's neutral sources onto that harness's real
skill-discovery surface (see `tools/harness_watch.json` for the per-harness
config and `src/adapters/` for the code). Harnesses ship new versions and change
those surfaces; when they do, an install can land where the harness never looks.
This command catches that early.

## What to do

1. Run the watcher (pass `$ARGUMENTS` through — e.g. a comma-separated harness
   list, or `--offline` for the network-free config-consistency check):

   ```
   python3 tools/harness_watch.py $ARGUMENTS
   ```

   It reads every harness's injected config from `tools/harness_watch.json`
   (nothing is hard-coded) and, for each, fetches the first-party docs and checks
   they still contain / omit the recorded strings. `reachable`-mode harnesses
   (JS-rendered docs it can't string-check) are flagged for manual review.

2. **If it reports `OK` for everything:** report that nothing drifted and stop.

3. **For any `DRIFT` or `~ review by hand`:** do NOT blindly edit anything.
   Re-verify against the harness's **first-party docs** (use WebFetch/WebSearch
   on the `docs_url`, exactly as the original grounding was done):
   - Where does the harness now discover skills? Is our `adapter_skill_path`
     still correct? Is it still on the open `.agents/skills/` tree (`tree: shared`)
     or has it moved?
   - If the harness changed, update the matching adapter in `src/adapters/`
     (route skills to the right emitter — `emit_shared_skills` for the shared
     tree, `emit_skill_files` with a harness dialect for a native tree), then
     regenerate its digest (`cargo run -- update --target <name>`) and update the
     tests, docs, and `capability_registry.json`.
   - Whether or not the code changed, update the harness's entry in
     `tools/harness_watch.json` (`expect_contains`/`expect_absent`, `verified_on`,
     `note`) so the next run reflects the new reality.

4. **For `MISCONFIGURED`:** the config entry is internally inconsistent (e.g. a
   `shared` harness whose `adapter_skill_path` isn't `.agents/skills/...`). Fix
   `tools/harness_watch.json` to match what the adapter actually does.

Report a short summary: what drifted, what you verified, and what you changed
(or that no change was needed).
