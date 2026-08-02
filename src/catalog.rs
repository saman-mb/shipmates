use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
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

#[derive(Debug, Clone)]
pub struct Manifest {
    pub targets: Vec<String>,
    pub target_status: HashMap<String, String>,
    pub crew_canonical_root: PathBuf,
    pub commands_canonical_root: PathBuf,
}

pub fn reject_positional(label: &str, text: &str) -> Result<(), String> {
    for (i, line) in text.lines().enumerate() {
        let mut prev_char = ' ';
        for (j, c) in line.chars().enumerate() {
            if c == '$' && prev_char != '\\' {
                let rest = &line[j+1..];
                if rest.starts_with('{') {
                    if let Some(c2) = rest.chars().nth(1) {
                        if c2.is_ascii_digit() {
                            return Err(format!("{}:{}: a command has no positional arguments", label, i + 1));
                        }
                    }
                } else {
                    if let Some(c2) = rest.chars().next() {
                        if c2.is_ascii_digit() {
                            return Err(format!("{}:{}: a command has no positional arguments", label, i + 1));
                        }
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
    let mut lines = content.lines();
    if lines.next().unwrap_or("").trim() != "---" {
        return Err(format!("{:?}: missing opening frontmatter", path));
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
            return Err(format!("{:?}:{}: expected `key: value`", path, i + 2));
        }
        let key = parts[0].trim().to_string();
        let value = parts[1].trim().to_string();
        if values.contains_key(&key) {
            return Err(format!("{:?}:{}: duplicate key {:?}", path, i + 2, key));
        }
        values.insert(key, value);
    }
    if close_idx == 0 {
        return Err(format!("{:?}: unterminated frontmatter", path));
    }
    let remaining = content.lines().skip(close_idx + 1).collect::<Vec<&str>>().join("\n");
    Ok((values, remaining))
}
