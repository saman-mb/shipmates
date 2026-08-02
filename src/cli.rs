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
        project: Option<String>,
        
        #[arg(long)]
        dir: Option<String>,
        
        #[arg(long, default_value_t = false)]
        uninstall: bool,
        
        #[arg(long, default_value_t = false)]
        force: bool,
    },
    Build,
    Check,
    Update,
    Targets,
}
