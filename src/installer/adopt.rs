//! Classify a file already sitting at a path the payload wants to write.
//!
//! An upgrade meets three kinds of file at a payload path: one Shipmates put
//! there and still owns, one Shipmates put there but no receipt records
//! (installed before receipts, copied by hand, restored from a backup), and one
//! that belongs to somebody else entirely. Only the middle kind may be adopted
//! — backed up, rewritten from the payload, and claimed — so a flagship never
//! stays frozen on old bytes while the tree around it upgrades (#386).
//!
//! Ownership is decided from the file's own declaration: an artifact whose
//! frontmatter `name:` equals the name its path implies is claiming to be that
//! artifact. Everything else fails closed as third party, including anything
//! this module cannot read or does not recognise.

use crate::catalog::parse_frontmatter_from;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Collision {
    /// The file declares itself to be the very artifact the payload installs
    /// here. Adopt it: back up, write the payload, claim it in the receipt.
    Adoptable,
    /// Anything else. Never touched without `--force`.
    ThirdParty,
}

/// The artifact name a payload-relative path implies, for the shapes every
/// adapter emits:
///
/// - `…/skills/<name>/SKILL.md` — the stem is `SKILL`, so the name is the
///   directory, never the file stem
/// - `…/agents/<name>.md`, `…/agents/<name>.agent.md` (Copilot),
///   `…/agents/<name>.toml` (Codex)
/// - `…/commands/<name>.md`
/// - `…/tools/<name>.ts` (opencode)
///
/// Any other path is unrecognised, which classifies as third party.
pub fn artifact_name(rel: &Path) -> Option<String> {
    let parts: Vec<&str> = rel
        .components()
        .filter_map(|component| match component {
            std::path::Component::Normal(value) => value.to_str(),
            _ => None,
        })
        .collect();
    let [.., parent, file] = parts.as_slice() else {
        return None;
    };
    match *parent {
        _ if *file == "SKILL.md" => {
            let grandparent = parts.get(parts.len().checked_sub(3)?)?;
            (*grandparent == "skills").then(|| (*parent).to_string())
        }
        "agents" => file
            .strip_suffix(".agent.md")
            .or_else(|| file.strip_suffix(".md"))
            .or_else(|| file.strip_suffix(".toml"))
            .map(str::to_string),
        "commands" => file.strip_suffix(".md").map(str::to_string),
        "tools" => file.strip_suffix(".ts").map(str::to_string),
        _ => None,
    }
    .filter(|name| !name.is_empty())
}

/// Whether a file's frontmatter claims `name`. A file with no parseable
/// frontmatter — a TOML agent, an opencode `.ts` tool, a user's prose — never
/// matches, which is the fail-closed answer.
pub fn frontmatter_name_matches(content: &str, name: &str) -> bool {
    match parse_frontmatter_from(content, name) {
        Ok((frontmatter, _)) => frontmatter.get("name").map(|n| n == name).unwrap_or(false),
        Err(_) => false,
    }
}

/// Classify the bytes found at a payload path.
///
/// Fail closed: only a YAML-frontmatter artifact whose `name:` matches the
/// path is adoptable. Companion assets (`.py`, `.ts`, Codex `.toml`) cannot
/// declare that identity, so they refuse without `--force` rather than being
/// overwritten on a plain install.
pub fn classify(rel: &Path, existing: &[u8]) -> Collision {
    let Some(name) = artifact_name(rel) else {
        return Collision::ThirdParty;
    };
    let file = rel.file_name().and_then(|value| value.to_str()).unwrap_or("");
    if file != "SKILL.md" && !file.ends_with(".md") {
        return Collision::ThirdParty;
    }
    let Ok(text) = std::str::from_utf8(existing) else {
        return Collision::ThirdParty;
    };
    if frontmatter_name_matches(text, &name) {
        Collision::Adoptable
    } else {
        Collision::ThirdParty
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn name(rel: &str) -> Option<String> {
        artifact_name(&PathBuf::from(rel))
    }

    #[test]
    fn artifact_name_reads_the_skill_directory_not_the_file_stem() {
        assert_eq!(
            name(".claude/skills/report-bug/SKILL.md").as_deref(),
            Some("report-bug")
        );
        assert_eq!(
            name(".agents/skills/shipmates-gh/SKILL.md").as_deref(),
            Some("shipmates-gh")
        );
        // `skills` must actually be the grandparent — a bare SKILL.md is not
        // an artifact we can name.
        assert_eq!(name(".claude/notes/x/SKILL.md"), None);
    }

    #[test]
    fn artifact_name_covers_every_adapter_shape() {
        assert_eq!(name(".claude/agents/sdet.md").as_deref(), Some("sdet"));
        assert_eq!(
            name(".github/agents/sdet.agent.md").as_deref(),
            Some("sdet")
        );
        assert_eq!(name(".codex/agents/sdet.toml").as_deref(), Some("sdet"));
        assert_eq!(
            name(".opencode/commands/ship-issue.md").as_deref(),
            Some("ship-issue")
        );
        assert_eq!(
            name(".opencode/tools/shipmates-gh.ts").as_deref(),
            Some("shipmates-gh")
        );
        assert_eq!(name(".claude/settings.json"), None);
        assert_eq!(name("README.md"), None);
    }

    #[test]
    fn matching_frontmatter_name_is_adoptable() {
        let rel = PathBuf::from(".claude/skills/report-bug/SKILL.md");
        let body = "---\nname: report-bug\ndescription: d\n---\nbody\n";
        assert_eq!(classify(&rel, body.as_bytes()), Collision::Adoptable);
    }

    #[test]
    fn foreign_unreadable_and_unrecognised_files_fail_closed() {
        let skill = PathBuf::from(".claude/skills/report-bug/SKILL.md");
        for bytes in [
            "---\nname: my-own-thing\n---\nkeep me\n".as_bytes(),
            "just prose\n".as_bytes(),
            &[0xff, 0xfe, 0x00, 0x9c],
        ] {
            assert_eq!(classify(&skill, bytes), Collision::ThirdParty);
        }
    }

    #[test]
    fn payload_companion_assets_fail_closed() {
        for rel in [
            ".claude/skills/shipmates-gh/gh.py",
            ".opencode/tools/shipmates-gh.ts",
            ".codex/agents/sdet.toml",
            ".claude/rules/shipmates-contributor.md",
        ] {
            assert_eq!(
                classify(&PathBuf::from(rel), b"user bytes\n"),
                Collision::ThirdParty,
                "{rel}"
            );
        }
    }
}
