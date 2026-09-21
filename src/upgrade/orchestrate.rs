//! The `shipmates status` / `shipmates upgrade` orchestration: discover roots,
//! refresh them, audit each install, classify findings, and aggregate the exit
//! code.
//!
//! This module lives in the library crate, so it cannot reach the binary's
//! private helpers. Refreshing a root means spawning this same binary's own
//! `update --dir <root>` path — the payload is embedded in the binary, so it is
//! always the running version that lands.

use std::ffi::OsStr;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime};

use anyhow::Context;
use semver::Version;
use serde::Serialize;

use crate::catalog::{CanonicalCommand, CanonicalRole, CanonicalTool, CatalogSource};
use crate::doctor;
use crate::installer::manifest_db::ReceiptRepository;
use crate::upgrade::audit::{AuditDepth, audit_install};
use crate::upgrade::classify::findings_for;
use crate::upgrade::file::{FileBugsOpts, FileReport, file_bugs};
use crate::upgrade::index::{InstallsIndex, index_path};
use crate::upgrade::release::{
    can_exec_self_upgrade, describe_channel, detect_channel, fetch_latest, self_upgrade_command,
};
use crate::upgrade::types::{
    AUDIT_FINDINGS, BUGS_FILED, Channel, CheckReport, ERROR, Finding, InstallReport, InstallState,
    OK, PrunedRoot, StatusReport, UNKNOWN, UPGRADE_AVAILABLE, UPGRADE_FAILED, UnmanagedRoot,
};

/// Flags for `shipmates upgrade`.
pub struct UpgradeOpts {
    pub json: bool,
    pub pre: bool,
    pub dry_run: bool,
    pub fix: bool,
    pub dirs: Vec<PathBuf>,
    pub file_bugs: bool,
    pub self_upgrade: bool,
    pub resume: bool,
}

/// The `upgrade --json` payload (field names are part of the stable contract).
#[derive(Serialize)]
struct UpgradeReport {
    upgraded: bool,
    latest_release: Option<String>,
    running_version: String,
    channel: Channel,
    pruned: Vec<PrunedRoot>,
    installs: Vec<InstallReport>,
    findings: Vec<Finding>,
}

/// `shipmates status`: read-only enumeration of every known install.
pub fn run_status(json: bool, dirs: &[PathBuf]) -> anyhow::Result<i32> {
    let source = CatalogSource::Embedded;
    let (roles, cmds, tools) = load_catalog(&source)?;

    let index_file = index_path()?;
    let index = InstallsIndex::load(&index_file)?;

    let roots = discover_roots(dirs, &index)?;
    let pruned = dead_index_roots(&index);
    let mut installs = collect_reports(&roots, &roles, &cmds, &tools, &source)?;
    installs.extend(index_reconciliation(&index)?);
    let unmanaged = unmanaged_roots(&roots, &index)?;

    let report = StatusReport {
        shipmates_version: env!("CARGO_PKG_VERSION").to_string(),
        installs,
        unmanaged,
        pruned,
    };

    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_status_human(&report);
    }
    Ok(OK)
}

/// `shipmates upgrade --check`: three-way version report, read-only.
pub fn run_check(json: bool, pre: bool, dirs: &[PathBuf]) -> anyhow::Result<i32> {
    let source = CatalogSource::Embedded;
    let (roles, cmds, tools) = load_catalog(&source)?;

    let index_file = index_path()?;
    let index = InstallsIndex::load(&index_file)?;
    let roots = discover_roots(dirs, &index)?;
    let installs = collect_reports(&roots, &roles, &cmds, &tools, &source)?;

    let running_version = env!("CARGO_PKG_VERSION").to_string();
    let channel = detect_channel();
    let latest_release = fetch_latest(pre).ok().map(|info| info.version);
    let (upgrade_available, running_is_newer, unknown) =
        compare_versions(&running_version, latest_release.as_deref());

    let report = CheckReport {
        latest_release,
        running_version,
        upgrade_available,
        running_is_newer,
        unknown,
        pre,
        channel,
        installs,
    };

    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_check_human(&report);
    }
    Ok(check_exit(upgrade_available, unknown))
}

/// `shipmates upgrade`: upgrade the binary (optionally), refresh every root, and
/// audit the result.
pub fn run_upgrade(opts: &UpgradeOpts) -> anyhow::Result<i32> {
    let source = CatalogSource::Embedded;
    let (roles, cmds, tools) = load_catalog(&source)?;

    let _lock = if should_acquire_lock(opts) {
        Some(acquire_lock()?)
    } else {
        None
    };

    let running_version = env!("CARGO_PKG_VERSION").to_string();
    let channel = detect_channel();

    // 2. Release check degrades, never aborts.
    let latest = match fetch_latest(opts.pre) {
        Ok(info) => Some(info),
        Err(error) => {
            eprintln!(
                "warning: release check failed ({error}); continuing with the current binary"
            );
            None
        }
    };

    let (upgrade_available, running_is_newer) = match &latest {
        Some(info) => {
            match (
                Version::parse(&running_version),
                Version::parse(&info.version),
            ) {
                (Ok(running), Ok(latest)) => (running < latest, running > latest),
                _ => (false, false),
            }
        }
        None => (false, false),
    };

    // 3./4. Self-upgrade: never downgrade; only the channels that can execute;
    // Source/Unknown refuse with a clear line; dry-run prints only. A re-exec
    // (`--resume`) never re-enters this branch.
    if should_self_upgrade(opts, running_is_newer) {
        if upgrade_available {
            if can_exec_self_upgrade(channel) {
                if opts.dry_run {
                    eprintln!(
                        "would run: {}",
                        self_upgrade_command(channel).unwrap_or_default()
                    );
                } else {
                    let command = self_upgrade_command(channel).unwrap_or_default();
                    let status = Command::new("sh")
                        .arg("-c")
                        .arg(&command)
                        .status()
                        .with_context(|| format!("running self-upgrade: {command}"))?;
                    if !status.success() {
                        anyhow::bail!("self-upgrade failed ({command}): {status}");
                    }
                    // Drop the upgrade lock before the re-exec so the fresh
                    // binary is free to take it again (and `--resume` skips it).
                    drop(_lock);
                    // Re-exec the freshly-upgraded binary to refresh payloads.
                    // Prefer the channel's binary on `PATH`: after a brew
                    // upgrade the running image's Cellar path may be gone.
                    let exe = resolve_self_binary();
                    let mut child = Command::new(&exe);
                    child.args(reexec_args(opts));
                    let status = child
                        .status()
                        .with_context(|| format!("re-executing {}", exe.display()))?;
                    return Ok(status.code().unwrap_or(ERROR));
                }
            } else {
                refuse_self_upgrade(channel);
            }
        } else if let Some(info) = &latest {
            eprintln!("shipmates v{} is already current", info.version);
        }
    }

    // 5. Roots: explicit --dir ∪ index ∪ home (when home carries receipts).
    let index_file = index_path()?;
    let mut index = InstallsIndex::load(&index_file)?;
    let roots = discover_roots(&opts.dirs, &index)?;

    // 6. Prune dead index records. `--dry-run` reports them read-only; a real
    // run persists the prune (the one write this run owns).
    let pruned = prune_index(opts, &mut index, &index_file)?;

    // 7. Per root: refresh, fix, then a full audit of each install.
    let mut installs: Vec<InstallReport> = Vec::new();
    let mut findings: Vec<Finding> = Vec::new();
    let mut any_refresh_failed = false;

    for root in &roots {
        let harnesses = receipt_harnesses(root)?;
        if harnesses.is_empty() {
            continue;
        }

        let refresh = if opts.dry_run {
            Ok(())
        } else {
            refresh_root(root, opts.json)
        };
        let refresh_failed = refresh.is_err();
        let refresh_detail = refresh.err().map(|error| error.to_string());
        if refresh_failed {
            any_refresh_failed = true;
        }

        for harness in &harnesses {
            if opts.fix && !opts.dry_run {
                let _ = doctor::fix(root, harness, &roles, &cmds, &tools, false, &source, "");
            }
            let outcome = audit_install(
                root,
                harness,
                &roles,
                &cmds,
                &tools,
                &source,
                AuditDepth::Full,
            )?;
            let report = outcome.report().clone();
            findings.extend(findings_for(
                &outcome,
                refresh_failed,
                refresh_detail.as_deref(),
            ));
            installs.push(report);
        }
    }

    // 8. Classify (done per install above) and, when asked, file deduped bugs.
    let mut file_report = FileReport::default();
    if opts.file_bugs && !opts.dry_run && !findings.is_empty() {
        file_report = file_bugs(&findings, &FileBugsOpts::default())?;
    }

    // 9. Report.
    let any_findings = !findings.is_empty();
    let filed_any = !file_report.filed.is_empty() || !file_report.commented.is_empty();
    let report = UpgradeReport {
        upgraded: false,
        latest_release: latest.as_ref().map(|info| info.version.clone()),
        running_version,
        channel,
        pruned,
        installs,
        findings,
    };

    if opts.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_upgrade_human(&report, &file_report);
    }

    // 10. Exit code aggregation: refresh failure > findings > filed > ok.
    let mut codes: Vec<i32> = Vec::new();
    if any_refresh_failed {
        codes.push(UPGRADE_FAILED);
    }
    if any_findings {
        codes.push(AUDIT_FINDINGS);
    }
    if opts.file_bugs && filed_any {
        codes.push(BUGS_FILED);
    }
    Ok(aggregate_exit(&codes))
}

/// Load the catalog the audit compares against. The embedded payload is the
/// "running binary's payload" the issue calls for, whatever the cwd.
fn load_catalog(
    source: &CatalogSource,
) -> anyhow::Result<(
    Vec<CanonicalRole>,
    Vec<CanonicalCommand>,
    Vec<CanonicalTool>,
)> {
    Ok((
        source.load_roles()?,
        source.load_commands()?,
        source.load_tools()?,
    ))
}

/// Roots to scan: explicit `--dir` ∪ index roots ∪ home (when it carries a
/// receipt), canonicalized and de-duplicated in first-seen order.
fn discover_roots(dirs: &[PathBuf], index: &InstallsIndex) -> anyhow::Result<Vec<PathBuf>> {
    let mut roots: Vec<PathBuf> = dirs.to_vec();
    roots.extend(index.roots());
    if let Some(home) = home::home_dir()
        && !receipt_harnesses(&home)?.is_empty()
    {
        roots.push(home);
    }
    Ok(canonicalize_dedupe(roots))
}

fn canonicalize_dedupe(roots: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    for root in roots {
        let canonical = std::fs::canonicalize(&root).unwrap_or(root);
        if !out.contains(&canonical) {
            out.push(canonical);
        }
    }
    out
}

/// Index records whose root directory no longer exists, for the status report.
/// `run_upgrade` uses `InstallsIndex::prune_dead` instead, which also persists.
fn dead_index_roots(index: &InstallsIndex) -> Vec<PrunedRoot> {
    index
        .records
        .iter()
        .filter(|record| !Path::new(&record.root).exists())
        .map(|record| PrunedRoot {
            root: record.root.clone(),
            reason: "missing".to_string(),
        })
        .collect()
}

/// Roots with no receipts that are not known to the index — left alone, but
/// reported rather than silently skipped.
fn unmanaged_roots(roots: &[PathBuf], index: &InstallsIndex) -> anyhow::Result<Vec<UnmanagedRoot>> {
    let known: Vec<String> = index
        .records
        .iter()
        .map(|record| record.root.clone())
        .collect();
    let mut out: Vec<UnmanagedRoot> = Vec::new();
    for root in roots {
        if receipt_harnesses(root)?.is_empty()
            && !known.contains(&root.to_string_lossy().to_string())
        {
            out.push(UnmanagedRoot {
                root: root.to_string_lossy().to_string(),
                reason: "no-receipt".to_string(),
            });
        }
    }
    Ok(out)
}

/// Reconcile index records against their receipts: a live root whose receipt
/// file is gone is `Moved` (receipts dir gone entirely) or `MissingReceipt`
/// (receipts dir present, this harness's file absent).
fn index_reconciliation(index: &InstallsIndex) -> anyhow::Result<Vec<InstallReport>> {
    let mut out: Vec<InstallReport> = Vec::new();
    for record in &index.records {
        let root = Path::new(&record.root);
        if !root.exists() {
            continue; // reported as pruned
        }
        let repository = ReceiptRepository::new(root);
        let receipts_dir = repository.receipts_dir()?;
        if !receipts_dir.exists() {
            out.push(synthetic_report(
                &record.root,
                &record.harness,
                InstallState::Moved,
            ));
            continue;
        }
        if !repository.receipt_path(&record.harness)?.exists() {
            out.push(synthetic_report(
                &record.root,
                &record.harness,
                InstallState::MissingReceipt,
            ));
        }
    }
    Ok(out)
}

fn synthetic_report(root: &str, harness: &str, state: InstallState) -> InstallReport {
    InstallReport {
        root: root.to_string(),
        harness: harness.to_string(),
        receipt_version: None,
        layout: None,
        managed: false,
        drift_count: 0,
        tools: Vec::new(),
        state,
    }
}

/// Read-only collection of one `InstallReport` per receipt found under `roots`.
/// A missing root simply contributes nothing.
pub(crate) fn collect_reports(
    roots: &[PathBuf],
    roles: &[CanonicalRole],
    cmds: &[CanonicalCommand],
    tools: &[CanonicalTool],
    source: &CatalogSource,
) -> anyhow::Result<Vec<InstallReport>> {
    let mut reports: Vec<InstallReport> = Vec::new();
    for root in roots {
        for harness in receipt_harnesses(root)? {
            let outcome = audit_install(
                root,
                &harness,
                roles,
                cmds,
                tools,
                source,
                AuditDepth::Status,
            )?;
            reports.push(outcome.into_report());
        }
    }
    Ok(reports)
}

/// Harness names that have a receipt file under `root`, regardless of whether
/// that receipt parses (so a corrupt receipt is audited, not skipped).
fn receipt_harnesses(root: &Path) -> anyhow::Result<Vec<String>> {
    // A missing root (a deleted project, a typo'd --dir) contributes nothing and
    // must never be fatal — the caller reports it as unmanaged/pruned instead.
    if !root.is_dir() {
        return Ok(Vec::new());
    }
    let dir = ReceiptRepository::new(root).receipts_dir()?;
    let entries = match std::fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("reading receipts under {}", root.display()));
        }
    };
    let mut harnesses: Vec<String> = Vec::new();
    for entry in entries {
        let entry = entry.with_context(|| format!("reading receipts under {}", root.display()))?;
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        if let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) {
            harnesses.push(stem.to_string());
        }
    }
    harnesses.sort();
    harnesses.dedup();
    Ok(harnesses)
}

/// Refresh one root by spawning this same binary's `update --dir <root>`. A
/// root with no receipts is a no-op (no child spawn). Non-zero child status is
/// the caller's refresh failure; it does not abort the rest of the run.
fn refresh_root(root: &Path, capture: bool) -> anyhow::Result<()> {
    if receipt_harnesses(root)?.is_empty() {
        return Ok(());
    }
    let exe = std::env::current_exe().context("resolving the current executable")?;
    let mut command = Command::new(&exe);
    command
        .args(["update", "--dir"])
        .arg(root)
        .args(["--harness", "all"])
        .stdin(Stdio::null());

    if capture {
        let output = command.output().with_context(|| {
            format!("running {} update --dir {}", exe.display(), root.display())
        })?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!(
                "refresh failed for {}: {} — {}",
                root.display(),
                output.status,
                stderr.trim()
            );
        }
    } else {
        let status = command.status().with_context(|| {
            format!("running {} update --dir {}", exe.display(), root.display())
        })?;
        if !status.success() {
            anyhow::bail!("refresh failed for {}: {}", root.display(), status);
        }
    }
    Ok(())
}

/// The argv for the post-upgrade re-exec: `upgrade --resume` plus the captain's
/// forwarded flags. Never forwards `--self` or `--dry-run`; always `--resume`.
fn reexec_args(opts: &UpgradeOpts) -> Vec<String> {
    let mut args = vec!["upgrade".to_string(), "--resume".to_string()];
    if opts.json {
        args.push("--json".to_string());
    }
    if opts.pre {
        args.push("--pre".to_string());
    }
    if opts.fix {
        args.push("--fix".to_string());
    }
    if opts.file_bugs {
        args.push("--file-bugs".to_string());
    }
    for dir in &opts.dirs {
        args.push("--dir".to_string());
        args.push(dir.to_string_lossy().into_owned());
    }
    args
}

/// The post-upgrade binary to re-exec: the first `shipmates` on `PATH` when one
/// exists, else the running executable. After `brew upgrade`, the running
/// image's Cellar path may already be gone, so `PATH` is preferred.
fn resolve_self_binary() -> PathBuf {
    resolve_self_binary_with(std::env::var_os("PATH").as_deref())
}

fn resolve_self_binary_with(path_var: Option<&OsStr>) -> PathBuf {
    if let Some(exe) = path_var.and_then(first_shipmates_on_path) {
        return exe;
    }
    std::env::current_exe().unwrap_or_else(|_| PathBuf::from("shipmates"))
}

fn first_shipmates_on_path(path_var: &OsStr) -> Option<PathBuf> {
    for dir in std::env::split_paths(path_var) {
        let candidate = dir.join("shipmates");
        if candidate.is_file() && is_executable(&candidate) {
            return Some(candidate);
        }
    }
    None
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|meta| meta.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

/// The upgrade lock is skipped for `--dry-run` and for the post-upgrade re-exec.
fn should_acquire_lock(opts: &UpgradeOpts) -> bool {
    !opts.dry_run && !opts.resume
}

/// A re-exec (`--resume`) never re-enters the self-upgrade branch.
fn should_self_upgrade(opts: &UpgradeOpts, running_is_newer: bool) -> bool {
    opts.self_upgrade && !opts.resume && !running_is_newer
}

/// Report (and, outside `--dry-run`, persist) dead index records.
fn prune_index(
    opts: &UpgradeOpts,
    index: &mut InstallsIndex,
    index_file: &Path,
) -> anyhow::Result<Vec<PrunedRoot>> {
    if opts.dry_run {
        Ok(dead_index_roots(index))
    } else {
        index.prune_dead(index_file)
    }
}

/// Precedence-ordered exit aggregation: 3 (upgrade/refresh failed) beats 4
/// (audit findings) beats 5 (bugs filed) beats 0 (ok).
pub(crate) fn aggregate_exit(codes: &[i32]) -> i32 {
    // A handled finding (a deduped bug was filed or commented) outranks the bare
    // finding code, so `--file-bugs` can report 5 instead of always collapsing to 4.
    for preferred in [UPGRADE_FAILED, BUGS_FILED, AUDIT_FINDINGS] {
        if codes.contains(&preferred) {
            return preferred;
        }
    }
    OK
}

/// The `upgrade --check` exit mapping: 11 (unknown) > 10 (available) > 0 (ok).
pub(crate) fn check_exit(upgrade_available: bool, unknown: bool) -> i32 {
    if unknown {
        UNKNOWN
    } else if upgrade_available {
        UPGRADE_AVAILABLE
    } else {
        OK
    }
}

/// Compare the running version against an optional latest release.
fn compare_versions(running: &str, latest: Option<&str>) -> (bool, bool, bool) {
    let Some(latest) = latest else {
        return (false, false, true); // offline → unknown
    };
    match (Version::parse(running), Version::parse(latest)) {
        (Ok(running), Ok(latest)) => (running < latest, running > latest, false),
        _ => (false, false, true),
    }
}

fn refuse_self_upgrade(channel: Channel) {
    match channel {
        Channel::Source => {
            eprintln!("refusing to self-upgrade a source build — git pull and rebuild");
        }
        Channel::Cargo => {
            eprintln!(
                "cannot self-upgrade a cargo install — run `cargo install shipmates --locked`"
            );
        }
        Channel::Unknown => {
            eprintln!("cannot determine the install channel — update shipmates manually");
        }
        Channel::Brew | Channel::CargoDist => {}
    }
}

fn lock_path() -> anyhow::Result<PathBuf> {
    if let Some(raw) = std::env::var_os("SHIPMATES_LOCK")
        && !raw.is_empty()
    {
        return Ok(PathBuf::from(raw));
    }
    let home = home::home_dir()
        .ok_or_else(|| anyhow::anyhow!("HOME is not set; cannot locate the upgrade lock"))?;
    Ok(home.join(".shipmates").join("upgrade.lock"))
}

#[derive(Debug)]
struct LockGuard {
    path: PathBuf,
}

impl Drop for LockGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

/// A lock older than this is presumed stale (left by a crashed run) and retaken.
const STALE_LOCK_AGE: Duration = Duration::from_secs(60 * 60);

/// Take the non-blocking upgrade lock. A fresh held lock is a clear error; a
/// lock older than [`STALE_LOCK_AGE`] is removed and retaken.
fn acquire_lock() -> anyhow::Result<LockGuard> {
    let path = lock_path()?;
    acquire_lock_at(&path)
}

fn acquire_lock_at(path: &Path) -> anyhow::Result<LockGuard> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("creating lock directory {}", parent.display()))?;
    }
    match try_acquire_lock(path) {
        Ok(guard) => Ok(guard),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            if is_stale_lock(path) {
                eprintln!("warning: removing stale upgrade lock {}", path.display());
                fs::remove_file(path)
                    .with_context(|| format!("removing stale upgrade lock {}", path.display()))?;
                try_acquire_lock(path).map_err(|e| {
                    anyhow::Error::new(e)
                        .context(format!("acquiring upgrade lock {}", path.display()))
                })
            } else {
                Err(anyhow::anyhow!(
                    "another shipmates upgrade is already running (lock {} is held); retry when it finishes",
                    path.display()
                ))
            }
        }
        Err(error) => {
            Err(anyhow::Error::new(error)
                .context(format!("acquiring upgrade lock {}", path.display())))
        }
    }
}

fn try_acquire_lock(path: &Path) -> std::io::Result<LockGuard> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    let _ = file.write_all(format!("{}\n", std::process::id()).as_bytes());
    Ok(LockGuard {
        path: path.to_path_buf(),
    })
}

/// A lock left behind by a crashed run is not held by anyone; after an hour it
/// is presumed stale rather than permanent.
fn is_stale_lock(path: &Path) -> bool {
    let Ok(meta) = fs::metadata(path) else {
        return false;
    };
    let Ok(modified) = meta.modified() else {
        return false;
    };
    SystemTime::now()
        .duration_since(modified)
        .map(|age| age > STALE_LOCK_AGE)
        .unwrap_or(false)
}

fn state_str(state: InstallState) -> &'static str {
    match state {
        InstallState::Ok => "ok",
        InstallState::Drift => "drift",
        InstallState::MissingReceipt => "missing-receipt",
        InstallState::Moved => "moved",
        InstallState::CorruptReceipt => "corrupt-receipt",
        InstallState::Unmanaged => "unmanaged",
    }
}

fn print_status_human(report: &StatusReport) {
    println!("shipmates v{}", report.shipmates_version);
    for install in &report.installs {
        println!(
            "{} [{}] v{} ({})",
            install.root,
            install.harness,
            install.receipt_version.as_deref().unwrap_or("?"),
            state_str(install.state),
        );
    }
    for root in &report.unmanaged {
        println!("{} unmanaged ({})", root.root, root.reason);
    }
    for root in &report.pruned {
        println!("{} pruned ({})", root.root, root.reason);
    }
    println!(
        "{} install(s), {} unmanaged, {} pruned",
        report.installs.len(),
        report.unmanaged.len(),
        report.pruned.len(),
    );
}

fn print_check_human(report: &CheckReport) {
    println!("shipmates upgrade --check");
    println!("running: {}", report.running_version);
    match &report.latest_release {
        Some(latest) => println!("latest: {latest} ({})", describe_channel(report.channel)),
        None => println!("latest: unknown (offline)"),
    }
    if report.running_is_newer {
        println!("running build is newer than the latest release");
    } else if report.upgrade_available {
        println!("upgrade available");
        if let Some(command) = self_upgrade_command(report.channel) {
            println!("run: {command}");
        }
    } else if !report.unknown {
        println!("up to date");
    }
    for install in &report.installs {
        println!(
            "{} [{}] v{} ({}, drift {})",
            install.root,
            install.harness,
            install.receipt_version.as_deref().unwrap_or("?"),
            state_str(install.state),
            install.drift_count,
        );
    }
    println!("{} install(s)", report.installs.len());
}

fn print_upgrade_human(report: &UpgradeReport, file_report: &FileReport) {
    println!("shipmates upgrade");
    println!("running: {}", report.running_version);
    match &report.latest_release {
        Some(latest) => {
            println!("latest: {latest}");
            if let (Ok(running), Ok(latest)) = (
                Version::parse(&report.running_version),
                Version::parse(latest),
            ) && running < latest
            {
                if let Some(command) = self_upgrade_command(report.channel) {
                    println!("upgrade this binary with: {command}");
                }
            }
        }
        None => println!("latest: unknown (offline)"),
    }
    for install in &report.installs {
        println!(
            "{} [{}] v{} ({}, drift {})",
            install.root,
            install.harness,
            install.receipt_version.as_deref().unwrap_or("?"),
            state_str(install.state),
            install.drift_count,
        );
    }
    for root in &report.pruned {
        println!("{} pruned ({})", root.root, root.reason);
    }
    if !file_report.filed.is_empty() {
        println!("filed {} issue(s)", file_report.filed.len());
    }
    if !file_report.commented.is_empty() {
        println!("commented on {} issue(s)", file_report.commented.len());
    }
    if !file_report.skipped.is_empty() {
        println!(
            "skipped {} finding(s) (filing cap)",
            file_report.skipped.len()
        );
    }
    println!(
        "{} install(s), {} finding(s)",
        report.installs.len(),
        report.findings.len(),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::installer::manifest_db::{InstallReceipt, ReceiptRepository};

    fn save_receipt(root: &Path, harness: &str, version: &str) {
        let roots = match harness {
            "claude-code" => vec![".claude".to_string()],
            _ => vec![".opencode".to_string()],
        };
        let receipt = InstallReceipt::new(version, harness, "skills", roots, vec![]).unwrap();
        ReceiptRepository::new(root).save(&receipt).unwrap();
    }

    #[test]
    fn aggregate_exit_precedence() {
        assert_eq!(aggregate_exit(&[]), OK);
        assert_eq!(aggregate_exit(&[OK]), OK);
        assert_eq!(aggregate_exit(&[BUGS_FILED, OK]), BUGS_FILED);
        assert_eq!(
            aggregate_exit(&[AUDIT_FINDINGS, BUGS_FILED, OK]),
            BUGS_FILED
        );
        assert_eq!(
            aggregate_exit(&[UPGRADE_FAILED, AUDIT_FINDINGS, BUGS_FILED, OK]),
            UPGRADE_FAILED
        );
    }

    #[test]
    fn check_exit_mapping() {
        assert_eq!(check_exit(false, false), OK);
        assert_eq!(check_exit(true, false), UPGRADE_AVAILABLE);
        assert_eq!(check_exit(false, true), UNKNOWN);
        assert_eq!(check_exit(true, true), UNKNOWN);
    }

    #[test]
    fn collect_reports_over_roots_including_missing() {
        let dir = tempfile::tempdir().unwrap();
        let live = dir.path().join("live");
        std::fs::create_dir_all(&live).unwrap();
        save_receipt(&live, "claude-code", env!("CARGO_PKG_VERSION"));
        let missing = dir.path().join("gone"); // never created

        let source = CatalogSource::Embedded;
        let (roles, cmds, tools) = load_catalog(&source).unwrap();
        let reports =
            collect_reports(&[live.clone(), missing], &roles, &cmds, &tools, &source).unwrap();
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].root, live.to_string_lossy());
        assert_eq!(reports[0].harness, "claude-code");
    }

    #[test]
    fn refresh_root_with_no_receipts_spawns_nothing() {
        let dir = tempfile::tempdir().unwrap();
        // An empty tempdir has no receipts, so this returns before spawning the
        // current executable (which would otherwise hang/fail in a unit test).
        refresh_root(dir.path(), false).unwrap();
    }

    #[test]
    fn missing_root_is_never_fatal() {
        let missing = std::path::Path::new("/definitely/not/here/shipmates-538");
        // A deleted root (or a typo'd --dir) yields no harnesses instead of the
        // ENOTDIR that receipts_dir() would otherwise surface.
        assert!(receipt_harnesses(missing).unwrap().is_empty());
        // And a refresh of a missing root is a no-op that spawns nothing.
        refresh_root(missing, false).unwrap();
    }

    fn opts(overrides: impl FnOnce(&mut UpgradeOpts)) -> UpgradeOpts {
        let mut opts = UpgradeOpts {
            json: false,
            pre: false,
            dry_run: false,
            fix: false,
            dirs: vec![],
            file_bugs: false,
            self_upgrade: false,
            resume: false,
        };
        overrides(&mut opts);
        opts
    }

    #[test]
    fn resume_skips_the_upgrade_lock() {
        assert!(!should_acquire_lock(&opts(|o| o.resume = true)));
        assert!(should_acquire_lock(&opts(|_| {})));
        assert!(!should_acquire_lock(&opts(|o| o.dry_run = true)));
    }

    #[test]
    fn resume_skips_the_self_upgrade_branch() {
        assert!(!should_self_upgrade(
            &opts(|o| {
                o.self_upgrade = true;
                o.resume = true;
            }),
            false
        ));
        assert!(should_self_upgrade(&opts(|o| o.self_upgrade = true), false));
        assert!(!should_self_upgrade(&opts(|o| o.self_upgrade = true), true));
    }

    #[test]
    fn reexec_forwards_flags_but_never_self_or_dry_run() {
        let dirs = vec![PathBuf::from("/a"), PathBuf::from("/b")];
        let args = reexec_args(&opts(|o| {
            o.json = true;
            o.pre = true;
            o.fix = true;
            o.file_bugs = true;
            o.dirs = dirs.clone();
            o.self_upgrade = true;
            o.dry_run = true;
        }));
        assert_eq!(args[0], "upgrade");
        assert!(args.contains(&"--resume".to_string()));
        for flag in ["--json", "--pre", "--fix", "--file-bugs"] {
            assert!(args.contains(&flag.to_string()), "missing {flag}: {args:?}");
        }
        assert!(!args.contains(&"--self".to_string()));
        assert!(!args.contains(&"--dry-run".to_string()));
        let dir_positions: Vec<usize> = args
            .iter()
            .enumerate()
            .filter(|(_, a)| *a == "--dir")
            .map(|(i, _)| i)
            .collect();
        assert_eq!(dir_positions.len(), 2);
        for (idx, dir) in dirs.iter().enumerate() {
            assert_eq!(args[dir_positions[idx] + 1], dir.to_string_lossy());
        }
    }

    #[test]
    fn dry_run_reports_dead_roots_without_persisting() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("installs.json");
        let gone = dir.path().join("gone");
        let mut index = InstallsIndex::default();
        index
            .register(&path, &gone, "claude-code", "0.12.0", "skills")
            .unwrap();

        let pruned = prune_index(&opts(|o| o.dry_run = true), &mut index, &path).unwrap();
        assert_eq!(pruned.len(), 1);
        assert_eq!(pruned[0].root, gone.to_string_lossy());

        // The record survives on disk: dry-run reports, never persists.
        let reloaded = InstallsIndex::load(&path).unwrap();
        assert_eq!(reloaded.records.len(), 1);
        assert_eq!(reloaded.records[0].root, gone.to_string_lossy());
    }

    fn write_lock_file(path: &Path, age: Duration) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        let file = std::fs::File::create(path).unwrap();
        let modified = SystemTime::now() - age;
        file.set_times(std::fs::FileTimes::new().set_modified(modified))
            .unwrap();
    }

    #[test]
    fn stale_lock_is_taken_over() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("upgrade.lock");
        write_lock_file(&path, STALE_LOCK_AGE + Duration::from_secs(60));

        let guard = acquire_lock_at(&path).unwrap();
        assert!(path.exists());
        drop(guard);
        assert!(!path.exists());
    }

    #[test]
    fn fresh_lock_is_not_stale() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("upgrade.lock");
        write_lock_file(&path, Duration::from_secs(60));

        let error = acquire_lock_at(&path).unwrap_err();
        assert!(error.to_string().contains("already running"), "{error}");
        assert!(path.exists());
    }

    #[test]
    fn resolve_self_binary_prefers_path_lookup() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("shipmates");
        std::fs::write(&bin, "#!/bin/sh\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        }

        let path_var = std::ffi::OsStr::new(dir.path().to_str().unwrap());
        assert_eq!(resolve_self_binary_with(Some(path_var)), bin);
    }
}
