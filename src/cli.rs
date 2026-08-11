use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    Install {
        #[arg(long, default_value = "claude-code")]
        harness: String,

        /// Install to the global home directory (default)
        #[arg(long, conflicts_with = "local", conflicts_with = "dir")]
        global: bool,

        /// Install to the current local directory
        #[arg(long, conflicts_with = "global", conflicts_with = "dir")]
        local: bool,

        /// Install to a specific directory
        #[arg(long, conflicts_with = "global", conflicts_with = "local")]
        dir: Option<String>,

        /// Which agent-invoked tools to install (comma-separated names, `all`,
        /// or `none`). Omit the flag and an interactive terminal will let you
        /// pick from the available tools; omit it in a non-interactive run and
        /// no tools are installed (a plain install ships only crew + commands).
        #[arg(long = "with-tools", value_delimiter = ',')]
        with_tools: Option<Vec<String>>,

        /// Skip migrating a superseded legacy `commands/<name>.md` layout to the
        /// skill that now supersedes it. Migration is on by default; this is the
        /// escape hatch for a user who wants their old files left in place.
        #[arg(long)]
        no_migrate: bool,

        /// Overwrite colliding files even when a receipt does not claim them.
        #[arg(long)]
        force: bool,
    },
    /// Remove files recorded by a valid install receipt.
    Uninstall {
        /// Harness to remove. Omit only when exactly one valid receipt is found.
        #[arg(long)]
        harness: Option<String>,

        /// Uninstall from the global home directory (default)
        #[arg(long, conflicts_with = "local", conflicts_with = "dir")]
        global: bool,

        /// Uninstall from the current local directory
        #[arg(long, conflicts_with = "global", conflicts_with = "dir")]
        local: bool,

        /// Uninstall from a specific directory
        #[arg(long, conflicts_with = "global", conflicts_with = "local")]
        dir: Option<String>,
    },
    Build {
        #[arg(long, default_value = "claude-code")]
        target: String,

        #[arg(long, default_value = ".")]
        root: String,

        #[arg(long)]
        out: Option<String>,

        #[arg(long, default_value_t = false)]
        check: bool,

        #[arg(long, default_value_t = false)]
        update: bool,
    },
    Check {
        #[arg(long, default_value = "claude-code")]
        target: String,

        #[arg(long, default_value = ".")]
        root: String,
    },
    Update {
        #[arg(long, default_value = "claude-code")]
        target: String,

        #[arg(long, default_value = ".")]
        root: String,
    },
    /// Diagnose (and, with `--fix`, repair) a harness install's health.
    Doctor {
        #[arg(long, default_value = "claude-code")]
        harness: String,

        /// Check the global home directory (default)
        #[arg(long, conflicts_with = "local", conflicts_with = "dir")]
        global: bool,

        /// Check the current local directory
        #[arg(long, conflicts_with = "global", conflicts_with = "dir")]
        local: bool,

        /// Check a specific directory
        #[arg(long, conflicts_with = "global", conflicts_with = "local")]
        dir: Option<String>,

        #[arg(long, default_value_t = false)]
        fix: bool,

        /// Skip the legacy-command migration sweep during `--fix`, matching
        /// `install --no-migrate`: missing/drifted files are still restored, but a
        /// superseded `commands/<name>.md` is left in place.
        #[arg(long, requires = "fix")]
        no_migrate: bool,
    },
    Targets,
    /// Run a harness lifecycle hook dispatcher. Hook shims pass their stdin
    /// through this command so parsing and policy stay in the binary.
    Hook {
        #[command(subcommand)]
        action: HookAction,
    },
    /// Drive a command run's finite-state machine. Installed pre-tool hooks call
    /// the same engine for tool-boundary decisions. Exit code is the transition
    /// verdict: 0 legal, 1 illegal, 2 error.
    State {
        #[command(subcommand)]
        action: StateAction,
    },
}

#[derive(Subcommand)]
pub enum StateAction {
    /// Write the run file at the command's first stage. Refuses to overwrite an
    /// existing run file (fail-closed).
    Init {
        /// Base directory the `.shipmates/run-<N>.json` file is resolved
        /// against. Defaults to the current directory — pass a worktree path so a
        /// hook can gate a run in another checkout without a `cd`.
        #[arg(long, default_value = ".")]
        dir: String,

        /// Numeric issue id — a `u64`, so a run id is never a path-traversal
        /// string.
        #[arg(long)]
        run: u64,

        #[arg(long)]
        command: String,
    },
    /// Report whether current→`--to` is a legal transition, without mutating the
    /// run file. Exit 0 legal, 1 illegal, 2 error.
    Assert {
        /// Base directory the run file is resolved against (default `.`).
        #[arg(long, default_value = ".")]
        dir: String,

        #[arg(long)]
        run: u64,

        #[arg(long)]
        to: String,
    },
    /// Assert, then atomically commit the new phase (charging a loop round on a
    /// loopback). Exit 0 on success, 1 illegal, 2 error.
    Advance {
        /// Base directory the run file is resolved against (default `.`).
        #[arg(long, default_value = ".")]
        dir: String,

        #[arg(long)]
        run: u64,

        #[arg(long)]
        to: String,
    },
    /// Print the run file's JSON. Exit 2 if it is missing or malformed.
    Status {
        /// Base directory the run file is resolved against (default `.`).
        #[arg(long, default_value = ".")]
        dir: String,

        #[arg(long)]
        run: u64,
    },
    /// Decide whether a tool invocation is allowed at the run's current phase,
    /// per the command's `tool_gates` bindings. The first gate whose `match` is a
    /// substring of `--tool` applies; the run must be AT-OR-PAST that gate's
    /// `require` stage. Exit 0 allow (ungated or satisfied), 1 deny (gated but
    /// too early), 2 error (bad/missing run file, or a `require` naming no stage).
    Gate {
        /// Base directory the run file is resolved against (default `.`). A hook
        /// shim passes the worktree path here instead of `cd`-ing into it.
        #[arg(long, default_value = ".")]
        dir: String,

        #[arg(long)]
        run: u64,

        /// The shell command string the tool would run, matched against each
        /// gate's `match` substring.
        #[arg(long)]
        tool: String,
    },
    /// Record a CI-green attestation for the current pull-request head.
    CiAttest {
        #[arg(long, default_value = ".")]
        dir: String,

        #[arg(long)]
        run: u64,

        #[arg(long)]
        pr: u64,
    },
    /// Snapshot the current phase and fix_rounds as a recoverable checkpoint.
    Checkpoint {
        #[arg(long, default_value = ".")]
        dir: String,

        #[arg(long)]
        run: u64,
    },
    /// Detect which convention files (AGENTS.md, CLAUDE.md, README.md) exist at
    /// the project root and print them as JSON.
    Conventions {
        #[arg(long, default_value = ".")]
        dir: String,
    },
}

#[derive(Subcommand)]
pub enum HookAction {
    /// Read a harness pre-tool event from stdin and emit its native deny form.
    Gate {
        #[arg(long)]
        harness: String,
    },
    /// Inject the active run summary into a session/compaction hook.
    Context {
        #[arg(long)]
        harness: String,

        #[arg(long)]
        event: String,
    },
    /// Record a low-sensitivity lifecycle event in the active run file.
    Record {
        #[arg(long)]
        harness: String,

        #[arg(long)]
        event: String,
    },
    /// Snapshot the current phase and fix_rounds as a recoverable checkpoint.
    Checkpoint {
        #[arg(long)]
        harness: String,
    },
    /// Inject detected convention files (AGENTS.md, CLAUDE.md, README.md) as
    /// context for a session-start hook.
    Conventions {
        #[arg(long)]
        harness: String,
    },
    /// Validate the spawned role is legal for the run's current phase, then
    /// inject role-specific context. Deny an out-of-phase spawn.
    SubagentStart {
        #[arg(long)]
        harness: String,
    },
    /// Record a tool event and auto-advance the FSM on unambiguous terminal
    /// signals (e.g. successful `gh pr merge` → complete).
    PostToolUseAdvance {
        #[arg(long)]
        harness: String,
    },
    /// Keep an identified run from ending in a non-terminal phase.
    Stop {
        #[arg(long)]
        harness: String,
    },
}
