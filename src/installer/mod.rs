pub mod manifest_db;
pub mod migrate;

use std::fs::{self, File};
use std::io::Write;
use std::path::Path;

/// Write `content` to `path` atomically and durably.
///
/// The bytes go to a temp file which is `fsync`'d before being `rename`d into
/// place; the parent directory is then `fsync`'d (Unix). Those fsyncs are what
/// make "a backup is verified on disk before its original is deleted" hold
/// across a crash or power loss, not merely within the process — without them a
/// rename can be durable while the file's contents are not.
pub fn atomic_write(path: &Path, content: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temp_path = path.with_extension("tmp");
    {
        let mut file = File::create(&temp_path)?;
        file.write_all(content.as_bytes())?;
        file.sync_all()?;
    }
    fs::rename(&temp_path, path)?;
    // fsync the parent directory so the rename entry itself survives a crash.
    #[cfg(unix)]
    if let Some(parent) = path.parent() {
        let dir = if parent.as_os_str().is_empty() {
            Path::new(".")
        } else {
            parent
        };
        if let Ok(dir_file) = File::open(dir) {
            let _ = dir_file.sync_all();
        }
    }
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
