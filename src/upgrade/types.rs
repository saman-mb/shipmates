//! Shared types for the `shipmates status` / `shipmates upgrade` surface.
//!
//! These are the JSON-stable shapes the commands emit; field names are part of
//! the public contract and are asserted by the CLI tests, so rename with care.

use serde::{Deserialize, Serialize};

/// How this running binary was installed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Channel {
    Brew,
    Cargo,
    CargoDist,
    Source,
    Unknown,
}

/// Health classification of a single install.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum InstallState {
    Ok,
    Drift,
    MissingReceipt,
    Moved,
    CorruptReceipt,
    Unmanaged,
}

/// Per-install health as reported by `status` / `upgrade`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstallReport {
    pub root: String,
    pub harness: String,
    pub receipt_version: Option<String>,
    pub layout: Option<String>,
    pub managed: bool,
    pub drift_count: usize,
    pub tools: Vec<String>,
    pub state: InstallState,
}

/// A root that has no install receipt and is therefore left alone.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnmanagedRoot {
    pub root: String,
    pub reason: String,
}

/// A stale index record whose root directory no longer exists.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrunedRoot {
    pub root: String,
    pub reason: String,
}

/// The `shipmates status` payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusReport {
    pub shipmates_version: String,
    pub installs: Vec<InstallReport>,
    pub unmanaged: Vec<UnmanagedRoot>,
    pub pruned: Vec<PrunedRoot>,
}

/// The `shipmates upgrade --check` payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckReport {
    pub latest_release: Option<String>,
    pub running_version: String,
    pub upgrade_available: bool,
    pub running_is_newer: bool,
    pub unknown: bool,
    pub pre: bool,
    pub channel: Channel,
    pub installs: Vec<InstallReport>,
}

/// The issue classification table's finding classes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FindingClass {
    RefreshFailed,
    UnrepairableDrift,
    CorruptReceipt,
    PostUpgradeMismatch,
    ToolFailure,
}

/// One attributable post-upgrade finding.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Finding {
    pub class: FindingClass,
    pub harness: String,
    pub root: String,
    pub detail: String,
    pub fingerprint: String,
}

// Exit codes, chosen to avoid anyhow's `1` and clap's usage `2`.
pub const OK: i32 = 0;
pub const ERROR: i32 = 1;
pub const USAGE: i32 = 2;
pub const UPGRADE_FAILED: i32 = 3;
pub const AUDIT_FINDINGS: i32 = 4;
pub const BUGS_FILED: i32 = 5;
pub const UPGRADE_AVAILABLE: i32 = 10;
pub const UNKNOWN: i32 = 11;
