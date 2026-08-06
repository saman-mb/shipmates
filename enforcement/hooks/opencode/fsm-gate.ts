// Shipmates FSM tool-gate — opencode `tool.execute.before` plugin.
//
// Turns a `shipmates state gate` verdict into an opencode tool decision. opencode
// has no exit-code hook contract like the other harnesses; a plugin instead
// DENIES a tool call by throwing an Error from `tool.execute.before`. This plugin
// gates ONLY the shell (`bash`) tool and, like the reference Claude Code shim,
// only ever blocks a run it can unambiguously identify — everything else falls
// through to a silent ALLOW, so it can never block work it does not understand.
//
//   Discovery:  the run is the issue number N in a `feat/issue-<N>[-<slug>]`
//               branch with a `.shipmates/run-<N>.json` file. Anything else —
//               `main`, a detached HEAD, a `feat/bundle-*` branch, a missing run
//               file, a non-git dir, or no `shipmates` on PATH — is ALLOWED.
//               Never block an ambiguous/unknown run (fail-safe).
//
//   Verdict:    `shipmates state gate` exits 0 allow / 1 deny / 2 error.
//               deny  → throw an Error (opencode aborts the tool call).
//               allow → return (the tool runs).
//               error → return (fail-safe allow; an engine fault must not wedge
//                       the session).
//
// Deny form: `throw new Error(reason)` from `tool.execute.before`.
//
// KNOWN GAP (opencode #5894): `tool.execute.before` does NOT fire for tool calls
// made inside a spawned subagent, so a builder subagent's `bash` calls are not
// gated by this plugin. The gate therefore covers the primary agent only until
// opencode propagates tool hooks into subagents. Tracked upstream at
// https://github.com/sst/opencode/issues/5894 — documented, not worked around.
//
// Install-time wiring (dropping this into `.opencode/plugin/`) is #217; opencode
// auto-loads any plugin under that directory.

import type { Plugin } from "@opencode-ai/plugin"
import { existsSync } from "node:fs"
import { join } from "node:path"

export const FsmGate: Plugin = async ({ $, directory, worktree }) => {
  // Discovery keys off the git worktree root; fall back to the plugin's
  // directory, then the process cwd.
  const dir = worktree ?? directory ?? process.cwd()

  return {
    "tool.execute.before": async (input, output) => {
      // Only the shell tool is gated. Any other tool → allow.
      if (input.tool !== "bash") return
      const command = output?.args?.command
      if (typeof command !== "string" || command.length === 0) return

      let denyReason: string | null = null
      try {
        // Discover the run from the branch name. `.nothrow()`/`.quiet()` keep a
        // non-git dir or a failed rev-parse from throwing; an empty/non-matching
        // branch (main, feat/bundle-*, detached HEAD) → allow.
        const branch = (
          await $`git -C ${dir} rev-parse --abbrev-ref HEAD`.nothrow().quiet().text()
        ).trim()
        const match = branch.match(/^feat\/issue-(\d+)/)
        if (!match) return
        const n = match[1]

        // The run file must exist for this issue, or there is nothing to gate.
        if (!existsSync(join(dir, ".shipmates", `run-${n}.json`))) return

        // Ask the engine. Exit 1 = deny; 0 = allow; 2 (or a missing binary,
        // caught below) = fail-safe allow.
        const res = await $`shipmates state gate --dir ${dir} --run ${n} --tool ${command}`
          .nothrow()
          .quiet()
        if (res.exitCode === 1) {
          const stderr = (res.stderr?.toString() ?? "").trim()
          denyReason = stderr || "shipmates FSM gate: tool not permitted at this phase"
        }
      } catch {
        // Any discovery/engine fault (including `shipmates` not on PATH) →
        // fail-safe ALLOW. Never fail open by blocking on an internal error.
        return
      }

      // Only a definite engine deny (exit 1) throws — opencode aborts the call.
      if (denyReason !== null) {
        throw new Error(denyReason)
      }
    },
  }
}
