use clap::{Args, Parser, Subcommand};

/// Shared `--global` / `--local` / `--dir` trio for install-lifecycle commands.
#[derive(Args, Debug, Clone)]
pub struct LocationOpts {
    /// Global home directory (default when neither `--local` nor `--dir` is set)
    #[arg(
        long,
        conflicts_with_all = ["local", "dir"],
        help_heading = "Where"
    )]
    pub global: bool,

    /// Current working directory
    #[arg(
        long,
        conflicts_with_all = ["global", "dir"],
        help_heading = "Where"
    )]
    pub local: bool,

    /// Explicit project or root directory
    #[arg(
        long,
        value_name = "PATH",
        conflicts_with_all = ["global", "local"],
        help_heading = "Where"
    )]
    pub dir: Option<String>,
}

#[derive(Parser)]
#[command(
    name = "shipmates",
    author,
    version,
    about = "Install and refresh specialist AI agent crews for coding harnesses",
    long_about = "Shipmates drops a crew of specialist agents and slash-command workflows \
into the tree your coding harness already reads (.claude/, .opencode/, .agents/, …).

Start here:
  shipmates targets              # harness names this binary supports
  shipmates install              # first-time install (interactive in a terminal)
  shipmates update               # after upgrading the shipmates binary
  shipmates doctor               # check an install; add --fix to repair
  shipmates uninstall            # remove a receipt-owned install

Contributor commands (payload digests / CI) are listed separately below.",
    after_help = "See also: shipmates help <COMMAND>   (full flags + examples)\n\
Website: https://saman-mb.github.io/shipmates/docs/install/"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// First-time install: drop the crew (+ tools by default) into a harness
    #[command(
        long_about = "Install the Shipmates crew and commands into one or more harness trees.

Omit --harness in a terminal to pick harness(es) interactively; non-interactive
runs default to claude-code. Omit --with-tools to install every bundled tool;
pass none for crew + commands only, or name a subset (former short names like
scrub still select shipmates-scrub).

Where defaults to the global home directory (~). Use --local for . or --dir PATH.

Examples:
  shipmates install
  shipmates install --harness claude-code
  shipmates install --harness opencode --local --with-tools none
  shipmates install --harness cursor --dir ~/proj --with-tools termgif,scrub
  shipmates install --harness all --force
  shipmates install --from-cwd",
        after_help = "Tip: after upgrading the shipmates binary, prefer `shipmates update` over a blind reinstall."
    )]
    Install {
        /// Harness name, or `all`. Omit in a terminal to pick interactively
        #[arg(long, value_name = "NAME", help_heading = "What")]
        harness: Option<String>,

        #[command(flatten)]
        location: LocationOpts,

        /// Tools: omit = all; `none` = crew only; or comma-separated names / `all`
        #[arg(
            long = "with-tools",
            value_name = "NAMES|all|none",
            value_delimiter = ',',
            help_heading = "What"
        )]
        with_tools: Option<Vec<String>>,

        /// Skip legacy-command and identity-rename sweeps (superseded names stay)
        #[arg(long, help_heading = "Safety")]
        no_migrate: bool,

        /// Overwrite colliding files even when not claimed by a Shipmates receipt
        #[arg(long, help_heading = "Safety")]
        force: bool,

        /// Build from this directory's crew/commands/toolbox instead of the embedded payload
        #[arg(long = "from-cwd", help_heading = "Source")]
        from_cwd: bool,
    },

    /// Refresh an existing install from this binary (keeps tools unless overridden)
    #[command(
        long_about = "Refresh files from the payload embedded in this shipmates binary.

Requires at least one install receipt. Omit --harness to refresh every installed
harness (or pick interactively when several exist). Omit --with-tools to keep the
tools each receipt already claims.

Where defaults to the global home directory (~). Use --local for . or --dir PATH.

Examples:
  shipmates update
  shipmates update --harness claude-code
  shipmates update --harness opencode --local
  shipmates update --with-tools all
  shipmates update --with-tools none
  shipmates update --from-cwd

Note: payload digest regeneration for contributors is `shipmates build --update`,
not this command.",
        after_help = "Tip: run `shipmates doctor` first if you only want a health report without writing."
    )]
    Update {
        /// Harness to refresh. Omit to refresh all installed receipts (interactive when several)
        #[arg(long, value_name = "NAME", help_heading = "What")]
        harness: Option<String>,

        #[command(flatten)]
        location: LocationOpts,

        /// Replace tools: omit = keep receipt tools; `all` / `none` / names to change
        #[arg(
            long = "with-tools",
            value_name = "NAMES|all|none",
            value_delimiter = ',',
            help_heading = "What"
        )]
        with_tools: Option<Vec<String>>,

        /// Skip legacy-command and identity-rename sweeps
        #[arg(long, help_heading = "Safety")]
        no_migrate: bool,

        /// Refresh from this directory's source trees instead of the embedded payload
        #[arg(long = "from-cwd", help_heading = "Source")]
        from_cwd: bool,
    },

    /// Remove files claimed by a valid install receipt
    #[command(
        long_about = "Uninstall only what a Shipmates receipt says this install owns.

Unmanaged files (your settings, logs, other tools) are left untouched with a warning.
Omit --harness only when exactly one valid receipt exists under the target root.

Where defaults to the global home directory (~). Use --local for . or --dir PATH.

Examples:
  shipmates uninstall
  shipmates uninstall --harness claude-code
  shipmates uninstall --harness opencode --local
  shipmates uninstall --dir /path/to/project --harness cursor
  shipmates uninstall --from-cwd"
    )]
    Uninstall {
        /// Harness to remove. Required when more than one receipt is present
        #[arg(long, value_name = "NAME", help_heading = "What")]
        harness: Option<String>,

        #[command(flatten)]
        location: LocationOpts,

        /// Recognize the payload from this directory's source trees
        #[arg(long = "from-cwd", help_heading = "Source")]
        from_cwd: bool,
    },

    /// Report install health; add `--fix` to repair drift
    #[command(
        long_about = "Diagnose a harness install against the payload in this binary.

Read-only by default. Pass --fix to restore missing or drifted Shipmates-owned
files (backs up replaced content). --no-migrate only applies with --fix.

Where defaults to the global home directory (~). Use --local for . or --dir PATH.
--harness defaults to claude-code.

Examples:
  shipmates doctor
  shipmates doctor --harness opencode --local
  shipmates doctor --fix
  shipmates doctor --fix --no-migrate
  shipmates doctor --dir /path/to/project --harness cursor --fix
  shipmates doctor --from-cwd"
    )]
    Doctor {
        /// Harness to diagnose
        #[arg(
            long,
            value_name = "NAME",
            default_value = "claude-code",
            help_heading = "What"
        )]
        harness: String,

        #[command(flatten)]
        location: LocationOpts,

        /// Repair missing or drifted Shipmates-owned files
        #[arg(long, help_heading = "Repair")]
        fix: bool,

        /// With `--fix`, restore files but leave superseded legacy / pre-prefix names in place
        #[arg(long, requires = "fix", help_heading = "Repair")]
        no_migrate: bool,

        /// Diagnose against this directory's source trees instead of the embedded payload
        #[arg(long = "from-cwd", help_heading = "Source")]
        from_cwd: bool,
    },

    /// List harness names this binary can install (`--harness` / `--target` values)
    #[command(
        long_about = "Print every harness target this shipmates binary knows how to install.

Use these names with install/update/uninstall/doctor `--harness` and with
contributor `build`/`check` `--target`.

Example:
  shipmates targets
  shipmates install --harness $(shipmates targets | head -1)"
    )]
    Targets,

    /// Contributor: emit a harness payload (or refresh digests with `--update`)
    #[command(
        next_help_heading = "Contributor commands",
        long_about = "Contributor tooling — build a harness payload tree from canonical sources.

Everyday users want `install` / `update`, not this command.

Modes:
  (default)   Write rendered files under --out (default: harnesses/<target>/)
  --check     Verify digests without writing digests
  --update    Regenerate tests/payload-digests/<target>.sha256

Examples:
  shipmates build --target claude-code
  shipmates build --target opencode --out /tmp/payload
  shipmates build --target claude-code --update
  shipmates build --target claude-code --check"
    )]
    Build {
        /// Harness target to build
        #[arg(long, value_name = "NAME", default_value = "claude-code")]
        target: String,

        /// Repo root containing crew/ and commands/
        #[arg(long, value_name = "PATH", default_value = ".")]
        root: String,

        /// Output directory for a normal build (ignored by --check / --update)
        #[arg(long, value_name = "PATH")]
        out: Option<String>,

        /// Verify the built payload against committed digests (no digest write)
        #[arg(long)]
        check: bool,

        /// Regenerate reference payload digests under tests/payload-digests/
        #[arg(long)]
        update: bool,
    },

    /// Contributor: assert a built payload matches committed digests
    #[command(
        long_about = "Contributor tooling — fail if the freshly built payload does not match
tests/payload-digests/<target>.sha256.

Everyday users want `doctor`, not this command.

Examples:
  shipmates check --target claude-code
  shipmates check --target opencode --root ."
    )]
    Check {
        /// Harness target to check
        #[arg(long, value_name = "NAME", default_value = "claude-code")]
        target: String,

        /// Repo root containing crew/, commands/, and tests/payload-digests/
        #[arg(long, value_name = "PATH", default_value = ".")]
        root: String,
    },
}
