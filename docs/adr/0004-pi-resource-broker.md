# ADR 0004 — One machine-wide broker for pi local work

**Status:** Accepted — contract for #601. Not a runtime attestation.
**Date:** 2026-10-28
**Decided by:** issue #601

---

## Context

The pi crew switch caps how many seats one session may run. That cap is a count of seats. It is not a measure of memory, context, or engine slots, and it cannot see another pi session on the same machine.

A seat count mis-describes the machine. Four small seats on one resident model may fit. Two large models with long contexts may not. Independent sessions can each make a reasonable local decision and still over-allocate together.

The switch stays the policy for who may run local or cloud. This record decides how local work is admitted once a captain opts in. It does not change the harness record: pi `crew_resolve` is `no`, and `command_e2e` is `no`. Nothing here is evidence of a live broker on macOS, Linux, or Windows.

## Decision

**One broker per OS user, per machine, and only when the captain opts in.** It is the single admission authority for pi sessions on that user that route local model work through it. It is not one broker per repository, per worktree, or per pi session. A second OS user is a separate broker and is outside this coordination.

**Off starts no broker.** The crew switch at `Off` keeps today's behaviour: no measurement, no queue, no daemon. Installing the extension does not start one.

**Resources are a separate policy, rolled out shadow before enforce.**

| Value | Meaning |
|---|---|
| `off` | No broker. Same absence as the switch at `Off`. |
| `shadow` | Measure and report. Do not block a launch. This ships before enforce. |
| `enforce` | Admit only what fits. Queue the rest. Never bypass admission because the broker is down — local work fails closed. Cloud-only work is unchanged. |

Enforce is not the install default. Shadow is the step that must exist first. Automatic model loading is a later opt-in, after loaded-only admission, and is not part of turning the broker on.

**Queue instead of silent substitution.** An explicit model pin is never replaced with another model, a smaller one, or a cloud model. If it cannot fit, the job waits or stops with a reason. A smaller later job may pass a waiting job only when that does not starve the older job.

**A lease is not a seat counter.** A lease reserves the resources of one run — memory pools, context, and an execution slot — and is keyed by run id for the whole run, not by model id and not by the launch call. Shared resident weights are charged once. Each workload still pays its own context and slot. The existing ceiling may remain as an optional extra seat cap. It never substitutes for measured capacity.

Admission uses a fresh measurement of what is available, minus pending reservations that telemetry does not yet show, minus captain-configured headroom. There is no shipped default that is claimed to be right for every machine. Unknown or low-confidence telemetry does not unlock automatic loading. It falls back to serial local admission.

The broker does not kill running work, and it does not unload a model it does not own.

### External applications

**Other applications are outside coordination.** The broker coordinates pi sessions that use it. A model client, editor, or server that does not go through the broker is not governed. Visible external consumption still reduces the capacity pi may reserve. That reduction is observation, not control.

A shared inference server used by several machines is a separate deployment of the same broker beside that server. Independent laptop brokers are not described as distributed consensus.

### What is not fully governed

**A launch the extension cannot observe for its whole lifetime is not fully governed.** The reservation has to cover the run, not only the tool call that started it. Captain chats, crew seats, background runs, and workflow children are in scope only when this extension can see them from admission through completion, cancellation, or failure. A launch that escapes that watch — started outside the hook, or ended without a lifecycle signal the extension can see — is not described as held and released for its whole life. Stale owners are reconciled. They are not called fully governed.

### What this record does not claim

- No live proof on macOS, Linux, or Windows.
- No claim that pi crew spawn works. The harness row remains the evidence.
- No claim that engine adapters, sockets, named pipes, or telemetry probes have been proven because they are named in #601.
- No claim that another application is coordinated because its memory use is visible.

## Consequences

- The pi crew page states this contract and the same limits. It does not tell a captain that enforce is on, that a live multi-session run has been shown, or that an unobserved launch is fully governed.
- Role tags, doer-order checks, and explicit pins stay. The broker does not override them.
- A failed or absent broker under `enforce` does not fall back to unlimited parallel launches.
- Implementation choices in #601 — transport, journal, telemetry library — are not restated here. They can change without changing this contract. If a later change moves admission back to a per-session seat count, or starts a broker while the switch is `Off`, this record is wrong and must be revised in the same change.
