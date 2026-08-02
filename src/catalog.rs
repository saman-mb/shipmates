use regex::Regex;
use std::collections::{HashMap, HashSet};
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
    pub loop_max: usize,
    pub stages: Vec<serde_json::Value>,
    pub narrative: String,
    pub invocation: String,
    pub board: String,
    pub source: PathBuf,
}

pub fn reject_positional(label: &str, text: &str) -> Result<(), String> {
    for (i, line) in text.lines().enumerate() {
        let mut prev_char = ' ';
        for (j, c) in line.chars().enumerate() {
            if c == '$' && prev_char != '\\' {
                let rest = &line[j+1..];
                if rest.starts_with('{') {
                    if let Some(c2) = rest.chars().nth(1)
                        && c2.is_ascii_digit() {
                            return Err(format!("{}:{}: a command has no positional arguments", label, i + 1));
                        }
                } else {
                    if let Some(c2) = rest.chars().next()
                        && c2.is_ascii_digit() {
                            return Err(format!("{}:{}: a command has no positional arguments", label, i + 1));
                        }
                }
            }
            prev_char = c;
        }
    }
    Ok(())
}

pub fn parse_frontmatter(path: &Path) -> Result<(HashMap<String, String>, String), String> {
    let content = fs::read_to_string(path).map_err(|e| e.to_string())?;
    parse_frontmatter_from(&content, &path.to_string_lossy())
}

pub fn parse_frontmatter_from(content: &str, label: &str) -> Result<(HashMap<String, String>, String), String> {
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
    let remaining = content.lines().skip(close_idx + 1).collect::<Vec<&str>>().join("\n");
    Ok((values, remaining))
}

pub fn load_roles(path: &Path) -> anyhow::Result<Vec<CanonicalRole>> {
    let mut roles = Vec::new();
    let _re = Regex::new(r"^[a-z0-9-]+$")?;
    let _ = HashSet::<String>::new();
    if !path.exists() {
        return Ok(roles);
    }    for entry in walkdir::WalkDir::new(path).into_iter().filter_map(|e| e.ok()) {
        if entry.path().is_file() && entry.path().extension().is_some_and(|ext| ext == "md") {
            let (fm, body) = parse_frontmatter(entry.path()).map_err(|e| anyhow::anyhow!(e))?;
            roles.push(CanonicalRole {
                name: fm.get("name").cloned().unwrap_or_default(),
                description: fm.get("description").cloned().unwrap_or_default(),
                capabilities: fm.get("capabilities").map(|s| s.split(',').map(|s| s.trim().to_string()).collect()).unwrap_or_default(),
                writes: fm.get("writes").map(|s| s == "true").unwrap_or(false),
                web_scopes: Vec::new(),
                read_scopes: Vec::new(),
                tool_order: Vec::new(),
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
    for entry in walkdir::WalkDir::new(path).into_iter().filter_map(|e| e.ok()) {
        if entry.path().is_file() && entry.path().extension().is_some_and(|ext| ext == "md") {
            let (fm, body) = parse_frontmatter(entry.path()).map_err(|e| anyhow::anyhow!(e))?;
            reject_positional(&entry.path().to_string_lossy(), &body).map_err(|e| anyhow::anyhow!(e))?;
            commands.push(CanonicalCommand {
                name: fm.get("name").cloned().unwrap_or_default(),
                description: fm.get("description").cloned().unwrap_or_default(),
                argument_hint: fm.get("argument-hint").cloned().unwrap_or_default(),
                allowed_tools: fm.get("allowed-tools").cloned().unwrap_or_default(),
                disable_model_invocation: fm.get("disable-model-invocation").map(|s| s == "true").unwrap_or(false),
                arguments: Vec::new(),
                loop_max: 0,
                stages: Vec::new(),
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
            let (fm, body) = parse_frontmatter_from(content, rel).map_err(|e| anyhow::anyhow!(e))?;
            roles.push(CanonicalRole {
                name: fm.get("name").cloned().unwrap_or_else(|| name.to_string()),
                description: fm.get("description").cloned().unwrap_or_default(),
                capabilities: fm.get("capabilities").map(|s| s.split(',').map(|s| s.trim().to_string()).collect()).unwrap_or_default(),
                writes: fm.get("writes").map(|s| s == "true").unwrap_or(false),
                web_scopes: Vec::new(),
                read_scopes: Vec::new(),
                tool_order: Vec::new(),
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
            let (fm, body) = parse_frontmatter_from(content, rel).map_err(|e| anyhow::anyhow!(e))?;
            reject_positional(rel, &body).map_err(|e| anyhow::anyhow!(e))?;
            commands.push(CanonicalCommand {
                name: fm.get("name").cloned().unwrap_or_else(|| name.to_string()),
                description: fm.get("description").cloned().unwrap_or_default(),
                argument_hint: fm.get("argument-hint").cloned().unwrap_or_default(),
                allowed_tools: fm.get("allowed-tools").cloned().unwrap_or_default(),
                disable_model_invocation: fm.get("disable-model-invocation").map(|s| s == "true").unwrap_or(false),
                arguments: Vec::new(),
                loop_max: 0,
                stages: Vec::new(),
                narrative: body,
                invocation: String::new(),
                board: String::new(),
                source: std::path::PathBuf::from(rel),
            });
        }
    }
    Ok(commands)
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
}
