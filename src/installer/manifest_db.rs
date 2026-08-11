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
        let sha256 = digest::compute_sha256(path)
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
        if !matches!(self.layout.as_str(), LAYOUT_SKILLS | LAYOUT_COMMANDS) {
            bail!("unsupported receipt layout {:?}", self.layout);
        }
        if self.roots.is_empty() {
            bail!("receipt must contain at least one root");
        }
        let mut previous: Option<&str> = None;
        for root in &self.roots {
            validate_relative_path(root, "receipt root")?;
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

    pub fn receipts_dir(&self) -> PathBuf {
        self.target_dir.join(RECEIPTS_DIR)
    }

    pub fn receipt_path(&self, harness: &str) -> Result<PathBuf> {
        validate_harness(harness)?;
        Ok(self.receipts_dir().join(format!("{harness}.json")))
    }

    /// Read one receipt. Missing receipt means no prior Shipmates install.
    pub fn load(&self, harness: &str) -> Result<Option<InstallReceipt>> {
        let path = self.receipt_path(harness)?;
        if !path.exists() {
            return Ok(None);
        }
        Ok(Some(read_receipt(&path, harness)?))
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
        let directory = match fs::read_dir(self.receipts_dir()) {
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
    Ok(())
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
        let mut value = serde_json::to_value(receipt("other", vec![])).unwrap();
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
