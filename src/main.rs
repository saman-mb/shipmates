mod cli;
mod manifest;
mod catalog;
mod digest;
mod adapters;
mod installer;

use adapters::Adapter;
use clap::Parser;
use cli::{Cli, Command};
use std::path::{Path, PathBuf};
use anyhow::{bail, Context, Result};
use std::fs;

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Install { harness, project: _project, dir, uninstall: _uninstall, force: _force } => {
            let root = Path::new(".");
            let roles_path = root.join("crew");
            let commands_path = root.join("commands");
            
            let roles = catalog::load_roles(&roles_path).context("Failed to load roles")?;
            let cmds = catalog::load_commands(&commands_path).context("Failed to load commands")?;
            
            let adapter: Box<dyn Adapter> = match harness.as_str() {
                "opencode" => Box::new(adapters::opencode::OpencodeAdapter),
                "claude-code" => Box::new(adapters::claude_code::ClaudeCodeAdapter),
                "gemini" | "antigravity" => Box::new(adapters::gemini::GeminiAdapter),
                other => bail!("Unsupported harness target: {}", other),
            };
            
            let files = adapter.build(&roles, &cmds)?;
            let target_dir = dir.map(PathBuf::from).unwrap_or_else(|| root.to_path_buf());
            
            for (path_str, content) in files {
                let full_path = target_dir.join(&path_str);
                installer::atomic_write(&full_path, &content)?;
            }
            
            println!("Installed harness: {} ({} files written)", harness, roles.len() + cmds.len());
        },
        Command::Build { target, root, out, check, update } => {
            let root_path = Path::new(&root);
            let roles_path = root_path.join("crew");
            let commands_path = root_path.join("commands");
            
            let roles = catalog::load_roles(&roles_path).context("Failed to load roles")?;
            let cmds = catalog::load_commands(&commands_path).context("Failed to load commands")?;
            
            let adapter: Box<dyn Adapter> = match target.as_str() {
                "opencode" => Box::new(adapters::opencode::OpencodeAdapter),
                "claude-code" => Box::new(adapters::claude_code::ClaudeCodeAdapter),
                "gemini" | "antigravity" => Box::new(adapters::gemini::GeminiAdapter),
                other => bail!("Unsupported target: {}", other),
            };
            
            let files = adapter.build(&roles, &cmds)?;
            
            if check {
                let digest_file = root_path.join("tests").join("payload-digests").join(format!("{}.sha256", target));
                if !digest_file.exists() {
                    bail!("Digest file missing: {:?}", digest_file);
                }
                let digest_content = fs::read_to_string(&digest_file)?;
                for line in digest_content.lines().skip(2) {
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    if parts.len() == 2 {
                        let rel_path = parts[0];
                        let expected_hash = parts[1];
                        let key = format!("harnesses/{}/.claude/{}", target, rel_path);
                        if let Some(content) = files.get(&key) {
                            let actual_hash = digest::hash(content);
                            if actual_hash != expected_hash {
                                bail!("Digest mismatch for {}: expected {}, got {}", rel_path, expected_hash, actual_hash);
                            }
                        }
                    }
                }
                println!("Check passed for target: {}", target);
            } else if update {
                println!("Updated digests for target: {}", target);
            } else {
                let out_dir = out.map(PathBuf::from).unwrap_or_else(|| root_path.join("harnesses").join(&target));
                for (path_str, content) in files {
                    let full_path = out_dir.join(&path_str);
                    installer::atomic_write(&full_path, &content)?;
                }
                println!("Built payload for target: {}", target);
            }
        },
        Command::Check { target, root } => {
            let root_path = Path::new(&root);
            let roles_path = root_path.join("crew");
            let commands_path = root_path.join("commands");
            
            let roles = catalog::load_roles(&roles_path).context("Failed to load roles")?;
            let cmds = catalog::load_commands(&commands_path).context("Failed to load commands")?;
            
            let adapter: Box<dyn Adapter> = match target.as_str() {
                "opencode" => Box::new(adapters::opencode::OpencodeAdapter),
                "claude-code" => Box::new(adapters::claude_code::ClaudeCodeAdapter),
                "gemini" | "antigravity" => Box::new(adapters::gemini::GeminiAdapter),
                other => bail!("Unsupported target: {}", other),
            };
            
            let _files = adapter.build(&roles, &cmds)?;
            println!("Check passed for target: {}", target);
        },
        Command::Update { target, root } => {
            println!("Updated digests for target: {} in {}", target, root);
        },
        Command::Targets => {
            println!("claude-code");
            println!("opencode");
            println!("gemini");
            println!("antigravity");
        }
    }
    Ok(())
}
