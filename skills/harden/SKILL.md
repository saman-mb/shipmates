---
name: harden
description: Security-harden a surface — threat-model it, rank the findings, remediate the blockers, and re-review until every finding has a fix or an explicit accepted-risk note (and secrets/dependency scans are clean).
argument-hint: <what to harden — a module, an endpoint, an auth flow, or the whole app>
allowed-tools: Bash, Read, Write, Edit, Agent, Grep, Glob, WebSearch, WebFetch
disable-model-invocation: true
---

# /harden — threat-model → remediate → re-review

Take a surface and make it defensibly secure: a `security-engineer` threat-models it, findings are
ranked and fixed, and the loop closes only when **every finding is either remediated or carries an
explicit, reasoned accepted-risk note** — and the secrets/dependency scans are clean. The gate is
"nothing Critical/High left unaddressed," not "looks fine."

Input (**$ARGUMENTS**): the surface to harden — a module, an endpoint/route, an auth or payment flow,
a dependency set, or the whole app. If empty, ask which surface (don't silently pick the whole repo).

---

## Config

- `REVIEWER` = `security-engineer`. `BUILDER` = `senior-engineer`. `MAX_ROUNDS` = `4`.
- `MODE` = `report` (default: produce a prioritised remediation report, fixing what's safe and
  in-scope) or `fix-pr` (apply remediations on a worktree branch and open a CI-gated PR, like
  `/ship-issue`). Infer from the request; when ambiguous, default to `report`.
- **Threat model / sensitivity** = from the repo (what data it handles, what's exposed). Assume the
  repo is public and input is hostile.

## Stage 0 — Scope the attack surface

Map what's in scope: the entry points (routes, CLI, message handlers), the trust boundaries, what data
crosses them, and what an attacker controls. List the assets worth protecting. Keep the surface bounded
to `$ARGUMENTS` — note explicitly what you're *not* covering so it isn't mistaken for cleared.

## Stage 1 — Threat-model & find  (agent: `security-engineer`)

Spawn the `security-engineer` to walk the surface with **STRIDE** across each trust boundary and review
against OWASP fundamentals (broken access control / IDOR, injection, secrets & crypto, supply chain,
secure defaults / defence in depth). Run the repo's existing SCA / secret scanners as corroboration —
but reason about data flow, don't just collect lint. Return findings ranked **Critical/High/Medium/Low**,
each with the **exploit path** (inputs → impact), exact location, and a specific fix.

## Stage 2 — Triage

Separate **blockers** (Critical/High, and any hardcoded secret / clear injection / broken authz) from
**nits** (Low, defence-in-depth niceties). For each blocker: fix it, or — if it's a deliberate,
justified risk — record an **accepted-risk** note (what, why it's acceptable, who'd own it). Nothing
Critical/High may be silently dropped.

## Stage 3 — Remediate  (agent: `senior-engineer`)

Spawn a `senior-engineer` with the exact blocker list; apply **scoped** fixes (parameterise the query,
add the authz check, move the secret to config, pin/upgrade the dep) — no unrelated rewrites. Where a
fix changes behaviour, add/adjust a test so it's covered. Rotate any exposed secret is a human action —
flag it loudly; don't just delete it from the diff and consider it solved.

## Stage 4 — Re-review  (agent: `security-engineer`, fresh pass)

Re-run the `security-engineer` against the remediated surface: confirm each blocker is actually closed
(not moved), no fix introduced a new hole, and the secret/dependency scans are clean. Loop Stages 3–4
up to `MAX_ROUNDS`; if blockers remain after that, **STOP and escalate** with the open findings — never
declare a surface hardened while a Critical/High stands.

## Stage 5 — Report (and PR, if `MODE=fix-pr`)

Deliver: the threat model summary, the findings table (severity → status: fixed / accepted-risk /
deferred), the remaining risk in plain words, and any human follow-ups (secret rotation, infra/config
changes outside the repo). In `fix-pr` mode, open a CI-gated PR (reuse `/ship-issue`'s worktree + CI
gate + trailers) and file Medium/Low items as labelled follow-up issues.

---

### Guardrails
- **Proportionate, not paranoid** — harden to the real threat model; don't gold-plate a low-value surface.
- Show the **exploit path**, not a scanner hit — a finding without a plausible attack isn't a blocker.
- Never write a secret/token into a commit, PR, issue, or log; flag exposed secrets for **rotation**
  (deleting them from the diff does not un-leak them).
- Every Critical/High ends as fixed or explicitly accepted — never silently dropped.
- Bounded loop; escalate with open findings rather than rubber-stamping.
- Drops into `/ship-issue`'s acceptance board too: on a security-sensitive story the `security-engineer`
  is the gated reviewer. If the role doesn't resolve to a `.claude/agents/*.md`, fall back to
  `general-purpose` with the brief inlined and note it.
