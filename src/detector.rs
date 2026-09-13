//! Automated harness detection — identify which coding harnesses are present on
//! the user's system via CLI binaries on PATH, home configuration directories,
//! project-level trees, and existing install receipts (#438).

use crate::adapters;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HarnessDetection {
    pub harness: String,
    pub reasons: Vec<String>,
}

/// Check if a binary filename exists in any directory on `PATH`.
pub fn binary_exists_on_path(binary: &str) -> bool {
    let Some(path_var) = std::env::var_os("PATH") else {
        return false;
    };
    for dir in std::env::split_paths(&path_var) {
        let candidate = dir.join(binary);
        if candidate.is_file() {
            return true;
        }
    }
    false
}

/// Detect project-level markers and receipts in `target_dir`.
pub fn detect_project_harnesses(dir: &Path) -> Vec<HarnessDetection> {
    let mut detections: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let supported = adapters::targets();

    let project_dirs: &[(&str, &[&str])] = &[
        ("claude-code", &[".claude"]),
        ("opencode", &[".opencode"]),
        ("antigravity", &[".agents/agents", ".agents"]),
        ("codex", &[".codex"]),
        ("cursor", &[".cursor"]),
        ("github-copilot", &[".github/agents", ".github"]),
        ("pi", &[".pi"]),
        ("windsurf", &[".windsurf"]),
    ];

    for (harness, rel_paths) in project_dirs {
        if !supported.contains(harness) {
            continue;
        }
        for rel in *rel_paths {
            let path = dir.join(rel);
            if path.is_dir() {
                detections
                    .entry((*harness).to_string())
                    .or_default()
                    .push(format!("project directory {}/ exists", rel));
                break;
            }
        }
    }

    // Project receipts
    let manifest_dir = dir.join(".shipmates").join("manifest");
    if manifest_dir.is_dir() {
        for target in &supported {
            if manifest_dir.join(format!("{}.json", target)).is_file() {
                detections
                    .entry((*target).to_string())
                    .or_default()
                    .push("local shipmates install receipt present".to_string());
            }
        }
    }

    detections
        .into_iter()
        .map(|(harness, reasons)| HarnessDetection { harness, reasons })
        .collect()
}

/// Detect all installed coding harnesses.
///
/// Looks at:
/// 1. Binaries on PATH (`claude`, `opencode`, `agy`, `codex`, `cursor`, `copilot`, `pi`, `windsurf`)
/// 2. User-scope config dirs in home directory (`~/.claude/`, `~/.codex/`, `~/.gemini/`, etc.)
/// 3. Project markers in `target_dir` (`.claude/`, `.opencode/`, `.codex/`, `.agents/`, etc.)
/// 4. Shipmates install receipts in `target_dir` and home.
pub fn detect_harnesses(target_dir: Option<&Path>) -> Vec<HarnessDetection> {
    let home = home::home_dir();
    let mut detections: BTreeMap<String, Vec<String>> = BTreeMap::new();

    let supported = adapters::targets();

    // 1. Binaries on PATH
    let binary_mappings: &[(&str, &[&str])] = &[
        ("claude-code", &["claude"]),
        ("opencode", &["opencode"]),
        ("antigravity", &["agy", "antigravity"]),
        ("codex", &["codex"]),
        ("cursor", &["cursor"]),
        ("github-copilot", &["copilot"]),
        ("pi", &["pi"]),
        ("windsurf", &["windsurf"]),
    ];

    for (harness, binaries) in binary_mappings {
        if !supported.contains(harness) {
            continue;
        }
        for binary in *binaries {
            if binary_exists_on_path(binary) {
                detections
                    .entry((*harness).to_string())
                    .or_default()
                    .push(format!("binary '{}' found on PATH", binary));
                break;
            }
        }
    }

    // 2. Global home directories
    if let Some(ref home_path) = home {
        let home_dirs: &[(&str, &[&str])] = &[
            ("claude-code", &[".claude"]),
            ("opencode", &[".config/opencode", ".opencode"]),
            ("antigravity", &[".gemini", ".antigravity"]),
            ("codex", &[".codex"]),
            ("cursor", &[".cursor"]),
            ("github-copilot", &[".config/github-copilot", ".github"]),
            ("pi", &[".pi"]),
            ("windsurf", &[".windsurf", ".codeium/windsurf"]),
        ];

        for (harness, rel_paths) in home_dirs {
            if !supported.contains(harness) {
                continue;
            }
            for rel in *rel_paths {
                let path = home_path.join(rel);
                if path.is_dir() {
                    detections
                        .entry((*harness).to_string())
                        .or_default()
                        .push(format!("user configuration directory ~/{} exists", rel));
                    break;
                }
            }
        }

        // Global receipts
        let global_manifest = home_path.join(".shipmates").join("manifest");
        if global_manifest.is_dir() {
            for target in &supported {
                if global_manifest.join(format!("{}.json", target)).is_file() {
                    detections
                        .entry((*target).to_string())
                        .or_default()
                        .push("global shipmates install receipt present".to_string());
                }
            }
        }
    }

    // 3. Project-level markers & receipts in target_dir
    if let Some(dir) = target_dir {
        for proj in detect_project_harnesses(dir) {
            detections
                .entry(proj.harness)
                .or_default()
                .extend(proj.reasons);
        }
    }

    detections
        .into_iter()
        .map(|(harness, reasons)| HarnessDetection { harness, reasons })
        .collect()
}

/// Detect names of all installed coding harnesses (sorted).
pub fn detect_installed_harness_names(target_dir: Option<&Path>) -> Vec<String> {
    let mut names: BTreeSet<String> = detect_harnesses(target_dir)
        .into_iter()
        .map(|d| d.harness)
        .collect();

    // Preserve deterministic ordering matching adapters::targets()
    adapters::targets()
        .into_iter()
        .filter(|t| names.remove(*t))
        .map(String::from)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_detect_project_dirs() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join(".claude")).unwrap();
        fs::create_dir_all(dir.path().join(".opencode")).unwrap();

        let detected = detect_project_harnesses(dir.path());
        let names: Vec<String> = detected.into_iter().map(|d| d.harness).collect();
        assert!(names.contains(&"claude-code".to_string()));
        assert!(names.contains(&"opencode".to_string()));
        assert!(!names.contains(&"codex".to_string()));
    }

    #[test]
    fn test_detect_receipts() {
        let dir = tempfile::tempdir().unwrap();
        let manifest_dir = dir.path().join(".shipmates").join("manifest");
        fs::create_dir_all(&manifest_dir).unwrap();
        fs::write(manifest_dir.join("cursor.json"), "{}").unwrap();

        let detected = detect_project_harnesses(dir.path());
        let names: Vec<String> = detected.into_iter().map(|d| d.harness).collect();
        assert!(names.contains(&"cursor".to_string()));
        assert!(!names.contains(&"claude-code".to_string()));
    }
}
