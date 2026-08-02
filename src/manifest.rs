use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

#[derive(Debug, Deserialize, Serialize)]
#[allow(dead_code)]
pub struct Manifest {
    pub schema_version: u32,
    pub schema: String,
    pub project_instructions: HashMap<String, String>,
    pub crew: CrewConfig,
    pub commands: CommandsConfig,
    pub targets: Vec<String>,
    pub target_status: HashMap<String, String>,
    pub compatibility: Option<CompatibilityConfig>,
}

#[derive(Debug, Deserialize, Serialize)]
#[allow(dead_code)]
pub struct CrewConfig {
    pub canonical_root: String,
    pub format: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[allow(dead_code)]
pub struct CommandsConfig {
    pub canonical_root: String,
    pub format: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[allow(dead_code)]
pub struct CompatibilityConfig {
    pub target: String,
    pub roots: Vec<String>,
    pub exempt: Vec<String>,
}

#[allow(dead_code)]
pub fn load_manifest(root: &Path) -> Result<Manifest, String> {
    let manifest_path = root.join("canonical").join("manifest.json");
    let content = fs::read_to_string(&manifest_path).map_err(|e| format!("Failed to read manifest: {}", e))?;
    let manifest: Manifest = serde_json::from_str(&content).map_err(|e| format!("Failed to parse manifest: {}", e))?;
    Ok(manifest)
}

#[derive(Debug, Deserialize, Serialize)]
#[allow(dead_code)]
pub struct CapabilityRegistry {
    pub schema_version: u32,
    pub capabilities: Vec<String>,
    pub harnesses: HashMap<String, HarnessCapabilities>,
}

#[derive(Debug, Deserialize, Serialize)]
#[allow(dead_code)]
pub struct HarnessCapabilities {
    pub agent_path: String,
    pub skill_path: String,
    pub project_instructions: String,
    pub permission_model: String,
    pub scopes: HashMap<String, String>,
    pub tools: serde_json::Value,
}

#[allow(dead_code)]
pub fn load_capability_registry(root: &Path) -> Result<CapabilityRegistry, String> {
    let registry_path = root.join("tools").join("capability_registry.json");
    let content = fs::read_to_string(&registry_path).map_err(|e| format!("Failed to read registry: {}", e))?;
    let registry: CapabilityRegistry = serde_json::from_str(&content).map_err(|e| format!("Failed to parse registry: {}", e))?;
    Ok(registry)
}
