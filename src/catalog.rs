use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct CanonicalRole {
    pub name: String,
    pub description: String,
    pub capabilities: Vec<String>,
    pub writes: bool,
    pub web_scopes: Vec<String>,
    pub read_scopes: Vec<String>,
    pub tool_order: Vec<String>,
    /// Static per-role reasoning effort (`low|medium|high`), stamped into the
    /// frontmatter of the harnesses whose agent format carries the key. `None`
    /// emits nothing. A model is never emitted — that is a runtime decision
    /// (#205); effort is the one static per-role knob (#204).
    pub effort: Option<String>,
    pub source: PathBuf,
    pub body: String,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct CanonicalCommand {
    pub name: String,
    pub description: String,
    pub argument_hint: String,
    pub allowed_tools: String,
    pub disable_model_invocation: bool,
    pub arguments: Vec<String>,
    pub narrative: String,
    pub invocation: String,
    pub board: String,
    pub source: PathBuf,
}

/// A tool: an agent-invoked capability the crew reaches for implicitly, never a
/// slash command. Defined once, harness-neutrally, as `toolbox/<name>/tool.md`
/// plus any bundled runnable assets (a script, templates). Each adapter maps it
/// to that harness's native tool surface — opencode's `.opencode/tools/*.ts`,
/// or a model-invoked Agent Skill elsewhere.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct CanonicalTool {
    pub name: String,
    pub description: String,
    pub body: String,
    /// (relative filename, contents) for every bundled file except `tool.md`.
    pub assets: Vec<(String, String)>,
    /// Runtime packages the tool self-provisions (from the `requires:` frontmatter
    /// key, comma-separated). The installer pre-warms these so an installed tool
    /// works without the user pip-installing anything.
    pub requires: Vec<String>,
    pub source: PathBuf,
}

pub fn reject_positional(label: &str, text: &str) -> Result<(), String> {
    for (i, line) in text.lines().enumerate() {
        let mut prev_char = ' ';
        for (j, c) in line.chars().enumerate() {
            if c == '$' && prev_char != '\\' {
                let rest = &line[j + 1..];
                if rest.starts_with('{') {
                    if let Some(c2) = rest.chars().nth(1)
                        && c2.is_ascii_digit()
                    {
                        return Err(format!(
                            "{}:{}: a command has no positional arguments",
                            label,
                            i + 1
                        ));
                    }
                } else {
                    if let Some(c2) = rest.chars().next()
                        && c2.is_ascii_digit()
                    {
                        return Err(format!(
                            "{}:{}: a command has no positional arguments",
                            label,
                            i + 1
                        ));
                    }
                }
            }
            prev_char = c;
        }
    }
    Ok(())
}

fn parse_list(fm: &HashMap<String, String>, key: &str) -> Vec<String> {
    fm.get(key)
        .map(|value| {
            value
                .split(',')
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

pub fn validate_role_name(name: &str, label: &str) -> anyhow::Result<()> {
    let valid = !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !name.starts_with('-')
        && !name.ends_with('-')
        && !name.contains("--");
    if !valid {
        anyhow::bail!("{}: invalid role name {:?}", label, name);
    }
    Ok(())
}

/// Parse and validate the `effort:` frontmatter key. Empty/absent ⇒ `None`.
/// Any value other than `low|medium|high` is rejected so a typo fails the build
/// rather than emitting an effort the target silently ignores.
fn parse_effort(fm: &HashMap<String, String>, label: &str) -> anyhow::Result<Option<String>> {
    let effort = fm
        .get("effort")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    // The accepted set is deliberately the cross-harness intersection
    // `low|medium|high`: Claude Code alone supports more (up to `max`), but one
    // canonical value ships to every harness, so widening it would emit an
    // effort codex/opencode reject.
    if let Some(v) = &effort
        && !matches!(v.as_str(), "low" | "medium" | "high")
    {
        anyhow::bail!(
            "{}: invalid effort {:?}; expected one of low|medium|high",
            label,
            v
        );
    }
    Ok(effort)
}

/// Comma-separated `requires:` frontmatter → the tool's runtime package list.
fn parse_requires(fm: &HashMap<String, String>) -> Vec<String> {
    fm.get("requires")
        .map(|s| {
            s.split(',')
                .map(|x| x.trim().to_string())
                .filter(|x| !x.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

pub fn parse_frontmatter(path: &Path) -> Result<(HashMap<String, String>, String), String> {
    let content = fs::read_to_string(path).map_err(|e| e.to_string())?;
    parse_frontmatter_from(&content, &path.to_string_lossy())
}

pub fn parse_frontmatter_from(
    content: &str,
    label: &str,
) -> Result<(HashMap<String, String>, String), String> {
    let mut lines = content.lines();
    if lines.next().unwrap_or("").trim() != "---" {
        return Err(format!("{:?}: missing opening frontmatter", label));
    }
    let mut values = HashMap::new();
    let mut close_idx = 0;
    for (i, line) in lines.clone().enumerate() {
        if line.trim() == "---" {
            close_idx = i + 1; // plus 1 for the first line we skipped
            break;
        }
        if line.trim().is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.splitn(2, ':').collect();
        if parts.len() != 2 {
            return Err(format!("{:?}:{}: expected `key: value`", label, i + 2));
        }
        let key = parts[0].trim().to_string();
        let value = parts[1].trim().to_string();
        if values.contains_key(&key) {
            return Err(format!("{:?}:{}: duplicate key {:?}", label, i + 2, key));
        }
        values.insert(key, value);
    }
    if close_idx == 0 {
        return Err(format!("{:?}: unterminated frontmatter", label));
    }
    let remaining = content
        .lines()
        .skip(close_idx + 1)
        .collect::<Vec<&str>>()
        .join("\n");
    Ok((values, remaining))
}

pub fn load_roles(path: &Path) -> anyhow::Result<Vec<CanonicalRole>> {
    let mut roles = Vec::new();
    if !path.exists() {
        return Ok(roles);
    }
    for entry in walkdir::WalkDir::new(path)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if entry.path().is_file() && entry.path().extension().is_some_and(|ext| ext == "md") {
            let (fm, body) = parse_frontmatter(entry.path()).map_err(|e| anyhow::anyhow!(e))?;
            let name = fm.get("name").cloned().unwrap_or_default();
            validate_role_name(&name, &entry.path().to_string_lossy())?;
            roles.push(CanonicalRole {
                name,
                description: fm.get("description").cloned().unwrap_or_default(),
                capabilities: fm
                    .get("capabilities")
                    .map(|s| s.split(',').map(|s| s.trim().to_string()).collect())
                    .unwrap_or_default(),
                writes: fm.get("writes").map(|s| s == "true").unwrap_or(false),
                web_scopes: parse_list(&fm, "web-scopes"),
                read_scopes: parse_list(&fm, "read-scopes"),
                tool_order: parse_list(&fm, "tool-order"),
                effort: parse_effort(&fm, &entry.path().to_string_lossy())?,
                source: entry.path().to_path_buf(),
                body,
            });
        }
    }
    Ok(roles)
}

pub fn load_commands(path: &Path) -> anyhow::Result<Vec<CanonicalCommand>> {
    let mut commands = Vec::new();
    if !path.exists() {
        return Ok(commands);
    }
    for entry in walkdir::WalkDir::new(path)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if entry.path().is_file() && entry.path().extension().is_some_and(|ext| ext == "md") {
            let (fm, body) = parse_frontmatter(entry.path()).map_err(|e| anyhow::anyhow!(e))?;
            reject_positional(&entry.path().to_string_lossy(), &body)
                .map_err(|e| anyhow::anyhow!(e))?;
            commands.push(CanonicalCommand {
                name: fm.get("name").cloned().unwrap_or_default(),
                description: fm.get("description").cloned().unwrap_or_default(),
                argument_hint: fm.get("argument-hint").cloned().unwrap_or_default(),
                allowed_tools: fm.get("allowed-tools").cloned().unwrap_or_default(),
                disable_model_invocation: fm
                    .get("disable-model-invocation")
                    .map(|s| s == "true")
                    .unwrap_or(false),
                arguments: parse_list(&fm, "arguments"),
                narrative: body,
                invocation: String::new(),
                board: String::new(),
                source: entry.path().to_path_buf(),
            });
        }
    }
    Ok(commands)
}

/// Roles compiled into the binary at build time by `build.rs` — the payload
/// a `brew`/`cargo`-installed `shipmates` ships, since it has no checkout to
/// read `crew/` from at runtime.
pub fn load_roles_embedded() -> anyhow::Result<Vec<CanonicalRole>> {
    let mut roles = Vec::new();
    for (rel, content) in crate::embedded::embedded_sources() {
        if let Some(name) = rel.strip_prefix("crew/") {
            let (fm, body) =
                parse_frontmatter_from(content, rel).map_err(|e| anyhow::anyhow!(e))?;
            let name = fm.get("name").cloned().unwrap_or_else(|| name.to_string());
            validate_role_name(&name, rel)?;
            roles.push(CanonicalRole {
                name,
                description: fm.get("description").cloned().unwrap_or_default(),
                capabilities: fm
                    .get("capabilities")
                    .map(|s| s.split(',').map(|s| s.trim().to_string()).collect())
                    .unwrap_or_default(),
                writes: fm.get("writes").map(|s| s == "true").unwrap_or(false),
                web_scopes: parse_list(&fm, "web-scopes"),
                read_scopes: parse_list(&fm, "read-scopes"),
                tool_order: parse_list(&fm, "tool-order"),
                effort: parse_effort(&fm, rel)?,
                source: std::path::PathBuf::from(rel),
                body,
            });
        }
    }
    Ok(roles)
}

/// Commands compiled into the binary at build time by `build.rs`.
pub fn load_commands_embedded() -> anyhow::Result<Vec<CanonicalCommand>> {
    let mut commands = Vec::new();
    for (rel, content) in crate::embedded::embedded_sources() {
        if let Some(name) = rel.strip_prefix("commands/") {
            let (fm, body) =
                parse_frontmatter_from(content, rel).map_err(|e| anyhow::anyhow!(e))?;
            reject_positional(rel, &body).map_err(|e| anyhow::anyhow!(e))?;
            commands.push(CanonicalCommand {
                name: fm.get("name").cloned().unwrap_or_else(|| name.to_string()),
                description: fm.get("description").cloned().unwrap_or_default(),
                argument_hint: fm.get("argument-hint").cloned().unwrap_or_default(),
                allowed_tools: fm.get("allowed-tools").cloned().unwrap_or_default(),
                disable_model_invocation: fm
                    .get("disable-model-invocation")
                    .map(|s| s == "true")
                    .unwrap_or(false),
                arguments: parse_list(&fm, "arguments"),
                narrative: body,
                invocation: String::new(),
                board: String::new(),
                source: std::path::PathBuf::from(rel),
            });
        }
    }
    Ok(commands)
}

/// Tools loaded from an on-disk `toolbox/` tree (the repo dev loop).
pub fn load_tools(path: &Path) -> anyhow::Result<Vec<CanonicalTool>> {
    let mut tools = Vec::new();
    if !path.exists() {
        return Ok(tools);
    }
    let mut dirs: Vec<PathBuf> = fs::read_dir(path)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    for dir in dirs {
        let tool_md = dir.join("tool.md");
        if !tool_md.is_file() {
            continue;
        }
        let (fm, body) = parse_frontmatter(&tool_md).map_err(|e| anyhow::anyhow!(e))?;
        let name = fm
            .get("name")
            .cloned()
            .unwrap_or_else(|| dir.file_name().unwrap().to_string_lossy().to_string());
        let mut assets: Vec<(String, String)> = Vec::new();
        for entry in walkdir::WalkDir::new(&dir)
            .sort_by_file_name()
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let p = entry.path();
            if p.is_file() && p != tool_md {
                let rel = p
                    .strip_prefix(&dir)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                let content = fs::read_to_string(p).map_err(|e| anyhow::anyhow!(e))?;
                assets.push((rel, content));
            }
        }
        tools.push(CanonicalTool {
            name,
            description: fm.get("description").cloned().unwrap_or_default(),
            body,
            assets,
            requires: parse_requires(&fm),
            source: tool_md,
        });
    }
    Ok(tools)
}

/// Tools compiled into the binary at build time by `build.rs` (the brew/cargo
/// install path, which has no checkout to read `toolbox/` from).
pub fn load_tools_embedded() -> anyhow::Result<Vec<CanonicalTool>> {
    use std::collections::BTreeMap;
    // Group embedded `toolbox/<dir>/<file>` entries by their tool directory.
    let mut groups: BTreeMap<String, Vec<(String, &str)>> = BTreeMap::new();
    for (rel, content) in crate::embedded::embedded_sources() {
        if let Some(rest) = rel.strip_prefix("toolbox/")
            && let Some((dir, file_rel)) = rest.split_once('/')
        {
            groups
                .entry(dir.to_string())
                .or_default()
                .push((file_rel.to_string(), content));
        }
    }
    let mut tools = Vec::new();
    for (dir, mut files) in groups {
        files.sort();
        let mut name = dir.clone();
        let mut description = String::new();
        let mut body = String::new();
        let mut requires: Vec<String> = Vec::new();
        let mut assets: Vec<(String, String)> = Vec::new();
        let mut found = false;
        for (file_rel, content) in files {
            if file_rel == "tool.md" {
                let (fm, b) = parse_frontmatter_from(content, &format!("toolbox/{}/tool.md", dir))
                    .map_err(|e| anyhow::anyhow!(e))?;
                name = fm.get("name").cloned().unwrap_or(dir.clone());
                description = fm.get("description").cloned().unwrap_or_default();
                requires = parse_requires(&fm);
                body = b;
                found = true;
            } else {
                assets.push((file_rel, content.to_string()));
            }
        }
        if !found {
            continue;
        }
        tools.push(CanonicalTool {
            name,
            description,
            body,
            assets,
            requires,
            source: PathBuf::from(format!("toolbox/{}/tool.md", dir)),
        });
    }
    Ok(tools)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reject_positional() {
        assert!(reject_positional("test", "normal text").is_ok());
        assert!(reject_positional("test", "some $1 text").is_err());
        assert!(reject_positional("test", "some ${1} text").is_err());
        assert!(reject_positional("test", "some \\$1 text").is_ok()); // escaped
    }

    #[test]
    fn test_parse_frontmatter_valid() {
        use std::io::Write;
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(file, "---").unwrap();
        writeln!(file, "name: test").unwrap();
        writeln!(file, "description: test desc").unwrap();
        writeln!(file, "---").unwrap();
        writeln!(file, "body text").unwrap();

        let (fm, body) = parse_frontmatter(file.path()).unwrap();
        assert_eq!(fm.get("name").unwrap(), "test");
        assert_eq!(fm.get("description").unwrap(), "test desc");
        assert_eq!(body, "body text");
    }

    #[test]
    fn test_parse_frontmatter_invalid() {
        use std::io::Write;
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(file, "no frontmatter here").unwrap();

        assert!(parse_frontmatter(file.path()).is_err());
    }

    #[test]
    fn test_effort_valid_is_parsed_and_absent_is_none() {
        let mut fm = HashMap::new();
        assert_eq!(parse_effort(&fm, "x").unwrap(), None);
        fm.insert("effort".to_string(), "  high  ".to_string());
        assert_eq!(parse_effort(&fm, "x").unwrap().as_deref(), Some("high"));
        fm.insert("effort".to_string(), "".to_string());
        assert_eq!(parse_effort(&fm, "x").unwrap(), None);
    }

    #[test]
    fn test_loader_rejects_invalid_effort() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.md");
        fs::write(
            &path,
            "---\nname: bad\ndescription: d\neffort: extreme\n---\nbody",
        )
        .unwrap();
        let err = load_roles(dir.path()).unwrap_err();
        assert!(err.to_string().contains("invalid effort"), "{err}");
    }

    #[test]
    fn test_loader_preserves_scope_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("role.md");
        fs::write(
            &path,
            "---\nname: role\ndescription: d\ncapabilities: read,web\nweb-scopes: search\nread-scopes: read\ntool-order: bash,read\n---\nbody",
        )
        .unwrap();
        let roles = load_roles(dir.path()).unwrap();
        assert_eq!(roles[0].web_scopes, vec!["search"]);
        assert_eq!(roles[0].read_scopes, vec!["read"]);
        assert_eq!(roles[0].tool_order, vec!["bash", "read"]);
    }

    #[test]
    fn test_loader_rejects_path_traversal_role_name() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("role.md");
        fs::write(&path, "---\nname: ../escape\ndescription: d\n---\nbody").unwrap();
        let err = load_roles(dir.path()).unwrap_err();
        assert!(err.to_string().contains("invalid role name"), "{err}");
    }
}
