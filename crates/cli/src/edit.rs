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
use fringe_retro_core::games::ultima4::{self, Ultima4Save};
use fringe_retro_core::games::ultima5::{self, Ultima5Save};
use fringe_retro_core::games::GameKind;
use fringe_retro_core::schema::{Field, FieldKind};

/// A loaded save of a known, editable game.
enum Loaded {
    Ultima1(Ultima1Save),
    Ultima2(Ultima2Save),
    Ultima3Roster(Ultima3Roster),
    Ultima3Party(Ultima3Party),
    Ultima4(Ultima4Save),
    Ultima5(Ultima5Save),
}

/// A character/slot within a save that can be edited.
pub struct Entity {
    pub index: usize,
    pub label: String,
}

/// One editable field: its stable key, display label, current value, schema kind, and
/// optional display section (used to group the editor's field list).
pub struct FieldRow {
    pub key: &'static str,
    pub label: &'static str,
    pub value: String,
    pub kind: FieldKind,
    pub section: Option<&'static str>,
}

impl FieldRow {
    /// The ordered list of values to cycle through for enum/letter/boolean fields (used by
    /// the editor's picker). `None` for free-text fields such as names and numbers.
    pub fn pick_options(&self) -> Option<Vec<String>> {
        match self.kind {
            FieldKind::Enum { variants, .. } | FieldKind::Letter { variants } => {
                Some(variants.iter().map(|(_, name)| name.to_string()).collect())
            }
            FieldKind::Bool => Some(vec!["no".to_string(), "yes".to_string()]),
            _ => None,
        }
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
        } else if bytes.len() == ultima4::SAVE_LEN {
            Loaded::Ultima4(Ultima4Save::from_bytes(bytes)?)
        } else if bytes.len() == ultima5::SAVE_LEN {
            Loaded::Ultima5(Ultima5Save::from_bytes(bytes)?)
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

    /// Which game this session is editing.
    pub fn kind(&self) -> GameKind {
        match &self.save {
            Loaded::Ultima1(_) => GameKind::Ultima1,
            Loaded::Ultima2(_) => GameKind::Ultima2,
            Loaded::Ultima3Roster(_) | Loaded::Ultima3Party(_) => GameKind::Ultima3,
            Loaded::Ultima4(_) => GameKind::Ultima4,
            Loaded::Ultima5(_) => GameKind::Ultima5,
        }
    }

    /// An empty in-memory session for a game kind, backed by a zeroed buffer. Used to
    /// dry-run edits (e.g. validating a template) without needing a real save file.
    /// Returns `None` for games that aren't editable.
    pub fn scratch(kind: GameKind) -> Option<Session> {
        let save = match kind {
            GameKind::Ultima1 => {
                Loaded::Ultima1(Ultima1Save::from_bytes(vec![0u8; ultima1::SAVE_LEN]).ok()?)
            }
            GameKind::Ultima2 => {
                Loaded::Ultima2(Ultima2Save::from_bytes(vec![0u8; ultima2::SAVE_LEN]).ok()?)
            }
            GameKind::Ultima3 => Loaded::Ultima3Roster(
                Ultima3Roster::from_bytes(vec![0u8; ultima3::ROSTER_LEN]).ok()?,
            ),
            GameKind::Ultima4 => {
                Loaded::Ultima4(Ultima4Save::from_bytes(vec![0u8; ultima4::SAVE_LEN]).ok()?)
            }
            GameKind::Ultima5 => {
                Loaded::Ultima5(Ultima5Save::from_bytes(vec![0u8; ultima5::SAVE_LEN]).ok()?)
            }
            GameKind::Wasteland => return None,
        };
        Some(Session {
            path: PathBuf::new(),
            save,
            dirty: false,
        })
    }

    /// Apply several validated edits in sequence (e.g. a template's fields). Stops at the
    /// first invalid edit; validate against a [`Session::scratch`] first to avoid partial
    /// application.
    pub fn apply(&mut self, entity: usize, fields: &[(String, String)]) -> Result<()> {
        for (key, value) in fields {
            self.set(entity, key, value)?;
        }
        Ok(())
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
                // Entity 0 is the party header; entities 1..=n are the party members.
                let mut entities = vec![Entity {
                    index: 0,
                    label: "Party settings".to_string(),
                }];
                let members = p.party_size().min(ultima3::PARTY_MEMBER_COUNT);
                entities.extend((0..members).map(|m| Entity {
                    index: m + 1,
                    label: format!("{}. {}", m + 1, p.summary(m)),
                }));
                entities
            }
            Loaded::Ultima4(s) => {
                // Entity 0 is the party/game state; entities 1..=n are the players.
                let mut entities = vec![Entity {
                    index: 0,
                    label: "Party & Virtues".to_string(),
                }];
                entities.extend(s.occupied_players().into_iter().map(|i| Entity {
                    index: i + 1,
                    label: format!("{}. {}", i + 1, s.player_summary(i)),
                }));
                entities
            }
            Loaded::Ultima5(s) => {
                // Entity 0 is the party/game state; entities 1..=n are the characters.
                let mut entities = vec![Entity {
                    index: 0,
                    label: "Party & Provisions".to_string(),
                }];
                entities.extend(s.occupied_characters().into_iter().map(|i| Entity {
                    index: i + 1,
                    label: format!("{}. {}", i + 1, s.character_summary(i)),
                }));
                entities
            }
        }
    }

    fn fields(&self, entity: usize) -> &'static [Field] {
        match &self.save {
            Loaded::Ultima1(_) => Ultima1Save::fields(),
            Loaded::Ultima2(_) => Ultima2Save::fields(),
            Loaded::Ultima3Roster(_) => ultima3::record_fields(),
            Loaded::Ultima3Party(_) => {
                if entity == 0 {
                    ultima3::header_fields()
                } else {
                    ultima3::record_fields()
                }
            }
            Loaded::Ultima4(_) => {
                if entity == 0 {
                    ultima4::party_fields()
                } else {
                    ultima4::player_fields()
                }
            }
            Loaded::Ultima5(_) => {
                if entity == 0 {
                    ultima5::party_fields()
                } else {
                    ultima5::character_fields()
                }
            }
        }
    }

    fn value(&self, entity: usize, key: &str) -> Option<String> {
        match &self.save {
            Loaded::Ultima1(s) => s.get_field(key),
            Loaded::Ultima2(s) => s.get_field(key),
            Loaded::Ultima3Roster(r) => r.get_field(entity, key),
            Loaded::Ultima3Party(p) => {
                if entity == 0 {
                    p.header_get_field(key)
                } else {
                    p.get_field(entity - 1, key)
                }
            }
            Loaded::Ultima4(s) => {
                if entity == 0 {
                    s.party_get(key)
                } else {
                    s.player_get(entity - 1, key)
                }
            }
            Loaded::Ultima5(s) => {
                if entity == 0 {
                    s.party_get(key)
                } else {
                    s.character_get(entity - 1, key)
                }
            }
        }
    }

    /// The editable fields (with current in-memory values) of the given entity.
    pub fn rows(&self, entity: usize) -> Vec<FieldRow> {
        self.fields(entity)
            .iter()
            .map(|f| FieldRow {
                key: f.key,
                label: f.label,
                value: self.value(entity, f.key).unwrap_or_default(),
                kind: f.kind,
                section: f.section,
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
            Loaded::Ultima3Party(p) => {
                if entity == 0 {
                    p.header_set_field(key, value)
                } else {
                    p.set_field(entity - 1, key, value)
                }
            }
            Loaded::Ultima4(s) => {
                if entity == 0 {
                    s.party_set(key, value)
                } else {
                    s.player_set(entity - 1, key, value)
                }
            }
            Loaded::Ultima5(s) => {
                if entity == 0 {
                    s.party_set(key, value)
                } else {
                    s.character_set(entity - 1, key, value)
                }
            }
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
            Loaded::Ultima4(s) => s.write(&self.path),
            Loaded::Ultima5(s) => s.write(&self.path),
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
        assert!(gold.pick_options().is_none()); // numeric field: free text
        let transport = rows.iter().find(|r| r.key == "transport").unwrap();
        let options = transport.pick_options().unwrap(); // enum field: picker
        assert!(options.contains(&"Aircar".to_string()));
    }

    #[test]
    fn unsupported_size_is_none() {
        let (_dir, path) = write_temp(&[0u8; 123]);
        assert!(Session::load(&path).unwrap().is_none());
    }
}
