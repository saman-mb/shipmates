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
use std::collections::HashMap;

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
            if !allowed_roots.contains(&root.as_str()) && !is_legacy_instructions_root(root) {
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
            if (!allowed_receipt_path(&self.harness, &file.path)
                && !is_legacy_steering_receipt_path(&self.harness, &file.path))
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
            | "pi"
            | "windsurf"
    ) {
        bail!("unsupported harness {:?}", harness);
    }
    Ok(())
}

/// Every root a harness may own, current and legacy. The single source of truth
/// for an install's footprint: receipt validation bounds ownership with it, and
/// `doctor` bounds its hygiene sweep with it, so a tree the harness has stopped
/// writing to is still swept for the litter it left there.
pub(crate) fn allowed_roots(harness: &str) -> &'static [&'static str] {
    match harness {
        "claude-code" => &[".claude"],
        "opencode" => &[".opencode", ".shipmates"],
        "antigravity" => &[".agents", ".gemini", ".shipmates"],
        "codex" => &[".agents", ".codex", ".shipmates"],
        // `.agents` is legacy-only for cursor: pre-#405 installs shipped skills
        // to the shared tree, and their receipts must still load so install can
        // clean the orphans up. Nothing writes there any more.
        "cursor" => &[".agents", ".cursor"],
        "github-copilot" => &[".agents", ".copilot", ".github"],
        // `.agents` is legacy-only for pi: every install before the crew moved
        // to `.pi/agents/` shipped skills there, and its receipt must still load
        // so `update` can refresh those files.
        "pi" => &[".pi", ".agents", ".shipmates"],
        "windsurf" => &[".windsurf", ".shipmates"],
        _ => &[],
    }
}

/// The harness's own **user-scope** configuration root, when it differs from
/// the workspace tree the payload is built in.
///
/// A global install (`--global`, which is the default) writes into `$HOME`.
/// For most harnesses that already lands correctly, because their global tree is
/// the workspace dotdir at home — `~/.claude/`, `~/.codex/`, `~/.cursor/`,
/// `~/.windsurf/` — so they carry no entry here and nothing changes for them.
///
/// Two do not, and joining the workspace path to `$HOME` writes files nothing
/// reads:
///
/// * **antigravity** reads `~/.gemini/config/` — agents at
///   `.gemini/config/agents/<name>/agent.md`, skills at
///   `.gemini/config/skills/<name>/SKILL.md`. Verified against the shipped
///   `agy` binary, which embeds `~/.gemini/config/` as its global root.
/// * **pi** reads `~/.pi/agent/` — agents at `.pi/agent/agents/<name>.md`,
///   skills at `.pi/agent/skills/<name>/SKILL.md`.
///
/// Being the global root rather than a workspace path, it is stored relative to
/// the install target (which is `$HOME` for a global install).
pub(crate) fn global_root(harness: &str) -> Option<&'static str> {
    match harness {
        "antigravity" => Some(".gemini/config"),
        "pi" => Some(".pi/agent"),
        // Codex's home is `$CODEX_HOME`, defaulting to `~/.codex`, and it
        // discovers and installs skills at `$CODEX_HOME/skills/<name>`. Its crew
        // already land at `.codex/agents/`, which this leaves where it is.
        "codex" => Some(".codex"),
        // Copilot's configuration directory defaults to `~/.copilot`. Its own
        // CLI reports "Skills are automatically discovered from:
        // ~/.copilot/skills/", and resolves a non-project agent to
        // `<config-dir>/agents`. `.github/` is the *workspace* location only.
        "github-copilot" => Some(".copilot"),
        _ => None,
    }
}

/// Rewrite a payload-relative path into the harness's global location, or `None`
/// when the harness needs no relocation or the path is not a resource subtree.
///
/// The payload is built once, in its workspace shape, and the committed digest
/// pins exactly those bytes — so a global install relocates **at write time**
/// rather than shipping a second, scope-variant payload. Only the two resource
/// subtrees that have a global meaning (`agents/`, `skills/`) move; anything
/// else, such as `.shipmates/contributor-steering.md`, stays put.
///
/// The resource is taken from the second path component rather than the first,
/// because pi keeps its crew and its skills in *different* payload dotdirs
/// (`.pi/agents/…` and `.agents/skills/…`) while both belong under
/// `~/.pi/agent/` once installed globally.
pub(crate) fn global_relocate(harness: &str, rel: &str) -> Option<String> {
    let root = global_root(harness)?;
    let mut parts = rel.splitn(3, '/');
    let _dotdir = parts.next()?;
    let resource = parts.next()?;
    let rest = parts.next()?;
    if resource != "agents" && resource != "skills" {
        return None;
    }
    Some(format!("{root}/{resource}/{rest}"))
}

/// Whether `target_dir` is the user's home directory — which is what `--global`
/// (the default) resolves to, and therefore what makes an install a *global* one.
///
/// Scope is inferred from the destination rather than threaded through as a flag,
/// because the destination is the thing that decides which tree the harness will
/// actually read.
pub(crate) fn is_global_target(target_dir: &Path) -> bool {
    let Some(home) = home::home_dir() else {
        return false;
    };
    match (std::fs::canonicalize(target_dir), std::fs::canonicalize(&home)) {
        (Ok(target), Ok(home)) => target == home,
        _ => target_dir == home,
    }
}

/// Rewrite a whole container-prefixed payload into the harness's global layout.
///
/// Returns the input unchanged for a harness that needs no relocation, so this is
/// safe to call unconditionally. It is applied once, before the plan, the receipt
/// and the migration table are derived from it — everything downstream then
/// agrees, which is the point: a receipt claiming workspace paths while the files
/// sat in the global tree is exactly the drift `doctor` exists to catch.
pub(crate) fn relocate_payload(
    harness: &str,
    container: &str,
    built: &HashMap<String, String>,
) -> HashMap<String, String> {
    if global_root(harness).is_none() {
        return built.clone();
    }
    let prefix = format!("{container}/");
    let mut out = HashMap::with_capacity(built.len());
    for (key, content) in built {
        let new_key = match key.strip_prefix(&prefix) {
            Some(rel) => match global_relocate(harness, rel) {
                Some(relocated) => format!("{prefix}{relocated}"),
                None => key.clone(),
            },
            None => key.clone(),
        };
        out.insert(new_key, content.clone());
    }
    out
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
                // Current shape: a DIRECTORY per agent, `<name>/agent.md`.
                // Antigravity discovers `{workspace}/.agents/agents/{name}/`
                // and reads the `agent.md` inside it.
                || (parts.len() == 4
                    && root == ".agents"
                    && parts[1] == "agents"
                    && parts[3] == "agent.md")
                // Legacy flat `<name>.md`, emitted before the shape was
                // corrected. Accepted on the read side so a pre-change receipt
                // still loads and `update`/`uninstall` can clear the files
                // Antigravity never read.
                || (parts.len() == 3
                    && root == ".agents"
                    && parts[1] == "agents"
                    && parts[2].ends_with(".md"))
                // Global scope: `~/.gemini/config/agents/<name>/agent.md` and
                // `~/.gemini/config/skills/<name>/…`. Tight for the same reason
                // as pi's: `~/.gemini/config/` also holds mcp_config.json,
                // hooks.json, workflows/ and plugins/, none of which are ours.
                || (parts.len() == 5
                    && root == ".gemini"
                    && parts[1] == "config"
                    && parts[2] == "agents"
                    && !parts[3].is_empty()
                    && parts[4] == "agent.md")
                || (parts.len() >= 5
                    && root == ".gemini"
                    && parts[1] == "config"
                    && parts[2] == "skills"
                    && !parts[3].is_empty()
                    && (parts[4] == "SKILL.md"
                        || (parts.len() == 5 && parts[4].ends_with(".py"))))
                || is_skill_tree(".agents")
        }
        "codex" => {
            is_steering_receipt_path(harness, path)
                || is_shipmates_steering_path(path)
                || (parts.len() == 3
                    && root == ".codex"
                    && parts[1] == "agents"
                    && parts[2].ends_with(".toml"))
                // Global scope: `~/.codex/skills/<name>/…`. Codex discovers
                // skills from `$CODEX_HOME/skills`, not from the shared
                // `.agents/skills` tree — that one is its *workspace* location.
                || (parts.len() >= 4
                    && root == ".codex"
                    && parts[1] == "skills"
                    && !parts[2].is_empty()
                    && (parts[3] == "SKILL.md"
                        || (parts.len() == 4 && parts[3].ends_with(".py"))))
                || is_skill_tree(".agents")
        }
        "cursor" => {
            is_steering_receipt_path(harness, path)
                || (parts.len() == 3
                    && root == ".cursor"
                    && parts[1] == "rules"
                    && parts[2].ends_with(".mdc"))
                // Cursor's skills live in its own tree, not the shared
                // `.agents/skills/` one: only `.cursor/skills/` is observed to
                // load (#405), and shipping both would double the picker (#403).
                || is_skill_tree(".cursor")
                // Still accepted on the read side so a pre-#405 receipt loads
                // and its `.agents` orphans can be removed on upgrade.
                || is_skill_tree(".agents")
        }
        "windsurf" => {
            is_steering_receipt_path(harness, path)
                || is_shipmates_steering_path(path)
                || is_skill_tree(".windsurf")
        }
        "pi" => {
            is_skill_tree(".agents")
                || (parts.len() == 3
                    && root == ".pi"
                    && parts[1] == "agents"
                    && parts[2].ends_with(".md"))
                // Global scope: `~/.pi/agent/agents/<name>.md` and
                // `~/.pi/agent/skills/<name>/…`. Kept as tight as the
                // workspace arms above: the harness's global tree is a lived-in
                // config directory, so only the two resource subtrees are
                // claimable, and only at the depths Shipmates writes.
                || (parts.len() == 4
                    && root == ".pi"
                    && parts[1] == "agent"
                    && parts[2] == "agents"
                    && parts[3].ends_with(".md"))
                || (parts.len() >= 5
                    && root == ".pi"
                    && parts[1] == "agent"
                    && parts[2] == "skills"
                    && !parts[3].is_empty()
                    && (parts[4] == "SKILL.md"
                        || (parts.len() == 5 && parts[4].ends_with(".py"))))
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
                // Global scope: `~/.copilot/agents/*.agent.md` and
                // `~/.copilot/skills/<name>/…`. Copilot's configuration dir is
                // `~/.copilot`; `.github/` is the workspace location only.
                || (parts.len() == 3
                    && root == ".copilot"
                    && parts[1] == "agents"
                    && parts[2].ends_with(".agent.md"))
                || (parts.len() >= 4
                    && root == ".copilot"
                    && parts[1] == "skills"
                    && !parts[2].is_empty()
                    && (parts[3] == "SKILL.md"
                        || (parts.len() == 4 && parts[3].ends_with(".py"))))
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
                if segment == ".shipmates" && !is_allowed_shipmates_path(path) {
                    bail!("{field} may not address .shipmates: {:?}", path);
                }
            }
            _ => bail!("{field} contains an unsafe path component: {:?}", path),
        }
    }
    Ok(())
}

/// Paths under `.shipmates/` that install may own (not the receipt store).
fn is_allowed_shipmates_path(path: &str) -> bool {
    path == ".shipmates" || path == ".shipmates/contributor-steering.md"
}

/// #295 installed steering at root instructions files; tolerate in old receipts.
fn is_legacy_instructions_root(root: &str) -> bool {
    root == "CLAUDE.md" || root == "AGENTS.md"
}

fn is_legacy_steering_receipt_path(harness: &str, path: &str) -> bool {
    match harness {
        "claude-code" => path == "CLAUDE.md",
        "opencode" | "codex" | "cursor" | "github-copilot" | "antigravity" | "pi" | "windsurf" => {
            path == "AGENTS.md"
        }
        _ => false,
    }
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
    fn cursor_receipt_owns_its_own_skill_tree_and_still_loads_legacy_agents_paths() {
        // #405: the cursor payload now lands in `.cursor/skills/` only.
        assert!(allowed_receipt_path(
            "cursor",
            ".cursor/skills/ship-issue/SKILL.md"
        ));
        // A pre-#405 receipt still validates, so install can load it and clear
        // the orphaned shared-tree copies instead of failing closed.
        assert!(allowed_receipt_path(
            "cursor",
            ".agents/skills/ship-issue/SKILL.md"
        ));
        // The widening is cursor-only and skills-only: no other harness may
        // claim a `.cursor` path, and cursor still can't claim arbitrary ones.
        for harness in ["codex", "github-copilot", "antigravity", "claude-code"] {
            assert!(!allowed_receipt_path(
                harness,
                ".cursor/skills/ship-issue/SKILL.md"
            ));
        }
        assert!(!allowed_receipt_path("cursor", ".cursor/mcp.json"));
        assert!(!allowed_receipt_path("cursor", ".cursor/skills/evil.sh"));
    }

    #[test]
    fn global_relocation_moves_only_resource_subtrees_into_the_harness_global_root() {
        // Antigravity's global tree is `~/.gemini/config/`, not `.agents/` at
        // home — joining the workspace path to $HOME wrote files nothing reads.
        assert_eq!(
            global_relocate("antigravity", ".agents/agents/sdet/agent.md").as_deref(),
            Some(".gemini/config/agents/sdet/agent.md")
        );
        assert_eq!(
            global_relocate("antigravity", ".agents/skills/ship-issue/SKILL.md").as_deref(),
            Some(".gemini/config/skills/ship-issue/SKILL.md")
        );
        // pi keeps crew and skills in *different* payload dotdirs, and both
        // belong under `~/.pi/agent/` once installed globally.
        assert_eq!(
            global_relocate("pi", ".pi/agents/sdet.md").as_deref(),
            Some(".pi/agent/agents/sdet.md")
        );
        assert_eq!(
            global_relocate("pi", ".agents/skills/ship-issue/SKILL.md").as_deref(),
            Some(".pi/agent/skills/ship-issue/SKILL.md")
        );
        // A harness whose global tree IS its workspace dotdir at home keeps no
        // entry, so relocation is a no-op and stays a no-op.
        for harness in ["claude-code", "cursor", "windsurf", "opencode"] {
            assert_eq!(global_root(harness), None, "{harness} must declare no global root");
            assert_eq!(
                global_relocate(harness, ".claude/agents/sdet.md"),
                None,
                "{harness} must not be relocated"
            );
        }
        // Codex's crew already live under its own home; only its skills move.
        assert_eq!(
            global_relocate("codex", ".codex/agents/sdet.toml").as_deref(),
            Some(".codex/agents/sdet.toml")
        );
        assert_eq!(
            global_relocate("codex", ".agents/skills/ship-issue/SKILL.md").as_deref(),
            Some(".codex/skills/ship-issue/SKILL.md")
        );
        // Copilot's crew and skills both leave `.github/`/`.agents/` for `~/.copilot`.
        assert_eq!(
            global_relocate("github-copilot", ".github/agents/sdet.agent.md").as_deref(),
            Some(".copilot/agents/sdet.agent.md")
        );
        assert_eq!(
            global_relocate("github-copilot", ".agents/skills/ship-issue/SKILL.md").as_deref(),
            Some(".copilot/skills/ship-issue/SKILL.md")
        );
        // Only the two resource subtrees move; steering and anything else stay.
        assert_eq!(global_relocate("pi", ".shipmates/contributor-steering.md"), None);
        assert_eq!(
            global_relocate("antigravity", ".shipmates/contributor-steering.md"),
            None
        );
        assert_eq!(
            global_relocate("github-copilot", ".github/instructions/shipmates.instructions.md"),
            None
        );
    }

    #[test]
    fn global_receipt_paths_are_claimable_narrowly() {
        assert!(allowed_receipt_path("pi", ".pi/agent/agents/sdet.md"));
        assert!(allowed_receipt_path(
            "pi",
            ".pi/agent/skills/ship-issue/SKILL.md"
        ));
        assert!(allowed_receipt_path(
            "antigravity",
            ".gemini/config/agents/sdet/agent.md"
        ));
        assert!(allowed_receipt_path(
            "antigravity",
            ".gemini/config/skills/ship-issue/SKILL.md"
        ));
        // The global tree is a lived-in config directory: claim only the two
        // resource subtrees under it, never a settings file or an arbitrary one.
        assert!(!allowed_receipt_path("pi", ".pi/agent/settings.json"));
        assert!(!allowed_receipt_path("pi", ".pi/agent/agents/evil.sh"));
        assert!(!allowed_receipt_path("antigravity", ".gemini/config/mcp_config.json"));
        assert!(!allowed_receipt_path(
            "antigravity",
            ".gemini/config/agents/evil.sh"
        ));
        // And no other harness may claim either global tree.
        for harness in ["codex", "claude-code", "cursor", "opencode", "windsurf"] {
            assert!(!allowed_receipt_path(harness, ".pi/agent/agents/sdet.md"), "{harness}");
            assert!(
                !allowed_receipt_path(harness, ".gemini/config/agents/sdet/agent.md"),
                "{harness}"
            );
        }
        // codex and copilot keep their own global trees, and each is narrow.
        assert!(allowed_receipt_path("codex", ".codex/skills/ship-issue/SKILL.md"));
        assert!(!allowed_receipt_path("codex", ".codex/config.toml"));
        assert!(allowed_receipt_path(
            "github-copilot",
            ".copilot/agents/sdet.agent.md"
        ));
        assert!(allowed_receipt_path(
            "github-copilot",
            ".copilot/skills/ship-issue/SKILL.md"
        ));
        assert!(!allowed_receipt_path("github-copilot", ".copilot/settings.json"));
        assert!(!allowed_receipt_path(
            "github-copilot",
            ".copilot/agents/sdet.md"
        ));
    }

    #[test]
    fn antigravity_crew_is_a_directory_per_agent_and_the_flat_shape_still_loads() {
        // Antigravity discovers `{workspace}/.agents/agents/{name}/` and reads
        // the `agent.md` inside it.
        assert!(allowed_receipt_path(
            "antigravity",
            ".agents/agents/sdet/agent.md"
        ));
        // A sibling file in that directory is not ours to own.
        assert!(!allowed_receipt_path(
            "antigravity",
            ".agents/agents/sdet/notes.md"
        ));
        // The legacy flat shape is still claimable on the read side, so a
        // pre-correction receipt loads and its dead files can be cleared.
        assert!(allowed_receipt_path("antigravity", ".agents/agents/sdet.md"));
    }

    /// The registry documents each harness's global root; the installer is what
    /// honours it. Keep them in step, or a documented scope path silently stops
    /// being the path that is written.
    #[test]
    fn registry_global_roots_match_the_installer() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tools/capability_registry.json");
        let registry: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        for (name, entry) in registry["harnesses"].as_object().unwrap() {
            let documented = entry
                .get("global_root")
                .and_then(|value| value.as_str())
                .filter(|value| !value.is_empty());
            assert_eq!(
                documented,
                global_root(name),
                "{name}: capability_registry.json's global_root disagrees with manifest_db::global_root"
            );
        }
    }

    #[test]
    fn pi_receipt_owns_its_crew_tree_and_still_loads_legacy_skill_paths() {
        // #437: pi's crew moved to `.pi/agents/` in pi's own dialect. It cannot
        // live in the shared `.agents/agents/` tree, which is Antigravity's and
        // which pi reads as a legacy location — the two harnesses need
        // incompatible `tools:` shapes, so one file cannot serve both.
        assert!(allowed_receipt_path("pi", ".pi/agents/sdet.md"));
        // A pre-#437 receipt recorded only `.agents/` paths. It must still
        // validate, so `update` can refresh that install and `uninstall` can
        // clear it rather than failing closed on a tree nobody can manage.
        assert!(allowed_receipt_path(
            "pi",
            ".agents/skills/ship-issue/SKILL.md"
        ));
        // The new tree is pi-specific and `.agents` is legacy-only for pi: no
        // other harness may claim a `.pi` path, and pi may not claim arbitrary
        // ones under it.
        for harness in [
            "codex",
            "github-copilot",
            "antigravity",
            "claude-code",
            "cursor",
            "opencode",
        ] {
            assert!(!allowed_receipt_path(harness, ".pi/agents/sdet.md"));
        }
        assert!(!allowed_receipt_path("pi", ".pi/settings.json"));
        assert!(!allowed_receipt_path("pi", ".pi/agents/evil.sh"));
        assert!(!allowed_receipt_path("pi", ".pi/agents/nested/sdet.md"));
    }

    #[test]
    fn cursor_receipt_round_trips_a_native_skill_path() {
        let dir = tempdir().unwrap();
        let repository = ReceiptRepository::new(dir.path());
        let expected = InstallReceipt::new(
            "0.1.3",
            "cursor",
            LAYOUT_SKILLS,
            vec![".cursor".into()],
            vec![file(".cursor/skills/a/SKILL.md", HASH)],
        )
        .unwrap();

        repository.save(&expected).unwrap();

        assert_eq!(repository.load("cursor").unwrap(), Some(expected));
    }

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
