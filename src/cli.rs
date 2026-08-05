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
}
