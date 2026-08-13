# Prompt cost discipline

Prompt cost is a design constraint. Spend context on decisions that change the outcome; keep
repeated instructions and low-signal output out of the main context.

## Six principles

1. **Amortize fixed overhead.** Bundle cohesive, low-risk work when one plan, worktree, validation
   pass, and review can serve it. Never bundle unrelated work merely to reduce token count.
2. **Value-gate seats.** Add a specialist only when the change can plausibly benefit from that
   decision. A gated-out reviewer is an explicit result, not an omission.
3. **Route by difficulty.** Use the cheapest capable model and effort for mechanical work; reserve
   stronger reasoning for planning, architecture, security, and acceptance decisions. Choose this at
   spawn time, never in canonical content.
4. **Keep prompts cache-friendly.** Put stable instructions and role context first. Put invocation
   arguments, issue bodies, diffs, and other volatile material in one runtime-input section at the
   bottom. Keep a shared stable prefix before role-specific instructions across subagent spawns.
5. **Return decisions, not transcripts.** Require compact structured output: status or verdict first,
   criterion-level findings and only supporting evidence next, then blockers, changed files, rationale,
   and next action. Do not return command logs or a narrative of every step.
6. **Avoid paid repetition.** Reuse context and results already proven in the current run. Repeat a
   check only when new information or a changed artifact makes it decision-relevant; record what was
   checked rather than replaying a transcript.

## Reusable command preamble

The marker below is expanded into every rendered command. Keep this block short and stable: command
authors reference it instead of copying cost rules into each workflow.

<!-- command-preamble:start -->
## Cost discipline

- Stable workflow instructions come before runtime input. Read and parse the complete runtime-input
  section at the end before acting; do not weave volatile issue text, arguments, diffs, or generated
  output through this prefix.
- Spend subagent seats only where their decision can change the outcome. Route model and effort at
  spawn by work difficulty; never hardcode a model in canonical content.
- Ask every subagent for a compact structured return: decision/status first, criterion findings and
  minimal evidence, then blockers, changed files with one-line rationale, and next action as relevant.
  Return decisions, not transcripts or raw logs.

<!-- command-preamble:end -->

## Reusable subagent preamble

Adapters expand this marker before each role's instructions, giving every subagent a stable common
prefix while preserving harness-neutral role content.

<!-- subagent-preamble:start -->
## Return discipline

Return one compact structured result, not a transcript. Lead with `STATUS` or `VERDICT`; include only
criterion-level findings (`CRITERION: result — evidence`) and evidence needed to support it; finish with
`BLOCKERS`, `CHANGED`, `RATIONALE`, and `NEXT` fields when applicable. Omit raw command logs and narration
of routine steps.

<!-- subagent-preamble:end -->

## Authoring checklist

- Put one shared preamble marker near the start of every command and keep its runtime input section at
  the end of the stable workflow.
- Give reviewers a status/verdict and one finding per acceptance criterion.
- Give builders changed paths and one-line rationale per path or group; list verification commands and
  results separately.
- Keep issue text, user guidance, diffs, and other untrusted or volatile data quoted and below stable
  instructions. Never introduce positional argument placeholders; `$ARGUMENTS` is the only command
  input token.

See [CONTRIBUTING.md](../CONTRIBUTING.md) for source and validation conventions.
