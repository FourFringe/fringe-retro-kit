//! In-memory editing sessions.
//!
//! A [`Session`] loads a save into a mutable buffer, applies **validated** field edits
//! without touching disk, and writes everything **once** on [`Session::save`] (a single
//! timestamped backup plus one atomic write). This gives batch editing: make all the
//! tweaks you want, then save one file with the end result — no per-change save files.

use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};
use fringe_retro_core::backup;
use fringe_retro_core::games::ultima1::{self, Ultima1Save};
use fringe_retro_core::games::ultima2::{self, Ultima2Save};
use fringe_retro_core::games::ultima3::{self, Ultima3Party, Ultima3Roster};
use fringe_retro_core::schema::{Field, FieldKind};

/// A loaded save of a known, editable game.
enum Loaded {
    Ultima1(Ultima1Save),
    Ultima2(Ultima2Save),
    Ultima3Roster(Ultima3Roster),
    Ultima3Party(Ultima3Party),
}

/// A character/slot within a save that can be edited.
pub struct Entity {
    pub index: usize,
    pub label: String,
}

/// One editable field: its stable key, display label, current value, and schema kind.
pub struct FieldRow {
    pub key: &'static str,
    pub label: &'static str,
    pub value: String,
    pub kind: FieldKind,
}

impl FieldRow {
    /// For enum/letter fields, an inline hint listing each value with the token you type to
    /// pick it: the number for enums (e.g. `1) Dagger  2) Mace`), the letter for
    /// letter-fields (e.g. `M) Male  F) Female`).
    pub fn choice_hint(&self) -> Option<String> {
        let parts: Vec<String> = match self.kind {
            FieldKind::Enum { variants, .. } => variants
                .iter()
                .map(|(key, name)| format!("{key}) {name}"))
                .collect(),
            FieldKind::Letter { variants } => variants
                .iter()
                .map(|(key, name)| format!("{}) {name}", *key as u8 as char))
                .collect(),
            _ => return None,
        };
        Some(parts.join("  "))
    }
}

/// An in-memory editing session for one save file.
pub struct Session {
    path: PathBuf,
    save: Loaded,
    dirty: bool,
}

impl Session {
    /// Load a save file if it's a known, editable game; `Ok(None)` if the size is unknown.
    pub fn load(path: &Path) -> Result<Option<Session>> {
        let bytes = std::fs::read(path)?;
        let save = if bytes.len() == ultima3::PARTY_LEN {
            Loaded::Ultima3Party(Ultima3Party::from_bytes(bytes)?)
        } else if bytes.len() == ultima3::ROSTER_LEN {
            Loaded::Ultima3Roster(Ultima3Roster::from_bytes(bytes)?)
        } else if bytes.len() == ultima2::SAVE_LEN {
            Loaded::Ultima2(Ultima2Save::from_bytes(bytes)?)
        } else if bytes.len() == ultima1::SAVE_LEN {
            Loaded::Ultima1(Ultima1Save::from_bytes(bytes)?)
        } else {
            return Ok(None);
        };
        Ok(Some(Session {
            path: path.to_path_buf(),
            save,
            dirty: false,
        }))
    }

    /// Whether there are unsaved edits.
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// The path of the save file this session edits.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The editable entities (characters). Single-character games return exactly one.
    pub fn entities(&self) -> Vec<Entity> {
        match &self.save {
            Loaded::Ultima1(s) => vec![Entity {
                index: 0,
                label: s.name(),
            }],
            Loaded::Ultima2(s) => vec![Entity {
                index: 0,
                label: s.get_field("name").unwrap_or_default(),
            }],
            Loaded::Ultima3Roster(r) => r
                .occupied_slots()
                .into_iter()
                .map(|slot| Entity {
                    index: slot,
                    label: format!("Slot {}: {}", slot + 1, r.summary(slot)),
                })
                .collect(),
            Loaded::Ultima3Party(p) => {
                let members = p.party_size().min(ultima3::PARTY_MEMBER_COUNT);
                (0..members)
                    .map(|m| Entity {
                        index: m,
                        label: format!("{}. {}", m + 1, p.summary(m)),
                    })
                    .collect()
            }
        }
    }

    fn fields(&self) -> &'static [Field] {
        match &self.save {
            Loaded::Ultima1(_) => Ultima1Save::fields(),
            Loaded::Ultima2(_) => Ultima2Save::fields(),
            Loaded::Ultima3Roster(_) | Loaded::Ultima3Party(_) => ultima3::record_fields(),
        }
    }

    fn value(&self, entity: usize, key: &str) -> Option<String> {
        match &self.save {
            Loaded::Ultima1(s) => s.get_field(key),
            Loaded::Ultima2(s) => s.get_field(key),
            Loaded::Ultima3Roster(r) => r.get_field(entity, key),
            Loaded::Ultima3Party(p) => p.get_field(entity, key),
        }
    }

    /// The editable fields (with current in-memory values) of the given entity.
    pub fn rows(&self, entity: usize) -> Vec<FieldRow> {
        self.fields()
            .iter()
            .map(|f| FieldRow {
                key: f.key,
                label: f.label,
                value: self.value(entity, f.key).unwrap_or_default(),
                kind: f.kind,
            })
            .collect()
    }

    /// Apply a validated edit to the in-memory buffer. Marks the session dirty on success;
    /// returns the validation error (unchanged buffer) on failure.
    pub fn set(&mut self, entity: usize, key: &str, value: &str) -> Result<()> {
        match &mut self.save {
            Loaded::Ultima1(s) => s.set_field(key, value),
            Loaded::Ultima2(s) => s.set_field(key, value),
            Loaded::Ultima3Roster(r) => r.set_field(entity, key, value),
            Loaded::Ultima3Party(p) => p.set_field(entity, key, value),
        }
        .map_err(|e| anyhow!("{e}"))?;
        self.dirty = true;
        Ok(())
    }

    /// Write all accumulated edits to disk once: a timestamped backup, then an atomic
    /// write. Returns the backup path. Clears the dirty flag.
    pub fn save(&mut self) -> Result<PathBuf> {
        let backup_path = backup::create(&self.path)?;
        match &self.save {
            Loaded::Ultima1(s) => s.write(&self.path),
            Loaded::Ultima2(s) => s.write(&self.path),
            Loaded::Ultima3Roster(r) => r.write(&self.path),
            Loaded::Ultima3Party(p) => p.write(&self.path),
        }?;
        self.dirty = false;
        Ok(backup_path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal valid Ultima I save (820 bytes) with a name and a couple of fields.
    fn ultima1_save_bytes() -> Vec<u8> {
        let mut buf = vec![0u8; ultima1::SAVE_LEN];
        buf[0..4].copy_from_slice(b"Enki");
        buf[0x24..0x26].copy_from_slice(&100u16.to_le_bytes()); // gold
        buf
    }

    fn write_temp(bytes: &[u8]) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("PLAYER1.U1");
        std::fs::write(&path, bytes).unwrap();
        (dir, path)
    }

    #[test]
    fn batch_edits_then_single_save() {
        let (_dir, path) = write_temp(&ultima1_save_bytes());
        let mut session = Session::load(&path).unwrap().unwrap();
        assert!(!session.is_dirty());

        // Several in-memory edits — none touch disk yet.
        session.set(0, "gold", "500").unwrap();
        session.set(0, "strength", "42").unwrap();
        session.set(0, "name", "Mondain").unwrap();
        assert!(session.is_dirty());

        // The file on disk is still the original until we save.
        let before = std::fs::read(&path).unwrap();
        assert_eq!(&before[0..4], b"Enki");

        let backup_path = session.save().unwrap();
        assert!(!session.is_dirty());
        assert!(backup_path.exists());

        // Reload from disk: all three edits are present in one written file.
        let reloaded = Session::load(&path).unwrap().unwrap();
        assert_eq!(reloaded.value(0, "gold").as_deref(), Some("500"));
        assert_eq!(reloaded.value(0, "strength").as_deref(), Some("42"));
        assert_eq!(reloaded.value(0, "name").as_deref(), Some("Mondain"));
    }

    #[test]
    fn invalid_edit_is_rejected_and_not_dirty() {
        let (_dir, path) = write_temp(&ultima1_save_bytes());
        let mut session = Session::load(&path).unwrap().unwrap();
        assert!(session.set(0, "gold", "banana").is_err());
        assert!(session.set(0, "transport", "spaceship").is_err());
        assert!(!session.is_dirty()); // failed edits leave the session clean
    }

    #[test]
    fn rows_expose_fields_and_options() {
        let (_dir, path) = write_temp(&ultima1_save_bytes());
        let session = Session::load(&path).unwrap().unwrap();
        let rows = session.rows(0);
        let gold = rows.iter().find(|r| r.key == "gold").unwrap();
        assert_eq!(gold.value, "100");
        assert!(gold.choice_hint().is_none()); // numeric field
        let transport = rows.iter().find(|r| r.key == "transport").unwrap();
        let hint = transport.choice_hint().unwrap(); // enum field
        assert!(hint.contains("Aircar"));
        assert!(hint.contains(") ")); // numbered, e.g. "3) Aircar"
    }

    #[test]
    fn unsupported_size_is_none() {
        let (_dir, path) = write_temp(&[0u8; 123]);
        assert!(Session::load(&path).unwrap().is_none());
    }
}
