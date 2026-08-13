use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;

#[allow(dead_code)]
pub fn compute_sha256(path: &Path) -> Result<String, std::io::Error> {
    let content = fs::read(path)?;
    Ok(hash_bytes(&content))
}

/// Return the lowercase SHA-256 digest of raw bytes.
pub fn hash_bytes(content: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content);
    hex::encode(hasher.finalize())
}

pub fn hash(content: &str) -> String {
    hash_bytes(content.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn hash_bytes_hashes_binary_content() {
        let bytes = [0, 1, 2, 0xff];
        assert_eq!(
            hash_bytes(&bytes),
            "3d1f57c984978ef98a18378c8166c1cb8ede02c03eeb6aee7e2f121dfeee3e56"
        );
    }

    #[test]
    fn compute_sha256_uses_raw_file_bytes() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("payload");
        let bytes = [0, 1, 2, 0xff];
        fs::write(&path, bytes).unwrap();

        assert_eq!(compute_sha256(&path).unwrap(), hash_bytes(&bytes));
    }
}
