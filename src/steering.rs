//! Steering management: legacy contributor-steering migration (#295) and
//! canonical user-scope (global) steering instructions (#417, #430, #438).

use crate::catalog;
use crate::installer;
use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

// --- Legacy contributor-steering markers (#295) ---
pub const START: &str = "<!-- shipmates:contributor-steering -->\n";
pub const END: &str = "\n<!-- /shipmates:contributor-steering -->\n";

const LEGACY_CLAUDE_HEADING: &str = "# Shipmates contributor steering";

// --- Canonical user-scope global steering markers (#417, #430) ---
pub const GLOBAL_STEERING_START: &str = "<!-- shipmates:global-steering -->\n";
pub const GLOBAL_STEERING_END: &str = "\n<!-- /shipmates:global-steering -->\n";

/// Remove the marked legacy contributor steering section, if present.
pub fn strip_section(existing: &str) -> String {
    let Some(start) = existing.find(START) else {
        return existing.to_string();
    };
    let Some(end_rel) = existing[start..].find(END) else {
        return existing.to_string();
    };
    let end = start + end_rel + END.len();
    format!("{}{}", &existing[..start], &existing[end..])
        .trim_end()
        .to_string()
}

pub fn has_section(existing: &str) -> bool {
    existing.contains(START) && existing.contains(END)
}

/// True when root `CLAUDE.md` is the steering-only file #295 wrote at install.
pub fn is_legacy_claude_steering(content: &str) -> bool {
    has_section(content)
        || content.starts_with(LEGACY_CLAUDE_HEADING)
        || catalog::load_steering_embedded()
            .ok()
            .is_some_and(|embedded| content.trim() == embedded.trim())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LegacyMigration {
    Write { path: PathBuf, content: String },
    Remove { path: PathBuf },
}

/// Best-effort cleanup of #295 steering in root project-instructions files.
pub fn plan_legacy_migration(target_dir: &Path) -> std::io::Result<Vec<LegacyMigration>> {
    if !catalog::is_shipmates_contributor_tree(target_dir) {
        return Ok(Vec::new());
    }

    let mut actions = Vec::new();

    let agents = target_dir.join("AGENTS.md");
    if agents.is_file() {
        let existing = std::fs::read_to_string(&agents)?;
        if has_section(&existing) {
            let stripped = strip_section(&existing);
            if stripped.trim().is_empty() {
                actions.push(LegacyMigration::Remove { path: agents });
            } else {
                actions.push(LegacyMigration::Write {
                    path: agents,
                    content: stripped,
                });
            }
        }
    }

    let claude = target_dir.join("CLAUDE.md");
    if claude.is_file() {
        let existing = std::fs::read_to_string(&claude)?;
        if is_legacy_claude_steering(&existing) {
            actions.push(LegacyMigration::Remove { path: claude });
        }
    }

    Ok(actions)
}

// =========================================================================
// Global Steering (#417, #430, #438)
// =========================================================================

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GlobalSteeringTier {
    /// Dedicated modular file (e.g. Cursor ~/.cursor/rules/shipmates.mdc)
    TierA,
    /// Delimited managed block in user-scope file (Claude, Codex, OpenCode, Antigravity, Pi, Grok Build)
    TierB,
    /// Documented gap where harness has no global instruction file
    Gap(&'static str),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GlobalSteeringStatus {
    Installed { path: PathBuf, up_to_date: bool },
    Missing { path: PathBuf },
    Gap(&'static str),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SteeringOutcome {
    Created(PathBuf),
    Updated(PathBuf),
    Unchanged(PathBuf),
    #[allow(dead_code)]
    Removed(PathBuf),
    Gap(&'static str),
}

/// Return the steering tier for a harness.
pub fn global_steering_tier(harness: &str) -> GlobalSteeringTier {
    match harness {
        "cursor" => GlobalSteeringTier::TierA,
        // Devin CLI loads a user-scope instruction file from its config
        // directory (`~/.config/devin/AGENTS.md`), so it carries real global
        // steering; the pre-rename Windsurf entry was a Gap because only
        // Cascade's workspace rules were documented.
        "claude-code" | "codex" | "opencode" | "antigravity" | "pi" | "grok-build" | "devin" => {
            GlobalSteeringTier::TierB
        }
        "github-copilot" => GlobalSteeringTier::Gap(
            "GitHub Copilot has no global instruction file; configure via VS Code settings.json (github.copilot.chat.codeGeneration.instructions)"
        ),
        _ => GlobalSteeringTier::Gap("Unsupported or undocumented global steering for harness"),
    }
}

/// User-scope path for a harness's global steering relative to `home`.
pub fn global_steering_path(harness: &str, home: &Path) -> Option<PathBuf> {
    match harness {
        "cursor" => Some(home.join(".cursor").join("rules").join("shipmates.mdc")),
        "claude-code" => Some(home.join(".claude").join("CLAUDE.md")),
        "codex" => Some(home.join(".codex").join("AGENTS.md")),
        "opencode" => Some(home.join(".config").join("opencode").join("AGENTS.md")),
        "antigravity" => Some(home.join(".gemini").join("GEMINI.md")),
        "pi" => Some(home.join(".pi").join("agent").join("AGENTS.md")),
        // Grok reads named instruction files from `$GROK_HOME` and every
        // `$GROK_HOME/rules/*.md`, so `~/.grok/AGENTS.md` is its user-scope
        // instructions file — source-verified against the CLI.
        "grok-build" => Some(home.join(".grok").join("AGENTS.md")),
        // Devin CLI loads a user-scope instruction file from its config
        // directory: `~/.config/devin/AGENTS.md` (first-party docs).
        "devin" => Some(home.join(".config").join("devin").join("AGENTS.md")),
        _ => None,
    }
}

/// Render Tier A dedicated rule file (Cursor .mdc frontmatter + body).
pub fn render_tier_a(content: &str) -> String {
    format!(
        "---\ndescription: Shipmates global steering heuristics and workflow routing\nglobs: \"*\"\nalwaysApply: true\n---\n\n{}\n",
        content.trim()
    )
}

/// Check if `content` contains a global steering managed block.
pub fn has_global_steering_block(content: &str) -> bool {
    content.contains(GLOBAL_STEERING_START) && content.contains(GLOBAL_STEERING_END)
}

/// Extract content inside the global steering managed block.
pub fn extract_global_steering_block(content: &str) -> Option<String> {
    let start_idx = content.find(GLOBAL_STEERING_START)?;
    let content_start = start_idx + GLOBAL_STEERING_START.len();
    let end_rel = content[content_start..].find(GLOBAL_STEERING_END)?;
    let content_end = content_start + end_rel;
    Some(content[content_start..content_end].trim().to_string())
}

/// Replace or strip the global steering managed block.
#[allow(dead_code)]
pub fn strip_global_steering_block(existing: &str) -> String {
    let Some(start) = existing.find(GLOBAL_STEERING_START) else {
        return existing.to_string();
    };
    let Some(end_rel) = existing[start..].find(GLOBAL_STEERING_END) else {
        return existing.to_string();
    };
    let end = start + end_rel + GLOBAL_STEERING_END.len();
    format!("{}{}", &existing[..start], &existing[end..])
        .trim()
        .to_string()
}

/// Check global steering status for a harness without modifying disk.
pub fn check_global_steering(
    harness: &str,
    home: &Path,
    expected_content: &str,
) -> Result<GlobalSteeringStatus> {
    match global_steering_tier(harness) {
        GlobalSteeringTier::Gap(msg) => Ok(GlobalSteeringStatus::Gap(msg)),
        GlobalSteeringTier::TierA => {
            let path = global_steering_path(harness, home)
                .context("resolving global steering path")?;
            if !path.is_file() {
                return Ok(GlobalSteeringStatus::Missing { path });
            }
            let current = fs::read_to_string(&path)?;
            let want = render_tier_a(expected_content);
            let up_to_date = current.trim() == want.trim();
            Ok(GlobalSteeringStatus::Installed { path, up_to_date })
        }
        GlobalSteeringTier::TierB => {
            let path = global_steering_path(harness, home)
                .context("resolving global steering path")?;
            if !path.is_file() {
                return Ok(GlobalSteeringStatus::Missing { path });
            }
            let current = fs::read_to_string(&path)?;
            if !has_global_steering_block(&current) {
                return Ok(GlobalSteeringStatus::Missing { path });
            }
            let extracted = extract_global_steering_block(&current).unwrap_or_default();
            let up_to_date = extracted.trim() == expected_content.trim();
            Ok(GlobalSteeringStatus::Installed { path, up_to_date })
        }
    }
}

/// Install or update canonical global steering for `harness` in `home`.
pub fn install_global_steering(
    harness: &str,
    home: &Path,
    content: &str,
) -> Result<SteeringOutcome> {
    match global_steering_tier(harness) {
        GlobalSteeringTier::Gap(msg) => Ok(SteeringOutcome::Gap(msg)),
        GlobalSteeringTier::TierA => {
            let path = global_steering_path(harness, home)
                .context("resolving global steering path")?;
            let want = render_tier_a(content);
            if path.is_file() {
                let current = fs::read_to_string(&path)?;
                if current.trim() == want.trim() {
                    return Ok(SteeringOutcome::Unchanged(path));
                }
                installer::apply::backup_existing(&path, current.as_bytes())?;
                installer::atomic_write(&path, &want)?;
                Ok(SteeringOutcome::Updated(path))
            } else {
                installer::atomic_write(&path, &want)?;
                Ok(SteeringOutcome::Created(path))
            }
        }
        GlobalSteeringTier::TierB => {
            let path = global_steering_path(harness, home)
                .context("resolving global steering path")?;
            let managed_block = format!(
                "{}{}\n{}",
                GLOBAL_STEERING_START,
                content.trim(),
                GLOBAL_STEERING_END
            );
            if path.is_file() {
                let current = fs::read_to_string(&path)?;
                if has_global_steering_block(&current) {
                    let extracted = extract_global_steering_block(&current).unwrap_or_default();
                    if extracted.trim() == content.trim() {
                        return Ok(SteeringOutcome::Unchanged(path));
                    }
                    // Replace existing block
                    let start_idx = current.find(GLOBAL_STEERING_START).unwrap();
                    let end_idx = current.find(GLOBAL_STEERING_END).unwrap() + GLOBAL_STEERING_END.len();
                    let updated = format!("{}{}{}", &current[..start_idx], managed_block, &current[end_idx..]);
                    installer::apply::backup_existing(&path, current.as_bytes())?;
                    installer::atomic_write(&path, &updated)?;
                    Ok(SteeringOutcome::Updated(path))
                } else {
                    // Append managed block cleanly
                    let mut updated = current.trim_end().to_string();
                    if !updated.is_empty() {
                        updated.push_str("\n\n");
                    }
                    updated.push_str(&managed_block);
                    installer::apply::backup_existing(&path, current.as_bytes())?;
                    installer::atomic_write(&path, &updated)?;
                    Ok(SteeringOutcome::Updated(path))
                }
            } else {
                installer::atomic_write(&path, &managed_block)?;
                Ok(SteeringOutcome::Created(path))
            }
        }
    }
}

/// Uninstall global steering for `harness` from `home`.
#[allow(dead_code)]
pub fn uninstall_global_steering(harness: &str, home: &Path) -> Result<SteeringOutcome> {
    match global_steering_tier(harness) {
        GlobalSteeringTier::Gap(msg) => Ok(SteeringOutcome::Gap(msg)),
        GlobalSteeringTier::TierA => {
            let path = global_steering_path(harness, home)
                .context("resolving global steering path")?;
            if path.is_file() {
                fs::remove_file(&path)?;
                Ok(SteeringOutcome::Removed(path))
            } else {
                Ok(SteeringOutcome::Unchanged(path))
            }
        }
        GlobalSteeringTier::TierB => {
            let path = global_steering_path(harness, home)
                .context("resolving global steering path")?;
            if !path.is_file() {
                return Ok(SteeringOutcome::Unchanged(path));
            }
            let current = fs::read_to_string(&path)?;
            if !has_global_steering_block(&current) {
                return Ok(SteeringOutcome::Unchanged(path));
            }
            let stripped = strip_global_steering_block(&current);
            if stripped.trim().is_empty() {
                fs::remove_file(&path)?;
            } else {
                installer::atomic_write(&path, &format!("{}\n", stripped.trim()))?;
            }
            Ok(SteeringOutcome::Removed(path))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn strip_leaves_base_intact() {
        let merged = format!("{START}steer{END}");
        let base = format!("# AGENTS\n\n{merged}");
        assert_eq!(strip_section(&base).trim(), "# AGENTS");
    }

    #[test]
    fn detects_legacy_claude_steering_file() {
        assert!(is_legacy_claude_steering(
            "# Shipmates contributor steering\n\nchecklists\n"
        ));
        assert!(!is_legacy_claude_steering("# My Project\n\nReal CLAUDE.md\n"));
    }

    #[test]
    fn plan_removes_orphan_claude_and_strips_agents() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("commands")).unwrap();
        fs::write(dir.path().join("commands/shipmates-ship-issue.md"), "---\n---\n").unwrap();
        fs::create_dir_all(dir.path().join("toolbox")).unwrap();
        fs::create_dir_all(dir.path().join("tools")).unwrap();
        fs::write(dir.path().join("tools/gen_command_pages.py"), "# gen").unwrap();

        fs::write(
            dir.path().join("CLAUDE.md"),
            "# Shipmates contributor steering\n\nold install\n",
        )
        .unwrap();
        fs::write(
            dir.path().join("AGENTS.md"),
            format!("# Repo\n\n{START}steer{END}\n"),
        )
        .unwrap();

        let actions = plan_legacy_migration(dir.path()).unwrap();
        assert_eq!(actions.len(), 2);
        assert!(actions.iter().any(|a| matches!(
            a,
            LegacyMigration::Remove { path } if path.ends_with("CLAUDE.md")
        )));
        assert!(actions.iter().any(|a| matches!(
            a,
            LegacyMigration::Write { content, .. } if content.contains("# Repo")
        )));
    }

    #[test]
    fn grok_build_global_steering_is_the_user_instructions_file() {
        let home = Path::new("/home/captain");
        assert_eq!(global_steering_tier("grok-build"), GlobalSteeringTier::TierB);
        assert_eq!(
            global_steering_path("grok-build", home),
            Some(home.join(".grok").join("AGENTS.md"))
        );
    }

    #[test]
    fn test_global_steering_tier_a_install_and_idempotency() {
        let home = tempfile::tempdir().unwrap();
        let content = "# Global Rules\n\nRule 1: Be helpful.\n";

        let outcome = install_global_steering("cursor", home.path(), content).unwrap();
        assert!(matches!(outcome, SteeringOutcome::Created(_)));

        let rule_file = home.path().join(".cursor/rules/shipmates.mdc");
        assert!(rule_file.is_file());
        let read = fs::read_to_string(&rule_file).unwrap();
        assert!(read.contains("alwaysApply: true"));
        assert!(read.contains("Rule 1: Be helpful."));

        let status = check_global_steering("cursor", home.path(), content).unwrap();
        assert_eq!(
            status,
            GlobalSteeringStatus::Installed {
                path: rule_file.clone(),
                up_to_date: true
            }
        );

        // Second run is idempotent (Unchanged)
        let outcome2 = install_global_steering("cursor", home.path(), content).unwrap();
        assert_eq!(outcome2, SteeringOutcome::Unchanged(rule_file.clone()));

        // Uninstall
        let outcome3 = uninstall_global_steering("cursor", home.path()).unwrap();
        assert_eq!(outcome3, SteeringOutcome::Removed(rule_file.clone()));
        assert!(!rule_file.exists());
    }

    #[test]
    fn test_global_steering_tier_b_inject_update_idempotent() {
        let home = tempfile::tempdir().unwrap();
        let content = "# Global Rules\n\n- Instinct: worktree isolation\n";

        // Pre-create user's CLAUDE.md
        let claude_dir = home.path().join(".claude");
        fs::create_dir_all(&claude_dir).unwrap();
        let user_claude = claude_dir.join("CLAUDE.md");
        fs::write(&user_claude, "# My Personal Preferences\n\n- Speak terse.\n").unwrap();

        // Install appends cleanly without destroying user content
        let outcome = install_global_steering("claude-code", home.path(), content).unwrap();
        assert_eq!(outcome, SteeringOutcome::Updated(user_claude.clone()));

        let after_install = fs::read_to_string(&user_claude).unwrap();
        assert!(after_install.contains("# My Personal Preferences"));
        assert!(after_install.contains(GLOBAL_STEERING_START));
        assert!(after_install.contains("- Instinct: worktree isolation"));
        assert!(after_install.contains(GLOBAL_STEERING_END));

        // Status check reports up to date
        let status = check_global_steering("claude-code", home.path(), content).unwrap();
        assert_eq!(
            status,
            GlobalSteeringStatus::Installed {
                path: user_claude.clone(),
                up_to_date: true
            }
        );

        // Idempotent re-run
        let outcome2 = install_global_steering("claude-code", home.path(), content).unwrap();
        assert_eq!(outcome2, SteeringOutcome::Unchanged(user_claude.clone()));

        // Update with modified content replaces block in-place
        let new_content = "# Global Rules\n\n- Instinct: parallel fan-out\n";
        let outcome3 = install_global_steering("claude-code", home.path(), new_content).unwrap();
        assert_eq!(outcome3, SteeringOutcome::Updated(user_claude.clone()));

        let after_update = fs::read_to_string(&user_claude).unwrap();
        assert!(after_update.contains("# My Personal Preferences"));
        assert!(!after_update.contains("- Instinct: worktree isolation"));
        assert!(after_update.contains("- Instinct: parallel fan-out"));

        // Uninstall leaves user preferences intact
        let outcome4 = uninstall_global_steering("claude-code", home.path()).unwrap();
        assert_eq!(outcome4, SteeringOutcome::Removed(user_claude.clone()));
        let after_uninstall = fs::read_to_string(&user_claude).unwrap();
        assert!(after_uninstall.contains("# My Personal Preferences"));
        assert!(!has_global_steering_block(&after_uninstall));
    }
}
