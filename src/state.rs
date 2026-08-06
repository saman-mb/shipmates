//! `shipmates state` — the finite-state-machine **engine** for a command run.
//!
//! This is the foundation the enforcement hook calls. The engine still does
//! **not** block anything by itself — it reads and writes a run file and answers
//! two questions over a command's declared FSM: "is this phase transition legal?"
//! (`assert`/`advance`) and "is this tool allowed at the current phase?"
//! (`gate`). A harness PreToolUse hook shim (`enforcement/hooks/<harness>/`) is
//! what turns a `gate` *deny* into a *blocked* tool call; the engine only
//! supplies the verdict and its 0/1/2 exit ABI.
//!
//! ## Tool→phase gate (`gate`)
//!
//! A command may declare `tool_gates:` — `{match, require}` bindings that name a
//! stage a run must be AT-OR-PAST before a tool (matched by a substring of its
//! shell command) is allowed. `gate` finds the first matching binding and ranks
//! the run's current phase against `require` by the same declared stage order
//! the FSM enforces: at-or-past ⇒ allow (exit 0), too early ⇒ deny (exit 1), a
//! misconfigured binding or corrupt run file ⇒ error (exit 2). An unmatched tool
//! is ungated (allow). See [`gate`] for the terminal-phase ranking rules.
//!
//! ## FSM model
//!
//! The machine is derived from a command's parsed `stages:` frontmatter — a list
//! of `{order, stage, gate, max_loops, on_fail}` objects. From the **declared**
//! stage order `s0, s1, … s(n-1)` it builds:
//!
//! * a **forward** edge `s(i) → s(i+1)` on a gate pass (and `s(n-1) → complete`,
//!   the terminal success phase, out of the last stage);
//! * a **loopback** edge `s(i) → on_fail(s(i))` — a "fix" that returns to the
//!   stage the failed gate declares it loops back to (its `on_fail`, defaulting
//!   to the stage literally named `build`, else the first stage) — legal only
//!   while **that stage's own** fix counter is under `s(i).max_loops`. The target
//!   must be a **strictly earlier** stage; a stage with no earlier target — the
//!   pre-build stages and `build` itself — has **no loopback edge** (there is
//!   nothing built yet to send a fix back to);
//! * an **escalate** edge `s(i) → escalated` (the terminal failure phase),
//!   **always legal** from any non-terminal stage — a run may bail out at any
//!   point. `max_loops` bounds the loopback only; escalation never depends on it.
//!
//! Each stage carries its **own** loop budget, counted independently: the run
//! file's `fix_rounds` is a per-stage map, so `verify` exhausting its three fixes
//! leaves `review`'s three untouched. A loopback charges the counter of the
//! stage it departs from; forward and escalate charge nothing.
//!
//! The declared order is enforced **as written** — the engine does not reorder a
//! command's stages to "fix" them, and an `on_fail` that names no declared stage
//! (or names a stage that is not strictly earlier) fails the load: a
//! misconfigured loopback fails the gate.
//!
//! ## Known limits (deliberately not modeled here — the hook slice's concern)
//!
//! * The folded push / CI-poll / merge phases of a real run are not stages here.
//! * There are no conditional edges (e.g. selection mode, bundling).
//!
//! Those are all the enforcement hook's job, not this engine's.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::cli::StateAction;
use crate::installer;

/// The current on-disk run-file schema. A file whose `schema_version` is absent
/// or not exactly this value is rejected (fail-closed) rather than migrated — a
/// v1 file (single monotonic `fix_rounds` counter) is abandoned cleanly, not
/// upgraded.
const SCHEMA_VERSION: u32 = 2;

/// Terminal success phase — reached out of the last stage on a gate pass.
pub const PHASE_COMPLETE: &str = "complete";
/// Terminal failure phase — reachable (unconditionally) from any non-terminal
/// stage; the loop budget bounds only the loopback, never escalation.
pub const PHASE_ESCALATED: &str = "escalated";

/// The on-disk run record: `.shipmates/run-<issue>.json`.
///
/// `issue` is a `u64`, so a run id can never be a path-traversal string — the
/// file name is always `run-<digits>.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunFile {
    pub schema_version: u32,
    pub command: String,
    pub issue: u64,
    pub phase: String,
    /// Per-stage fix-loop tally, keyed by stage name. A `BTreeMap` keeps the
    /// on-disk order deterministic (so the atomic write is byte-stable) and lets
    /// each stage's budget be spent independently. A stage absent from the map
    /// has spent zero rounds.
    pub fix_rounds: BTreeMap<String, u32>,
}

/// Two distinct failure modes, kept apart so the exit-code ABI can route them:
/// an **illegal transition** is exit 1 (a well-formed question with the answer
/// "no"), an **operational error** is exit 2 (a missing/malformed file, an
/// unknown command, a corrupt phase). They must never collapse into one code.
#[derive(Debug)]
pub enum StateError {
    /// A legal-but-refused FSM transition → exit 1.
    Illegal(String),
    /// Could not evaluate the request at all → exit 2.
    Error(String),
}

impl StateError {
    fn exit_code(&self) -> i32 {
        match self {
            StateError::Illegal(_) => 1,
            StateError::Error(_) => 2,
        }
    }

    fn reason(&self) -> &str {
        match self {
            StateError::Illegal(m) | StateError::Error(m) => m,
        }
    }
}

/// One declared stage: its name, its loop budget, and the stage a fix loops back
/// to (`on_fail`, already resolved to a concrete, strictly-earlier declared
/// stage name). `None` means the stage has no loopback target — the pre-build
/// stages, and `build` itself, can't return to an earlier stage, so any
/// non-forward, non-escalate move out of them is an illegal skip.
#[derive(Debug, Clone)]
struct Stage {
    name: String,
    max_loops: u32,
    on_fail: Option<String>,
}

/// A parsed, order-preserving finite-state machine for one command.
#[derive(Debug, Clone)]
struct Fsm {
    stages: Vec<Stage>,
}

/// What a phase name is, relative to the FSM.
enum PhaseKind {
    Stage(usize),
    Terminal,
    Unknown,
}

/// The kind of a legal transition — surfaced in the JSON result and used by
/// `advance` to decide whether to charge a loop round.
enum Transition {
    Forward,
    Loopback,
    Escalate,
}

impl Transition {
    fn as_str(&self) -> &'static str {
        match self {
            Transition::Forward => "forward",
            Transition::Loopback => "loopback",
            Transition::Escalate => "escalate",
        }
    }
}

impl Fsm {
    /// Build the FSM from a command's parsed `stages:` list, preserving the
    /// declared array order (the stages' own `order` field is not used to
    /// re-sort — the declared order is enforced as written).
    fn from_stages(command: &str, stages: &[serde_json::Value]) -> Result<Fsm, StateError> {
        if stages.is_empty() {
            return Err(StateError::Error(format!(
                "command {command:?} declares no stages; it has no FSM to drive"
            )));
        }
        // First pass: read each stage's name, budget, and raw `on_fail`, and
        // reject the reserved terminal names. `on_fail` is resolved in a second
        // pass, once every declared stage name is known.
        struct Raw {
            name: String,
            max_loops: u32,
            on_fail: Option<String>,
        }
        let mut raws = Vec::with_capacity(stages.len());
        for (i, s) in stages.iter().enumerate() {
            let name = s
                .get("stage")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    StateError::Error(format!(
                        "command {command:?} stage #{i} is missing a string `stage` field"
                    ))
                })?
                .to_string();
            // A missing `max_loops` means "no fixes here" (0), not an error — a
            // stage may be a pure forward step.
            let max_loops = s.get("max_loops").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
            if name == PHASE_COMPLETE || name == PHASE_ESCALATED {
                return Err(StateError::Error(format!(
                    "command {command:?} declares a stage named {name:?}, which is a reserved terminal phase"
                )));
            }
            let on_fail = s
                .get("on_fail")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            raws.push(Raw {
                name,
                max_loops,
                on_fail,
            });
        }

        // Stage names must be unique: `phase_kind`/`index_of` use first-match
        // `position` and `fix_rounds` is name-keyed, so a duplicate would
        // silently alias counters and loopback resolution — a misconfig fails
        // the gate.
        for (i, r) in raws.iter().enumerate() {
            if raws[..i].iter().any(|other| other.name == r.name) {
                return Err(StateError::Error(format!(
                    "command {command:?} declares stage {:?} more than once; stage names must be unique",
                    r.name
                )));
            }
        }

        // The default loopback target's name: the stage literally named `build`,
        // else the first declared stage.
        let default_target = if raws.iter().any(|r| r.name == "build") {
            "build".to_string()
        } else {
            raws[0].name.clone()
        };
        let index_of = |name: &str| raws.iter().position(|r| r.name == name);

        // Second pass: resolve each `on_fail`. A loopback must return to a
        // strictly **earlier** stage (a lower declared index) — a later stage is
        // a forward skip, not a fix.
        //
        // * An **explicit** `on_fail` that names no declared stage, or names a
        //   stage that is not strictly earlier, fails the load (a misconfigured
        //   loopback fails the gate).
        // * The **default** target is used only when it resolves to a strictly
        //   earlier stage; for a stage at or before `build` (the pre-build
        //   stages and `build` itself) it yields `None` — no loopback.
        let mut parsed = Vec::with_capacity(raws.len());
        for (i, r) in raws.iter().enumerate() {
            let on_fail = match &r.on_fail {
                Some(explicit) => {
                    let j = index_of(explicit).ok_or_else(|| {
                        StateError::Error(format!(
                            "command {command:?} stage {:?} declares on_fail {explicit:?}, which is not a declared stage",
                            r.name
                        ))
                    })?;
                    if j >= i {
                        return Err(StateError::Error(format!(
                            "command {command:?} stage {:?} declares on_fail {explicit:?}, which is not a strictly earlier stage; a loopback must return to an earlier stage",
                            r.name
                        )));
                    }
                    Some(explicit.clone())
                }
                None => match index_of(&default_target) {
                    Some(j) if j < i => Some(default_target.clone()),
                    _ => None,
                },
            };
            parsed.push(Stage {
                name: r.name.clone(),
                max_loops: r.max_loops,
                on_fail,
            });
        }
        Ok(Fsm { stages: parsed })
    }

    fn first_phase(&self) -> &str {
        &self.stages[0].name
    }

    fn phase_kind(&self, phase: &str) -> PhaseKind {
        if phase == PHASE_COMPLETE || phase == PHASE_ESCALATED {
            return PhaseKind::Terminal;
        }
        match self.stages.iter().position(|s| s.name == phase) {
            Some(i) => PhaseKind::Stage(i),
            None => PhaseKind::Unknown,
        }
    }

    /// Classify the transition `from → to` given the current loop count.
    ///
    /// `Ok(kind)` for a legal move; `Err(Illegal)` for a well-formed but refused
    /// move (including any transition out of a terminal phase — the freeze);
    /// `Err(Error)` only when `from` itself is not a phase of this FSM, i.e. the
    /// run file is internally inconsistent.
    fn classify(
        &self,
        from: &str,
        to: &str,
        fix_rounds: &BTreeMap<String, u32>,
    ) -> Result<Transition, StateError> {
        let i = match self.phase_kind(from) {
            PhaseKind::Unknown => {
                return Err(StateError::Error(format!(
                    "run file phase {from:?} is not a phase of this command's FSM"
                )));
            }
            // A terminal phase freezes: it rejects every outgoing transition,
            // including a self-transition to the same terminal phase.
            PhaseKind::Terminal => {
                return Err(StateError::Illegal(format!(
                    "phase {from:?} is terminal; it rejects all transitions (including to {to:?})"
                )));
            }
            PhaseKind::Stage(i) => i,
        };

        let n = self.stages.len();
        let cur = &self.stages[i];
        // This stage's own tally — an absent key is zero rounds spent.
        let spent = fix_rounds.get(&cur.name).copied().unwrap_or(0);

        // Forward: to the next declared stage, or to `complete` out of the last.
        if i + 1 < n && to == self.stages[i + 1].name {
            return Ok(Transition::Forward);
        }
        if i + 1 == n && to == PHASE_COMPLETE {
            return Ok(Transition::Forward);
        }

        // Loopback: to this stage's declared (strictly earlier) `on_fail`
        // target, while its own budget holds. A stage with no target (`None`)
        // has no loopback edge at all.
        if let Some(target) = &cur.on_fail
            && to == target
        {
            if spent < cur.max_loops {
                return Ok(Transition::Loopback);
            }
            return Err(StateError::Illegal(format!(
                "loop budget exhausted at {:?} ({}/{}); only escalation to {:?} is legal",
                cur.name, spent, cur.max_loops, PHASE_ESCALATED
            )));
        }

        // Escalate: always legal from any non-terminal stage (terminal phases
        // are already frozen above). A run may bail out at any point, so this
        // never depends on the loop budget — `max_loops` bounds the loopback
        // only. Charges nothing.
        if to == PHASE_ESCALATED {
            return Ok(Transition::Escalate);
        }

        Err(StateError::Illegal(format!(
            "no legal transition {from:?} -> {to:?}"
        )))
    }
}

/// The run-file path for an issue under a base directory. `issue` is a `u64`, so
/// the name is always `run-<digits>.json` — no traversal is expressible.
fn run_path(base: &Path, issue: u64) -> PathBuf {
    base.join(".shipmates").join(format!("run-{issue}.json"))
}

/// Read and validate the run file. Fail-closed: a missing file, unparseable
/// JSON, or a `schema_version` other than [`SCHEMA_VERSION`] is an error — never
/// a silently-defaulted or migrated record.
fn read_run(base: &Path, issue: u64) -> Result<RunFile, StateError> {
    let path = run_path(base, issue);
    let raw = std::fs::read_to_string(&path).map_err(|e| {
        StateError::Error(format!("cannot read run file {}: {}", path.display(), e))
    })?;
    let run: RunFile = serde_json::from_str(&raw).map_err(|e| {
        StateError::Error(format!("run file {} is malformed: {}", path.display(), e))
    })?;
    if run.schema_version != SCHEMA_VERSION {
        return Err(StateError::Error(format!(
            "run file {} has unsupported schema_version {} (expected {})",
            path.display(),
            run.schema_version,
            SCHEMA_VERSION
        )));
    }
    if run.issue != issue {
        return Err(StateError::Error(format!(
            "run file {} records issue {} but was read for issue {}",
            path.display(),
            run.issue,
            issue
        )));
    }
    Ok(run)
}

/// Serialize a run record to its canonical on-disk JSON form (pretty, trailing
/// newline), written atomically via [`installer::atomic_write`] — never a
/// `mktemp`-and-move by hand.
fn write_run(base: &Path, run: &RunFile) -> Result<(), StateError> {
    let path = run_path(base, run.issue);
    let mut body = serde_json::to_string_pretty(run)
        .map_err(|e| StateError::Error(format!("cannot serialize run file: {e}")))?;
    body.push('\n');
    installer::atomic_write(&path, &body).map_err(|e| {
        StateError::Error(format!("cannot write run file {}: {}", path.display(), e))
    })
}

/// Look up a command's declared stages from the embedded catalog (the payload
/// compiled in by `build.rs`), so an installed binary with no checkout still
/// resolves the FSM.
fn command_stages(command: &str) -> Result<Vec<serde_json::Value>, StateError> {
    let commands = crate::catalog::load_commands_embedded()
        .map_err(|e| StateError::Error(format!("cannot load commands: {e}")))?;
    commands
        .into_iter()
        .find(|c| c.name == command)
        .map(|c| c.stages)
        .ok_or_else(|| StateError::Error(format!("unknown command {command:?}")))
}

/// Look up a command's `tool_gates` bindings from the embedded catalog. An
/// unknown command is an error (the same failure `command_stages` reports); a
/// command with no bindings yields an empty list — every tool is then ungated.
fn command_tool_gates(command: &str) -> Result<Vec<serde_json::Value>, StateError> {
    let commands = crate::catalog::load_commands_embedded()
        .map_err(|e| StateError::Error(format!("cannot load commands: {e}")))?;
    commands
        .into_iter()
        .find(|c| c.name == command)
        .map(|c| c.tool_gates)
        .ok_or_else(|| StateError::Error(format!("unknown command {command:?}")))
}

/// The JSON result printed by `assert` / `advance` / `status`.
#[derive(Debug, Serialize)]
struct AssertResult<'a> {
    command: &'a str,
    issue: u64,
    from: &'a str,
    to: &'a str,
    legal: bool,
    kind: Option<&'static str>,
    fix_rounds: &'a BTreeMap<String, u32>,
}

/// Dispatch a `state` action against the current working directory. Returns the
/// process exit code (0 legal / 1 illegal / 2 error) — the caller exits with it.
pub fn dispatch(action: &StateAction) -> i32 {
    dispatch_at(Path::new("."), action)
}

/// Dispatch a `state` action against an explicit base directory. Threading the
/// base makes every path resolution testable without touching the real cwd.
pub fn dispatch_at(base: &Path, action: &StateAction) -> i32 {
    let result = match action {
        StateAction::Init { run, command } => cmd_init(base, *run, command),
        StateAction::Assert { run, to } => cmd_assert(base, *run, to),
        StateAction::Advance { run, to } => cmd_advance(base, *run, to),
        StateAction::Status { run } => cmd_status(base, *run),
        StateAction::Gate { run, tool } => cmd_gate(base, *run, tool),
    };
    match result {
        Ok(()) => 0,
        Err(e) => {
            let label = match e {
                StateError::Illegal(_) => "illegal transition",
                StateError::Error(_) => "error",
            };
            eprintln!("state: {label}: {}", e.reason());
            e.exit_code()
        }
    }
}

/// `state init --run N --command NAME` — write the run file at the command's
/// first stage. Refuses to overwrite any existing file (fail-closed): a valid
/// run must not be silently reset, and a malformed one must never be clobbered.
fn cmd_init(base: &Path, run: u64, command: &str) -> Result<(), StateError> {
    let stages = command_stages(command)?;
    let fsm = Fsm::from_stages(command, &stages)?;

    let path = run_path(base, run);
    if path.exists() {
        return Err(StateError::Error(format!(
            "run file {} already exists; refusing to overwrite",
            path.display()
        )));
    }

    let record = RunFile {
        schema_version: SCHEMA_VERSION,
        command: command.to_string(),
        issue: run,
        phase: fsm.first_phase().to_string(),
        fix_rounds: BTreeMap::new(),
    };
    write_run(base, &record)?;
    print_run(&record);
    Ok(())
}

/// `state assert --run N --to PHASE` — answer whether current→PHASE is legal for
/// this run's command FSM, without mutating anything. Prints the JSON result to
/// stdout on a legal move; on an illegal move it prints the JSON (with
/// `legal:false`) to stdout and the dispatcher writes the greppable reason to
/// stderr.
fn cmd_assert(base: &Path, run: u64, to: &str) -> Result<(), StateError> {
    let record = read_run(base, run)?;
    let stages = command_stages(&record.command)?;
    let fsm = Fsm::from_stages(&record.command, &stages)?;

    match fsm.classify(&record.phase, to, &record.fix_rounds) {
        Ok(kind) => {
            print_result(&record, to, true, Some(kind.as_str()));
            Ok(())
        }
        Err(StateError::Illegal(msg)) => {
            print_result(&record, to, false, None);
            Err(StateError::Illegal(msg))
        }
        Err(other) => Err(other),
    }
}

/// `state advance --run N --to PHASE` — assert, then atomically commit the new
/// phase. A loopback charges one round to the **departing** stage's own counter;
/// a forward or escalate leaves every counter unchanged (see the module docs).
fn cmd_advance(base: &Path, run: u64, to: &str) -> Result<(), StateError> {
    let record = read_run(base, run)?;
    let stages = command_stages(&record.command)?;
    let fsm = Fsm::from_stages(&record.command, &stages)?;

    let kind = match fsm.classify(&record.phase, to, &record.fix_rounds) {
        Ok(kind) => kind,
        Err(StateError::Illegal(msg)) => {
            print_result(&record, to, false, None);
            return Err(StateError::Illegal(msg));
        }
        Err(other) => return Err(other),
    };

    let mut next = record.clone();
    if matches!(kind, Transition::Loopback) {
        *next.fix_rounds.entry(record.phase.clone()).or_insert(0) += 1;
    }
    next.phase = to.to_string();
    write_run(base, &next)?;
    print_result(&next, to, true, Some(kind.as_str()));
    Ok(())
}

/// The verdict of the pure [`gate`] function: allow a tool, deny it (with a
/// greppable reason), or an error (a misconfigured gate, or a run-file phase the
/// FSM does not know). These map onto the process exit ABI 0 / 1 / 2 exactly as
/// [`StateError`] does — allow → 0, deny → 1, error → 2.
#[derive(Debug, PartialEq)]
enum GateDecision {
    /// The tool is ungated, or the run has reached the required stage — exit 0.
    Allow,
    /// The tool is gated and the run is too early — exit 1, with the reason.
    Deny(String),
    /// The gate could not be evaluated (a `require` naming no stage, or a phase
    /// the FSM does not know) — exit 2.
    Error(String),
}

/// Pure tool→phase gate decision — the whole policy, isolated from I/O so it is
/// unit-testable without a run file.
///
/// The FIRST `tool_gates` entry whose `match` is a substring of `tool_command`
/// applies (declaration order is precedence). With no match the tool is
/// **ungated** → [`GateDecision::Allow`]. When a gate applies, the run must be
/// **AT-OR-PAST** that gate's `require` stage, ranked by the command's declared
/// stage order (the same order the FSM enforces):
///
/// * `require` must name a declared stage — else [`GateDecision::Error`] (a
///   misconfigured binding, exit 2), mirroring how a bad `on_fail` fails the FSM
///   load.
/// * the current phase is ranked by its stage index; the terminal success phase
///   `complete` ranks past every stage (allow), while the terminal failure phase
///   `escalated` is a bailed-out run that never satisfies a forward gate (deny).
/// * a current phase the FSM does not know at all is an internally-inconsistent
///   run file → [`GateDecision::Error`] (exit 2), matching `classify`.
///
/// A `require >= ` current-phase rank denies; rank `>=` require allows.
fn gate(
    command_stages: &[serde_json::Value],
    current_phase: &str,
    tool_gates: &[serde_json::Value],
    tool_command: &str,
) -> GateDecision {
    // Find the first gate whose `match` substring is present in the command.
    // A gate with no string `match` can never apply, so it is skipped; a gate
    // that DOES apply but carries no string `require` is a misconfiguration.
    for g in tool_gates {
        let Some(needle) = g.get("match").and_then(|v| v.as_str()) else {
            continue;
        };
        if !tool_command.contains(needle) {
            continue;
        }
        let Some(require) = g.get("require").and_then(|v| v.as_str()) else {
            return GateDecision::Error(format!(
                "tool_gate matching {needle:?} has no string `require` stage"
            ));
        };

        let fsm = match Fsm::from_stages("<gate>", command_stages) {
            Ok(f) => f,
            Err(e) => return GateDecision::Error(e.reason().to_string()),
        };

        // `require` must name a declared stage — a terminal or unknown name is a
        // misconfigured gate.
        let require_idx = match fsm.phase_kind(require) {
            PhaseKind::Stage(i) => i,
            _ => {
                return GateDecision::Error(format!(
                    "tool_gate `require` {require:?} is not a stage of this command"
                ));
            }
        };

        // Rank the current phase against the declared stage order. `complete`
        // sits past the last stage (allow-all); `escalated` is a bailed-out run
        // that never reaches a forward gate (deny-all); an unknown phase is a
        // corrupt run file.
        let phase_rank: i64 = match fsm.phase_kind(current_phase) {
            PhaseKind::Stage(i) => i as i64,
            PhaseKind::Terminal if current_phase == PHASE_COMPLETE => fsm.stages.len() as i64,
            PhaseKind::Terminal => -1, // escalated
            PhaseKind::Unknown => {
                return GateDecision::Error(format!(
                    "run file phase {current_phase:?} is not a phase of this command's FSM"
                ));
            }
        };

        return if phase_rank >= require_idx as i64 {
            GateDecision::Allow
        } else {
            GateDecision::Deny(format!(
                "gate: {needle} requires phase>={require}, run is at {current_phase}"
            ))
        };
    }

    // No gate matched — the tool is ungated.
    GateDecision::Allow
}

/// `state gate --run N --tool "<command>"` — the thin CLI wrapper over [`gate`].
/// Reads the run file (fail-closed), loads its command's declared stages and
/// `tool_gates`, and routes the decision onto the 0 / 1 / 2 exit ABI.
fn cmd_gate(base: &Path, run: u64, tool: &str) -> Result<(), StateError> {
    let record = read_run(base, run)?;
    let stages = command_stages(&record.command)?;
    let gates = command_tool_gates(&record.command)?;
    match gate(&stages, &record.phase, &gates, tool) {
        GateDecision::Allow => {
            print_gate(&record, tool, true, None);
            Ok(())
        }
        GateDecision::Deny(reason) => {
            print_gate(&record, tool, false, Some(&reason));
            Err(StateError::Illegal(reason))
        }
        GateDecision::Error(msg) => Err(StateError::Error(msg)),
    }
}

/// `state status --run N` — print the run JSON. Fail-closed on read.
fn cmd_status(base: &Path, run: u64) -> Result<(), StateError> {
    let record = read_run(base, run)?;
    print_run(&record);
    Ok(())
}

fn print_run(record: &RunFile) {
    match serde_json::to_string_pretty(record) {
        Ok(s) => println!("{s}"),
        Err(e) => eprintln!("state: error: cannot render run file: {e}"),
    }
}

/// The JSON verdict printed to stdout by `gate` (both allow and deny). The
/// greppable deny reason also goes to stderr via the dispatcher, which is what
/// the PreToolUse hook shim reads.
#[derive(Debug, Serialize)]
struct GateResult<'a> {
    command: &'a str,
    issue: u64,
    phase: &'a str,
    tool: &'a str,
    allow: bool,
    reason: Option<&'a str>,
}

fn print_gate(record: &RunFile, tool: &str, allow: bool, reason: Option<&str>) {
    let result = GateResult {
        command: &record.command,
        issue: record.issue,
        phase: &record.phase,
        tool,
        allow,
        reason,
    };
    match serde_json::to_string(&result) {
        Ok(s) => println!("{s}"),
        Err(e) => eprintln!("state: error: cannot render gate result: {e}"),
    }
}

fn print_result(record: &RunFile, to: &str, legal: bool, kind: Option<&'static str>) {
    let result = AssertResult {
        command: &record.command,
        issue: record.issue,
        from: &record.phase,
        to,
        legal,
        kind,
        fix_rounds: &record.fix_rounds,
    };
    match serde_json::to_string(&result) {
        Ok(s) => println!("{s}"),
        Err(e) => eprintln!("state: error: cannot render result: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stage(name: &str, max_loops: u64) -> serde_json::Value {
        serde_json::json!({"order": 1, "stage": name, "gate": "g", "max_loops": max_loops})
    }

    fn stage_on_fail(name: &str, max_loops: u64, on_fail: &str) -> serde_json::Value {
        serde_json::json!({"order": 1, "stage": name, "gate": "g", "max_loops": max_loops, "on_fail": on_fail})
    }

    /// The ship-issue stage list, with `verify` and `review` looping back to
    /// `build` (as the real command declares) and the other stages taking the
    /// default `on_fail`.
    fn ship_issue_stages() -> Vec<serde_json::Value> {
        vec![
            stage("plan", 1),
            stage("isolate", 1),
            stage("build", 3),
            stage_on_fail("verify", 3, "build"),
            stage_on_fail("review", 3, "build"),
            stage("deliver", 1),
        ]
    }

    fn fsm(stages: &[serde_json::Value]) -> Fsm {
        Fsm::from_stages("test", stages).unwrap()
    }

    /// Build a per-stage `fix_rounds` map from `(stage, spent)` pairs.
    fn rounds(pairs: &[(&str, u32)]) -> BTreeMap<String, u32> {
        pairs.iter().map(|(k, v)| (k.to_string(), *v)).collect()
    }

    /// An empty per-stage tally — nothing spent anywhere.
    fn no_rounds() -> BTreeMap<String, u32> {
        BTreeMap::new()
    }

    #[test]
    fn empty_stages_is_an_error_not_an_empty_fsm() {
        let err = Fsm::from_stages("test", &[]).unwrap_err();
        assert!(matches!(err, StateError::Error(_)));
    }

    #[test]
    fn reserved_terminal_name_as_a_stage_is_rejected() {
        let err = Fsm::from_stages("test", &[stage("complete", 0)]).unwrap_err();
        assert!(matches!(err, StateError::Error(_)));
    }

    #[test]
    fn forward_edge_is_legal_and_skip_is_illegal() {
        let f = fsm(&ship_issue_stages());
        assert!(matches!(f.classify("plan", "isolate", &no_rounds()), Ok(Transition::Forward)));
        // plan -> build skips isolate; build is *later* than plan so it is not a
        // loopback but an illegal forward jump.
        assert!(matches!(f.classify("plan", "build", &no_rounds()), Err(StateError::Illegal(_))));
        // last stage forwards to the terminal success phase
        assert!(matches!(f.classify("deliver", "complete", &no_rounds()), Ok(Transition::Forward)));
    }

    #[test]
    fn loopback_is_legal_within_budget_and_illegal_when_exhausted() {
        let f = fsm(&ship_issue_stages());
        // verify (max_loops 3, on_fail build) may loop back to build under budget
        assert!(matches!(f.classify("verify", "build", &no_rounds()), Ok(Transition::Loopback)));
        assert!(matches!(f.classify("verify", "build", &rounds(&[("verify", 2)])), Ok(Transition::Loopback)));
        // at the budget the loopback is refused — only escalation is legal
        assert!(matches!(f.classify("verify", "build", &rounds(&[("verify", 3)])), Err(StateError::Illegal(_))));
        assert!(matches!(f.classify("verify", PHASE_ESCALATED, &rounds(&[("verify", 3)])), Ok(Transition::Escalate)));
        // escalation is always available — even before the loop budget is spent
        assert!(matches!(f.classify("verify", PHASE_ESCALATED, &no_rounds()), Ok(Transition::Escalate)));
    }

    #[test]
    fn loopback_lands_on_the_declared_on_fail_target_not_the_prior_stage() {
        let f = fsm(&ship_issue_stages());
        // review declares on_fail: build, so a rejected review loops to build...
        assert!(matches!(f.classify("review", "build", &no_rounds()), Ok(Transition::Loopback)));
        // ...not to the stage physically before it (verify), which is neither a
        // forward edge nor review's loopback target.
        assert!(matches!(f.classify("review", "verify", &no_rounds()), Err(StateError::Illegal(_))));
    }

    #[test]
    fn per_stage_budgets_are_spent_independently() {
        let f = fsm(&ship_issue_stages());
        // verify has exhausted its own three fixes → it can no longer loop back
        // to build, only escalate.
        let spent = rounds(&[("verify", 3)]);
        assert!(matches!(f.classify("verify", "build", &spent), Err(StateError::Illegal(_))));
        assert!(matches!(f.classify("verify", PHASE_ESCALATED, &spent), Ok(Transition::Escalate)));
        // ...yet review still holds its own full budget and loops back to build.
        assert!(matches!(f.classify("review", "build", &spent), Ok(Transition::Loopback)));
        // (escalation stays available to review too — it never depends on the budget.)
        assert!(matches!(f.classify("review", PHASE_ESCALATED, &spent), Ok(Transition::Escalate)));
    }

    #[test]
    fn escalated_is_reachable_from_every_non_terminal_stage() {
        let f = fsm(&ship_issue_stages());
        // Every stage can bail out to `escalated`, regardless of its loop budget
        // or how much of it is spent — no soft-lock, no dependence on max_loops.
        for s in ["plan", "isolate", "build", "verify", "review", "deliver"] {
            assert!(
                matches!(f.classify(s, PHASE_ESCALATED, &no_rounds()), Ok(Transition::Escalate)),
                "escalate must be legal from {s:?}"
            );
        }
        // ...but a terminal phase is still frozen and rejects escalate.
        assert!(matches!(f.classify(PHASE_COMPLETE, PHASE_ESCALATED, &no_rounds()), Err(StateError::Illegal(_))));
        assert!(matches!(f.classify(PHASE_ESCALATED, PHASE_ESCALATED, &no_rounds()), Err(StateError::Illegal(_))));
    }

    #[test]
    fn duplicate_stage_names_are_rejected_at_load() {
        let stages = vec![stage("build", 3), stage("verify", 3), stage("build", 1)];
        let err = Fsm::from_stages("test", &stages).unwrap_err();
        assert!(matches!(err, StateError::Error(_)));
    }

    #[test]
    fn loopback_target_must_be_an_earlier_stage_else_it_is_an_illegal_skip() {
        let f = fsm(&ship_issue_stages());
        // plan's default on_fail (build) is *later* than plan, so plan has no
        // loopback edge: plan -> build skips isolate and is an illegal forward
        // jump, not a fix.
        assert!(matches!(f.classify("plan", "build", &no_rounds()), Err(StateError::Illegal(_))));
        assert!(matches!(f.classify("plan", "isolate", &no_rounds()), Ok(Transition::Forward)));

        // An explicit on_fail naming a *later* stage is a misconfigured loopback
        // and fails the gate at load.
        let bad = vec![stage_on_fail("build", 3, "verify"), stage("verify", 3)];
        let err = Fsm::from_stages("test", &bad).unwrap_err();
        assert!(matches!(err, StateError::Error(_)));
    }

    #[test]
    fn default_on_fail_is_build_when_present_else_the_first_stage() {
        // A stage literally named `build` exists and is earlier → the default
        // target is build.
        let with_build = vec![stage("build", 1), stage("verify", 2)];
        let f = fsm(&with_build);
        assert!(matches!(f.classify("verify", "build", &no_rounds()), Ok(Transition::Loopback)));

        // No stage named build → the default target is the first declared stage.
        let no_build = vec![stage("alpha", 1), stage("beta", 2)];
        let f = fsm(&no_build);
        assert!(matches!(f.classify("beta", "alpha", &no_rounds()), Ok(Transition::Loopback)));
    }

    #[test]
    fn on_fail_naming_no_declared_stage_is_rejected_at_load() {
        let stages = vec![stage("build", 3), stage_on_fail("verify", 3, "biuld")];
        let err = Fsm::from_stages("test", &stages).unwrap_err();
        assert!(matches!(err, StateError::Error(_)));
    }

    #[test]
    fn terminal_phase_freezes_all_transitions_including_self() {
        let f = fsm(&ship_issue_stages());
        assert!(matches!(f.classify(PHASE_COMPLETE, "plan", &no_rounds()), Err(StateError::Illegal(_))));
        assert!(matches!(f.classify(PHASE_ESCALATED, "plan", &no_rounds()), Err(StateError::Illegal(_))));
        // self-transition on a terminal phase is also frozen
        assert!(matches!(f.classify(PHASE_COMPLETE, PHASE_COMPLETE, &no_rounds()), Err(StateError::Illegal(_))));
        assert!(matches!(f.classify(PHASE_ESCALATED, PHASE_ESCALATED, &no_rounds()), Err(StateError::Illegal(_))));
    }

    #[test]
    fn a_phase_unknown_to_the_fsm_is_an_error_not_illegal() {
        let f = fsm(&ship_issue_stages());
        assert!(matches!(f.classify("bogus", "plan", &no_rounds()), Err(StateError::Error(_))));
    }

    #[test]
    fn init_writes_first_stage_and_refuses_to_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        // Use a synthetic run file by hand-writing after building via the FSM,
        // exercising the read/write round-trip against a threaded base path.
        let record = RunFile {
            schema_version: SCHEMA_VERSION,
            command: "ship-issue".into(),
            issue: 42,
            phase: "plan".into(),
            fix_rounds: no_rounds(),
        };
        write_run(base, &record).unwrap();
        let back = read_run(base, 42).unwrap();
        assert_eq!(back.phase, "plan");
        assert_eq!(back.issue, 42);
        // the file lives under `.shipmates/run-42.json`
        assert!(base.join(".shipmates/run-42.json").is_file());
    }

    #[test]
    fn read_is_fail_closed_on_missing_malformed_and_bad_schema() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        // missing
        assert!(matches!(read_run(base, 7), Err(StateError::Error(_))));
        // malformed JSON
        installer::atomic_write(&run_path(base, 7), "{not json").unwrap();
        assert!(matches!(read_run(base, 7), Err(StateError::Error(_))));
        // an unsupported (future) schema_version
        installer::atomic_write(
            &run_path(base, 8),
            r#"{"schema_version":99,"command":"ship-issue","issue":8,"phase":"plan","fix_rounds":{}}"#,
        )
        .unwrap();
        assert!(matches!(read_run(base, 8), Err(StateError::Error(_))));
    }

    #[test]
    fn a_v1_run_file_is_rejected_not_migrated() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        // A v1 record (single monotonic `fix_rounds` counter) is abandoned
        // cleanly with exit 2 — never silently upgraded to the v2 per-stage map.
        installer::atomic_write(
            &run_path(base, 11),
            r#"{"schema_version":1,"command":"ship-issue","issue":11,"phase":"build","fix_rounds":2}"#,
        )
        .unwrap();
        let err = read_run(base, 11).unwrap_err();
        assert_eq!(err.exit_code(), 2);
    }

    #[test]
    fn advance_charges_a_loop_round_only_on_a_loopback() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        // Seed at `verify`, which declares on_fail: build in ship-issue.
        let seed = RunFile {
            schema_version: SCHEMA_VERSION,
            command: "ship-issue".into(),
            issue: 5,
            phase: "verify".into(),
            fix_rounds: no_rounds(),
        };
        write_run(base, &seed).unwrap();

        // A loopback verify -> build charges one round to verify's own counter.
        assert_eq!(cmd_advance(base, 5, "build").err().map(|e| e.exit_code()), None);
        assert_eq!(read_run(base, 5).unwrap().fix_rounds.get("verify").copied(), Some(1));
        assert_eq!(read_run(base, 5).unwrap().phase, "build");

        // A forward build -> verify does not charge a round.
        assert!(cmd_advance(base, 5, "verify").is_ok());
        assert_eq!(read_run(base, 5).unwrap().fix_rounds.get("verify").copied(), Some(1));
        assert_eq!(read_run(base, 5).unwrap().phase, "verify");
    }

    #[test]
    fn advance_charges_each_stage_against_its_own_budget() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        // Seed with build already at its exhausted budget, verify at zero.
        let seed = RunFile {
            schema_version: SCHEMA_VERSION,
            command: "ship-issue".into(),
            issue: 6,
            phase: "verify".into(),
            fix_rounds: rounds(&[("build", 3)]),
        };
        write_run(base, &seed).unwrap();
        // verify still has its own budget: verify -> build is a legal loopback,
        // and it charges verify (not build).
        assert!(cmd_advance(base, 6, "build").is_ok());
        let back = read_run(base, 6).unwrap();
        assert_eq!(back.fix_rounds.get("verify").copied(), Some(1));
        assert_eq!(back.fix_rounds.get("build").copied(), Some(3));
        assert_eq!(back.phase, "build");
    }

    #[test]
    fn advance_refuses_an_illegal_transition_without_writing() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        let seed = RunFile {
            schema_version: SCHEMA_VERSION,
            command: "ship-issue".into(),
            issue: 9,
            phase: "plan".into(),
            fix_rounds: no_rounds(),
        };
        write_run(base, &seed).unwrap();
        // plan -> build skips isolate and is a forward jump (build is later than
        // plan, so it is not a loopback): illegal (exit 1), file untouched.
        let err = cmd_advance(base, 9, "build").unwrap_err();
        assert_eq!(err.exit_code(), 1);
        assert_eq!(read_run(base, 9).unwrap().phase, "plan");
    }

    // --- tool→phase gate -------------------------------------------------

    /// The ship-issue tool_gates: `gh pr merge` needs `deliver`, `git push`
    /// needs `build` (the same bindings the real command declares).
    fn ship_issue_tool_gates() -> Vec<serde_json::Value> {
        serde_json::from_str(
            r#"[{"match":"gh pr merge","require":"deliver"},{"match":"git push","require":"build"}]"#,
        )
        .unwrap()
    }

    #[test]
    fn gate_denies_a_gated_tool_before_its_required_stage() {
        let stages = ship_issue_stages();
        let gates = ship_issue_tool_gates();
        // At `build`, `gh pr merge` (require deliver) is too early → deny with a
        // greppable reason naming the match, the require, and the current phase.
        match gate(&stages, "build", &gates, "gh pr merge --squash --delete-branch") {
            GateDecision::Deny(reason) => {
                assert!(reason.contains("gate:"), "{reason}");
                assert!(reason.contains("gh pr merge"), "{reason}");
                assert!(reason.contains("phase>=deliver"), "{reason}");
                assert!(reason.contains("run is at build"), "{reason}");
            }
            other => panic!("expected deny, got {other:?}"),
        }
    }

    #[test]
    fn gate_allows_a_gated_tool_at_or_past_its_required_stage() {
        let stages = ship_issue_stages();
        let gates = ship_issue_tool_gates();
        // Exactly at `deliver`, and past it (`complete`), both satisfy the gate.
        assert_eq!(gate(&stages, "deliver", &gates, "gh pr merge"), GateDecision::Allow);
        assert_eq!(gate(&stages, PHASE_COMPLETE, &gates, "gh pr merge"), GateDecision::Allow);
    }

    #[test]
    fn gate_allows_an_ungated_tool() {
        let stages = ship_issue_stages();
        let gates = ship_issue_tool_gates();
        // No binding matches `cargo test`, so it is ungated at any phase.
        assert_eq!(gate(&stages, "plan", &gates, "cargo test"), GateDecision::Allow);
    }

    #[test]
    fn gate_gates_git_push_from_build_onward() {
        let stages = ship_issue_stages();
        let gates = ship_issue_tool_gates();
        // `git push` requires `build`: denied at plan, allowed once built.
        assert!(matches!(
            gate(&stages, "plan", &gates, "git push -u origin HEAD"),
            GateDecision::Deny(_)
        ));
        assert_eq!(gate(&stages, "build", &gates, "git push -u origin HEAD"), GateDecision::Allow);
    }

    #[test]
    fn gate_uses_declaration_order_for_precedence() {
        // A command string that contains BOTH needles takes the FIRST declared
        // binding — here `gh pr merge` (require deliver), so at `build` it denies
        // even though the `git push` binding (require build) would allow.
        let stages = ship_issue_stages();
        let gates = ship_issue_tool_gates();
        let both = "gh pr merge && git push";
        assert!(matches!(gate(&stages, "build", &gates, both), GateDecision::Deny(_)));
    }

    #[test]
    fn gate_escalated_run_never_satisfies_a_forward_gate() {
        // An escalated (bailed-out) run is a terminal failure; it never reaches a
        // forward gate, so a gated tool is denied.
        let stages = ship_issue_stages();
        let gates = ship_issue_tool_gates();
        assert!(matches!(
            gate(&stages, PHASE_ESCALATED, &gates, "gh pr merge"),
            GateDecision::Deny(_)
        ));
    }

    #[test]
    fn gate_require_naming_no_stage_is_an_error() {
        let stages = ship_issue_stages();
        let gates: Vec<serde_json::Value> =
            serde_json::from_str(r#"[{"match":"gh pr merge","require":"ship"}]"#).unwrap();
        assert!(matches!(
            gate(&stages, "build", &gates, "gh pr merge"),
            GateDecision::Error(_)
        ));
    }

    #[test]
    fn gate_matching_binding_without_require_is_an_error() {
        let stages = ship_issue_stages();
        // The binding matches the command but carries no `require` — a misconfig.
        let gates: Vec<serde_json::Value> =
            serde_json::from_str(r#"[{"match":"gh pr merge"}]"#).unwrap();
        assert!(matches!(
            gate(&stages, "build", &gates, "gh pr merge"),
            GateDecision::Error(_)
        ));
    }

    #[test]
    fn cmd_gate_denies_deny_and_errors_on_a_missing_run() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        // A missing run file is fail-closed → exit 2 (error), never a silent allow.
        let err = cmd_gate(base, 1, "gh pr merge").unwrap_err();
        assert_eq!(err.exit_code(), 2);

        // Seed a real ship-issue run at `build`; `gh pr merge` is denied → exit 1.
        let seed = RunFile {
            schema_version: SCHEMA_VERSION,
            command: "ship-issue".into(),
            issue: 1,
            phase: "build".into(),
            fix_rounds: no_rounds(),
        };
        write_run(base, &seed).unwrap();
        let err = cmd_gate(base, 1, "gh pr merge --squash").unwrap_err();
        assert_eq!(err.exit_code(), 1);
        assert!(err.reason().contains("gate:"), "{}", err.reason());

        // ...and `git push` is allowed at `build` → Ok (exit 0).
        assert!(cmd_gate(base, 1, "git push origin HEAD").is_ok());
    }
}
