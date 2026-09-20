use anyhow::{Context, Result, bail};
use clap::Parser;
use shipmates::cli::{Cli, Command, install_force_hint};
use shipmates::installer::manifest_db::InstallReceipt;
use shipmates::{adapters, catalog, detector, digest, doctor, installer, steering};
use std::fs;
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};

use shipmates::catalog::CanonicalTool;

/// How optional tools are chosen for an install/update run.
enum ToolSelection {
    /// Same set for every harness in the run.
    Explicit(Vec<CanonicalTool>),
    /// Keep whatever each harness receipt already claims (update default).
    FromReceipt,
}

fn harness_blurb(name: &str) -> &'static str {
    match name {
        "claude-code" => "agents + skills in .claude (runtime-verified)",
        "opencode" => "agents + commands in .opencode",
        "antigravity" => "agents + skills in .agents (agy)",
        "codex" => "TOML crew in .codex + skills in .agents",
        "cursor" => "skills in .cursor/skills (first-party slash picker)",
        "github-copilot" => "crew in .github/agents + skills in .agents",
        "pi" => "crew in .pi/agents + skills in .agents (global: crew only)",
        "grok-build" => "agents + skills in .grok",
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
        if line.trim().is_empty() {
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

fn resolve_install_harnesses(
    harness: Option<String>,
    target_dir: Option<&Path>,
) -> Result<Vec<String>> {
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
        // Detection is a hint only (#489). Never auto-install every detected
        // harness: a loose marker used to silently install the wrong tree.
        None if std::io::stdin().is_terminal() => {
            let detected = detector::detect_installed_harness_names(target_dir);
            if !detected.is_empty() {
                println!(
                    "Detected installed harness(es): {} — confirm below (Enter = claude-code).",
                    detected.join(", ")
                );
            }
            Ok(prompt_for_harnesses(&available))
        }
        None => {
            let detected = detector::detect_installed_harness_names(target_dir);
            if !detected.is_empty() {
                println!(
                    "Detected installed harness(es): {} — non-interactive install defaults to \
                     claude-code. Pass --harness NAME (or --harness all) to override.",
                    detected.join(", ")
                );
            }
            Ok(vec!["claude-code".into()])
        }
    }
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

/// Tools a receipt already claims, matched by the shared `CanonicalTool::owns_path`
/// predicate so both the skill-dir and native-file (`…/shipmates-termgif.ts`)
/// install forms map back to the tool they belong to (#412).
fn tools_from_receipt(receipt: &InstallReceipt, available: &[CanonicalTool]) -> Vec<CanonicalTool> {
    available
        .iter()
        .filter(|tool| {
            receipt
                .files
                .iter()
                .any(|file| tool.owns_path(Path::new(&file.path)))
        })
        .cloned()
        .collect()
}

/// Render a tool count, adding the before → after delta when this run changed it.
fn tool_change(tools: usize, previous: Option<usize>) -> String {
    match previous {
        Some(previous) if previous != tools => format!("tools: {} → {}", previous, tools),
        _ => format!("tools: {}", tools),
    }
}

fn select_tools(
    with_tools: Option<Vec<String>>,
    available: Vec<CanonicalTool>,
) -> Result<Vec<CanonicalTool>> {
    match with_tools {
        Some(want) => {
            let want: Vec<String> = want.into_iter().filter(|w| !w.is_empty()).collect();
            if want.iter().any(|t| t == "none") {
                Ok(Vec::new())
            } else if want.iter().any(|t| t == "all") {
                Ok(available)
            } else {
                for w in &want {
                    if !available
                        .iter()
                        .any(|t| installer::rename::matches_requested_tool(w, &t.name))
                    {
                        let names: Vec<&str> = available
                            .iter()
                            .map(|t| installer::rename::canonical_tool_name(&t.name))
                            .collect();
                        bail!("unknown tool: {} (available: {})", w, names.join(", "));
                    }
                }
                Ok(available
                    .into_iter()
                    .filter(|t| {
                        want.iter()
                            .any(|w| installer::rename::matches_requested_tool(w, &t.name))
                    })
                    .collect())
            }
        }
        None => Ok(available),
    }
}

fn run_install_loop(
    target_dir: &Path,
    harnesses: &[String],
    tools: ToolSelection,
    available_tools: &[CanonicalTool],
    roles: &[catalog::CanonicalRole],
    cmds: &[catalog::CanonicalCommand],
    install_steering: Option<&str>,
    no_migrate: bool,
    force: bool,
    migrate_steering: bool,
    force_hint_for: &dyn Fn(&str) -> String,
) -> Result<()> {
    let mut provision_scripts: Vec<PathBuf> = Vec::new();
    let install_all = harnesses.len() > 1;
    let fail_fast = !install_all;
    let mut installed: Vec<(String, HarnessInstall)> = Vec::new();
    let mut failures: Vec<(String, String)> = Vec::new();

    for harness in harnesses {
        // The previous receipt is both the `--with-tools` source and the baseline
        // for the summary's tool delta.
        let (_, previous_receipt, _) = installer::plan::read_receipt(target_dir, harness);
        let previous_tools = previous_receipt
            .as_ref()
            .map(|receipt| tools_from_receipt(receipt, available_tools).len());
        let selected_tools = match &tools {
            ToolSelection::Explicit(tools) => tools.clone(),
            ToolSelection::FromReceipt => previous_receipt
                .as_ref()
                .map(|receipt| tools_from_receipt(receipt, available_tools))
                .unwrap_or_default(),
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

        match install_harness(
            harness,
            target_dir,
            roles,
            cmds,
            &selected_tools,
            install_steering,
            &provision_filenames,
            no_migrate,
            force,
            previous_tools,
            &force_hint_for(harness),
        ) {
            Ok(outcome) => {
                for script in &outcome.provision_scripts {
                    if !provision_scripts
                        .iter()
                        .any(|known| known.file_name() == script.file_name())
                    {
                        provision_scripts.push(script.clone());
                    }
                }
                installed.push((harness.clone(), outcome));
            }
            Err(error) if fail_fast => return Err(error),
            Err(error) => {
                println!("Failed harness: {} — {:#}", harness, error);
                failures.push((harness.clone(), format!("{:#}", error)));
            }
        }
    }

    if !fail_fast {
        println!("\nHarness summary:");
        for (harness, outcome) in &installed {
            // Keep the `<harness>: installed v<version>` prefix stable — the CLI
            // e2e suite greps it — and carry the tool count/delta after it.
            println!(
                "  {}: installed v{} ({})",
                harness,
                outcome.version,
                tool_change(outcome.tools.len(), outcome.previous_tools)
            );
        }
        for (harness, error) in &failures {
            println!("  {}: failed — {}", harness, error);
        }
    }
    if !provision_scripts.is_empty() {
        provision_tool_deps(&provision_scripts);
    }
    if migrate_steering {
        for action in steering::plan_legacy_migration(target_dir)? {
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
    if !failures.is_empty() {
        if installed.is_empty() {
            println!(
                "\n{} of {} harnesses failed; none installed.                  Re-run after fixing the cause.",
                failures.len(),
                harnesses.len()
            );
        } else {
            println!(
                "\n{} of {} harnesses failed; the rest are installed at v{}.                  Re-run the failed harness after fixing the cause.",
                failures.len(),
                harnesses.len(),
                env!("CARGO_PKG_VERSION")
            );
        }
        std::process::exit(1);
    }
    Ok(())
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

/// What one harness's install produced, for the cross-harness summary.
struct HarnessInstall {
    version: String,
    provision_scripts: Vec<PathBuf>,
    /// Tools this run selected, in catalog order.
    tools: Vec<CanonicalTool>,
    /// Tools the previous receipt claimed, when one existed — the delta baseline.
    previous_tools: Option<usize>,
}

/// Install one harness. Every failure mode returns `Err` rather than exiting, so
/// `--harness all` can report a failed target and carry on with the rest (#384).
#[allow(clippy::too_many_arguments)]
fn install_harness(
    harness: &str,
    target_dir: &Path,
    roles: &[catalog::CanonicalRole],
    cmds: &[catalog::CanonicalCommand],
    selected_tools: &[catalog::CanonicalTool],
    install_steering: Option<&str>,
    provision_filenames: &std::collections::HashSet<String>,
    no_migrate: bool,
    force: bool,
    previous_tools: Option<usize>,
    force_hint: &str,
) -> Result<HarnessInstall> {
    let mut provision_scripts: Vec<PathBuf> = Vec::new();
    let adapter = adapters::select(harness)?;
    let built = adapters::build_payload(adapter.as_ref(), roles, cmds, install_steering)?;
    // A global install (target = $HOME) writes into each harness's own
    // user-scope tree, which for antigravity and pi is NOT the workspace path
    // joined to home. Relocate once, here, so the plan, the receipt and the
    // migration table all describe where the files actually land.
    //
    // The tool payload is relocated with the rest, except global pi, which omits
    // command/tool skills so they cannot collide with a project tree Pi also
    // loads (#513).
    let global = installer::manifest_db::is_global_target(target_dir);
    let container = adapter.container();
    let relocate = |payload: std::collections::HashMap<String, String>| {
        if global {
            installer::manifest_db::relocate_payload(harness, container, &payload)
        } else {
            payload
        }
    };
    let built = relocate(built);
    let tools_payload = relocate(adapter.build_tools(selected_tools));
    let payload_prefix = format!("{}/", adapter.container());

    // A path behind a symlink is SKIPPED — not written through and not fatal.
    //
    // Shipmates never writes through a symlink: that containment property is not
    // negotiable. But refusing one path must not abandon the other forty, which
    // is what sharing a skills tree across harnesses used to cost a captain —
    // the whole install aborted on a path that was already correct. Every skipped
    // path is named below, and the tools summary / returned tool list reflect
    // what actually landed — never the requested set (#489).
    let (built, mut skipped_symlinked) =
        partition_symlinked(target_dir, &payload_prefix, built)?;
    let (tools_payload, skipped_tools) =
        partition_symlinked(target_dir, &payload_prefix, tools_payload)?;
    skipped_symlinked.extend(skipped_tools);
    let landed_tools: Vec<catalog::CanonicalTool> = selected_tools
        .iter()
        .filter(|tool| {
            tools_payload
                .keys()
                .any(|key| tool.owns_path(Path::new(key)))
        })
        .cloned()
        .collect();
    let plan = installer::plan::InstallPlan::from_payload(
        adapter.as_ref(),
        harness,
        built.clone(),
        tools_payload,
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

    let rename_payload: std::collections::HashMap<String, String> = plan
        .files
        .iter()
        .map(|(path, content)| (path.to_string_lossy().into_owned(), content.clone()))
        .collect();
    let rename_candidates =
        installer::rename::plan(target_dir, &rename_payload, adapter.container())?;
    let rename_items = if no_migrate {
        Vec::new()
    } else {
        rename_candidates.clone()
    };
    for item in &rename_candidates {
        installer::manifest_db::resolve_target_relative(target_dir, &item.old_path)?;
        installer::manifest_db::resolve_target_relative(target_dir, &item.new_path)?;
    }

    // Identity rename, then layout migration, then receipt
    // publication. If a later step fails, earlier steps roll back
    // so a rename never becomes an irreversible side effect of an
    // unsuccessful install. Paths deliberately left in place remain
    // claimed so a later install can retry.
    let mut preserved_paths = std::collections::BTreeSet::new();
    let mut rename_report = None;
    let mut migration_report = None;
    if no_migrate {
        preserved_paths.extend(installer::rename::preserved_old_paths(&rename_candidates));
        preserved_paths.extend(
            migration_candidates
                .iter()
                .map(|item| item.legacy_path.to_string_lossy().into_owned()),
        );
    } else {
        let needs_backup = !rename_items.is_empty() || !migration_items.is_empty();
        let backup_root = needs_backup.then(|| installer::migrate::new_backup_root(target_dir));
        if !rename_items.is_empty() {
            let backup_root = backup_root.as_ref().expect("backup root");
            let report = installer::rename::apply(
                target_dir,
                &rename_items,
                &rename_payload,
                adapter.container(),
                backup_root,
            )?;
            for item in &rename_items {
                if !report
                    .renamed
                    .iter()
                    .any(|renamed| renamed.old_path == item.old_path)
                {
                    preserved_paths.insert(item.old_path.to_string_lossy().into_owned());
                }
            }
            if !report.renamed.is_empty() {
                installer::rename::print_map(&report);
            }
            rename_report = Some(report);
        }
        if !migration_items.is_empty() {
            let backup_root = backup_root.as_ref().expect("backup root");
            let report = installer::migrate::apply(target_dir, &migration_items, backup_root)?;
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
    }

    let apply_result = if preserved_paths.is_empty() {
        installer::apply::apply(target_dir, &plan, force, force_hint)
    } else {
        installer::apply::apply_with_preserved_paths(
            target_dir,
            &plan,
            force,
            &preserved_paths,
            force_hint,
        )
    };
    let result = match apply_result {
        Ok(result) => result,
        Err(error) => {
            let migrate_rollback = match migration_report.as_ref() {
                Some(report) => installer::migrate::rollback(target_dir, report),
                None => Ok(()),
            };
            let rename_rollback = match rename_report.as_ref() {
                Some(report) => installer::rename::rollback(target_dir, report),
                None => Ok(()),
            };
            return Err(combine_rollback_error(
                combine_rollback_error(error, migrate_rollback),
                rename_rollback,
            ));
        }
    };
    if let Some(receipt) = &result.receipt {
        for file in &receipt.files {
            let rel = PathBuf::from(&file.path);
            if let Some(fname) = rel.file_name().and_then(|name| name.to_str())
                && provision_filenames.contains(fname)
                && !provision_scripts
                    .iter()
                    .any(|p| p.file_name().and_then(|s| s.to_str()) == Some(fname))
            {
                provision_scripts.push(installer::manifest_db::resolve_target_relative(
                    target_dir, &rel,
                )?);
            }
        }
    }

    if let Some(previous) = &result.previous_version
        && previous != &plan.version
    {
        println!("Upgrading shipmates v{} → v{}", previous, plan.version);
        println!(
            "{} files changed, {} new, {} removed",
            result.summary.changed, result.summary.new, result.summary.removed
        );
    }
    for warning in &result.warnings {
        println!("{}", warning);
    }
    if !skipped_symlinked.is_empty() {
        skipped_symlinked.sort();
        skipped_symlinked.dedup();
        println!(
            "Warning: incomplete install — {} path(s) sit behind a symlink and were left alone \
             (Shipmates never writes through one): {}",
            skipped_symlinked.len(),
            skipped_symlinked.join(", ")
        );
        if landed_tools.len() != selected_tools.len() {
            println!(
                "  Tools that landed: {} of {} requested — receipt and summary count only what \
                 was written.",
                landed_tools.len(),
                selected_tools.len()
            );
        }
    }

    let tool_change = tool_change(landed_tools.len(), previous_tools);
    if landed_tools.is_empty() {
        println!(
            "Installed harness: {} ({} files written, {})",
            harness, result.written, tool_change
        );
    } else {
        let names: Vec<&str> = landed_tools
            .iter()
            .map(|tool| tool.name.as_str())
            .collect();
        println!(
            "Installed harness: {} ({} files written, {} — {})",
            harness,
            result.written,
            tool_change,
            names.join(", ")
        );
    }
    // #454: guidance only — do not remap the target or invent a doctor check.
    // Global pi crew lands under ~/.pi/agent/; a nearer ancestor carrying
    // `.pi/` or `.agents/` still shadows that home tree from a nested cwd.
    if let Some(hint) = pi_global_install_hint(harness, global) {
        println!("{hint}");
    }
    Ok(HarnessInstall {
        version: plan.version,
        provision_scripts,
        tools: landed_tools,
        previous_tools,
    })
}

/// Post-install guidance for a home/global pi install (#454).
///
/// Returns `Some` only when `harness` is `pi` and the install target is the
/// user's home directory (`--global` / `$HOME`). Project-local installs
/// (`--local`, `--dir <project>`) must stay quiet — they are the preferred shape.
fn pi_global_install_hint(harness: &str, global: bool) -> Option<&'static str> {
    if harness == "pi" && global {
        Some(
            "Note: pi loads ~/.pi/agent/skills and project .agents/skills in the same session, \
             so a global install does not write command skills (that dual tree is what printed \
             [Skill conflicts]). Crew land at ~/.pi/agent/agents/. Prefer `--local` or \
             `--dir <project>` for pi — skills install to .agents/skills.",
        )
    } else {
        None
    }
}

/// Split a payload into the paths that may be written and those sitting behind a
/// symlink, which are skipped rather than written through.
///
/// Never writes through a symlink; never abandons the rest of the payload because
/// of one. An *unsafe* path is still a hard error — that is a programming fault,
/// not a fact about the captain's environment.
fn partition_symlinked(
    target_dir: &Path,
    prefix: &str,
    payload: std::collections::HashMap<String, String>,
) -> Result<(std::collections::HashMap<String, String>, Vec<String>)> {
    let mut kept = std::collections::HashMap::with_capacity(payload.len());
    let mut skipped = Vec::new();
    for (key, content) in payload {
        if let Some(rel) = key.strip_prefix(prefix) {
            match installer::manifest_db::classify_target_path(target_dir, Path::new(rel))? {
                Some(installer::manifest_db::Blocked::Unsafe) => {
                    anyhow::bail!("unsafe install path: {rel}")
                }
                Some(installer::manifest_db::Blocked::Symlinked) => {
                    skipped.push(rel.to_string());
                    continue;
                }
                None => {}
            }
        }
        kept.insert(key, content);
    }
    Ok((kept, skipped))
}

fn format_with_tools_flag(with_tools: Option<&Vec<String>>) -> Option<String> {
    with_tools.map(|names| names.join(","))
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

/// Reconstruct `--with-tools` for a doctor force hint from what the receipt
/// still claims. `none` when no tool paths remain; `all` when any do. `None`
/// when there is no readable receipt (omit the flag — install default applies).
fn with_tools_flag_from_receipt(
    target_dir: &Path,
    harness: &str,
    tools: &[catalog::CanonicalTool],
) -> Option<String> {
    let (_, receipt, _) = installer::plan::read_receipt(target_dir, harness);
    let receipt = receipt?;
    let has_tool = receipt.files.iter().any(|file| {
        let path = Path::new(&file.path);
        tools.iter().any(|tool| tool.owns_path(path))
    });
    Some(if has_tool {
        "all".to_string()
    } else {
        "none".to_string()
    })
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
            from_cwd,
        } => {
            let source = catalog::resolve_source_from_env(from_cwd)?;
            let roles = source.load_roles()?;
            let cmds = source.load_commands()?;
            let available = source.load_tools()?;
            let available_for_receipt = available.clone();
            let with_tools_flag = format_with_tools_flag(with_tools.as_ref());
            let selected_tools = select_tools(with_tools, available)?;
            let target_dir = resolve_target_dir(location.local, location.dir.clone())?;
            let install_steering = source.steering_for_target(&target_dir)?;
            let harnesses = resolve_install_harnesses(harness, Some(&target_dir))?;

            run_install_loop(
                &target_dir,
                &harnesses,
                ToolSelection::Explicit(selected_tools),
                &available_for_receipt,
                &roles,
                &cmds,
                install_steering.as_deref(),
                no_migrate,
                force,
                install_steering.is_some(),
                &|h| install_force_hint(h, &location, with_tools_flag.as_deref()),
            )?;

            // Global steering is user-scope only (#489). A project --local/--dir
            // install must not rewrite ~/.claude/CLAUDE.md (and friends); that
            // used to fire on every install after #438.
            if installer::manifest_db::is_global_target(&target_dir)
                && let Some(home_path) = home::home_dir()
                && let Ok(global_content) = source.load_global_steering()
            {
                println!(
                    "\nInstalling canonical global steering (heuristics and workflow routing)..."
                );
                for h in &harnesses {
                    match steering::install_global_steering(h, &home_path, &global_content) {
                        Ok(steering::SteeringOutcome::Created(p)) => {
                            println!("  {} — installed global steering at {}", h, p.display());
                        }
                        Ok(steering::SteeringOutcome::Updated(p)) => {
                            println!("  {} — updated global steering at {}", h, p.display());
                        }
                        Ok(steering::SteeringOutcome::Unchanged(p)) => {
                            println!("  {} — global steering up to date at {}", h, p.display());
                        }
                        Ok(steering::SteeringOutcome::Gap(msg)) => {
                            println!("  {} — note: {}", h, msg);
                        }
                        Ok(steering::SteeringOutcome::Removed(_)) => {}
                        Err(e) => {
                            eprintln!("  {} — failed to install global steering: {}", h, e);
                        }
                    }
                }
            }
        }
        Command::Uninstall {
            harness,
            location,
            from_cwd,
        } => {
            let target_dir = resolve_target_dir(location.local, location.dir)?;
            let selected = installer::uninstall::select_receipt(&target_dir, harness.as_deref())?;
            let Some(selected) = selected else {
                println!("No install receipt found; nothing to uninstall.");
                return Ok(());
            };
            let harness_name = selected.receipt.harness.clone();
            let source = catalog::resolve_source_from_env(from_cwd)?;
            let roles = source.load_roles()?;
            let cmds = source.load_commands()?;
            let tools = source.load_tools()?;
            let known_payload = installer::uninstall::payload_for(
                &selected.receipt.harness,
                &roles,
                &cmds,
                &tools,
                &source.load_steering()?,
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
            // Always strip user-scope steering for this harness on uninstall so
            // a prior local install that wrote home files (#489) can still be
            // cleaned up, and a global install does not leave the managed block.
            if let Some(home_path) = home::home_dir() {
                match steering::uninstall_global_steering(&harness_name, &home_path) {
                    Ok(steering::SteeringOutcome::Removed(p)) => {
                        println!("  removed global steering at {}", p.display());
                    }
                    Ok(steering::SteeringOutcome::Unchanged(_))
                    | Ok(steering::SteeringOutcome::Gap(_))
                    | Ok(steering::SteeringOutcome::Created(_))
                    | Ok(steering::SteeringOutcome::Updated(_)) => {}
                    Err(e) => {
                        eprintln!("  failed to remove global steering: {e}");
                    }
                }
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
            let files = adapters::build_payload(adapter.as_ref(), &roles, &cmds, Some(&steering))?;

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
            let files = adapters::build_payload(adapter.as_ref(), &roles, &cmds, Some(&steering))?;
            check_digests(&target, adapter.digest_root(), &files, root_path)?;
        }
        Command::Update {
            harness,
            location,
            with_tools,
            no_migrate,
            from_cwd,
        } => {
            let target_dir = resolve_target_dir(location.local, location.dir.clone())?;
            let harnesses = resolve_update_harnesses(&target_dir, harness)?;
            let source = catalog::resolve_source_from_env(from_cwd)?;
            let roles = source.load_roles()?;
            let cmds = source.load_commands()?;
            let available = source.load_tools()?;
            let with_tools_flag = format_with_tools_flag(with_tools.as_ref());
            let tools = match with_tools {
                Some(_) => ToolSelection::Explicit(select_tools(with_tools, available.clone())?),
                None => ToolSelection::FromReceipt,
            };
            let install_steering = source.steering_for_target(&target_dir)?;
            run_install_loop(
                &target_dir,
                &harnesses,
                tools,
                &available,
                &roles,
                &cmds,
                install_steering.as_deref(),
                no_migrate,
                true,
                install_steering.is_some(),
                &|h| install_force_hint(h, &location, with_tools_flag.as_deref()),
            )?;

            // Refresh canonical global steering only on a global/$HOME target (#489).
            if installer::manifest_db::is_global_target(&target_dir)
                && let Some(home_path) = home::home_dir()
                && let Ok(global_content) = source.load_global_steering()
            {
                for h in &harnesses {
                    let _ = steering::install_global_steering(h, &home_path, &global_content);
                }
            }
        }
        Command::Doctor {
            harness,
            location,
            fix,
            no_migrate,
            from_cwd,
        } => {
            let source = catalog::resolve_source_from_env(from_cwd)?;
            let roles = source.load_roles()?;
            let cmds = source.load_commands()?;
            let tools = source.load_tools()?;
            let target_dir = resolve_target_dir(location.local, location.dir.clone())?;
            // Replay the tools posture the receipt claims so a foreign-collision
            // force hint does not silently broaden a crew-only install (#392 nit).
            let with_tools = with_tools_flag_from_receipt(&target_dir, &harness, &tools);
            let force_hint =
                install_force_hint(&harness, &location, with_tools.as_deref());

            let report = if fix {
                doctor::fix(
                    &target_dir,
                    &harness,
                    &roles,
                    &cmds,
                    &tools,
                    no_migrate,
                    &source,
                    &force_hint,
                )?
            } else {
                doctor::diagnose(
                    &target_dir,
                    &harness,
                    &roles,
                    &cmds,
                    &tools,
                    &source,
                    &force_hint,
                )?
            };
            doctor::print_report(&report);
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
    use clap::CommandFactory;

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
        assert!(help.contains("Start here:"), "{help}");
        assert!(help.contains("Contributor commands"), "{help}");
    }

    #[test]
    fn install_help_documents_flags_value_names_and_examples() {
        let help = help_for("install");
        for needle in [
            "--harness <NAME>",
            "--dir <PATH>",
            "--with-tools <NAMES|all|none>",
            "--force",
            "--from-cwd",
            "Examples:",
            "Where:",
        ] {
            assert!(help.contains(needle), "missing `{needle}`:\n{help}");
        }
    }

    #[test]
    fn update_help_documents_refresh_semantics() {
        let help = help_for("update");
        for needle in [
            "--harness <NAME>",
            "Examples:",
            "shipmates update",
            "build --update",
            // Omitted --harness refreshes every receipt without a prompt
            // off a terminal; omitted --with-tools keeps each receipt's tools.
            "without a prompt",
            "`--harness all`",
            "keep the tools",
        ] {
            assert!(help.contains(needle), "missing `{needle}`:\n{help}");
        }
    }

    #[test]
    fn doctor_help_documents_fix() {
        let help = help_for("doctor");
        for needle in ["--fix", "Repair missing or drifted", "Examples:"] {
            assert!(help.contains(needle), "missing `{needle}`:\n{help}");
        }
    }

    #[test]
    fn location_flags_share_where_heading_across_user_commands() {
        for command in ["install", "update", "uninstall", "doctor"] {
            let help = help_for(command);
            assert!(help.contains("Where:"), "{command}:\n{help}");
            assert!(help.contains("--dir <PATH>"), "{command}");
        }
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
    fn cursor_blurb_names_first_party_skills_tree() {
        assert!(
            harness_blurb("cursor").contains(".cursor/skills"),
            "cursor blurb must name the first-party skills tree, got {}",
            harness_blurb("cursor")
        );
        assert!(
            !harness_blurb("cursor").contains(".agents"),
            "cursor no longer ships skills into the shared .agents tree"
        );
    }

    #[test]
    fn pi_blurb_names_shared_skills_and_global_crew_only() {
        assert!(
            harness_blurb("pi").contains("skills in .agents"),
            "project pi shares .agents/skills with sibling harnesses, got {}",
            harness_blurb("pi")
        );
        assert!(
            harness_blurb("pi").contains("global: crew only"),
            "global pi must not advertise command skills (#513), got {}",
            harness_blurb("pi")
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
    }

    fn receipt(paths: &[&str]) -> InstallReceipt {
        InstallReceipt {
            schema_version: installer::manifest_db::CURRENT_SCHEMA_VERSION,
            version: "0.0.0".into(),
            harness: "opencode".into(),
            layout: "commands".into(),
            roots: vec![".opencode".into()],
            files: paths
                .iter()
                .map(|path| installer::manifest_db::ReceiptFile {
                    path: (*path).to_string(),
                    sha256: "0".repeat(64),
                })
                .collect(),
        }
    }

    fn canonical_tool(name: &str) -> CanonicalTool {
        CanonicalTool {
            name: name.into(),
            description: String::new(),
            body: String::new(),
            assets: Vec::new(),
            requires: Vec::new(),
            source: PathBuf::new(),
        }
    }

    #[test]
    fn test_tools_from_receipt_maps_native_and_skill_install_forms() {
        // #412: opencode's native `.ts` file must map back to its tool, exactly
        // like the skill-directory form the other harnesses emit.
        let tools = [
            canonical_tool("shipmates-termgif"),
            canonical_tool("shipmates-scrub"),
        ];
        let claimed = receipt(&[
            ".opencode/tools/shipmates-termgif.ts",
            ".opencode/tools/termgif.py",
            ".claude/skills/shipmates-scrub/SKILL.md",
        ]);
        let selected_tools = tools_from_receipt(&claimed, &tools);
        let selected: Vec<&str> = selected_tools
            .iter()
            .map(|tool| tool.name.as_str())
            .collect();
        assert_eq!(selected, vec!["shipmates-termgif", "shipmates-scrub"]);

        let unrelated = receipt(&[".opencode/commands/ship-issue.md"]);
        assert!(tools_from_receipt(&unrelated, &tools).is_empty());
    }

    #[test]
    fn test_tool_change_renders_count_and_delta() {
        assert_eq!(tool_change(11, None), "tools: 11");
        assert_eq!(tool_change(11, Some(11)), "tools: 11");
        assert_eq!(tool_change(0, Some(11)), "tools: 11 → 0");
    }

    #[test]
    fn test_pi_global_install_hint_only_for_home_pi() {
        // #454: home/global pi install prefers project-local; other cases stay quiet.
        let hint = pi_global_install_hint("pi", true).expect("pi + global must hint");
        assert!(hint.contains("command skills"), "{hint}");
        assert!(hint.contains("~/.pi/agent/agents"), "{hint}");
        assert!(hint.contains(".agents/skills"), "{hint}");
        assert!(hint.contains("--local"), "{hint}");
        assert!(hint.contains("--dir <project>"), "{hint}");

        assert!(pi_global_install_hint("pi", false).is_none());
        assert!(pi_global_install_hint("claude-code", true).is_none());
        assert!(pi_global_install_hint("antigravity", true).is_none());
        assert!(pi_global_install_hint("codex", true).is_none());
    }

    #[test]
    fn test_install_force_hint_replays_cli_flags() {
        use shipmates::cli::LocationOpts;
        let dir = LocationOpts {
            global: false,
            local: false,
            dir: Some("/tmp/proj".into()),
        };
        let hint = install_force_hint("codex", &dir, Some("none"));
        assert_eq!(
            hint,
            "shipmates install --harness codex --dir /tmp/proj --with-tools none --force"
        );
        let spaced = LocationOpts {
            global: false,
            local: false,
            dir: Some("/tmp/my project".into()),
        };
        let hint = install_force_hint("claude-code", &spaced, Some("none"));
        assert_eq!(
            hint,
            "shipmates install --harness claude-code --dir '/tmp/my project' --with-tools none --force"
        );
        let local = LocationOpts {
            global: false,
            local: true,
            dir: None,
        };
        let hint = install_force_hint("claude-code", &local, None);
        assert_eq!(
            hint,
            "shipmates install --harness claude-code --local --force"
        );
        assert!(!hint.contains("shipmates install --force"));
    }
}
