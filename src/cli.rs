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
        /// or `none`). Omit the flag to install every bundled tool; pass `none`
        /// for crew + commands only, or name a subset. Former names (`scrub`)
        /// still select the namespaced tool (`shipmates-scrub`).
        #[arg(long = "with-tools", value_delimiter = ',')]
        with_tools: Option<Vec<String>>,

        /// Skip legacy-command layout migration and identity renames
        /// (`polish` → `shipmates-polish`). Install still writes the current
        /// payload; superseded names are left in place.
        #[arg(long)]
        no_migrate: bool,

        /// Overwrite existing colliding files, including files not claimed by
        /// a receipt. Without this flag, existing files are preserved.
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

        /// Skip the legacy-command and identity-rename sweeps during `--fix`,
        /// matching `install --no-migrate`: missing/drifted files are still
        /// restored, but a superseded `commands/<name>.md` or pre-prefix name
        /// is left in place.
        #[arg(long, requires = "fix")]
        no_migrate: bool,
    },
    Targets,
}
