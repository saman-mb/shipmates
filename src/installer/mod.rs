pub mod apply;
pub mod manifest_db;
pub mod migrate;
pub mod plan;
pub mod uninstall;

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// Per-process counter for temp-file uniqueness — combined with the PID it keeps
/// concurrent writers (and sibling files) from ever picking the same temp name.
static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// A unique, same-directory temp path for `path`: `<file>.<pid>.<counter>.tmp`.
///
/// A fixed `<stem>.tmp` (what `path.with_extension("tmp")` yields) collides with
/// a user file of that name sitting beside a managed one — on the doctor restore
/// path the temp is written into the live target dir, so the collision would
/// clobber the user's file. The PID + atomic counter make the name unguessable
/// and per-write unique while staying in the same directory, so the final
/// `rename` remains atomic.
fn temp_path_for(path: &Path) -> PathBuf {
    let n = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let stem = path
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or("shipmates");
    let name = format!("{}.{}.{}.tmp", stem, std::process::id(), n);
    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.join(name),
        _ => PathBuf::from(name),
    }
}

fn write_temp(temp_path: &Path, content: &str) -> std::io::Result<()> {
    let mut file = File::create(temp_path)?;
    file.write_all(content.as_bytes())?;
    file.sync_all()?;
    Ok(())
}

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
    let temp_path = temp_path_for(path);
    // Write + fsync the temp, cleaning it up on any failure so a partial temp
    // never lingers beside the target.
    if let Err(e) = write_temp(&temp_path, content) {
        let _ = fs::remove_file(&temp_path);
        return Err(e);
    }
    if let Err(e) = fs::rename(&temp_path, path) {
        let _ = fs::remove_file(&temp_path);
        return Err(e);
    }
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

    #[test]
    fn test_atomic_write_does_not_clobber_sibling_tmp() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("agent.md");
        // A user file that collides with the OLD fixed temp name `<stem>.tmp`
        // (`agent.md`.with_extension("tmp") == `agent.tmp`).
        let sibling = dir.path().join("agent.tmp");
        fs::write(&sibling, "user data").unwrap();

        atomic_write(&file_path, "payload").unwrap();

        assert_eq!(fs::read_to_string(&file_path).unwrap(), "payload");
        // The pre-existing sibling must be byte-untouched.
        assert!(sibling.exists());
        assert_eq!(fs::read_to_string(&sibling).unwrap(), "user data");
        // No stray temp left behind either.
        let leftovers: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .filter(|e| e.file_name() != "agent.tmp")
            .collect();
        assert!(leftovers.is_empty(), "unexpected temp file(s) left behind");
    }
}
