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

        #[arg(long)]
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

        #[arg(long)]
        dir: Option<String>,

        #[arg(long, default_value_t = false)]
        fix: bool,

        /// Skip the legacy-command migration sweep during `--fix`, matching
        /// `install --no-migrate`: missing/drifted files are still restored, but a
        /// superseded `commands/<name>.md` is left in place.
        #[arg(long)]
        no_migrate: bool,
    },
    Targets,
    /// Drive a command run's finite-state machine (the FSM **engine** the
    /// planned enforcement hook will call — it does not enforce anything on its
    /// own yet; see #114). Exit code is the transition verdict: 0 legal, 1
    /// illegal, 2 error.
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
        #[arg(long)]
        run: u64,

        #[arg(long)]
        to: String,
    },
    /// Assert, then atomically commit the new phase (charging a loop round on a
    /// loopback). Exit 0 on success, 1 illegal, 2 error.
    Advance {
        #[arg(long)]
        run: u64,

        #[arg(long)]
        to: String,
    },
    /// Print the run file's JSON. Exit 2 if it is missing or malformed.
    Status {
        #[arg(long)]
        run: u64,
    },
    /// Decide whether a tool invocation is allowed at the run's current phase,
    /// per the command's `tool_gates` bindings. The first gate whose `match` is a
    /// substring of `--tool` applies; the run must be AT-OR-PAST that gate's
    /// `require` stage. Exit 0 allow (ungated or satisfied), 1 deny (gated but
    /// too early), 2 error (bad/missing run file, or a `require` naming no stage).
    Gate {
        #[arg(long)]
        run: u64,

        /// The shell command string the tool would run, matched against each
        /// gate's `match` substring.
        #[arg(long)]
        tool: String,
    },
}
