//! The Save Library: a curated, permanent collection of named save **snapshots**.
//!
//! A snapshot captures a game's **complete** save set (all of `GameKind::save_files` that
//! exist) as one atomic unit, stored in a self-describing folder:
//!
//! ```text
//! <library>/<game-id>/<slug>/
//!     entry.toml          # game id, display name, notes, created timestamp
//!     <save files…>
//! ```
//!
//! There is no central index — the folders *are* the database (see
//! `PHASE-5-SAVE-LIBRARY.md`). Each snapshot is portable: because it carries its own
//! `entry.toml`, it stays valid if moved to another library or machine. Copies preserve
//! the source files' modification times, so a snapshot's "last updated" reflects when the
//! game last saved.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use anyhow::{anyhow, Context, Result};
use fringe_retro_core::backup;
use fringe_retro_core::games::GameKind;
use serde::{Deserialize, Serialize};

/// The metadata sidecar written into each snapshot folder.
const ENTRY_FILE: &str = "entry.toml";

/// The on-disk metadata for one snapshot (`entry.toml`).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct EntryMeta {
    /// The built-in game id (e.g. `ultima3`).
    game: String,
    /// The display name (source of truth; the folder slug is cosmetic).
    name: String,
    /// Optional free-form notes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    notes: Option<String>,
    /// When the snapshot was archived (ISO 8601, local time).
    created: String,
}

/// One archived snapshot in the library.
pub struct Snapshot {
    /// The game this snapshot belongs to.
    pub kind: GameKind,
    /// The folder name (a slug derived from the display name).
    pub slug: String,
    /// The display name (from `entry.toml`, or the slug if metadata is missing).
    pub name: String,
    /// Optional notes.
    pub notes: Option<String>,
    /// When the snapshot was archived (as stored; empty if unknown).
    pub created: String,
    /// The snapshot's folder.
    pub dir: PathBuf,
    /// The captured save-file names (everything in the folder except `entry.toml`).
    pub files: Vec<String>,
    /// The newest modification time across the captured files ("last updated").
    pub last_updated: Option<SystemTime>,
}

/// The result of restoring a snapshot over an active save directory.
pub struct RestoreOutcome {
    /// Files written into the save directory.
    pub restored: Vec<PathBuf>,
    /// Safety backups made of pre-existing active files.
    pub backups: Vec<PathBuf>,
}

/// A save library rooted at a directory.
pub struct Library {
    root: PathBuf,
}

impl Library {
    /// A library rooted at `root` (created on first write).
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Archive a game's active save set into a new named snapshot.
    ///
    /// Captures every file in `kind.save_files()` that exists in `save_dir`, preserving
    /// modification times. On a slug collision within the game's folder, a numeric suffix is
    /// appended (the caller can compare the returned `slug` with [`slugify`] to detect this).
    pub fn add(
        &self,
        kind: GameKind,
        save_dir: &Path,
        name: &str,
        notes: Option<&str>,
    ) -> Result<Snapshot> {
        let files: Vec<String> = kind
            .save_files()
            .iter()
            .filter(|f| save_dir.join(f).exists())
            .map(|f| f.to_string())
            .collect();
        if files.is_empty() {
            anyhow::bail!(
                "no save files found for {} in {}",
                kind.title(),
                save_dir.display()
            );
        }

        let game_dir = self.root.join(kind.id());
        let slug = unique_slug(&game_dir, &slugify(name));
        let dir = game_dir.join(&slug);
        std::fs::create_dir_all(&dir)
            .with_context(|| format!("failed to create snapshot folder {}", dir.display()))?;

        for f in &files {
            copy_file(&save_dir.join(f), &dir.join(f))?;
        }

        let meta = EntryMeta {
            game: kind.id().to_string(),
            name: name.trim().to_string(),
            notes: notes
                .map(|n| n.trim().to_string())
                .filter(|n| !n.is_empty()),
            created: chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string(),
        };
        write_entry(&dir, &meta)?;

        Ok(self.read_snapshot(kind, &dir))
    }

    /// List snapshots, optionally limited to one game, sorted by game then slug.
    pub fn list(&self, only: Option<GameKind>) -> Result<Vec<Snapshot>> {
        let kinds: Vec<GameKind> = match only {
            Some(k) => vec![k],
            None => GameKind::ALL.to_vec(),
        };
        let mut out = Vec::new();
        for kind in kinds {
            let game_dir = self.root.join(kind.id());
            if !game_dir.is_dir() {
                continue;
            }
            for entry in std::fs::read_dir(&game_dir)
                .with_context(|| format!("failed to read {}", game_dir.display()))?
            {
                let entry = entry?;
                if entry.file_type()?.is_dir() {
                    out.push(self.read_snapshot(kind, &entry.path()));
                }
            }
        }
        out.sort_by(|a, b| (a.kind.id(), &a.slug).cmp(&(b.kind.id(), &b.slug)));
        Ok(out)
    }

    /// Look up one snapshot by game and slug.
    pub fn get(&self, kind: GameKind, slug: &str) -> Result<Snapshot> {
        let dir = self.root.join(kind.id()).join(slug);
        if !dir.is_dir() {
            anyhow::bail!("no snapshot '{}/{slug}' in the library", kind.id());
        }
        Ok(self.read_snapshot(kind, &dir))
    }

    /// Restore a snapshot's files into `save_dir`.
    ///
    /// Each file that differs from (or is missing at) the destination is written; a safety
    /// backup of any pre-existing active file is made first. Files already identical are
    /// skipped. Restored files keep the snapshot's modification times.
    pub fn restore(&self, snapshot: &Snapshot, save_dir: &Path) -> Result<RestoreOutcome> {
        std::fs::create_dir_all(save_dir)
            .with_context(|| format!("failed to create {}", save_dir.display()))?;
        let mut outcome = RestoreOutcome {
            restored: Vec::new(),
            backups: Vec::new(),
        };
        for f in &snapshot.files {
            let src = snapshot.dir.join(f);
            let dst = save_dir.join(f);
            if dst.exists() && files_identical(&src, &dst)? {
                continue;
            }
            if dst.exists() {
                let backup = backup::create(&dst).map_err(|e| anyhow!("{e}"))?;
                outcome.backups.push(backup);
            }
            copy_file(&src, &dst)?;
            outcome.restored.push(dst);
        }
        Ok(outcome)
    }

    /// Rename a snapshot: update its display name and rename its folder to the new slug.
    ///
    /// The `entry.toml` name is the source of truth; the folder slug is cosmetic. If the new
    /// name yields the same slug, only the metadata changes; otherwise the folder is moved to
    /// the new slug (numeric-suffixed on collision).
    pub fn rename(&self, kind: GameKind, slug: &str, new_name: &str) -> Result<Snapshot> {
        let game_dir = self.root.join(kind.id());
        let dir = game_dir.join(slug);
        if !dir.is_dir() {
            anyhow::bail!("no snapshot '{}/{slug}' in the library", kind.id());
        }
        let new_name = new_name.trim();
        if new_name.is_empty() {
            anyhow::bail!("a snapshot name cannot be empty");
        }

        let mut meta = read_entry(&dir).unwrap_or_else(|| EntryMeta {
            game: kind.id().to_string(),
            name: slug.to_string(),
            notes: None,
            created: String::new(),
        });
        meta.name = new_name.to_string();

        let new_slug = slugify(new_name);
        let final_dir = if new_slug == slug {
            dir
        } else {
            let target = game_dir.join(unique_slug(&game_dir, &new_slug));
            std::fs::rename(&dir, &target).with_context(|| {
                format!("failed to move {} to {}", dir.display(), target.display())
            })?;
            target
        };
        write_entry(&final_dir, &meta)?;
        Ok(self.read_snapshot(kind, &final_dir))
    }

    /// Copy a snapshot into a new one (with a new name, defaulting to `"<name> copy"`). The
    /// duplicate gets a fresh `created` timestamp; notes are carried over.
    pub fn duplicate(
        &self,
        kind: GameKind,
        slug: &str,
        new_name: Option<&str>,
    ) -> Result<Snapshot> {
        let src = self.get(kind, slug)?;
        let name = new_name
            .map(str::trim)
            .filter(|n| !n.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| format!("{} copy", src.name));

        let game_dir = self.root.join(kind.id());
        let dst_dir = game_dir.join(unique_slug(&game_dir, &slugify(&name)));
        std::fs::create_dir_all(&dst_dir)
            .with_context(|| format!("failed to create {}", dst_dir.display()))?;
        for f in &src.files {
            copy_file(&src.dir.join(f), &dst_dir.join(f))?;
        }
        let meta = EntryMeta {
            game: kind.id().to_string(),
            name,
            notes: src.notes.clone(),
            created: chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string(),
        };
        write_entry(&dst_dir, &meta)?;
        Ok(self.read_snapshot(kind, &dst_dir))
    }

    /// Delete a snapshot folder and all its contents. Returns the deleted folder path.
    pub fn delete(&self, kind: GameKind, slug: &str) -> Result<PathBuf> {
        let dir = self.root.join(kind.id()).join(slug);
        if !dir.is_dir() {
            anyhow::bail!("no snapshot '{}/{slug}' in the library", kind.id());
        }
        std::fs::remove_dir_all(&dir)
            .with_context(|| format!("failed to delete {}", dir.display()))?;
        Ok(dir)
    }

    /// Read a snapshot from its folder, synthesizing sensible values if `entry.toml` is
    /// missing or unreadable (the folder is still a valid snapshot of its files).
    fn read_snapshot(&self, kind: GameKind, dir: &Path) -> Snapshot {
        let slug = dir
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let meta = read_entry(dir);
        let mut files: Vec<String> = std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|name| name != ENTRY_FILE)
            .collect();
        files.sort();
        let last_updated = files
            .iter()
            .filter_map(|f| std::fs::metadata(dir.join(f)).ok()?.modified().ok())
            .max();
        Snapshot {
            kind,
            name: meta
                .as_ref()
                .map(|m| m.name.clone())
                .unwrap_or_else(|| slug.clone()),
            notes: meta.as_ref().and_then(|m| m.notes.clone()),
            created: meta.map(|m| m.created).unwrap_or_default(),
            slug,
            dir: dir.to_path_buf(),
            files,
            last_updated,
        }
    }
}

/// Turn a display name into a filesystem-safe slug: lowercase alphanumerics, other runs
/// collapsed to single hyphens (never leading or trailing), trimmed. Empty names become
/// `snapshot`.
pub fn slugify(name: &str) -> String {
    let mut slug = String::new();
    let mut pending_dash = false;
    for c in name.trim().chars() {
        if c.is_ascii_alphanumeric() {
            if pending_dash && !slug.is_empty() {
                slug.push('-');
            }
            slug.push(c.to_ascii_lowercase());
            pending_dash = false;
        } else {
            pending_dash = true;
        }
    }
    if slug.is_empty() {
        "snapshot".to_string()
    } else {
        slug
    }
}

/// A slug that doesn't yet exist under `game_dir`, appending `-2`, `-3`, … on collision.
fn unique_slug(game_dir: &Path, base: &str) -> String {
    if !game_dir.join(base).exists() {
        return base.to_string();
    }
    for n in 2.. {
        let candidate = format!("{base}-{n}");
        if !game_dir.join(&candidate).exists() {
            return candidate;
        }
    }
    unreachable!("an unused slug always exists")
}

/// Copy `src` to `dst` atomically, preserving the source's modification time.
fn copy_file(src: &Path, dst: &Path) -> Result<()> {
    let bytes = std::fs::read(src).with_context(|| format!("failed to read {}", src.display()))?;
    let mtime = std::fs::metadata(src).and_then(|m| m.modified()).ok();
    fringe_retro_core::save::atomic_write(dst, &bytes).map_err(|e| anyhow!("{e}"))?;
    if let Some(mtime) = mtime {
        // Best-effort: a failure to stamp the mtime shouldn't fail the copy.
        if let Ok(f) = std::fs::OpenOptions::new().write(true).open(dst) {
            let _ = f.set_modified(mtime);
        }
    }
    Ok(())
}

/// Whether two files have identical contents.
fn files_identical(a: &Path, b: &Path) -> Result<bool> {
    Ok(std::fs::read(a)? == std::fs::read(b)?)
}

/// Write a snapshot's `entry.toml`.
fn write_entry(dir: &Path, meta: &EntryMeta) -> Result<()> {
    let text = toml::to_string(meta).context("failed to serialize entry.toml")?;
    let path = dir.join(ENTRY_FILE);
    std::fs::write(&path, text).with_context(|| format!("failed to write {}", path.display()))
}

/// Read a snapshot's `entry.toml`, or `None` if it's missing or unparseable.
fn read_entry(dir: &Path) -> Option<EntryMeta> {
    let text = std::fs::read_to_string(dir.join(ENTRY_FILE)).ok()?;
    toml::from_str(&text).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, bytes: &[u8]) {
        std::fs::write(path, bytes).unwrap();
    }

    /// A save dir with a fake Ultima III save set (roster + party).
    fn ultima3_save_dir() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let save = dir.path().to_path_buf();
        write(&save.join("ROSTER.ULT"), b"roster-bytes");
        write(&save.join("PARTY.ULT"), b"party-bytes");
        (dir, save)
    }

    #[test]
    fn slugify_makes_safe_names() {
        assert_eq!(slugify("Before the Abyss"), "before-the-abyss");
        assert_eq!(slugify("  Lord British!!  "), "lord-british");
        assert_eq!(slugify("A/B\\C"), "a-b-c");
        assert_eq!(slugify("***"), "snapshot");
    }

    #[test]
    fn add_captures_the_whole_save_set() {
        let lib_dir = tempfile::tempdir().unwrap();
        let (_g, save) = ultima3_save_dir();
        let library = Library::new(lib_dir.path());

        let snap = library
            .add(
                GameKind::Ultima3,
                &save,
                "Thief Party",
                Some("before the dungeon"),
            )
            .unwrap();

        assert_eq!(snap.slug, "thief-party");
        assert_eq!(snap.name, "Thief Party");
        assert_eq!(snap.notes.as_deref(), Some("before the dungeon"));
        assert_eq!(snap.files, vec!["PARTY.ULT", "ROSTER.ULT"]);
        assert!(snap.dir.join("entry.toml").exists());
        assert_eq!(
            std::fs::read(snap.dir.join("ROSTER.ULT")).unwrap(),
            b"roster-bytes"
        );
    }

    #[test]
    fn slug_collisions_get_a_numeric_suffix() {
        let lib_dir = tempfile::tempdir().unwrap();
        let (_g, save) = ultima3_save_dir();
        let library = Library::new(lib_dir.path());

        let a = library
            .add(GameKind::Ultima3, &save, "My Party", None)
            .unwrap();
        let b = library
            .add(GameKind::Ultima3, &save, "My Party", None)
            .unwrap();
        assert_eq!(a.slug, "my-party");
        assert_eq!(b.slug, "my-party-2");
    }

    #[test]
    fn list_and_get_round_trip() {
        let lib_dir = tempfile::tempdir().unwrap();
        let (_g, save) = ultima3_save_dir();
        let library = Library::new(lib_dir.path());
        library.add(GameKind::Ultima3, &save, "One", None).unwrap();
        library.add(GameKind::Ultima3, &save, "Two", None).unwrap();

        let all = library.list(None).unwrap();
        assert_eq!(all.len(), 2);
        let one = library.get(GameKind::Ultima3, "one").unwrap();
        assert_eq!(one.name, "One");
        assert!(library.get(GameKind::Ultima3, "missing").is_err());
    }

    #[test]
    fn restore_writes_files_and_backs_up_existing() {
        let lib_dir = tempfile::tempdir().unwrap();
        let (_g, save) = ultima3_save_dir();
        let library = Library::new(lib_dir.path());
        let snap = library.add(GameKind::Ultima3, &save, "Snap", None).unwrap();

        // Change the active saves, then restore.
        write(&save.join("ROSTER.ULT"), b"changed-roster");
        write(&save.join("PARTY.ULT"), b"changed-party");
        let outcome = library.restore(&snap, &save).unwrap();

        assert_eq!(outcome.restored.len(), 2);
        assert_eq!(outcome.backups.len(), 2); // both active files were backed up
        assert_eq!(
            std::fs::read(save.join("ROSTER.ULT")).unwrap(),
            b"roster-bytes"
        );
    }

    #[test]
    fn restore_is_a_noop_when_already_identical() {
        let lib_dir = tempfile::tempdir().unwrap();
        let (_g, save) = ultima3_save_dir();
        let library = Library::new(lib_dir.path());
        let snap = library.add(GameKind::Ultima3, &save, "Snap", None).unwrap();

        // The active saves already match the snapshot.
        let outcome = library.restore(&snap, &save).unwrap();
        assert!(outcome.restored.is_empty());
        assert!(outcome.backups.is_empty());
    }

    #[test]
    fn rename_moves_the_folder_and_updates_the_name() {
        let lib_dir = tempfile::tempdir().unwrap();
        let (_g, save) = ultima3_save_dir();
        let library = Library::new(lib_dir.path());
        let snap = library
            .add(GameKind::Ultima3, &save, "Old Name", None)
            .unwrap();
        let old_dir = snap.dir.clone();

        let renamed = library
            .rename(GameKind::Ultima3, "old-name", "New Name")
            .unwrap();
        assert_eq!(renamed.name, "New Name");
        assert_eq!(renamed.slug, "new-name");
        assert!(!old_dir.exists()); // the old folder was moved
        assert_eq!(
            library.get(GameKind::Ultima3, "new-name").unwrap().name,
            "New Name"
        );
        // Files travelled with the folder.
        assert_eq!(renamed.files, vec!["PARTY.ULT", "ROSTER.ULT"]);
    }

    #[test]
    fn rename_keeps_folder_when_slug_is_unchanged() {
        let lib_dir = tempfile::tempdir().unwrap();
        let (_g, save) = ultima3_save_dir();
        let library = Library::new(lib_dir.path());
        library
            .add(GameKind::Ultima3, &save, "My Party", None)
            .unwrap();

        // "My Party!" slugifies to the same "my-party"; only the display name changes.
        let renamed = library
            .rename(GameKind::Ultima3, "my-party", "My Party!")
            .unwrap();
        assert_eq!(renamed.slug, "my-party");
        assert_eq!(renamed.name, "My Party!");
    }

    #[test]
    fn duplicate_copies_files_and_defaults_the_name() {
        let lib_dir = tempfile::tempdir().unwrap();
        let (_g, save) = ultima3_save_dir();
        let library = Library::new(lib_dir.path());
        library
            .add(GameKind::Ultima3, &save, "Original", Some("keep me"))
            .unwrap();

        let dup = library
            .duplicate(GameKind::Ultima3, "original", None)
            .unwrap();
        assert_eq!(dup.name, "Original copy");
        assert_eq!(dup.slug, "original-copy");
        assert_eq!(dup.notes.as_deref(), Some("keep me")); // notes carried over
        assert_eq!(dup.files, vec!["PARTY.ULT", "ROSTER.ULT"]);
        assert_eq!(library.list(Some(GameKind::Ultima3)).unwrap().len(), 2);

        let named = library
            .duplicate(GameKind::Ultima3, "original", Some("Backup Copy"))
            .unwrap();
        assert_eq!(named.slug, "backup-copy");
    }

    #[test]
    fn delete_removes_the_snapshot() {
        let lib_dir = tempfile::tempdir().unwrap();
        let (_g, save) = ultima3_save_dir();
        let library = Library::new(lib_dir.path());
        library
            .add(GameKind::Ultima3, &save, "Doomed", None)
            .unwrap();

        let dir = library.delete(GameKind::Ultima3, "doomed").unwrap();
        assert!(!dir.exists());
        assert!(library.get(GameKind::Ultima3, "doomed").is_err());
        assert!(library.delete(GameKind::Ultima3, "doomed").is_err()); // already gone
    }
}
