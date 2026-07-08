//! Generic helpers for treating a save file as a raw byte buffer.
//!
//! The editing model is: load the entire file into memory, mutate only the offsets
//! we understand, and write the buffer back. This preserves every byte we don't
//! understand, by construction.

use std::io::Write as _;
use std::path::Path;

use crate::Result;

/// Write `bytes` to `path` atomically.
///
/// The data is written to a temporary file in the *same directory* as `path` and then
/// renamed over the target. On the same filesystem that rename is atomic, so a reader
/// (or a crash) never sees a half-written save: `path` is either the old file or the
/// complete new one.
pub fn atomic_write(path: impl AsRef<Path>, bytes: &[u8]) -> Result<()> {
    let path = path.as_ref();
    let dir = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));

    let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
    tmp.write_all(bytes)?;
    tmp.flush()?;
    // Rename the temp file over the target. `persist` returns a PersistError that wraps
    // the underlying io::Error, which converts into our error type.
    tmp.persist(path).map_err(|e| e.error)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data.bin");
        let data = b"hello world";
        atomic_write(&path, data).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), data);
    }

    #[test]
    fn overwrites_existing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data.bin");
        atomic_write(&path, b"first").unwrap();
        atomic_write(&path, b"second value").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"second value");
    }
}
