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

impl LocationOpts {
    /// Reconstruct the Where flag a captain typed for this location.
    /// Paths with shell-sensitive characters are single-quoted so a captain
    /// can copy-paste the force hint safely (#392 board nit).
    pub fn force_where_flag(&self) -> String {
        if let Some(dir) = &self.dir {
            format!("--dir {}", shell_single_quote(dir))
        } else if self.local {
            "--local".to_string()
        } else {
            "--global".to_string()
        }
    }
}

/// Single-quote a value for safe paste into a shell command line.
fn shell_single_quote(value: &str) -> String {
    if value.is_empty() {
        return "''".to_string();
    }
    if value
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "/._-".contains(c))
    {
        return value.to_string();
    }
    format!("'{}'", value.replace('\'', "'\\''"))
}

/// Build the exact `shipmates install … --force` invocation a refusal or doctor
/// foreign-collision message should recommend (#392). Never a bare
/// `shipmates install --force` — that drops the harness, root, and tools the
/// captain already chose.
///
/// `with_tools` is the raw CLI value (`none`, `all`, or a comma-joined list).
/// Pass `None` when the flag was omitted (install default).
pub fn install_force_hint(harness: &str, location: &LocationOpts, with_tools: Option<&str>) -> String {
    let mut parts = vec![
        "shipmates install".to_string(),
        format!("--harness {harness}"),
        location.force_where_flag(),
    ];
    if let Some(tools) = with_tools.filter(|t| !t.is_empty()) {
        parts.push(format!("--with-tools {}", shell_single_quote(tools)));
    }
    parts.push("--force".to_string());
    parts.join(" ")
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
  shipmates install              # first-time install (prompts / defaults to claude-code)
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
    /// First-time install: drop the crew, tools, and (on a global target) steering
    #[command(
        long_about = "Install the Shipmates crew, commands, and tools into one or more harness trees.

When --harness is omitted, shipmates reports any detected harnesses as a hint, then:
  - in a terminal, prompts interactively (Enter defaults to claude-code);
  - non-interactively, installs claude-code only.
Detection never auto-installs every matched harness — pass --harness NAME or --harness all
to opt in. Markers are specific (e.g. .github/agents, not bare .github/).

Omit --with-tools to install every bundled tool; pass none for crew + commands only, or name a
subset (former short names like scrub still select shipmates-scrub).

Where defaults to the global home directory (~). Use --local for . or --dir PATH. Canonical
global steering (heuristics and workflow routing) is installed into user instruction files only
when the install target is the home directory (default / --global), not on --local or --dir.

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
receipt without a prompt when stdin is not a terminal (the piped/agent path), or
pick interactively when several exist in a terminal; `--harness all` refreshes
every receipt without the prompt either way. Omit --with-tools to keep the tools
each receipt already claims, in every install form (skill directories and
opencode's native .opencode/tools/*.ts). Pass `--with-tools none` to remove the
tools or name a subset to replace them.

Where defaults to the global home directory (~). Use --local for . or --dir PATH.

Examples:
  shipmates update
  shipmates update --harness claude-code
  shipmates update --harness all
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

    /// Report every known install (harness, root, version, drift, tools)
    #[command(
        long_about = "Report every known install across the global install index and any --dir roots.

Read-only: never writes to disk or changes an install. Lists each install's harness, root,
receipt version, drift, and claimed tools. --json emits the machine-readable report.

Examples:
  shipmates status
  shipmates status --json
  shipmates status --dir ~/work/app"
    )]
    Status {
        /// Emit the report as JSON
        #[arg(long, help_heading = "Output")]
        json: bool,

        /// Extra root directory to scan (repeatable)
        #[arg(
            long,
            value_name = "PATH",
            action = clap::ArgAction::Append,
            help_heading = "Where"
        )]
        dir: Vec<String>,
    },

    /// Check for a newer release and refresh every known install from this binary
    #[command(
        long_about = "Check for a newer shipmates release, refresh every known install from this
binary, audit each, and optionally repair drift (--fix) or file upstream bugs
(--file-bugs).

--self executes the detected channel's upgrade command (brew / cargo-dist) with
everything else printed only. --pre includes prereleases in the release check.

Examples:
  shipmates upgrade --check
  shipmates upgrade --json
  shipmates upgrade --fix
  shipmates upgrade --self
  shipmates upgrade --dir ~/work/app",
        after_help = "Tip: --dry-run prints what would change without executing; --check is a
read-only three-way version report."
    )]
    Upgrade {
        /// Check for a newer release only (read-only); conflicts with --fix, --file-bugs, --self
        #[arg(
            long,
            conflicts_with_all = ["fix", "file_bugs", "self_upgrade"],
            help_heading = "What"
        )]
        check: bool,

        /// Emit the report as JSON
        #[arg(long, help_heading = "Output")]
        json: bool,

        /// Include prereleases in the release check
        #[arg(long, help_heading = "What")]
        pre: bool,

        /// Print what would change without executing (allowed with --self)
        #[arg(long, help_heading = "Safety")]
        dry_run: bool,

        /// Repair drift in each install after refresh
        #[arg(long, help_heading = "Repair")]
        fix: bool,

        /// Extra root directory to refresh (repeatable)
        #[arg(
            long,
            value_name = "PATH",
            action = clap::ArgAction::Append,
            help_heading = "Where"
        )]
        dir: Vec<String>,

        /// File deduped upstream bugs for findings (requires gh)
        #[arg(long, help_heading = "Safety")]
        file_bugs: bool,

        /// Execute the detected channel's upgrade command (brew / cargo-dist)
        #[arg(long = "self", help_heading = "What")]
        self_upgrade: bool,

        /// Internal re-exec after a successful self-upgrade
        #[arg(long, hide = true)]
        resume: bool,
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
