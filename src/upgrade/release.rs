//! Release feed lookup and install-channel detection for `shipmates upgrade`.

use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Command;

use semver::Version;
use serde::Deserialize;

use crate::upgrade::types::Channel;

/// Default GitHub release feed, overridable with `SHIPMATES_RELEASES_URL`.
const DEFAULT_RELEASES_URL: &str =
    "https://api.github.com/repos/saman-mb/shipmates/releases?per_page=20";

/// One release, as fetched from the GitHub releases feed.
#[derive(Debug, Clone, PartialEq)]
pub struct ReleaseInfo {
    pub version: String,
    pub prerelease: bool,
    pub url: String,
}

/// Every network and parse failure, typed so callers can downgrade gracefully.
#[derive(Debug)]
pub enum ReleaseError {
    Offline(String),
    Http(String),
    Parse(String),
}

impl fmt::Display for ReleaseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ReleaseError::Offline(msg) => write!(f, "offline: {msg}"),
            ReleaseError::Http(msg) => write!(f, "http error: {msg}"),
            ReleaseError::Parse(msg) => write!(f, "parse error: {msg}"),
        }
    }
}

impl std::error::Error for ReleaseError {}

/// Shape of one element in the GitHub releases JSON array.
#[derive(Debug, Deserialize)]
struct RawRelease {
    draft: bool,
    prerelease: bool,
    tag_name: String,
    html_url: String,
}

/// Newest stable release (or newest including prereleases when `pre`).
///
/// The feed is the GitHub releases array; ordering is by `semver::Version`, not
/// array order, and drafts are always skipped.
///
/// The transport is `curl` from `PATH` (Windows 10+ ships it too), not an HTTP
/// crate: this is one unauthenticated GET, and a full TLS stack was deliberately
/// pruned from the dependency closure (#496). A missing `curl`, a network
/// failure, or a non-2xx status all map to a typed `ReleaseError` so the caller
/// can degrade to "version unknown" instead of failing the run.
/// `SHIPMATES_CURL` overrides the binary for tests.
pub fn fetch_latest(pre: bool) -> Result<ReleaseInfo, ReleaseError> {
    let url = std::env::var("SHIPMATES_RELEASES_URL")
        .unwrap_or_else(|_| DEFAULT_RELEASES_URL.to_string());
    let curl = std::env::var("SHIPMATES_CURL").unwrap_or_else(|_| "curl".to_string());
    let user_agent = format!("shipmates/{}", env!("CARGO_PKG_VERSION"));
    let output = std::process::Command::new(&curl)
        .args([
            "-fsSL",
            "--max-time",
            "15",
            "-H",
            &format!("User-Agent: {user_agent}"),
            &url,
        ])
        .output()
        .map_err(|e| ReleaseError::Offline(format!("running {curl}: {e}")))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(ReleaseError::Http(format!(
            "{curl} {}: {}",
            output.status,
            stderr.trim()
        )));
    }
    let body = String::from_utf8(output.stdout)
        .map_err(|e| ReleaseError::Http(format!("release feed is not UTF-8: {e}")))?;
    select_latest(&body, pre)
}

/// Pick the highest release from a raw JSON body, without touching the network.
///
/// Skips drafts, skips prereleases unless `pre`, and orders by `semver::Version`
/// so a prerelease such as `0.4.0-rc.1` correctly ranks below `0.4.0`.
pub(crate) fn select_latest(body: &str, pre: bool) -> Result<ReleaseInfo, ReleaseError> {
    let releases: Vec<RawRelease> = serde_json::from_str(body)
        .map_err(|e| ReleaseError::Parse(format!("invalid release feed: {e}")))?;

    let mut best: Option<(Version, RawRelease)> = None;
    for release in releases {
        if release.draft {
            continue;
        }
        if release.prerelease && !pre {
            continue;
        }
        let version_str = release
            .tag_name
            .trim_start_matches('v')
            .trim_start_matches('V');
        let version = Version::parse(version_str)
            .map_err(|e| ReleaseError::Parse(format!("unparseable tag {version_str}: {e}")))?;
        match &best {
            Some((current, _)) if *current >= version => {}
            _ => best = Some((version, release)),
        }
    }

    let (version, release) =
        best.ok_or_else(|| ReleaseError::Parse("no usable releases in feed".to_string()))?;
    Ok(ReleaseInfo {
        prerelease: !version.pre.is_empty(),
        version: version.to_string(),
        url: release.html_url,
    })
}

/// Classify how this binary was installed, per the ordered channel list.
///
/// Path evidence wins before any external check: a Cellar path is Brew, a
/// `~/.cargo/bin` path is Cargo, then a cargo-dist receipt, then the
/// `brew list` fallback, then a source tree. The two external checks are
/// passed in as booleans so the ordering is unit-testable without invoking
/// `brew` or touching the receipt filesystem.
pub fn detect_channel() -> Channel {
    let home = home::home_dir().unwrap_or_default();
    let exe = std::env::current_exe().unwrap_or_default();
    detect_channel_from(
        &exe,
        &home,
        cargo_dist_receipt_exists(),
        brew_list_has_shipmates(),
    )
}

/// Pure, ordered channel decision: path evidence, cargo-dist receipt, then the
/// `brew list` fallback, then a source tree.
pub(crate) fn detect_channel_from(
    path: &Path,
    home: &Path,
    receipt_exists: bool,
    brew_present: bool,
) -> Channel {
    let path_channel = classify_exe_path(path, home);
    if matches!(path_channel, Channel::Brew | Channel::Cargo) {
        return path_channel;
    }
    if receipt_exists {
        return Channel::CargoDist;
    }
    if brew_present {
        return Channel::Brew;
    }
    path_channel // Source or Unknown
}

/// Pure, path-only classification: the branches that never shell out to `brew`
/// or touch the cargo-dist receipt. `detect_channel_from` composes this with the
/// two external checks at the contract's exact order.
pub(crate) fn classify_exe_path(path: &Path, home: &Path) -> Channel {
    if is_cellar(path) {
        return Channel::Brew;
    }
    if under_cargo_bin(path, home) {
        return Channel::Cargo;
    }
    if under_target_dir(path) || source_tree(path) {
        return Channel::Source;
    }
    Channel::Unknown
}

/// Exact command a captain would run, for the human report. `None` for Source/Unknown.
pub fn self_upgrade_command(channel: Channel) -> Option<String> {
    match channel {
        Channel::Brew => Some("brew upgrade shipmates".to_string()),
        Channel::Cargo => Some("cargo install shipmates --locked".to_string()),
        Channel::CargoDist => Some(
            "curl --proto '=https' --tlsv1.2 -LsSf https://github.com/saman-mb/shipmates/releases/latest/download/shipmates-installer.sh | sh"
                .to_string(),
        ),
        Channel::Source | Channel::Unknown => None,
    }
}

/// Whether `--self` may execute the upgrade command (Brew, CargoDist).
pub fn can_exec_self_upgrade(channel: Channel) -> bool {
    matches!(channel, Channel::Brew | Channel::CargoDist)
}

/// Human-readable channel name for the report.
pub fn describe_channel(channel: Channel) -> &'static str {
    match channel {
        Channel::Brew => "brew",
        Channel::Cargo => "cargo",
        Channel::CargoDist => "cargo-dist",
        Channel::Source => "source",
        Channel::Unknown => "unknown",
    }
}

fn is_cellar(path: &Path) -> bool {
    path.to_string_lossy().contains("/Cellar/shipmates/")
}

fn under_cargo_bin(path: &Path, home: &Path) -> bool {
    path.starts_with(home.join(".cargo").join("bin"))
}

fn under_target_dir(path: &Path) -> bool {
    let s = path.to_string_lossy();
    s.contains("/target/debug/") || s.contains("/target/release/")
}

fn brew_list_has_shipmates() -> bool {
    Command::new("brew")
        .args(["list", "--formula", "shipmates"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn cargo_dist_receipt_exists() -> bool {
    cargo_dist_data_dirs()
        .into_iter()
        .any(|dir| dir.join("shipmates").join("receipt.txt").is_file())
}

/// Platform data directories the cargo-dist installer may have written to,
/// checked in precedence order: `$XDG_DATA_HOME`, `~/.local/share`, macOS
/// `~/Library/Application Support`, and Windows `%APPDATA%`. The location is
/// best-effort: the file-existence signal is the only check, and an absent
/// receipt refuses safely (the channel falls through to the remaining evidence).
fn cargo_dist_data_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(xdg) = std::env::var_os("XDG_DATA_HOME").filter(|v| !v.is_empty()) {
        dirs.push(PathBuf::from(xdg));
    }
    if let Some(home) = home::home_dir() {
        dirs.push(home.join(".local").join("share"));
        #[cfg(target_os = "macos")]
        dirs.push(home.join("Library").join("Application Support"));
    }
    #[cfg(target_os = "windows")]
    if let Some(appdata) = std::env::var_os("APPDATA").filter(|v| !v.is_empty()) {
        dirs.push(PathBuf::from(appdata));
    }
    dirs
}

/// Walk the exe's ancestors looking for a `Cargo.toml` that names `shipmates`.
fn source_tree(path: &Path) -> bool {
    let mut dir = path.parent();
    while let Some(d) = dir {
        if cargo_toml_declares_shipmates(&d.join("Cargo.toml")) {
            return true;
        }
        dir = d.parent();
    }
    false
}

fn cargo_toml_declares_shipmates(cargo_toml: &Path) -> bool {
    let Ok(contents) = std::fs::read_to_string(cargo_toml) else {
        return false;
    };
    contents.lines().any(|line| {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("name") else {
            return false;
        };
        let rest = rest.trim_start();
        let Some(rest) = rest.strip_prefix('=') else {
            return false;
        };
        let value = rest.trim();
        value == "\"shipmates\"" || value == "'shipmates'"
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(tag: &str, draft: bool, prerelease: bool) -> String {
        format!(
            r#"{{"tag_name":"{tag}","draft":{draft},"prerelease":{prerelease},"html_url":"https://github.com/saman-mb/shipmates/releases/tag/{tag}"}}"#
        )
    }

    fn feed(entries: &[String]) -> String {
        format!("[{}]", entries.join(","))
    }

    #[test]
    fn select_latest_picks_highest_stable() {
        let body = feed(&[
            release("v0.3.1", false, false),
            release("v0.4.0", false, false),
            release("v0.2.0", false, false),
        ]);
        let info = select_latest(&body, false).unwrap();
        assert_eq!(info.version, "0.4.0");
        assert!(!info.prerelease);
    }

    #[test]
    fn select_latest_is_not_array_order_dependent() {
        let body = feed(&[
            release("v2.0.0", false, false),
            release("v1.0.0", false, false),
            release("v1.5.0", false, false),
        ]);
        let info = select_latest(&body, false).unwrap();
        assert_eq!(info.version, "2.0.0");
    }

    #[test]
    fn select_latest_skips_drafts() {
        let body = feed(&[
            release("v0.3.0", false, false),
            release("v0.4.0", true, false),
        ]);
        let info = select_latest(&body, false).unwrap();
        assert_eq!(info.version, "0.3.0");
    }

    #[test]
    fn select_latest_skips_prereleases_unless_asked() {
        let body = feed(&[
            release("v0.4.0", false, false),
            release("v0.5.0-rc.1", false, true),
        ]);
        let stable = select_latest(&body, false).unwrap();
        assert_eq!(stable.version, "0.4.0");
        let with_pre = select_latest(&body, true).unwrap();
        assert_eq!(with_pre.version, "0.5.0-rc.1");
        assert!(with_pre.prerelease);
    }

    #[test]
    fn semver_prerelease_ranks_below_release() {
        let rc = Version::parse("0.4.0-rc.1").unwrap();
        let stable = Version::parse("0.4.0").unwrap();
        assert!(rc < stable);

        // And through the feed path: when a prerelease and a release share a
        // version number, the release wins without `pre`.
        let body = feed(&[
            release("v0.4.0-rc.1", false, true),
            release("v0.4.0", false, false),
        ]);
        let info = select_latest(&body, false).unwrap();
        assert_eq!(info.version, "0.4.0");
    }

    #[test]
    fn select_latest_rejects_garbage() {
        assert!(matches!(
            select_latest("not json", false),
            Err(ReleaseError::Parse(_))
        ));
        assert!(matches!(
            select_latest("[]", false),
            Err(ReleaseError::Parse(_))
        ));
    }

    #[test]
    fn classify_exe_path_cellar_is_brew() {
        let home = Path::new("/Users/me");
        let exe = Path::new("/opt/homebrew/Cellar/shipmates/0.12.0/bin/shipmates");
        assert_eq!(classify_exe_path(exe, home), Channel::Brew);
    }

    #[test]
    fn classify_exe_path_cargo_bin_is_cargo() {
        let home = Path::new("/Users/me");
        let exe = Path::new("/Users/me/.cargo/bin/shipmates");
        assert_eq!(classify_exe_path(exe, home), Channel::Cargo);
    }

    #[test]
    fn classify_exe_path_target_release_is_source() {
        let home = Path::new("/Users/me");
        let exe = Path::new("/Users/me/Dev/shipmates/target/release/shipmates");
        assert_eq!(classify_exe_path(exe, home), Channel::Source);
    }

    #[test]
    fn classify_exe_path_source_tree_is_source() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"shipmates\"\nversion = \"0.12.0\"\n",
        )
        .unwrap();
        let exe = dir.path().join("shipmates");
        assert_eq!(classify_exe_path(&exe, dir.path()), Channel::Source);
    }

    #[test]
    fn classify_exe_path_unknown_is_unknown() {
        let home = Path::new("/Users/me");
        let exe = Path::new("/usr/local/bin/shipmates");
        assert_eq!(classify_exe_path(exe, home), Channel::Unknown);
    }

    #[test]
    fn classify_exe_path_ignores_unrelated_cargo_toml() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"other-crate\"\nversion = \"1.0.0\"\n",
        )
        .unwrap();
        let exe = dir.path().join("bin").join("shipmates");
        assert_eq!(classify_exe_path(&exe, dir.path()), Channel::Unknown);
    }

    #[test]
    fn channel_detection_prefers_cargo_path_over_brew_present() {
        // Path evidence wins: a cargo-bin path is Cargo even when the `brew list`
        // fallback would say Brew (no real brew call happens here).
        let home = Path::new("/Users/me");
        let exe = Path::new("/Users/me/.cargo/bin/shipmates");
        assert_eq!(detect_channel_from(exe, home, false, true), Channel::Cargo);
    }

    #[test]
    fn channel_detection_prefers_cellar_over_everything() {
        let home = Path::new("/Users/me");
        let exe = Path::new("/opt/homebrew/Cellar/shipmates/0.12.0/bin/shipmates");
        assert_eq!(detect_channel_from(exe, home, true, false), Channel::Brew);
    }

    #[test]
    fn channel_detection_receipt_before_brew_fallback() {
        let home = Path::new("/Users/me");
        let exe = Path::new("/usr/local/bin/shipmates");
        assert_eq!(
            detect_channel_from(exe, home, true, true),
            Channel::CargoDist
        );
    }

    #[test]
    fn channel_detection_brew_fallback_before_source() {
        let home = Path::new("/Users/me");
        let exe = Path::new("/usr/local/bin/shipmates");
        assert_eq!(detect_channel_from(exe, home, false, true), Channel::Brew);
    }

    #[test]
    fn channel_detection_source_is_last() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"shipmates\"\nversion = \"0.12.0\"\n",
        )
        .unwrap();
        let exe = dir.path().join("shipmates");
        assert_eq!(
            detect_channel_from(&exe, dir.path(), false, false),
            Channel::Source
        );
    }

    #[test]
    fn self_upgrade_command_matches_channel() {
        assert_eq!(
            self_upgrade_command(Channel::Brew).as_deref(),
            Some("brew upgrade shipmates")
        );
        assert_eq!(
            self_upgrade_command(Channel::Cargo).as_deref(),
            Some("cargo install shipmates --locked")
        );
        let dist = self_upgrade_command(Channel::CargoDist).unwrap();
        assert!(dist.contains("shipmates-installer.sh"));
        assert_eq!(self_upgrade_command(Channel::Source), None);
        assert_eq!(self_upgrade_command(Channel::Unknown), None);
    }

    #[test]
    fn can_exec_self_upgrade_only_brew_and_cargodist() {
        assert!(can_exec_self_upgrade(Channel::Brew));
        assert!(can_exec_self_upgrade(Channel::CargoDist));
        assert!(!can_exec_self_upgrade(Channel::Cargo));
        assert!(!can_exec_self_upgrade(Channel::Source));
        assert!(!can_exec_self_upgrade(Channel::Unknown));
    }

    #[test]
    fn describe_channel_returns_human_names() {
        assert_eq!(describe_channel(Channel::Brew), "brew");
        assert_eq!(describe_channel(Channel::Cargo), "cargo");
        assert_eq!(describe_channel(Channel::CargoDist), "cargo-dist");
        assert_eq!(describe_channel(Channel::Source), "source");
        assert_eq!(describe_channel(Channel::Unknown), "unknown");
    }
}
