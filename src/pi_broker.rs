//! Pi resource-broker filesystem and process surface.
//!
//! Layout contract (advisory; fail safe on doubt) is owned by the JS side:
//! - `src/adapters/pi_crew/broker.mjs` (`brokerEndpoint`)
//! - `src/adapters/pi_crew/index.ts` (`startDaemon` / `takeLock`)
//!
//! This module never invents a second layout rule. Doctor only *reports*;
//! uninstall alone may *stop*. Windows returns `Unsupported` (the JS broker
//! already refuses to dial there).

use crate::doctor::{Check, Severity};
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::Duration;

const BROKER_DIR: &str = "shipmates-resource-broker";
const ENDPOINT_NAME: &str = "endpoint.json";
const DAEMON_MARKER: &str = "--shipmates-broker-daemon";
const PROTOCOL_VERSION: u32 = 1;
const STOP_WAIT_MS: u64 = 1500;
const STOP_POLL_MS: u64 = 50;

#[derive(Debug, Clone, Deserialize)]
pub struct Endpoint {
    #[serde(rename = "protocolVersion")]
    pub protocol_version: u32,
    #[serde(rename = "listenPath")]
    pub listen_path: String,
    pub pid: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Liveness {
    NoHome,
    NoEndpoint,
    Malformed(String),
    NotRunning,
    ForeignPid { pid: u32 },
    Live { pid: u32 },
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Teardown {
    Stopped { pid: u32 },
    NotRunning,
    NoEndpoint,
    NoHome,
    Malformed(String),
    RefusedForeignPid { pid: u32 },
    Unsupported,
}

impl Teardown {
    pub fn warning_line(&self) -> Option<String> {
        match self {
            Teardown::Stopped { pid } => {
                Some(format!("stopped pi resource broker (pid {pid}) and removed its runtime directory"))
            }
            Teardown::NotRunning => {
                Some("pi resource broker was not running; removed leftover runtime directory if present".into())
            }
            Teardown::NoEndpoint | Teardown::NoHome => None,
            Teardown::Malformed(msg) => {
                Some(format!("Warning: pi resource broker endpoint unreadable ({msg}); left process alone"))
            }
            Teardown::RefusedForeignPid { pid } => Some(format!(
                "Warning: refused to stop pid {pid}: it is not the shipmates resource broker"
            )),
            Teardown::Unsupported => {
                Some("Warning: pi resource broker teardown is not supported on this platform".into())
            }
        }
    }
}

/// Agent directory the pi extension actually uses.
/// Honours `PI_CODING_AGENT_DIR`, else `~/.pi/agent`.
pub fn agent_dir() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("PI_CODING_AGENT_DIR") {
        if !dir.is_empty() {
            return Some(PathBuf::from(dir));
        }
    }
    home::home_dir().map(|h| h.join(".pi").join("agent"))
}

pub fn broker_home(agent: &Path) -> PathBuf {
    agent.join(BROKER_DIR)
}

pub fn extension_path(agent: &Path) -> PathBuf {
    agent.join("extensions").join("shipmates-crew").join("index.ts")
}

pub fn extension_present(agent: &Path) -> bool {
    extension_path(agent).is_file()
}

pub fn read_endpoint(home: &Path) -> Result<Endpoint, String> {
    let path = home.join(ENDPOINT_NAME);
    let bytes = fs::read(&path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            "missing".to_string()
        } else {
            e.to_string()
        }
    })?;
    let endpoint: Endpoint = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    if endpoint.protocol_version != PROTOCOL_VERSION {
        return Err(format!(
            "unsupported protocolVersion {}",
            endpoint.protocol_version
        ));
    }
    if endpoint.pid <= 1 || endpoint.pid > u32::MAX as i64 {
        return Err(format!("invalid pid {}", endpoint.pid));
    }
    if endpoint.listen_path.is_empty() || endpoint.listen_path.contains('\0') {
        return Err("invalid listenPath".into());
    }
    Ok(endpoint)
}

fn pid_alive(pid: u32) -> bool {
    if pid <= 1 {
        return false;
    }
    // Our own process is never "the broker".
    if pid == std::process::id() {
        return false;
    }
    #[cfg(unix)]
    {
        Command::new("kill")
            .args(["-0", &pid.to_string()])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        false
    }
}

/// True only when argv/command line contains the daemon marker we pass at spawn.
fn is_our_broker(pid: u32) -> bool {
    #[cfg(unix)]
    {
        let output = Command::new("ps")
            .args(["-p", &pid.to_string(), "-o", "args="])
            .output();
        match output {
            Ok(out) if out.status.success() => {
                let args = String::from_utf8_lossy(&out.stdout);
                args.contains(DAEMON_MARKER)
            }
            _ => {
                // Fallback: try `command=` column (BusyBox / some BSDs).
                let output = Command::new("ps")
                    .args(["-p", &pid.to_string(), "-o", "command="])
                    .output();
                match output {
                    Ok(out) if out.status.success() => {
                        String::from_utf8_lossy(&out.stdout).contains(DAEMON_MARKER)
                    }
                    _ => false,
                }
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        false
    }
}

pub fn liveness(home: &Path) -> Liveness {
    #[cfg(not(unix))]
    {
        let _ = home;
        return Liveness::Unsupported;
    }
    #[cfg(unix)]
    {
        if !home.exists() {
            return Liveness::NoHome;
        }
        let endpoint_path = home.join(ENDPOINT_NAME);
        if !endpoint_path.exists() {
            // Stale lock alone is not enough to act on.
            return Liveness::NoEndpoint;
        }
        match read_endpoint(home) {
            Err(msg) if msg == "missing" => Liveness::NoEndpoint,
            Err(msg) => Liveness::Malformed(msg),
            Ok(endpoint) => {
                let pid = endpoint.pid as u32;
                if !pid_alive(pid) {
                    return Liveness::NotRunning;
                }
                if is_our_broker(pid) {
                    Liveness::Live { pid }
                } else {
                    Liveness::ForeignPid { pid }
                }
            }
        }
    }
}

pub fn remove_home(home: &Path) {
    let _ = fs::remove_dir_all(home);
}

fn signal_term(pid: u32) -> bool {
    #[cfg(unix)]
    {
        Command::new("kill")
            .args(["-TERM", &pid.to_string()])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        false
    }
}

fn wait_until_dead(pid: u32) -> bool {
    let steps = (STOP_WAIT_MS / STOP_POLL_MS).max(1);
    for _ in 0..steps {
        if !pid_alive(pid) {
            return true;
        }
        thread::sleep(Duration::from_millis(STOP_POLL_MS));
    }
    !pid_alive(pid)
}

/// Stop the broker when identity is certain. Never escalate past TERM.
pub fn stop(home: &Path) -> Teardown {
    match liveness(home) {
        Liveness::NoHome => Teardown::NoHome,
        Liveness::NoEndpoint => {
            // Clean residue if any.
            if home.exists() {
                remove_home(home);
            }
            Teardown::NoEndpoint
        }
        Liveness::Malformed(msg) => Teardown::Malformed(msg),
        Liveness::NotRunning => {
            remove_home(home);
            Teardown::NotRunning
        }
        Liveness::ForeignPid { pid } => Teardown::RefusedForeignPid { pid },
        Liveness::Unsupported => Teardown::Unsupported,
        Liveness::Live { pid } => {
            if !signal_term(pid) {
                return Teardown::Malformed(format!("failed to signal pid {pid}"));
            }
            if !wait_until_dead(pid) {
                return Teardown::Malformed(format!(
                    "pid {pid} did not exit after TERM; left runtime directory in place"
                ));
            }
            remove_home(home);
            Teardown::Stopped { pid }
        }
    }
}

/// Tear down only when no pi extension remains at the agent dir.
/// Project-scope uninstall must not kill a broker a global install still needs.
pub fn teardown_after_uninstall() -> Teardown {
    let Some(agent) = agent_dir() else {
        return Teardown::NoHome;
    };
    if extension_present(&agent) {
        // Another install still owns the extension — leave the machine-wide broker.
        return Teardown::NoEndpoint;
    }
    let home = broker_home(&agent);
    stop(&home)
}

/// Doctor checks: unconditional, Warn-only, never fixable.
pub fn checks() -> Vec<Check> {
    let mut out = Vec::new();
    let Some(agent) = agent_dir() else {
        return out;
    };
    let home = broker_home(&agent);
    let has_extension = extension_present(&agent);
    match liveness(&home) {
        Liveness::Live { pid } if !has_extension => out.push(Check {
            name: "Pi resource broker".into(),
            severity: Severity::Warn,
            detail: format!(
                "live resource broker (pid {pid}) with no pi extension installed; run `shipmates uninstall --harness pi` once more, or remove {} and stop the process",
                home.display()
            ),
            fixable: false,
        }),
        Liveness::Live { pid } => out.push(Check {
            name: "Pi resource broker".into(),
            severity: Severity::Ok,
            detail: format!("resource broker is live (pid {pid})"),
            fixable: false,
        }),
        Liveness::ForeignPid { pid } => out.push(Check {
            name: "Pi resource broker".into(),
            severity: Severity::Warn,
            detail: format!(
                "endpoint names pid {pid}, which is not the shipmates resource broker; left alone"
            ),
            fixable: false,
        }),
        Liveness::Malformed(msg) => out.push(Check {
            name: "Pi resource broker".into(),
            severity: Severity::Warn,
            detail: format!("resource broker endpoint unreadable: {msg}"),
            fixable: false,
        }),
        Liveness::NotRunning if home.exists() => out.push(Check {
            name: "Pi resource broker".into(),
            severity: Severity::Warn,
            detail: format!(
                "stale resource broker directory at {} (process not running)",
                home.display()
            ),
            fixable: false,
        }),
        _ => {}
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    fn temp_home() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join(BROKER_DIR);
        fs::create_dir_all(&home).unwrap();
        (dir, home)
    }

    #[test]
    fn no_endpoint_is_safe() {
        let (_dir, home) = temp_home();
        assert_eq!(liveness(&home), Liveness::NoEndpoint);
        assert_eq!(stop(&home), Teardown::NoEndpoint);
        assert!(!home.exists());
    }

    #[test]
    fn malformed_endpoint_refuses_kill() {
        let (_dir, home) = temp_home();
        fs::write(home.join(ENDPOINT_NAME), b"{not-json").unwrap();
        match liveness(&home) {
            Liveness::Malformed(_) => {}
            other => panic!("expected Malformed, got {other:?}"),
        }
        match stop(&home) {
            Teardown::Malformed(_) => {}
            other => panic!("expected Malformed, got {other:?}"),
        }
        // Home must still exist — we refused to act.
        assert!(home.exists());
    }

    #[test]
    fn stale_pid_cleans_home() {
        let (_dir, home) = temp_home();
        let endpoint = serde_json::json!({
            "protocolVersion": 1,
            "listenPath": home.join("broker.sock").to_string_lossy(),
            // PID 1 is rejected by read_endpoint as invalid; use a high unused pid.
            "pid": 2_147_000_000i64,
        });
        // Actually pid must be valid range but not alive. Use max-ish that won't be us.
        let endpoint = serde_json::json!({
            "protocolVersion": 1,
            "listenPath": "/tmp/shipmates-test-broker.sock",
            "pid": 999_999,
        });
        let _ = endpoint;
        let endpoint = serde_json::json!({
            "protocolVersion": 1,
            "listenPath": "/tmp/shipmates-test-broker.sock",
            "pid": 999999,
        });
        fs::write(home.join(ENDPOINT_NAME), endpoint.to_string()).unwrap();
        assert_eq!(liveness(&home), Liveness::NotRunning);
        assert_eq!(stop(&home), Teardown::NotRunning);
        assert!(!home.exists());
    }

    #[test]
    fn foreign_live_pid_is_refused() {
        let (_dir, home) = temp_home();
        // Use our own parent-ish: current process is never "our broker" (no marker).
        // pid_alive(self) is false by design, so pick init? No — init is pid 1 invalid.
        // Spawn a short-lived sleep and use that pid.
        #[cfg(unix)]
        {
            let child = Command::new("sleep")
                .arg("30")
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .expect("spawn sleep");
            let pid = child.id();
            let endpoint = serde_json::json!({
                "protocolVersion": 1,
                "listenPath": "/tmp/shipmates-test-broker.sock",
                "pid": pid,
            });
            fs::write(home.join(ENDPOINT_NAME), endpoint.to_string()).unwrap();
            assert_eq!(liveness(&home), Liveness::ForeignPid { pid });
            assert_eq!(stop(&home), Teardown::RefusedForeignPid { pid });
            assert!(home.exists());
            assert!(pid_alive(pid), "foreign process must still be alive");
            let _ = Command::new("kill")
                .args(["-TERM", &pid.to_string()])
                .status();
            let _ = child;
        }
    }

    #[test]
    fn unsupported_protocol_version_is_malformed() {
        let (_dir, home) = temp_home();
        let endpoint = serde_json::json!({
            "protocolVersion": 99,
            "listenPath": "/tmp/x.sock",
            "pid": 12345,
        });
        fs::write(home.join(ENDPOINT_NAME), endpoint.to_string()).unwrap();
        match liveness(&home) {
            Liveness::Malformed(msg) => assert!(msg.contains("protocolVersion")),
            other => panic!("expected Malformed, got {other:?}"),
        }
    }

    #[test]
    fn agent_dir_honours_env() {
        let dir = tempfile::tempdir().unwrap();
        let custom = dir.path().join("custom-agent");
        fs::create_dir_all(&custom).unwrap();
        // SAFETY: test process is single-threaded for this env poke.
        unsafe {
            std::env::set_var("PI_CODING_AGENT_DIR", &custom);
        }
        assert_eq!(agent_dir().as_deref(), Some(custom.as_path()));
        unsafe {
            std::env::remove_var("PI_CODING_AGENT_DIR");
        }
    }

    #[test]
    fn teardown_skips_when_extension_present() {
        let dir = tempfile::tempdir().unwrap();
        let agent = dir.path().join("agent");
        let ext = extension_path(&agent);
        fs::create_dir_all(ext.parent().unwrap()).unwrap();
        fs::write(&ext, b"// present\n").unwrap();
        let home = broker_home(&agent);
        fs::create_dir_all(&home).unwrap();
        let endpoint = serde_json::json!({
            "protocolVersion": 1,
            "listenPath": "/tmp/x.sock",
            "pid": 999999,
        });
        fs::write(home.join(ENDPOINT_NAME), endpoint.to_string()).unwrap();
        unsafe {
            std::env::set_var("PI_CODING_AGENT_DIR", &agent);
        }
        // Extension present → no teardown.
        assert_eq!(teardown_after_uninstall(), Teardown::NoEndpoint);
        assert!(home.exists());
        // Remove extension → cleans stale.
        fs::remove_file(&ext).unwrap();
        assert_eq!(teardown_after_uninstall(), Teardown::NotRunning);
        assert!(!home.exists());
        unsafe {
            std::env::remove_var("PI_CODING_AGENT_DIR");
        }
    }
}
