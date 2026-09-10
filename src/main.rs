mod adapters;
mod catalog;
mod cli;
mod digest;
mod doctor;
mod embedded;
mod installer;
mod manifest;

use anyhow::{Context, Result, bail};
use clap::Parser;
use cli::{Cli, Command};
use installer::manifest_db::InstallReceipt;
use std::fs;
use std::io::{IsTerminal, Write};
use std::path::{Component, Path, PathBuf};

use catalog::{CanonicalCommand, CanonicalRole, CanonicalTool};

/// How optional tools are chosen for an install/update run.
enum ToolSelection {
    /// Same set for every harness in the run.
    Explicit(Vec<CanonicalTool>),
    /// Keep whatever each harness receipt already claims (update default).
    FromReceipt,
}

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

fn harness_blurb(name: &str) -> &'static str {
    match name {
        "claude-code" => "agents + skills in .claude (runtime-verified)",
        "opencode" => "agents + commands in .opencode",
        "antigravity" => "agents + skills in .agents (agy)",
        "codex" => "TOML crew in .codex + skills in .agents",
        "cursor" => "skills in .agents (shared Agent Skills tree)",
        "github-copilot" => "crew in .github/agents + skills in .agents",
        "windsurf" => "skills in .windsurf",
        _ => "harness payload",
    }
}

/// Parse one line of the harness picker. Empty → default `claude-code`.
fn select_harnesses_from_line(line: &str, available: &[&str]) -> Option<Vec<String>> {
    let trimmed = line.trim();
    let lower = trimmed.to_ascii_lowercase();
    if trimmed.is_empty() {
        return Some(vec!["claude-code".into()]);
    }
    if lower == "all" || lower == "a" {
        return Some(available.iter().map(|name| (*name).to_string()).collect());
    }
    let mut picked: Vec<String> = Vec::new();
    for token in trimmed
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|s| !s.is_empty())
    {
        if let Ok(n) = token.parse::<usize>() {
            if n >= 1 && n <= available.len() {
                let name = available[n - 1].to_string();
                if !picked.iter().any(|p| p == &name) {
                    picked.push(name);
                }
                continue;
            }
            return None;
        }
        if let Some(name) = available
            .iter()
            .find(|candidate| candidate.eq_ignore_ascii_case(token))
        {
            let name = (*name).to_string();
            if !picked.iter().any(|p| p == &name) {
                picked.push(name);
            }
            continue;
        }
        return None;
    }
    if picked.is_empty() {
        None
    } else {
        Some(picked)
    }
}

fn prompt_for_harnesses(available: &[&str]) -> Vec<String> {
    println!("\nWhich harness(es) should Shipmates install?\n");
    for (i, name) in available.iter().enumerate() {
        println!("  {}) {} — {}", i + 1, name, harness_blurb(name));
    }
    for _ in 0..3 {
        print!("\nSelect harnesses [e.g. 1 · 1,5 · all · Enter for claude-code]: ");
        let _ = std::io::stdout().flush();
        let mut line = String::new();
        if std::io::stdin().read_line(&mut line).is_err() {
            return vec!["claude-code".into()];
        }
        match select_harnesses_from_line(&line, available) {
            Some(harnesses) => return harnesses,
            None => println!(
                "  Pick numbers from 1 to {}, a harness name, 'all', or Enter for claude-code.",
                available.len()
            ),
        }
    }
    println!("  No valid selection — installing claude-code.");
    vec!["claude-code".into()]
}

fn prompt_among(labels: &[String], heading: &str, empty_default: &[String]) -> Vec<String> {
    let available: Vec<&str> = labels.iter().map(String::as_str).collect();
    println!("\n{heading}\n");
    for (i, name) in available.iter().enumerate() {
        println!("  {}) {} — {}", i + 1, name, harness_blurb(name));
    }
    for _ in 0..3 {
        print!("\nSelect [e.g. 1 · all · Enter for all installed]: ");
        let _ = std::io::stdout().flush();
        let mut line = String::new();
        if std::io::stdin().read_line(&mut line).is_err() {
            return empty_default.to_vec();
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return empty_default.to_vec();
        }
        match select_harnesses_from_line(&line, &available) {
            Some(harnesses) => return harnesses,
            None => println!(
                "  Pick numbers from 1 to {}, a harness name, or 'all'.",
                available.len()
            ),
        }
    }
    println!("  No valid selection — updating all installed harnesses.");
    empty_default.to_vec()
}

fn resolve_install_harnesses(harness: Option<String>) -> Result<Vec<String>> {
    let available = adapters::targets();
    match harness {
        Some(name) if name == "all" => Ok(available.iter().map(|s| (*s).to_string()).collect()),
        Some(name) => {
            if !available.iter().any(|candidate| *candidate == name) {
                bail!(
                    "Unsupported harness: {name} (available: {})",
                    available.join(", ")
                );
            }
            Ok(vec![name])
        }
        None if std::io::stdin().is_terminal() => Ok(prompt_for_harnesses(&available)),
        None => Ok(vec!["claude-code".into()]),
    }
}

fn resolve_named_tools(
    with_tools: Option<Vec<String>>,
    available: &[CanonicalTool],
) -> Result<Vec<CanonicalTool>> {
    match with_tools {
        None if !std::io::stdin().is_terminal() => Ok(Vec::new()),
        None if available.is_empty() => Ok(Vec::new()),
        None => Ok(prompt_for_tools(available)),
        Some(want) => {
            let want: Vec<String> = want.into_iter().filter(|w| !w.is_empty()).collect();
            if want.iter().any(|t| t == "none") {
                Ok(Vec::new())
            } else if want.iter().any(|t| t == "all") {
                Ok(available.to_vec())
            } else {
                for w in &want {
                    if !available.iter().any(|t| &t.name == w) {
                        let names: Vec<&str> = available.iter().map(|t| t.name.as_str()).collect();
                        bail!("unknown tool: {} (available: {})", w, names.join(", "));
                    }
                }
                Ok(available
                    .iter()
                    .filter(|t| want.contains(&t.name))
                    .cloned()
                    .collect())
            }
        }
    }
}

/// Tools a receipt already claims, matched by path component (skill/tool dir name).
fn tools_from_receipt(receipt: &InstallReceipt, available: &[CanonicalTool]) -> Vec<CanonicalTool> {
    available
        .iter()
        .filter(|tool| {
            receipt.files.iter().any(|file| {
                Path::new(&file.path).components().any(|component| {
                    matches!(component, Component::Normal(value) if value == tool.name.as_str())
                })
            })
        })
        .cloned()
        .collect()
}

fn resolve_update_harnesses(target_dir: &Path, harness: Option<String>) -> Result<Vec<String>> {
    let receipts = installer::manifest_db::ReceiptRepository::new(target_dir).load_all()?;
    if receipts.is_empty() {
        bail!(
            "No install receipt found under {}. Run `shipmates install` first.",
            target_dir.display()
        );
    }
    let installed: Vec<String> = receipts.into_iter().map(|r| r.harness).collect();
    match harness {
        Some(name) if name == "all" => Ok(installed),
        Some(name) => {
            if !installed.iter().any(|candidate| candidate == &name) {
                bail!(
                    "No install receipt for harness `{name}` under {}. Installed: {}. Run `shipmates install --harness {name}` first.",
                    target_dir.display(),
                    installed.join(", ")
                );
            }
            Ok(vec![name])
        }
        None if installed.len() == 1 => Ok(installed),
        None if std::io::stdin().is_terminal() => Ok(prompt_among(
            &installed,
            "Which installed harness(es) to update?",
            &installed,
        )),
        None => Ok(installed),
    }
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

fn load_catalog() -> Result<(
    Vec<CanonicalRole>,
    Vec<CanonicalCommand>,
    Vec<CanonicalTool>,
)> {
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
    Ok((roles, cmds, tools))
}

fn run_install(
    target_dir: &Path,
    harnesses: &[String],
    tools: ToolSelection,
    available_tools: &[CanonicalTool],
    roles: &[CanonicalRole],
    cmds: &[CanonicalCommand],
    no_migrate: bool,
    force: bool,
) -> Result<()> {
    let mut provision_scripts: Vec<PathBuf> = Vec::new();

    for harness in harnesses {
        let selected_tools = match &tools {
            ToolSelection::Explicit(tools) => tools.clone(),
            ToolSelection::FromReceipt => {
                let (_, previous, _) = installer::plan::read_receipt(target_dir, harness);
                previous
                    .as_ref()
                    .map(|receipt| tools_from_receipt(receipt, available_tools))
                    .unwrap_or_default()
            }
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

        let adapter = adapters::select(harness)?;
        let built = adapter.build(roles, cmds)?;
        let payload_prefix = format!("{}/", adapter.container());
        for key in built.keys() {
            if let Some(rel) = key.strip_prefix(&payload_prefix) {
                installer::manifest_db::resolve_target_relative(target_dir, Path::new(rel))?;
            }
        }
        let plan = installer::plan::InstallPlan::from_payload(
            adapter.as_ref(),
            harness,
            built.clone(),
            adapter.build_tools(&selected_tools),
        )?;
        let migration_candidates = if force {
            installer::migrate::plan(target_dir, &built, adapter.container())?
        } else {
            let (_, previous, _) = installer::plan::read_receipt(target_dir, harness);
            if let Some(owned) = previous.as_ref() {
                installer::migrate::plan(target_dir, &built, adapter.container())?
                    .into_iter()
                    .filter(|item| owned.file(&item.legacy_path.to_string_lossy()).is_some())
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
            installer::manifest_db::resolve_target_relative(target_dir, &item.legacy_path)?;
            installer::manifest_db::resolve_target_relative(target_dir, &item.superseded_by)?;
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
            let backup_root = installer::migrate::new_backup_root(target_dir);
            let report = installer::migrate::apply(target_dir, &migration_items, &backup_root)?;
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
            installer::apply::apply(target_dir, &plan, force)
        } else {
            installer::apply::apply_with_preserved_paths(
                target_dir,
                &plan,
                force,
                &preserved_paths,
            )
        };
        let result = match apply_result {
            Ok(result) => result,
            Err(error) => {
                let rollback = match migration_report.as_ref() {
                    Some(report) => installer::migrate::rollback(target_dir, report),
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
                            installer::manifest_db::resolve_target_relative(target_dir, &rel)?,
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
    Ok(())
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Install {
            harness,
            location,
            with_tools,
            no_migrate,
            force,
        } => {
            let (roles, cmds, available_tools) = load_catalog()?;
            let harnesses = resolve_install_harnesses(harness)?;
            let selected_tools = resolve_named_tools(with_tools, &available_tools)?;
            let target_dir = resolve_target_dir(location.local, location.dir)?;
            run_install(
                &target_dir,
                &harnesses,
                ToolSelection::Explicit(selected_tools),
                &available_tools,
                &roles,
                &cmds,
                no_migrate,
                force,
            )?;
        }
        Command::Uninstall { harness, location } => {
            let target_dir = resolve_target_dir(location.local, location.dir)?;
            let selected = installer::uninstall::select_receipt(&target_dir, harness.as_deref())?;
            let Some(selected) = selected else {
                println!("No install receipt found; nothing to uninstall.");
                return Ok(());
            };
            let (roles, cmds, tools) = load_catalog()?;
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

            let adapter = adapters::select(&target)?;
            let files = adapter.build(&roles, &cmds)?;

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

            let adapter = adapters::select(&target)?;
            let files = adapter.build(&roles, &cmds)?;
            check_digests(&target, adapter.digest_root(), &files, root_path)?;
        }
        Command::Update {
            harness,
            location,
            with_tools,
            no_migrate,
        } => {
            let target_dir = resolve_target_dir(location.local, location.dir)?;
            let harnesses = resolve_update_harnesses(&target_dir, harness)?;
            let (roles, cmds, available_tools) = load_catalog()?;
            let tools = match with_tools {
                Some(_) => {
                    ToolSelection::Explicit(resolve_named_tools(with_tools, &available_tools)?)
                }
                None => ToolSelection::FromReceipt,
            };
            // Force refresh: update means bring payload files to the binary's
            // current bytes, including paths that drifted outside a receipt.
            run_install(
                &target_dir,
                &harnesses,
                tools,
                &available_tools,
                &roles,
                &cmds,
                no_migrate,
                true,
            )?;
        }
        Command::Doctor {
            harness,
            location,
            fix,
            no_migrate,
        } => {
            let (roles, cmds, tools) = load_catalog()?;
            let target_dir = resolve_target_dir(location.local, location.dir)?;

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
                println!("{name}");
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
    use clap::CommandFactory;

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

    fn help_for(command: &str) -> String {
        let mut cmd = Cli::command();
        let sub = cmd.find_subcommand_mut(command).expect("subcommand");
        let mut buf = Vec::new();
        sub.write_long_help(&mut buf).unwrap();
        String::from_utf8(buf).unwrap()
    }

    #[test]
    fn top_level_help_separates_user_and_contributor_commands() {
        let mut buf = Vec::new();
        Cli::command().write_long_help(&mut buf).unwrap();
        let help = String::from_utf8(buf).unwrap();
        assert!(
            help.contains("Start here:"),
            "top-level long help should orient first-time users: {help}"
        );
        assert!(
            help.contains("Contributor commands"),
            "build/check should sit under a contributor heading: {help}"
        );
        for cmd in [
            "install",
            "update",
            "uninstall",
            "doctor",
            "targets",
            "build",
            "check",
        ] {
            assert!(help.contains(cmd), "missing {cmd} in top-level help");
        }
    }

    #[test]
    fn install_help_documents_flags_value_names_and_examples() {
        let help = help_for("install");
        for needle in [
            "--harness <NAME>",
            "--dir <PATH>",
            "--with-tools <NAMES|all|none>",
            "--force",
            "--no-migrate",
            "--global",
            "--local",
            "Examples:",
            "shipmates install --harness claude-code",
        ] {
            assert!(
                help.contains(needle),
                "install help missing `{needle}`:\n{help}"
            );
        }
    }

    #[test]
    fn update_help_documents_refresh_semantics() {
        let help = help_for("update");
        for needle in [
            "--harness <NAME>",
            "--with-tools <NAMES|all|none>",
            "Examples:",
            "shipmates update",
            "build --update",
        ] {
            assert!(
                help.contains(needle),
                "update help missing `{needle}`:\n{help}"
            );
        }
    }

    #[test]
    fn doctor_help_documents_fix() {
        let help = help_for("doctor");
        for needle in [
            "--harness <NAME>",
            "--fix",
            "Repair missing or drifted",
            "Examples:",
            "shipmates doctor --fix",
        ] {
            assert!(
                help.contains(needle),
                "doctor help missing `{needle}`:\n{help}"
            );
        }
    }

    #[test]
    fn build_and_check_help_mark_contributor_workflows() {
        let build = help_for("build");
        let check = help_for("check");
        assert!(build.contains("--target <NAME>"));
        assert!(build.contains("--root <PATH>"));
        assert!(build.contains("--update"));
        assert!(build.contains("Contributor"));
        assert!(build.contains("Examples:"));
        assert!(check.contains("--target <NAME>"));
        assert!(check.contains("Contributor"));
        assert!(check.contains("Examples:"));
    }

    #[test]
    fn location_flags_share_where_heading_across_user_commands() {
        for command in ["install", "update", "uninstall", "doctor"] {
            let help = help_for(command);
            assert!(
                help.contains("Where:"),
                "{command} help should group location flags under Where:\n{help}"
            );
            assert!(help.contains("--global"), "{command} missing --global");
            assert!(help.contains("--local"), "{command} missing --local");
            assert!(
                help.contains("--dir <PATH>"),
                "{command} missing --dir <PATH>"
            );
        }
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
        assert_eq!(select_tools_from_line("1, 9", &avail).map(|_| ()), None);
    }

    #[test]
    fn test_harness_line_empty_defaults_to_claude_code() {
        let avail = ["claude-code", "opencode", "cursor"];
        assert_eq!(
            select_harnesses_from_line("", &avail),
            Some(vec!["claude-code".into()])
        );
    }

    #[test]
    fn test_harness_line_all_and_names() {
        let avail = ["claude-code", "opencode", "cursor"];
        assert_eq!(
            select_harnesses_from_line("all", &avail).map(|v| v.len()),
            Some(3)
        );
        assert_eq!(
            select_harnesses_from_line("2, cursor", &avail),
            Some(vec!["opencode".into(), "cursor".into()])
        );
        assert_eq!(select_harnesses_from_line("9", &avail), None);
        assert_eq!(select_harnesses_from_line("nope", &avail), None);
    }

    #[test]
    fn test_tools_from_receipt_matches_path_components() {
        use installer::manifest_db::ReceiptFile;
        let receipt = InstallReceipt::new(
            "0.1.4".to_string(),
            "claude-code".to_string(),
            "skills".to_string(),
            vec![".claude".to_string()],
            vec![
                ReceiptFile {
                    path: ".claude/skills/badge/SKILL.md".to_string(),
                    sha256: "0".repeat(64),
                },
                ReceiptFile {
                    path: ".claude/skills/ship-issue/SKILL.md".to_string(),
                    sha256: "1".repeat(64),
                },
            ],
        )
        .unwrap();
        let available = [tool("badge"), tool("termgif"), tool("scrub")];
        let got: Vec<_> = tools_from_receipt(&receipt, &available)
            .into_iter()
            .map(|t| t.name)
            .collect();
        assert_eq!(got, vec!["badge".to_string()]);
    }
}
