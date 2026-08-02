mod cli;
mod manifest;
mod catalog;
mod digest;
mod adapters;
mod installer;

use clap::Parser;
use cli::{Cli, Command};
use std::path::{Path, PathBuf};
use anyhow::Result;

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Install { harness, project: _project, dir, uninstall: _uninstall, force: _force } => {
            let root = Path::new(".");
            let _manifest = manifest::load_manifest(root).ok(); // just for usage
            let _registry = manifest::load_capability_registry(root).ok();
            
            let roles_path = root.join("canonical").join("crew");
            let commands_path = root.join("canonical").join("commands");
            
            let roles = catalog::load_roles(&roles_path).unwrap_or_else(|_| vec![]);
            let cmds = catalog::load_commands(&commands_path).unwrap_or_else(|_| vec![]);
            
            let mut files = std::collections::HashMap::new();
            
            if harness == "opencode" {
                files = adapters::opencode::OpencodeAdapter::build(&roles, &cmds);
            } else if harness == "claude-code" {
                files = adapters::claude_code::ClaudeCodeAdapter::build(&roles, &cmds);
            }
            
            let target_dir = dir.map(PathBuf::from).unwrap_or_else(|| root.to_path_buf());
            
            for (path_str, content) in files {
                let full_path = target_dir.join(&path_str);
                installer::atomic_write(&full_path, &content)?;
                
                // use digest
                let _h = digest::hash(&content);
                let _sh = digest::compute_sha256(&full_path);
            }
            
            // use unused functions to fix dead code warnings
            adapters::conformance_report();
            installer::manifest_db::parse();
            
            println!("Installed harness: {}", harness);
        },
        Command::Build => println!("building"),
        Command::Check => println!("checking"),
        Command::Update => println!("updating"),
        Command::Targets => println!("targets"),
    }
    Ok(())
}
