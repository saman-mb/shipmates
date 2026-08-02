pub mod manifest_db;

use std::fs;
use std::path::Path;

pub fn atomic_write(path: &Path, content: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temp_path = path.with_extension("tmp");
    fs::write(&temp_path, content)?;
    fs::rename(temp_path, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_atomic_write() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("test_file.txt");
        let content = "hello world";
        
        atomic_write(&file_path, content).unwrap();
        
        assert_eq!(fs::read_to_string(&file_path).unwrap(), content);
        // temp file shouldn't exist
        assert!(!dir.path().join("test_file.tmp").exists());
    }
}
