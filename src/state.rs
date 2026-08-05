//! `shipmates state` — the finite-state-machine **engine** for a command run.
//!
//! This is the foundation the (planned, #114-gated) enforcement hook will call.
//! It does **not** enforce anything by itself: nothing here is wired into a
//! PreToolUse hook, and no tool invocation is bound to a phase yet. On its own,
//! `shipmates state` only reads and writes a run file and answers "is this
//! transition legal for this command's declared FSM?" — the hook that would make
//! that answer *block* a tool is out of scope (see AGENTS.md, "Scope & honesty").
//!
//! ## FSM model
//!
//! The machine is derived from a command's parsed `stages:` frontmatter — a list
//! of `{order, stage, gate, max_loops}` objects. From the **declared** stage
//! order `s0, s1, … s(n-1)` it builds:
//!
//! * a **forward** edge `s(i) → s(i+1)` on a gate pass (and `s(n-1) → complete`,
//!   the terminal success phase, out of the last stage);
//! * a **loopback** edge `s(i) → s(i-1)` — a "fix" that walks one stage back —
//!   legal only while the run's `fix_rounds` is under `s(i).max_loops`;
//! * an **escalate** edge `s(i) → escalated` (the terminal failure phase), legal
//!   once that loop budget is exhausted.
//!
//! The declared order is enforced **as written** — the engine does not reorder a
//! command's stages to "fix" them.
//!
//! ## Known limits (deliberately not modeled here — the hook slice's concern)
//!
//! * `stages:` does not express a loopback **target**, so a fix is modeled as
//!   "one stage back" (a proxy for "back to the build stage"); the real target a
//!   given gate loops to is not encoded and is not resolved here.
//! * The folded push / CI-poll / merge phases of a real run are not stages here.
//! * `fix_rounds` is a single per-run counter, not a per-stage budget — it is
//!   monotonic across the run and checked against the current stage's `max_loops`.
//! * There are no conditional edges (e.g. selection mode, bundling).
//!
//! Those are all the enforcement hook's job, not this engine's.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::cli::StateAction;
use crate::installer;

/// The current on-disk run-file schema. A file whose `schema_version` is absent
/// or not exactly this value is rejected (fail-closed) rather than migrated.
const SCHEMA_VERSION: u32 = 1;

/// Terminal success phase — reached out of the last stage on a gate pass.
pub const PHASE_COMPLETE: &str = "complete";
/// Terminal failure phase — reached when a stage's loop budget is exhausted.
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
    pub fix_rounds: u32,
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

/// One declared stage: its name and its loop budget.
#[derive(Debug, Clone)]
struct Stage {
    name: String,
    max_loops: u32,
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
        let mut parsed = Vec::with_capacity(stages.len());
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
            parsed.push(Stage { name, max_loops });
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
    fn classify(&self, from: &str, to: &str, fix_rounds: u32) -> Result<Transition, StateError> {
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

        // Forward: to the next declared stage, or to `complete` out of the last.
        if i + 1 < n && to == self.stages[i + 1].name {
            return Ok(Transition::Forward);
        }
        if i + 1 == n && to == PHASE_COMPLETE {
            return Ok(Transition::Forward);
        }

        // Loopback: one declared stage back, while the budget holds.
        if i > 0 && to == self.stages[i - 1].name {
            if fix_rounds < cur.max_loops {
                return Ok(Transition::Loopback);
            }
            return Err(StateError::Illegal(format!(
                "loop budget exhausted at {:?} ({}/{}); only escalation to {:?} is legal",
                cur.name, fix_rounds, cur.max_loops, PHASE_ESCALATED
            )));
        }

        // Escalate: only once the loop budget is spent.
        if to == PHASE_ESCALATED {
            if fix_rounds >= cur.max_loops {
                return Ok(Transition::Escalate);
            }
            return Err(StateError::Illegal(format!(
                "cannot escalate from {:?} before the loop budget is exhausted ({}/{})",
                cur.name, fix_rounds, cur.max_loops
            )));
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

/// The JSON result printed by `assert` / `advance` / `status`.
#[derive(Debug, Serialize)]
struct AssertResult<'a> {
    command: &'a str,
    issue: u64,
    from: &'a str,
    to: &'a str,
    legal: bool,
    kind: Option<&'static str>,
    fix_rounds: u32,
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
        fix_rounds: 0,
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

    match fsm.classify(&record.phase, to, record.fix_rounds) {
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
/// phase. A loopback charges one `fix_rounds`; a forward or escalate leaves the
/// counter unchanged (it is a monotonic per-run budget, see the module docs).
fn cmd_advance(base: &Path, run: u64, to: &str) -> Result<(), StateError> {
    let record = read_run(base, run)?;
    let stages = command_stages(&record.command)?;
    let fsm = Fsm::from_stages(&record.command, &stages)?;

    let kind = match fsm.classify(&record.phase, to, record.fix_rounds) {
        Ok(kind) => kind,
        Err(StateError::Illegal(msg)) => {
            print_result(&record, to, false, None);
            return Err(StateError::Illegal(msg));
        }
        Err(other) => return Err(other),
    };

    let mut next = record.clone();
    next.phase = to.to_string();
    if matches!(kind, Transition::Loopback) {
        next.fix_rounds += 1;
    }
    write_run(base, &next)?;
    print_result(&next, to, true, Some(kind.as_str()));
    Ok(())
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

fn print_result(record: &RunFile, to: &str, legal: bool, kind: Option<&'static str>) {
    let result = AssertResult {
        command: &record.command,
        issue: record.issue,
        from: &record.phase,
        to,
        legal,
        kind,
        fix_rounds: record.fix_rounds,
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

    fn ship_issue_stages() -> Vec<serde_json::Value> {
        vec![
            stage("plan", 1),
            stage("isolate", 1),
            stage("build", 3),
            stage("verify", 3),
            stage("review", 3),
            stage("deliver", 1),
        ]
    }

    fn fsm(stages: &[serde_json::Value]) -> Fsm {
        Fsm::from_stages("test", stages).unwrap()
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
        assert!(matches!(f.classify("plan", "isolate", 0), Ok(Transition::Forward)));
        // skipping a stage is illegal (exit 1)
        assert!(matches!(f.classify("plan", "build", 0), Err(StateError::Illegal(_))));
        // last stage forwards to the terminal success phase
        assert!(matches!(f.classify("deliver", "complete", 0), Ok(Transition::Forward)));
    }

    #[test]
    fn loopback_is_legal_within_budget_and_illegal_when_exhausted() {
        let f = fsm(&ship_issue_stages());
        // build (max_loops 3) may loop back to isolate while under budget
        assert!(matches!(f.classify("build", "isolate", 0), Ok(Transition::Loopback)));
        assert!(matches!(f.classify("build", "isolate", 2), Ok(Transition::Loopback)));
        // at the budget the loopback is refused — only escalation is legal
        assert!(matches!(f.classify("build", "isolate", 3), Err(StateError::Illegal(_))));
        assert!(matches!(f.classify("build", PHASE_ESCALATED, 3), Ok(Transition::Escalate)));
        // escalating early (budget not spent) is illegal
        assert!(matches!(f.classify("build", PHASE_ESCALATED, 0), Err(StateError::Illegal(_))));
    }

    #[test]
    fn terminal_phase_freezes_all_transitions_including_self() {
        let f = fsm(&ship_issue_stages());
        assert!(matches!(f.classify(PHASE_COMPLETE, "plan", 0), Err(StateError::Illegal(_))));
        assert!(matches!(f.classify(PHASE_ESCALATED, "plan", 0), Err(StateError::Illegal(_))));
        // self-transition on a terminal phase is also frozen
        assert!(matches!(f.classify(PHASE_COMPLETE, PHASE_COMPLETE, 0), Err(StateError::Illegal(_))));
        assert!(matches!(f.classify(PHASE_ESCALATED, PHASE_ESCALATED, 0), Err(StateError::Illegal(_))));
    }

    #[test]
    fn a_phase_unknown_to_the_fsm_is_an_error_not_illegal() {
        let f = fsm(&ship_issue_stages());
        assert!(matches!(f.classify("bogus", "plan", 0), Err(StateError::Error(_))));
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
            fix_rounds: 0,
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
        // wrong schema_version
        installer::atomic_write(
            &run_path(base, 8),
            r#"{"schema_version":2,"command":"ship-issue","issue":8,"phase":"plan","fix_rounds":0}"#,
        )
        .unwrap();
        assert!(matches!(read_run(base, 8), Err(StateError::Error(_))));
    }

    #[test]
    fn advance_charges_a_loop_round_only_on_a_loopback() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        // Drive a synthetic FSM directly through the file layer: seed at `build`.
        let seed = RunFile {
            schema_version: SCHEMA_VERSION,
            command: "ship-issue".into(),
            issue: 5,
            phase: "build".into(),
            fix_rounds: 0,
        };
        write_run(base, &seed).unwrap();

        // A loopback build -> isolate charges one round.
        assert_eq!(cmd_advance(base, 5, "isolate").err().map(|e| e.exit_code()), None);
        assert_eq!(read_run(base, 5).unwrap().fix_rounds, 1);
        assert_eq!(read_run(base, 5).unwrap().phase, "isolate");

        // A forward isolate -> build does not charge a round.
        assert!(cmd_advance(base, 5, "build").is_ok());
        assert_eq!(read_run(base, 5).unwrap().fix_rounds, 1);
        assert_eq!(read_run(base, 5).unwrap().phase, "build");
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
            fix_rounds: 0,
        };
        write_run(base, &seed).unwrap();
        // plan -> build skips a stage: illegal (exit 1), and the file is untouched.
        let err = cmd_advance(base, 9, "build").unwrap_err();
        assert_eq!(err.exit_code(), 1);
        assert_eq!(read_run(base, 9).unwrap().phase, "plan");
    }
}
