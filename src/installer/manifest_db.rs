//! Persistent ownership receipts for installed harness files.
//!
//! Receipts are deliberately independent from the installer and its output
//! formatting. The installer records what it wrote; consumers can later use
//! that record to compare, upgrade, or remove only Shipmates-owned files.

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::fs;
use std::path::{Component, Path, PathBuf};

use crate::digest;
use crate::installer::atomic_write;

pub const CURRENT_SCHEMA_VERSION: u32 = 1;
pub const RECEIPTS_DIR: &str = ".shipmates/receipts";
pub const LAYOUT_SKILLS: &str = "skills";
pub const LAYOUT_COMMANDS: &str = "commands";

/// One target-relative file owned by an install receipt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReceiptFile {
    pub path: String,
    pub sha256: String,
}

impl ReceiptFile {
    /// Build an entry from a file below `target_dir`, hashing its raw bytes.
    pub fn from_target_file(target_dir: &Path, path: &Path) -> Result<Self> {
        let relative = path.strip_prefix(target_dir).with_context(|| {
            format!(
                "receipt file {} is outside target {}",
                path.display(),
                target_dir.display()
            )
        })?;
        let relative = relative_path(relative, "receipt file")?;
        let path = resolve_target_relative(target_dir, Path::new(&relative))?;
        let sha256 = digest::compute_sha256(&path)
            .with_context(|| format!("hashing receipt file {}", path.display()))?;
        Ok(Self {
            path: relative,
            sha256,
        })
    }

    pub fn validate(&self) -> Result<()> {
        validate_relative_path(&self.path, "receipt file path")?;
        validate_sha256(&self.sha256)
    }
}

/// Install receipt persisted as `<target>/.shipmates/receipts/<harness>.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstallReceipt {
    pub schema_version: u32,
    pub version: String,
    pub harness: String,
    pub layout: String,
    pub roots: Vec<String>,
    pub files: Vec<ReceiptFile>,
}

impl InstallReceipt {
    pub fn new(
        version: impl Into<String>,
        harness: impl Into<String>,
        layout: impl Into<String>,
        roots: Vec<String>,
        files: Vec<ReceiptFile>,
    ) -> Result<Self> {
        let receipt = Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            version: version.into(),
            harness: harness.into(),
            layout: layout.into(),
            roots,
            files,
        };
        receipt.validate()?;
        Ok(receipt)
    }

    /// Validate all persisted invariants, including canonical file ordering.
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != CURRENT_SCHEMA_VERSION {
            bail!(
                "unsupported receipt schema version {}; supported version is {}",
                self.schema_version,
                CURRENT_SCHEMA_VERSION
            );
        }
        if self.version.is_empty() || self.version.contains('\0') {
            bail!("receipt version must be non-empty and contain no NUL bytes");
        }
        validate_harness(&self.harness)?;
        let allowed_roots = allowed_roots(&self.harness);
        if !matches!(self.layout.as_str(), LAYOUT_SKILLS | LAYOUT_COMMANDS) {
            bail!("unsupported receipt layout {:?}", self.layout);
        }
        if self.roots.is_empty() {
            bail!("receipt must contain at least one root");
        }
        let mut previous: Option<&str> = None;
        for root in &self.roots {
            validate_relative_path(root, "receipt root")?;
            if !allowed_roots.contains(&root.as_str()) {
                bail!(
                    "receipt root {:?} is not part of harness {} install layout",
                    root,
                    self.harness
                );
            }
            if let Some(previous) = previous {
                if root.as_str() <= previous {
                    bail!("receipt roots must be sorted and unique");
                }
            }
            previous = Some(root);
        }

        let mut previous: Option<&str> = None;
        for file in &self.files {
            file.validate()?;
            if !allowed_receipt_path(&self.harness, &file.path)
                || !self
                    .roots
                    .iter()
                    .any(|root| file.path == *root || file.path.starts_with(&format!("{root}/")))
            {
                bail!(
                    "receipt file path {:?} is not part of harness {} install layout",
                    file.path,
                    self.harness
                );
            }
            if let Some(previous) = previous {
                match file.path.as_str().cmp(previous) {
                    Ordering::Less => bail!("receipt files must be sorted by path"),
                    Ordering::Equal => bail!("receipt files must contain unique paths"),
                    Ordering::Greater => {}
                }
            }
            previous = Some(&file.path);
        }
        Ok(())
    }

    pub fn file(&self, path: &str) -> Option<&ReceiptFile> {
        self.files
            .binary_search_by(|file| file.path.as_str().cmp(path))
            .ok()
            .map(|index| &self.files[index])
    }
}

/// Persistent receipt repository rooted at one install target.
#[derive(Debug, Clone)]
pub struct ReceiptRepository {
    target_dir: PathBuf,
}

impl ReceiptRepository {
    pub fn new(target_dir: impl Into<PathBuf>) -> Self {
        Self {
            target_dir: target_dir.into(),
        }
    }

    pub fn target_dir(&self) -> &Path {
        &self.target_dir
    }

    pub fn receipts_dir(&self) -> Result<PathBuf> {
        resolve_target_relative(&self.target_dir, Path::new(RECEIPTS_DIR))
    }

    pub fn receipt_path(&self, harness: &str) -> Result<PathBuf> {
        validate_harness(harness)?;
        resolve_target_relative(
            &self.target_dir,
            &Path::new(RECEIPTS_DIR).join(format!("{harness}.json")),
        )
    }

    /// Read one receipt. Missing receipt means no prior Shipmates install.
    pub fn load(&self, harness: &str) -> Result<Option<InstallReceipt>> {
        let path = self.receipt_path(harness)?;
        match fs::symlink_metadata(&path) {
            Ok(_) => Ok(Some(read_receipt(&path, harness)?)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => {
                Err(error).with_context(|| format!("inspecting receipt {}", path.display()))
            }
        }
    }

    pub fn read(&self, harness: &str) -> Result<Option<InstallReceipt>> {
        self.load(harness)
    }

    /// Atomically persist a receipt at its harness-owned filename.
    pub fn save(&self, receipt: &InstallReceipt) -> Result<()> {
        receipt.validate()?;
        let path = self.receipt_path(&receipt.harness)?;
        let mut json = serde_json::to_string_pretty(receipt)?;
        json.push('\n');
        atomic_write(&path, &json)
            .with_context(|| format!("writing install receipt {}", path.display()))?;
        Ok(())
    }

    pub fn write(&self, receipt: &InstallReceipt) -> Result<()> {
        self.save(receipt)
    }

    /// Remove one receipt. Missing receipts are already removed.
    pub fn remove(&self, harness: &str) -> Result<bool> {
        let path = self.receipt_path(harness)?;
        match fs::remove_file(&path) {
            Ok(()) => Ok(true),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(error) => {
                Err(error).with_context(|| format!("removing receipt {}", path.display()))
            }
        }
    }

    /// Load every valid JSON receipt beside this target, sorted by harness.
    pub fn load_all(&self) -> Result<Vec<InstallReceipt>> {
        let receipts_dir = self.receipts_dir()?;
        let directory = match fs::read_dir(&receipts_dir) {
            Ok(directory) => directory,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error).context("reading install receipt directory"),
        };

        let mut receipts = Vec::new();
        for entry in directory {
            let entry = entry.context("reading install receipt directory entry")?;
            let file_type = entry
                .file_type()
                .context("reading install receipt file type")?;
            let path = entry.path();
            if path.extension().and_then(|extension| extension.to_str()) != Some("json") {
                continue;
            }
            if !file_type.is_file() {
                bail!("receipt {} is not a regular file", path.display());
            }
            let harness = path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .ok_or_else(|| {
                    anyhow::anyhow!("receipt filename is not valid UTF-8: {}", path.display())
                })?;
            validate_harness(harness)?;
            let path = resolve_target_relative(
                &self.target_dir,
                Path::new(RECEIPTS_DIR)
                    .join(format!("{harness}.json"))
                    .as_path(),
            )?;
            receipts.push(read_receipt(&path, harness)?);
        }
        receipts.sort_by(|left, right| left.harness.cmp(&right.harness));
        Ok(receipts)
    }

    pub fn all(&self) -> Result<Vec<InstallReceipt>> {
        self.load_all()
    }

    /// Return harnesses whose receipts claim a target-relative path.
    pub fn claims_for_path(&self, path: &Path) -> Result<Vec<String>> {
        let path = relative_path(path, "claimed path")?;
        Ok(self
            .load_all()?
            .into_iter()
            .filter(|receipt| receipt.file(&path).is_some())
            .map(|receipt| receipt.harness)
            .collect())
    }

    pub fn claims(&self, path: &Path) -> Result<Vec<String>> {
        self.claims_for_path(path)
    }

    pub fn path_claims(&self, path: &Path) -> Result<Vec<String>> {
        self.claims_for_path(path)
    }

    pub fn is_claimed(&self, path: &Path) -> Result<bool> {
        Ok(!self.claims_for_path(path)?.is_empty())
    }

    pub fn is_claimed_by_other(&self, path: &Path, harness: &str) -> Result<bool> {
        validate_harness(harness)?;
        Ok(self
            .claims_for_path(path)?
            .into_iter()
            .any(|claimant| claimant != harness))
    }
}

pub type ManifestDb = ReceiptRepository;
pub type Manifest = InstallReceipt;
pub type ManifestEntry = ReceiptFile;

/// Resolve one target-relative path without traversing symlinks.
///
/// Missing final components are allowed for installs, but every existing
/// component is inspected with `symlink_metadata`, including target roots,
/// receipt directories, harness roots, and the final path.
pub fn resolve_target_relative(target_dir: &Path, relative: &Path) -> Result<PathBuf> {
    if relative.as_os_str().is_empty() || relative.is_absolute() {
        bail!("unsafe target-relative path: {}", relative.display());
    }
    for component in relative.components() {
        if !matches!(component, Component::Normal(_)) {
            bail!("unsafe target-relative path: {}", relative.display());
        }
    }

    // The target itself must not be a symlink. Parent components belong to the
    // caller's path namespace (for example, macOS `/var`), while components
    // below this target are checked one by one below.
    reject_symlink(target_dir)?;
    let mut current = target_dir.to_path_buf();
    for component in relative.components() {
        current.push(component.as_os_str());
        reject_symlink(&current)?;
    }
    Ok(current)
}

fn reject_symlink(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            bail!(
                "refusing symlink component in target path {}",
                path.display()
            )
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => {
            Err(error).with_context(|| format!("inspecting target path {}", path.display()))
        }
    }
}

fn read_receipt(path: &Path, expected_harness: &str) -> Result<InstallReceipt> {
    let bytes =
        fs::read(path).with_context(|| format!("reading install receipt {}", path.display()))?;
    let receipt: InstallReceipt = serde_json::from_slice(&bytes)
        .with_context(|| format!("parsing install receipt {}", path.display()))?;
    if receipt.harness != expected_harness {
        bail!(
            "receipt {} records harness {:?}, expected {:?}",
            path.display(),
            receipt.harness,
            expected_harness
        );
    }
    receipt.validate()?;
    Ok(receipt)
}

fn validate_harness(harness: &str) -> Result<()> {
    if harness.is_empty()
        || !harness
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
    {
        bail!("invalid harness name {:?}", harness);
    }
    if !matches!(
        harness,
        "claude-code"
            | "opencode"
            | "antigravity"
            | "codex"
            | "cursor"
            | "github-copilot"
            | "windsurf"
    ) {
        bail!("unsupported harness {:?}", harness);
    }
    Ok(())
}

fn allowed_roots(harness: &str) -> &'static [&'static str] {
    match harness {
        "claude-code" => &[".claude"],
        "opencode" => &[".opencode", ".shipmates"],
        "antigravity" => &[".agents", ".shipmates"],
        "codex" => &[".agents", ".codex", ".shipmates"],
        "cursor" => &[".agents", ".cursor"],
        "github-copilot" => &[".agents", ".github"],
        "windsurf" => &[".windsurf", ".shipmates"],
        _ => &[],
    }
}

fn is_steering_receipt_path(harness: &str, path: &str) -> bool {
    match harness {
        "claude-code" => path == ".claude/rules/shipmates-contributor.md",
        "cursor" => path == ".cursor/rules/shipmates-contributor.mdc",
        "github-copilot" => path == ".github/instructions/shipmates.instructions.md",
        "opencode" | "codex" | "antigravity" | "windsurf" => {
            path == ".shipmates/contributor-steering.md"
        }
        _ => false,
    }
}

fn is_shipmates_steering_path(path: &str) -> bool {
    path == ".shipmates/contributor-steering.md"
}

/// Receipt paths are attacker-controlled input. Keep them inside the exact
/// payload trees each adapter can write; a syntactically relative path is not
/// enough because uninstall and doctor use receipts as deletion/overwrite
/// authority.
fn allowed_receipt_path(harness: &str, path: &str) -> bool {
    let parts = path.split('/').collect::<Vec<_>>();
    if parts.iter().any(|part| {
        *part == ".git"
            || (part.starts_with(".git") && *part != ".github")
            || part.starts_with("README")
    }) {
        return false;
    }
    let Some(root) = parts.first().copied() else {
        return false;
    };
    let is_skill_tree = |root: &str| {
        parts.len() >= 4
            && parts[0] == root
            && parts[1] == "skills"
            && !parts[2].is_empty()
            && (parts[3] == "SKILL.md" || (parts.len() == 4 && parts[3].ends_with(".py")))
    };
    match harness {
        "claude-code" => {
            is_steering_receipt_path(harness, path)
                || (parts.len() == 3
                    && root == ".claude"
                    && parts[1] == "agents"
                    && parts[2].ends_with(".md"))
                || is_skill_tree(".claude")
                || (parts.len() == 3
                    && root == ".claude"
                    && parts[1] == "commands"
                    && parts[2].ends_with(".md"))
        }
        "opencode" => {
            is_steering_receipt_path(harness, path)
                || is_shipmates_steering_path(path)
                || (parts.len() == 3
                    && root == ".opencode"
                    && parts[1] == "agents"
                    && parts[2].ends_with(".md"))
                || (parts.len() == 3
                    && root == ".opencode"
                    && parts[1] == "commands"
                    && parts[2].ends_with(".md"))
                || (parts.len() == 3
                    && root == ".opencode"
                    && parts[1] == "tools"
                    && (parts[2].ends_with(".ts") || parts[2].ends_with(".py")))
        }
        "antigravity" => {
            is_steering_receipt_path(harness, path)
                || is_shipmates_steering_path(path)
                || (parts.len() == 3
                    && root == ".agents"
                    && parts[1] == "agents"
                    && parts[2].ends_with(".md"))
                || is_skill_tree(".agents")
        }
        "codex" => {
            is_steering_receipt_path(harness, path)
                || is_shipmates_steering_path(path)
                || (parts.len() == 3
                    && root == ".codex"
                    && parts[1] == "agents"
                    && parts[2].ends_with(".toml"))
                || is_skill_tree(".agents")
        }
        "cursor" => {
            is_steering_receipt_path(harness, path)
                || (parts.len() == 3
                    && root == ".cursor"
                    && parts[1] == "rules"
                    && parts[2].ends_with(".mdc"))
                || is_skill_tree(".agents")
        }
        "windsurf" => {
            is_steering_receipt_path(harness, path)
                || is_shipmates_steering_path(path)
                || is_skill_tree(".windsurf")
        }
        "github-copilot" => {
            is_steering_receipt_path(harness, path)
                || (parts.len() == 3
                    && root == ".github"
                    && parts[1] == "instructions"
                    && parts[2].ends_with(".instructions.md"))
                || (parts.len() == 3
                    && root == ".github"
                    && parts[1] == "agents"
                    && parts[2].ends_with(".agent.md"))
                || is_skill_tree(".agents")
        }
        _ => false,
    }
}

fn validate_sha256(sha256: &str) -> Result<()> {
    if sha256.len() != 64
        || !sha256
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        bail!("invalid SHA-256 digest {:?}", sha256);
    }
    Ok(())
}

fn relative_path(path: &Path, field: &str) -> Result<String> {
    let value = path
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("{field} is not valid UTF-8"))?;
    validate_relative_path(value, field)?;
    Ok(value.to_string())
}

fn validate_relative_path(path: &str, field: &str) -> Result<()> {
    if path.is_empty()
        || path.contains('\0')
        || path.contains('\\')
        || path.starts_with('/')
        || path.ends_with('/')
        || path.contains("//")
    {
        bail!("{field} must be a safe target-relative path: {:?}", path);
    }
    if Path::new(path).is_absolute() {
        bail!("{field} must be relative: {:?}", path);
    }
    for component in Path::new(path).components() {
        match component {
            Component::Normal(segment) => {
                if segment == ".shipmates" {
                    bail!("{field} may not address .shipmates: {:?}", path);
                }
            }
            _ => bail!("{field} contains an unsafe path component: {:?}", path),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::digest;
    use tempfile::tempdir;

    fn file(path: &str, sha256: &str) -> ReceiptFile {
        ReceiptFile {
            path: path.into(),
            sha256: sha256.into(),
        }
    }

    fn receipt(harness: &str, files: Vec<ReceiptFile>) -> InstallReceipt {
        InstallReceipt::new(
            "0.1.3",
            harness,
            LAYOUT_SKILLS,
            vec![".agents".into()],
            files,
        )
        .unwrap()
    }

    const HASH: &str = "a000000000000000000000000000000000000000000000000000000000000000";

    #[test]
    fn save_loads_atomic_receipt_and_preserves_schema() {
        let dir = tempdir().unwrap();
        let repository = ReceiptRepository::new(dir.path());
        let expected = receipt("codex", vec![file(".agents/skills/a/SKILL.md", HASH)]);

        repository.save(&expected).unwrap();

        assert_eq!(repository.load("codex").unwrap(), Some(expected));
        assert!(repository.receipt_path("codex").unwrap().is_file());
        assert!(!dir.path().join(".shipmates/receipts/codex.tmp").exists());
    }

    #[test]
    fn load_all_and_claims_cover_shared_agent_skills() {
        let dir = tempdir().unwrap();
        let repository = ReceiptRepository::new(dir.path());
        let path = ".agents/skills/shared/SKILL.md";
        repository
            .save(&receipt("codex", vec![file(path, HASH)]))
            .unwrap();
        repository
            .save(&receipt("github-copilot", vec![file(path, HASH)]))
            .unwrap();

        assert_eq!(repository.load_all().unwrap().len(), 2);
        assert_eq!(
            repository.claims_for_path(Path::new(path)).unwrap(),
            vec!["codex", "github-copilot"]
        );
        assert!(
            repository
                .is_claimed_by_other(Path::new(path), "codex")
                .unwrap()
        );
    }

    #[test]
    fn validation_rejects_unsafe_paths_duplicates_and_uppercase_hashes() {
        for path in [
            "../outside",
            "/absolute",
            ".shipmates/receipt.json",
            "a\\b",
            "a//b",
        ] {
            let error = InstallReceipt::new(
                "0.1.3",
                "claude-code",
                LAYOUT_SKILLS,
                vec![".claude".into()],
                vec![file(path, HASH)],
            )
            .unwrap_err();
            assert!(error.to_string().contains("path"));
        }

        assert!(
            InstallReceipt::new(
                "0.1.3",
                "claude-code",
                LAYOUT_SKILLS,
                vec![".claude".into()],
                vec![file("b", HASH), file("a", HASH)],
            )
            .is_err()
        );
        assert!(
            InstallReceipt::new(
                "0.1.3",
                "claude-code",
                LAYOUT_SKILLS,
                vec![".claude".into()],
                vec![file("a", &HASH.to_ascii_uppercase())],
            )
            .is_err()
        );
    }

    #[test]
    fn load_rejects_filename_harness_mismatch_and_unknown_schema() {
        let dir = tempdir().unwrap();
        let repository = ReceiptRepository::new(dir.path());
        let path = repository.receipt_path("codex").unwrap();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut value = serde_json::to_value(receipt("codex", vec![])).unwrap();
        value["schema_version"] = serde_json::json!(99);
        fs::write(path, serde_json::to_vec(&value).unwrap()).unwrap();

        assert!(repository.load("codex").is_err());
    }

    #[test]
    fn receipt_file_hashes_raw_bytes_and_requires_target_descendant() {
        let dir = tempdir().unwrap();
        let target = dir.path().join("target");
        fs::create_dir_all(&target).unwrap();
        let payload = target.join(".agents/payload");
        fs::create_dir_all(payload.parent().unwrap()).unwrap();
        let bytes = [0, 1, 2, 0xff];
        fs::write(&payload, bytes).unwrap();
        let entry = ReceiptFile::from_target_file(&target, &payload).unwrap();

        assert_eq!(entry.path, ".agents/payload");
        assert_eq!(entry.sha256, digest::hash_bytes(&bytes));
        assert!(
            ReceiptFile::from_target_file(&target, dir.path().join("outside").as_path()).is_err()
        );
    }
}
