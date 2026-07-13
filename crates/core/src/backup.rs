//! Automatic, timestamped backups and restores.
//!
//! Before the first write to a save file we copy it to a timestamped backup, and all
//! writes go through [`crate::save::atomic_write`]. Combined, these make data loss
//! extremely unlikely.
//!
//! Backups live alongside the original file, named `<file>.<timestamp>.bak` (the
//! timestamp format sorts chronologically), so they are easy to find and browse.

use std::path::{Path, PathBuf};

use crate::{Error, Result};

const BACKUP_SUFFIX: &str = ".bak";

/// Copy `path` to a new timestamped backup beside it, returning the backup's path.
pub fn create(path: impl AsRef<Path>) -> Result<PathBuf> {
    let path = path.as_ref();
    let bytes = std::fs::read(path)?;
    let file_name = file_name(path)?;
    let stamp = chrono::Local::now().format("%Y-%m-%dT%H-%M-%S%.3f");

    // Guard against two backups within the same millisecond colliding.
    let mut backup_path = path.with_file_name(format!("{file_name}.{stamp}{BACKUP_SUFFIX}"));
    let mut counter = 1;
    while backup_path.exists() {
        backup_path = path.with_file_name(format!("{file_name}.{stamp}.{counter}{BACKUP_SUFFIX}"));
        counter += 1;
    }

    crate::save::atomic_write(&backup_path, &bytes)?;
    Ok(backup_path)
}

/// List existing backups for `path`, oldest first.
pub fn list(path: impl AsRef<Path>) -> Result<Vec<PathBuf>> {
    let path = path.as_ref();
    let dir = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let prefix = format!("{}.", file_name(path)?);

    let mut backups = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with(&prefix) && name.ends_with(BACKUP_SUFFIX) {
            backups.push(entry.path());
        }
    }
    // The timestamp format is lexicographically ordered, so a name sort is chronological.
    backups.sort();
    Ok(backups)
}

/// Restore `backup` over `target`, first backing up whatever is currently at `target`.
///
/// If `target` is already byte-identical to `backup`, this is a no-op: nothing is written
/// and no safety backup is made. Returns the path of the pre-restore safety backup, or
/// `None` when the restore was skipped because the file already matched.
pub fn restore(backup: impl AsRef<Path>, target: impl AsRef<Path>) -> Result<Option<PathBuf>> {
    let backup = backup.as_ref();
    let target = target.as_ref();
    let bytes = std::fs::read(backup)?;
    // Skip the write (and its safety backup) when the target already matches the backup.
    if std::fs::read(target).is_ok_and(|current| current == bytes) {
        return Ok(None);
    }
    let pre_restore = create(target)?;
    crate::save::atomic_write(target, &bytes)?;
    Ok(Some(pre_restore))
}

/// Take a manual snapshot of `path`: create a timestamped backup, but only if no existing
/// backup is already byte-identical to the current file. Returns the new backup's path, or
/// `None` when an identical backup already exists (nothing to bookmark).
pub fn snapshot(path: impl AsRef<Path>) -> Result<Option<PathBuf>> {
    let path = path.as_ref();
    let bytes = std::fs::read(path)?;
    for existing in list(path)? {
        if std::fs::read(&existing).is_ok_and(|b| b == bytes) {
            return Ok(None);
        }
    }
    Ok(Some(create(path)?))
}

fn file_name(path: &Path) -> Result<String> {
    Ok(path
        .file_name()
        .ok_or_else(|| Error::Format(format!("path has no file name: {}", path.display())))?
        .to_string_lossy()
        .into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_list_and_restore() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("PLAYER1.U1");
        std::fs::write(&path, b"original").unwrap();

        let first = create(&path).unwrap();
        assert!(first.exists());
        assert_eq!(std::fs::read(&first).unwrap(), b"original");
        assert_eq!(list(&path).unwrap().len(), 1);

        // Modify the save, then restore the backup.
        std::fs::write(&path, b"modified").unwrap();
        let pre_restore = restore(&first, &path).unwrap().unwrap();

        assert_eq!(std::fs::read(&path).unwrap(), b"original");
        assert_eq!(std::fs::read(&pre_restore).unwrap(), b"modified");
        // The original backup plus the pre-restore safety backup.
        assert_eq!(list(&path).unwrap().len(), 2);
    }

    #[test]
    fn restore_is_a_noop_when_already_identical() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("PLAYER1.U1");
        std::fs::write(&path, b"original").unwrap();

        let first = create(&path).unwrap();
        // The live file already matches the backup, so restoring should do nothing.
        let result = restore(&first, &path).unwrap();

        assert!(result.is_none());
        assert_eq!(std::fs::read(&path).unwrap(), b"original");
        // No safety backup was created.
        assert_eq!(list(&path).unwrap().len(), 1);
    }

    #[test]
    fn snapshot_skips_identical_and_creates_when_changed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("PLAYER1.U1");
        std::fs::write(&path, b"first").unwrap();

        // First snapshot creates a backup.
        assert!(snapshot(&path).unwrap().is_some());
        assert_eq!(list(&path).unwrap().len(), 1);

        // Snapshotting identical content again is a no-op.
        assert!(snapshot(&path).unwrap().is_none());
        assert_eq!(list(&path).unwrap().len(), 1);

        // After the file changes, a new snapshot is created.
        std::fs::write(&path, b"second").unwrap();
        assert!(snapshot(&path).unwrap().is_some());
        assert_eq!(list(&path).unwrap().len(), 2);
    }
}
