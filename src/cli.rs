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
    Targets,
}
