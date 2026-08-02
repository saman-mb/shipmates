use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;

#[allow(dead_code)]
pub fn compute_sha256(path: &Path) -> Result<String, std::io::Error> {
    let content = fs::read(path)?;
    let mut hasher = Sha256::new();
    hasher.update(&content);
    Ok(hex::encode(hasher.finalize()))
}

pub fn hash(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    hex::encode(hasher.finalize())
}
