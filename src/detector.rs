//! Automated harness detection — identify which coding harnesses are present on
//! the user's system via CLI binaries on PATH, home configuration directories,
//! project-level trees, and existing install receipts (#438).
//!
//! Detection is a **hint**, never an install authority (#489). Markers must be
//! specific enough that a bare GitHub repo (`.github/workflows`), a shared
//! `.agents/skills` tree, or a leftover `~/.github` cannot masquerade as an
//! intentional harness install.

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

    // Specific trees only. Bare `.github/` (every CI repo) and bare `.agents/`
    // (the shared skills tree other harnesses write) must not count (#489).
    let project_dirs: &[(&str, &[&str])] = &[
        ("claude-code", &[".claude"]),
        ("opencode", &[".opencode"]),
        ("antigravity", &[".agents/agents"]),
        ("codex", &[".codex"]),
        ("cursor", &[".cursor"]),
        ("github-copilot", &[".github/agents"]),
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

    // Project receipts live under `.shipmates/receipts/` (current) and, for a
    // brief earlier layout, `.shipmates/manifest/`.
    for receipts_rel in [".shipmates/receipts", ".shipmates/manifest"] {
        let receipts_dir = dir.join(receipts_rel);
        if !receipts_dir.is_dir() {
            continue;
        }
        for target in &supported {
            if receipts_dir.join(format!("{}.json", target)).is_file() {
                detections
                    .entry((*target).to_string())
                    .or_default()
                    .push(format!("local shipmates install receipt present ({receipts_rel})"));
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
/// 2. User-scope config dirs in home directory (`~/.claude/`, `~/.codex/`, `~/.gemini/config/`, etc.)
/// 3. Project markers in `target_dir` (`.claude/`, `.opencode/`, `.codex/`, `.agents/agents/`, etc.)
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
                    .push(format!("binary '{binary}' found on PATH"));
                break;
            }
        }
    }

    // 2. Global home directories — prefer harness-owned config roots, never a
    // bare `~/.github` (unrelated) or bare `~/.gemini` without Antigravity's
    // config tree (#489).
    if let Some(ref home_path) = home {
        let home_dirs: &[(&str, &[&str])] = &[
            ("claude-code", &[".claude"]),
            ("opencode", &[".config/opencode", ".opencode"]),
            ("antigravity", &[".gemini/config", ".antigravity"]),
            ("codex", &[".codex"]),
            ("cursor", &[".cursor"]),
            ("github-copilot", &[".config/github-copilot", ".copilot"]),
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
                        .push(format!("user configuration directory ~/{rel} exists"));
                    break;
                }
            }
        }

        // Global receipts
        for receipts_rel in [".shipmates/receipts", ".shipmates/manifest"] {
            let global_manifest = home_path.join(receipts_rel);
            if !global_manifest.is_dir() {
                continue;
            }
            for target in &supported {
                if global_manifest.join(format!("{}.json", target)).is_file() {
                    detections
                        .entry((*target).to_string())
                        .or_default()
                        .push(format!(
                            "global shipmates install receipt present ({receipts_rel})"
                        ));
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
        let receipts_dir = dir.path().join(".shipmates").join("receipts");
        fs::create_dir_all(&receipts_dir).unwrap();
        fs::write(receipts_dir.join("cursor.json"), "{}").unwrap();

        let detected = detect_project_harnesses(dir.path());
        let names: Vec<String> = detected.into_iter().map(|d| d.harness).collect();
        assert!(names.contains(&"cursor".to_string()));
        assert!(!names.contains(&"claude-code".to_string()));
    }

    #[test]
    fn bare_github_workflows_does_not_detect_copilot() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join(".github").join("workflows")).unwrap();

        let names: Vec<String> = detect_project_harnesses(dir.path())
            .into_iter()
            .map(|d| d.harness)
            .collect();
        assert!(
            !names.contains(&"github-copilot".to_string()),
            "bare .github/ must not imply Copilot; got {names:?}"
        );
    }

    #[test]
    fn github_agents_tree_detects_copilot() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join(".github").join("agents")).unwrap();

        let names: Vec<String> = detect_project_harnesses(dir.path())
            .into_iter()
            .map(|d| d.harness)
            .collect();
        assert!(names.contains(&"github-copilot".to_string()));
    }

    #[test]
    fn bare_agents_skills_does_not_detect_antigravity() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join(".agents").join("skills").join("ship-issue")).unwrap();

        let names: Vec<String> = detect_project_harnesses(dir.path())
            .into_iter()
            .map(|d| d.harness)
            .collect();
        assert!(
            !names.contains(&"antigravity".to_string()),
            "shared .agents/skills must not imply Antigravity; got {names:?}"
        );
    }

    #[test]
    fn agents_agents_tree_detects_antigravity() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join(".agents").join("agents").join("sdet")).unwrap();

        let names: Vec<String> = detect_project_harnesses(dir.path())
            .into_iter()
            .map(|d| d.harness)
            .collect();
        assert!(names.contains(&"antigravity".to_string()));
    }
}
