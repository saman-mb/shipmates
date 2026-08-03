mod cli;
mod manifest;
mod catalog;
mod digest;
mod embedded;
mod adapters;
mod installer;

use adapters::Adapter;
use clap::Parser;
use cli::{Cli, Command};
use std::path::{Path, PathBuf};
use anyhow::{bail, Context, Result};
use std::fs;

fn select(target: &str) -> Result<Box<dyn Adapter>> {
    let adapter: Box<dyn Adapter> = match target {
        "opencode" => Box::new(adapters::opencode::OpencodeAdapter),
        "claude-code" => Box::new(adapters::claude_code::ClaudeCodeAdapter),
        "antigravity" => Box::new(adapters::antigravity::AntigravityAdapter),
        "codex" => Box::new(adapters::codex::CodexAdapter),
        "cursor" => Box::new(adapters::cursor::CursorAdapter),
        "github-copilot" => Box::new(adapters::github_copilot::GithubCopilotAdapter),
        "windsurf" => Box::new(adapters::windsurf::WindsurfAdapter),
        "zed" => Box::new(adapters::zed::ZedAdapter),
        other => bail!("Unsupported target: {}", other),
    };
    Ok(adapter)
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Install { harness, dir, with_tools } => {
            let root = Path::new(".");
            let roles_path = root.join("crew");
            let commands_path = root.join("commands");
            let tools_path = root.join("toolbox");

            // A `brew`/`cargo`-installed binary has no checkout, so the
            // canonical sources are compiled in by `build.rs`. Fall back to
            // the on-disk `crew/` + `commands/` when present (the repo dev
            // loop), else the embedded payload.
            let roles = if roles_path.is_dir() {
                catalog::load_roles(&roles_path).context("Failed to load roles")?
            } else {
                catalog::load_roles_embedded().context("Failed to load embedded roles")?
            };
            let cmds = if commands_path.is_dir() {
                catalog::load_commands(&commands_path).context("Failed to load commands")?
            } else {
                catalog::load_commands_embedded().context("Failed to load embedded commands")?
            };

            // Tools are opt-in: nothing is loaded or installed unless
            // `--with-tools` names them (or `all`). This keeps a plain install
            // to crew + commands, as it has always been.
            let selected_tools = if with_tools.is_empty() {
                Vec::new()
            } else {
                let all = if tools_path.is_dir() {
                    catalog::load_tools(&tools_path).context("Failed to load tools")?
                } else {
                    catalog::load_tools_embedded().context("Failed to load embedded tools")?
                };
                if with_tools.iter().any(|t| t == "all") {
                    all
                } else {
                    for want in &with_tools {
                        if !all.iter().any(|t| &t.name == want) {
                            let available: Vec<&str> = all.iter().map(|t| t.name.as_str()).collect();
                            bail!("unknown tool: {} (available: {})", want, available.join(", "));
                        }
                    }
                    all.into_iter().filter(|t| with_tools.contains(&t.name)).collect()
                }
            };

            let target_dir = dir.map(PathBuf::from).unwrap_or_else(|| root.to_path_buf());

            // `--harness all` fans out to every supported target; a single name
            // installs just that one.
            let harnesses: Vec<String> = if harness == "all" {
                adapters::targets().iter().map(|s| s.to_string()).collect()
            } else {
                vec![harness]
            };

            for harness in &harnesses {
                let adapter = select(harness)?;
                let files = adapter.build(&roles, &cmds)?;
                let tool_files = adapter.build_tools(&selected_tools);
                // `harnesses/<target>/` — derived from the adapter's base dir so
                // the harness's own container (`harnesses/antigravity/.agents`)
                // is stripped, leaving `.agents/` at the target root.
                let base = adapter.base_dir();
                let container = base.rsplit_once('/').map(|(c, _)| c).unwrap_or(base);
                let strip = format!("{}/", container);

                // The real count of files this harness writes — 24 for the
                // crew-bearing targets (12 crew + 12 commands), 12 for the
                // skill-only ones, plus any opt-in tool files. Never the fixed
                // roles+commands total, which over-counted every skill-only
                // install.
                let mut written = 0usize;
                for (path_str, content) in files.into_iter().chain(tool_files) {
                    // Drop the `harnesses/<target>/` container so the harness's
                    // own tree (`.claude/`, `.opencode/`, …) lands at the target
                    // root, where the harness actually reads it.
                    let rel = path_str.strip_prefix(&strip).unwrap_or(&path_str);
                    let full_path = target_dir.join(rel);
                    installer::atomic_write(&full_path, &content)?;
                    written += 1;
                }

                if selected_tools.is_empty() {
                    println!("Installed harness: {} ({} files written)", harness, written);
                } else {
                    let names: Vec<&str> = selected_tools.iter().map(|t| t.name.as_str()).collect();
                    println!(
                        "Installed harness: {} ({} files written, tools: {})",
                        harness, written, names.join(", ")
                    );
                }
            }
        },
        Command::Build { target, root, out, check, update } => {
            let root_path = Path::new(&root);
            let roles_path = root_path.join("crew");
            let commands_path = root_path.join("commands");

            let roles = catalog::load_roles(&roles_path).context("Failed to load roles")?;
            let cmds = catalog::load_commands(&commands_path).context("Failed to load commands")?;

            let adapter = select(&target)?;
            let files = adapter.build(&roles, &cmds)?;

            if check {
                check_digests(&target, adapter.base_dir(), &files, root_path)?;
            } else if update {
                write_digests(&target, adapter.base_dir(), &files, root_path)?;
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

            let adapter = select(&target)?;
            let files = adapter.build(&roles, &cmds)?;
            check_digests(&target, adapter.base_dir(), &files, root_path)?;
        },
        Command::Update { target, root } => {
            let root_path = Path::new(&root);
            let roles_path = root_path.join("crew");
            let commands_path = root_path.join("commands");

            let roles = catalog::load_roles(&roles_path).context("Failed to load roles")?;
            let cmds = catalog::load_commands(&commands_path).context("Failed to load commands")?;

            let adapter = select(&target)?;
            let files = adapter.build(&roles, &cmds)?;
            write_digests(&target, adapter.base_dir(), &files, root_path)?;
        },
        Command::Targets => {
            for name in adapters::targets() {
                println!("{}", name);
            }
        }
    }
    Ok(())
}

/// Verify every entry in a payload digest matches the freshly built payload.
fn check_digests(target: &str, base_dir: &str, files: &std::collections::HashMap<String, String>, root_path: &Path) -> Result<()> {
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
            let key = format!("{}/{}", base_dir, rel_path);
            if let Some(content) = files.get(&key) {
                let actual_hash = digest::hash(content);
                if actual_hash != expected_hash {
                    bail!("Digest mismatch for {}: expected {}, got {}", rel_path, expected_hash, actual_hash);
                }
            } else {
                bail!("Payload is missing a digest entry: {}", rel_path);
            }
        }
    }
    println!("Check passed for target: {}", target);
    Ok(())
}

/// Write a fresh payload digest for a target.
fn write_digests(target: &str, base_dir: &str, files: &std::collections::HashMap<String, String>, root_path: &Path) -> Result<()> {
    let prefix = format!("{}/", base_dir);
    let mut entries: Vec<(String, String)> = files
        .iter()
        .filter_map(|(path, content)| {
            path.strip_prefix(&prefix).map(|rel| (rel.to_string(), digest::hash(content)))
        })
        .collect();
    entries.sort();

    let mut out = String::new();
    out.push_str("payload_digest_version=1\n");
    out.push_str(&format!("target={}\n", target));
    for (rel, hash) in entries {
        out.push_str(&format!("{} {}\n", rel, hash));
    }

    let digest_file = root_path.join("tests").join("payload-digests").join(format!("{}.sha256", target));
    fs::create_dir_all(digest_file.parent().unwrap())?;
    installer::atomic_write(&digest_file, &out)?;
    println!("Wrote digests for target: {}", target);
    Ok(())
}
