mod adapters;
mod catalog;
mod cli;
mod digest;
mod doctor;
mod embedded;
mod installer;
mod manifest;
mod steering;

use anyhow::{Context, Result, bail};
use clap::Parser;
use cli::{Cli, Command};
use std::fs;
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};

use catalog::CanonicalTool;

/// Parse one line of the tool picker against the available tools.
///
/// `Some(tools)` for a valid line — empty / `none` → no tools; `all` → every
/// tool; a comma/space-separated list of 1-based numbers → those tools, kept in
/// input order and de-duplicated. `None` means a token was not a number in
/// range, so the caller should re-prompt.
fn select_tools_from_line(line: &str, available: &[CanonicalTool]) -> Option<Vec<CanonicalTool>> {
    let trimmed = line.trim();
    let lower = trimmed.to_ascii_lowercase();
    if trimmed.is_empty() || lower == "none" || lower == "n" {
        return Some(Vec::new());
    }
    if lower == "all" || lower == "a" {
        return Some(available.to_vec());
    }
    let mut picked: Vec<CanonicalTool> = Vec::new();
    for token in trimmed
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|s| !s.is_empty())
    {
        match token.parse::<usize>() {
            Ok(n) if n >= 1 && n <= available.len() => {
                let tool = &available[n - 1];
                if !picked.iter().any(|p| p.name == tool.name) {
                    picked.push(tool.clone());
                }
            }
            _ => return None,
        }
    }
    Some(picked)
}

/// Interactively pick which optional tools to install (terminal only).
///
/// Reached only when `--with-tools` was omitted and stdin is a TTY. Re-prompts a
/// few times on an out-of-range entry, then defaults to none rather than looping
/// forever; a closed stdin (EOF) reads as an empty line, i.e. no tools.
fn prompt_for_tools(available: &[CanonicalTool]) -> Vec<CanonicalTool> {
    println!("\nOptional tools — the crew reach for these implicitly when a task needs one.");
    println!("They're off by default; pick any you'd like installed:\n");
    for (i, tool) in available.iter().enumerate() {
        let blurb: String = tool
            .description
            .split(['.', '\n'])
            .next()
            .unwrap_or("")
            .trim()
            .chars()
            .take(72)
            .collect();
        println!("  {}) {} — {}", i + 1, tool.name, blurb);
    }
    for _ in 0..3 {
        print!("\nSelect tools [e.g. 1,2 · all · Enter for none]: ");
        let _ = std::io::stdout().flush();
        let mut line = String::new();
        if std::io::stdin().read_line(&mut line).is_err() {
            return Vec::new();
        }
        match select_tools_from_line(&line, available) {
            Some(tools) => return tools,
            None => println!(
                "  Pick numbers from 1 to {} (or 'all', or Enter for none).",
                available.len()
            ),
        }
    }
    println!("  No valid selection — installing no tools.");
    Vec::new()
}

/// Pre-warm the runtime dependencies of the installed tool scripts, at install
/// time, so an installed tool runs without the user pip-installing anything.
///
/// Each script's `--provision` ensures its own deps (e.g. the image tools install
/// Pillow into a private cache). Best-effort by design: no pip, no network, or no
/// Python here never fails the install — the tool self-provisions on first run
/// instead. If Python is missing entirely, that is the user's to fix, and we say so.
fn provision_tool_deps(scripts: &[PathBuf]) {
    let python = ["python3", "python"].into_iter().find(|p| {
        std::process::Command::new(p)
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    });
    let Some(python) = python else {
        println!(
            "Note: the installed tool(s) need Python 3 to run; install it and they self-provision the rest on first use."
        );
        return;
    };
    for script in scripts {
        let name = script
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("tool");
        print!("Preparing {} …", name);
        let _ = std::io::stdout().flush();
        let ok = std::process::Command::new(python)
            .arg(script)
            .arg("--provision")
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        println!(
            "{}",
            if ok {
                " ready"
            } else {
                " will provision on first run"
            }
        );
    }
}

fn resolve_target_dir(local: bool, dir: Option<String>) -> Result<PathBuf> {
    if let Some(dir) = dir {
        Ok(PathBuf::from(dir))
    } else if local {
        Ok(Path::new(".").to_path_buf())
    } else {
        home::home_dir().context("Failed to determine home directory")
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Install {
            harness,
            global: _,
            local,
            dir,
            with_tools,
            no_migrate,
            force,
        } => {
            let root = Path::new(".");
            let roles_path = root.join("crew");
            let commands_path = root.join("commands");
            let tools_path = root.join("toolbox");

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

            let non_interactive_default = with_tools.is_none() && !std::io::stdin().is_terminal();
            let selected_tools = if non_interactive_default {
                Vec::new()
            } else {
                let available = if tools_path.is_dir() {
                    catalog::load_tools(&tools_path).context("Failed to load tools")?
                } else {
                    catalog::load_tools_embedded().context("Failed to load embedded tools")?
                };
                match with_tools {
                    Some(want) => {
                        let want: Vec<String> =
                            want.into_iter().filter(|w| !w.is_empty()).collect();
                        if want.iter().any(|t| t == "none") {
                            Vec::new()
                        } else if want.iter().any(|t| t == "all") {
                            available
                        } else {
                            for w in &want {
                                if !available.iter().any(|t| &t.name == w) {
                                    let names: Vec<&str> =
                                        available.iter().map(|t| t.name.as_str()).collect();
                                    bail!("unknown tool: {} (available: {})", w, names.join(", "));
                                }
                            }
                            available
                                .into_iter()
                                .filter(|t| want.contains(&t.name))
                                .collect()
                        }
                    }
                    None if available.is_empty() => Vec::new(),
                    None => prompt_for_tools(&available),
                }
            };

            let target_dir = resolve_target_dir(local, dir)?;
            let install_steering = catalog::steering_for_target(&target_dir, root)
                .map_err(|e| anyhow::anyhow!(e))?;
            let harnesses: Vec<String> = if harness == "all" {
                adapters::targets().iter().map(|s| s.to_string()).collect()
            } else {
                vec![harness]
            };

            let provision_filenames: std::collections::HashSet<String> = selected_tools
                .iter()
                .filter(|t| !t.requires.is_empty())
                .flat_map(|t| {
                    t.assets
                        .iter()
                        .map(|(rel, _)| rel.rsplit('/').next().unwrap_or(rel).to_string())
                })
                .filter(|f| f.ends_with(".py"))
                .collect();
            let mut provision_scripts: Vec<PathBuf> = Vec::new();

            for harness in &harnesses {
                let adapter = adapters::select(harness)?;
                let built = adapters::build_payload(
                    adapter.as_ref(),
                    &roles,
                    &cmds,
                    install_steering.as_deref(),
                )?;
                let payload_prefix = format!("{}/", adapter.container());
                for key in built.keys() {
                    if let Some(rel) = key.strip_prefix(&payload_prefix) {
                        installer::manifest_db::resolve_target_relative(
                            &target_dir,
                            Path::new(rel),
                        )?;
                    }
                }
                let plan = installer::plan::InstallPlan::from_payload(
                    adapter.as_ref(),
                    harness,
                    built.clone(),
                    adapter.build_tools(&selected_tools),
                )?;
                let migration_candidates = if force {
                    installer::migrate::plan(&target_dir, &built, adapter.container())?
                } else {
                    let (_, previous, _) = installer::plan::read_receipt(&target_dir, harness);
                    if let Some(owned) = previous.as_ref() {
                        installer::migrate::plan(&target_dir, &built, adapter.container())?
                            .into_iter()
                            .filter(|item| {
                                owned.file(&item.legacy_path.to_string_lossy()).is_some()
                            })
                            .collect()
                    } else {
                        Vec::new()
                    }
                };
                let migration_items = if no_migrate {
                    Vec::new()
                } else {
                    migration_candidates.clone()
                };
                for item in &migration_candidates {
                    installer::manifest_db::resolve_target_relative(
                        &target_dir,
                        &item.legacy_path,
                    )?;
                    installer::manifest_db::resolve_target_relative(
                        &target_dir,
                        &item.superseded_by,
                    )?;
                }

                // Migration runs before receipt publication. If backup/removal
                // fails, apply never publishes a receipt that drops legacy
                // ownership. Paths deliberately left in place remain claimed so
                // a later install can retry the migration.
                let mut preserved_paths = std::collections::BTreeSet::new();
                let mut migration_report = None;
                if no_migrate {
                    preserved_paths.extend(
                        migration_candidates
                            .iter()
                            .map(|item| item.legacy_path.to_string_lossy().into_owned()),
                    );
                } else if !migration_items.is_empty() {
                    let backup_root = installer::migrate::new_backup_root(&target_dir);
                    let report =
                        installer::migrate::apply(&target_dir, &migration_items, &backup_root)?;
                    for item in &migration_items {
                        if !report.migrated.contains(&item.legacy_path) {
                            preserved_paths.insert(item.legacy_path.to_string_lossy().into_owned());
                        }
                    }
                    migration_report = Some(report);
                    if let Some(report) = migration_report.as_ref()
                        && !report.migrated.is_empty()
                    {
                        println!(
                            "Migrated {} superseded command(s) → skills (backup: {})",
                            report.migrated.len(),
                            backup_root.display()
                        );
                        for (legacy, backup) in report.migrated.iter().zip(&report.backups) {
                            println!("  moved {} → {}", legacy.display(), backup.display());
                        }
                    }
                }

                let apply_result = if preserved_paths.is_empty() {
                    installer::apply::apply(&target_dir, &plan, force)
                } else {
                    installer::apply::apply_with_preserved_paths(
                        &target_dir,
                        &plan,
                        force,
                        &preserved_paths,
                    )
                };
                let result = match apply_result {
                    Ok(result) => result,
                    Err(error) => {
                        let rollback = match migration_report.as_ref() {
                            Some(report) => installer::migrate::rollback(&target_dir, report),
                            None => Ok(()),
                        };
                        return Err(combine_rollback_error(error, rollback));
                    }
                };
                if let Some(receipt) = &result.receipt {
                    for file in &receipt.files {
                        let rel = PathBuf::from(&file.path);
                        if let Some(fname) = rel.file_name().and_then(|name| name.to_str()) {
                            if provision_filenames.contains(fname)
                                && !provision_scripts
                                    .iter()
                                    .any(|p| p.file_name().and_then(|s| s.to_str()) == Some(fname))
                            {
                                provision_scripts.push(
                                    installer::manifest_db::resolve_target_relative(
                                        &target_dir,
                                        &rel,
                                    )?,
                                );
                            }
                        }
                    }
                }

                if let Some(previous) = &result.previous_version {
                    if previous != &plan.version {
                        println!("Upgrading shipmates v{} → v{}", previous, plan.version);
                        println!(
                            "{} files changed, {} new, {} removed",
                            result.summary.changed, result.summary.new, result.summary.removed
                        );
                    }
                }
                for warning in &result.warnings {
                    println!("{}", warning);
                }

                if selected_tools.is_empty() {
                    println!(
                        "Installed harness: {} ({} files written)",
                        harness, result.written
                    );
                } else {
                    let names: Vec<&str> = selected_tools
                        .iter()
                        .map(|tool| tool.name.as_str())
                        .collect();
                    println!(
                        "Installed harness: {} ({} files written, tools: {})",
                        harness,
                        result.written,
                        names.join(", ")
                    );
                }
            }
            if !provision_scripts.is_empty() {
                provision_tool_deps(&provision_scripts);
            }
            if install_steering.is_some() {
                for action in steering::plan_legacy_migration(&target_dir)? {
                    match action {
                        steering::LegacyMigration::Write { path, content } => {
                            crate::installer::atomic_write(&path, &content)?;
                            println!(
                                "Removed legacy contributor steering section from {}",
                                path.display()
                            );
                        }
                        steering::LegacyMigration::Remove { path } => {
                            if path.is_file() {
                                std::fs::remove_file(&path)?;
                                println!(
                                    "Removed legacy contributor steering file {}",
                                    path.display()
                                );
                            }
                        }
                    }
                }
            }
        }
        Command::Uninstall {
            harness,
            global: _,
            local,
            dir,
        } => {
            let target_dir = resolve_target_dir(local, dir)?;
            let selected = installer::uninstall::select_receipt(&target_dir, harness.as_deref())?;
            let Some(selected) = selected else {
                println!("No install receipt found; nothing to uninstall.");
                return Ok(());
            };
            let root = Path::new(".");
            let roles_path = root.join("crew");
            let commands_path = root.join("commands");
            let tools_path = root.join("toolbox");
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
            let tools = if tools_path.is_dir() {
                catalog::load_tools(&tools_path).context("Failed to load tools")?
            } else {
                catalog::load_tools_embedded().context("Failed to load embedded tools")?
            };
            let known_payload = installer::uninstall::payload_for(
                &selected.receipt.harness,
                &roles,
                &cmds,
                &tools,
            )?;
            let report = installer::uninstall::uninstall_with_payload(
                &target_dir,
                selected,
                &known_payload,
            )?;
            println!(
                "Uninstalled harness: {} ({} files removed)",
                report.harness, report.removed
            );
            for warning in report.warnings {
                println!("{}", warning);
            }
        }
        Command::Build {
            target,
            root,
            out,
            check,
            update,
        } => {
            let root_path = Path::new(&root);
            let roles_path = root_path.join("crew");
            let commands_path = root_path.join("commands");

            let roles = catalog::load_roles(&roles_path).context("Failed to load roles")?;
            let cmds = catalog::load_commands(&commands_path).context("Failed to load commands")?;
            let steering = catalog::load_steering(root_path).map_err(|e| anyhow::anyhow!(e))?;

            let adapter = adapters::select(&target)?;
            let files = adapters::build_payload(
                adapter.as_ref(),
                &roles,
                &cmds,
                Some(&steering),
            )?;

            if check {
                check_digests(&target, adapter.digest_root(), &files, root_path)?;
            } else if update {
                write_digests(&target, adapter.digest_root(), &files, root_path)?;
            } else {
                let out_dir = out
                    .map(PathBuf::from)
                    .unwrap_or_else(|| root_path.join("harnesses").join(&target));
                for (path_str, content) in files {
                    let full_path = out_dir.join(&path_str);
                    installer::atomic_write(&full_path, &content)?;
                }
                println!("Built payload for target: {}", target);
            }
        }
        Command::Check { target, root } => {
            let root_path = Path::new(&root);
            let roles_path = root_path.join("crew");
            let commands_path = root_path.join("commands");

            let roles = catalog::load_roles(&roles_path).context("Failed to load roles")?;
            let cmds = catalog::load_commands(&commands_path).context("Failed to load commands")?;
            let steering = catalog::load_steering(root_path).map_err(|e| anyhow::anyhow!(e))?;

            let adapter = adapters::select(&target)?;
            let files = adapters::build_payload(
                adapter.as_ref(),
                &roles,
                &cmds,
                Some(&steering),
            )?;
            check_digests(&target, adapter.digest_root(), &files, root_path)?;
        }
        Command::Update { target, root } => {
            let root_path = Path::new(&root);
            let roles_path = root_path.join("crew");
            let commands_path = root_path.join("commands");

            let roles = catalog::load_roles(&roles_path).context("Failed to load roles")?;
            let cmds = catalog::load_commands(&commands_path).context("Failed to load commands")?;
            let steering = catalog::load_steering(root_path).map_err(|e| anyhow::anyhow!(e))?;

            let adapter = adapters::select(&target)?;
            let files = adapters::build_payload(
                adapter.as_ref(),
                &roles,
                &cmds,
                Some(&steering),
            )?;
            write_digests(&target, adapter.digest_root(), &files, root_path)?;
        }
        Command::Doctor {
            harness,
            global: _,
            local,
            dir,
            fix,
            no_migrate,
        } => {
            let root = Path::new(".");
            let roles_path = root.join("crew");
            let commands_path = root.join("commands");
            let tools_path = root.join("toolbox");

            // Same on-disk-or-embedded fallback as `install`: the repo dev loop
            // reads `crew/` + `commands/` + `toolbox/`, a brew/cargo binary reads
            // the payload compiled in by `build.rs`.
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
            let tools = if tools_path.is_dir() {
                catalog::load_tools(&tools_path).context("Failed to load tools")?
            } else {
                catalog::load_tools_embedded().context("Failed to load embedded tools")?
            };

            // Honour `--dir`, or default to global home dir unless `--local`
            let target_dir = if let Some(d) = dir {
                PathBuf::from(d)
            } else if local {
                root.to_path_buf()
            } else {
                home::home_dir().context("Failed to determine home directory")?
            };

            let report = if fix {
                doctor::fix(&target_dir, &harness, &roles, &cmds, &tools, no_migrate)?
            } else {
                doctor::diagnose(&target_dir, &harness, &roles, &cmds, &tools)?
            };
            doctor::print_report(&report);
            // Exit 2 on problems via `std::process::exit` — not `bail!`, which
            // would print an error and exit 1 rather than the health-check code.
            if report.has_problems() {
                std::process::exit(2);
            }
        }
        Command::Targets => {
            for name in adapters::targets() {
                println!("{}", name);
            }
        }
    }
    Ok(())
}

/// Verify every entry in a payload digest matches the freshly built payload.
///
/// `digest_root` is the harness's install container (`harnesses/<target>`), so
/// a target that writes into more than one dotdir — Codex, with crew at
/// `.codex/` and skills at `.agents/` — is covered whole rather than only under
/// its `base_dir`.
fn check_digests(
    target: &str,
    digest_root: &str,
    files: &std::collections::HashMap<String, String>,
    root_path: &Path,
) -> Result<()> {
    let digest_file = root_path
        .join("tests")
        .join("payload-digests")
        .join(format!("{}.sha256", target));
    if !digest_file.exists() {
        bail!("Digest file missing: {:?}", digest_file);
    }
    let digest_content = fs::read_to_string(&digest_file)?;
    for line in digest_content.lines().skip(2) {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() == 2 {
            let rel_path = parts[0];
            let expected_hash = parts[1];
            let key = format!("{}/{}", digest_root, rel_path);
            if let Some(content) = files.get(&key) {
                let actual_hash = digest::hash(content);
                if actual_hash != expected_hash {
                    bail!(
                        "Digest mismatch for {}: expected {}, got {}",
                        rel_path,
                        expected_hash,
                        actual_hash
                    );
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
///
/// Keyed on the install container (`harnesses/<target>`) so every dotdir a
/// harness writes is recorded — see `check_digests`.
fn write_digests(
    target: &str,
    digest_root: &str,
    files: &std::collections::HashMap<String, String>,
    root_path: &Path,
) -> Result<()> {
    let prefix = format!("{}/", digest_root);
    let mut entries: Vec<(String, String)> = files
        .iter()
        .filter_map(|(path, content)| {
            path.strip_prefix(&prefix)
                .map(|rel| (rel.to_string(), digest::hash(content)))
        })
        .collect();
    entries.sort();

    let mut out = String::new();
    out.push_str("payload_digest_version=1\n");
    out.push_str(&format!("target={}\n", target));
    for (rel, hash) in entries {
        out.push_str(&format!("{} {}\n", rel, hash));
    }

    let digest_file = root_path
        .join("tests")
        .join("payload-digests")
        .join(format!("{}.sha256", target));
    fs::create_dir_all(digest_file.parent().unwrap())?;
    installer::atomic_write(&digest_file, &out)?;
    println!("Wrote digests for target: {}", target);
    Ok(())
}

fn combine_rollback_error(error: anyhow::Error, rollback: Result<()>) -> anyhow::Error {
    match rollback {
        Ok(()) => error,
        Err(rollback) => error.context(rollback.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tool(name: &str) -> CanonicalTool {
        CanonicalTool {
            name: name.to_string(),
            description: "desc".to_string(),
            body: String::new(),
            assets: vec![],
            requires: vec![],
            source: PathBuf::from(""),
        }
    }

    fn names(v: Option<Vec<CanonicalTool>>) -> Option<Vec<String>> {
        v.map(|ts| ts.into_iter().map(|t| t.name).collect())
    }

    #[test]
    fn test_tool_line_empty_and_none_select_nothing() {
        let avail = [tool("termgif"), tool("second")];
        assert_eq!(names(select_tools_from_line("", &avail)), Some(vec![]));
        assert_eq!(names(select_tools_from_line("   ", &avail)), Some(vec![]));
        assert_eq!(names(select_tools_from_line("none", &avail)), Some(vec![]));
        assert_eq!(names(select_tools_from_line("N", &avail)), Some(vec![]));
    }

    #[test]
    fn test_tool_line_all_selects_everything() {
        let avail = [tool("termgif"), tool("second")];
        assert_eq!(
            names(select_tools_from_line("all", &avail)),
            Some(vec!["termgif".into(), "second".into()])
        );
        assert_eq!(
            names(select_tools_from_line("A", &avail)),
            Some(vec!["termgif".into(), "second".into()])
        );
    }

    #[test]
    fn test_tool_line_numbers_pick_in_order_and_dedup() {
        let avail = [tool("termgif"), tool("second"), tool("third")];
        assert_eq!(
            names(select_tools_from_line("1", &avail)),
            Some(vec!["termgif".into()])
        );
        assert_eq!(
            names(select_tools_from_line("3, 1", &avail)),
            Some(vec!["third".into(), "termgif".into()])
        );
        // whitespace-separated and duplicates collapse
        assert_eq!(
            names(select_tools_from_line("2 2 2", &avail)),
            Some(vec!["second".into()])
        );
    }

    #[test]
    fn test_tool_line_out_of_range_or_garbage_is_reprompt() {
        let avail = [tool("termgif")];
        assert_eq!(select_tools_from_line("2", &avail).map(|_| ()), None);
        assert_eq!(select_tools_from_line("0", &avail).map(|_| ()), None);
        assert_eq!(select_tools_from_line("nope", &avail).map(|_| ()), None);
        // one bad token invalidates the whole line
        assert_eq!(select_tools_from_line("1, 9", &avail).map(|_| ()), None);
    }
}
