//! Hardcoded Ultima IV (Quest of the Avatar) party save support (`PARTY.SAV`).
//!
//! Format reference: the [`xu4`](https://github.com/xu4/u4) reimplementation's `savegame`
//! structures, cross-checked byte-for-byte against a real 502-byte `PARTY.SAV`.
//!
//! Unlike Ultima II/III (which use BCD), Ultima IV stores numbers as **plain little-endian
//! binary integers**. The file is an 8-byte header, then eight fixed 39-byte player records,
//! then a block of party/game state (food, gold, the eight virtues, inventory, location).
//! Edits mutate only known offsets in place, preserving unknown bytes.

use std::path::Path;

use crate::schema::{self, Endian, Field, FieldKind, Variants};
use crate::{Error, Result};

/// Total size of a `PARTY.SAV` file, in bytes.
pub const SAVE_LEN: usize = 502;
/// Bytes before the first player record (`unknown1` + `moves`).
const HEADER_LEN: usize = 8;
/// Size of one player record, in bytes.
const RECORD_LEN: usize = 0x27;
/// Number of player slots in the party.
pub const PLAYER_COUNT: usize = 8;
/// Length of a player's name field, including the null terminator.
const NAME_LEN: usize = 16;

// --- Enum tables (value -> label). ---

const SEX: Variants = &[(0x0B, "Male"), (0x0C, "Female")];
const CLASS: Variants = &[
    (0, "Mage"),
    (1, "Bard"),
    (2, "Fighter"),
    (3, "Druid"),
    (4, "Tinker"),
    (5, "Paladin"),
    (6, "Ranger"),
    (7, "Shepherd"),
];
const STATUS: Variants = &[
    (b'G' as u32, "Good"),
    (b'P' as u32, "Poisoned"),
    (b'S' as u32, "Sleeping"),
    (b'D' as u32, "Dead"),
];
const WEAPON: Variants = &[
    (0, "Hands"),
    (1, "Staff"),
    (2, "Dagger"),
    (3, "Sling"),
    (4, "Mace"),
    (5, "Axe"),
    (6, "Sword"),
    (7, "Bow"),
    (8, "Crossbow"),
    (9, "Flaming Oil"),
    (10, "Halberd"),
    (11, "Magic Axe"),
    (12, "Magic Sword"),
    (13, "Magic Bow"),
    (14, "Magic Wand"),
    (15, "Mystic Sword"),
];
const ARMOR: Variants = &[
    (0, "Skin"),
    (1, "Cloth"),
    (2, "Leather"),
    (3, "Chain Mail"),
    (4, "Plate Mail"),
    (5, "Magic Chain"),
    (6, "Magic Plate"),
    (7, "Mystic Robe"),
];

/// A little-endian `u16` field with an inclusive edit maximum.
const fn u16le(max: u32) -> FieldKind {
    FieldKind::Int {
        bytes: 2,
        endian: Endian::Little,
        max,
    }
}

/// A little-endian food value: stored ×100 on disk, shown/edited as the whole number.
const fn food(max: u32) -> FieldKind {
    FieldKind::Scaled {
        bytes: 4,
        endian: Endian::Little,
        scale: 100,
        max,
    }
}

/// A single-byte enum field.
const fn enum1(variants: Variants) -> FieldKind {
    FieldKind::Enum {
        bytes: 1,
        endian: Endian::Little,
        variants,
    }
}

/// A little-endian `u16` enum field.
const fn enum2(variants: Variants) -> FieldKind {
    FieldKind::Enum {
        bytes: 2,
        endian: Endian::Little,
        variants,
    }
}

/// An ASCII-letter enum field.
const fn letter(variants: Variants) -> FieldKind {
    FieldKind::Letter { variants }
}

// Display sections.
const S_CHARACTER: &str = "Character";
const S_ATTRIBUTES: &str = "Attributes";
const S_VITALS: &str = "Vitals";
const S_EQUIPPED: &str = "Equipped";
const S_PARTY: &str = "Party";
const S_VIRTUES: &str = "Virtues";
const S_INVENTORY: &str = "Inventory";
const S_LOCATION: &str = "Location";

/// The fields of one player record (offsets are within the 39-byte record).
#[rustfmt::skip]
const PLAYER_FIELDS: &[Field] = &[
    Field::new("name",         "Name",         0x14, FieldKind::Name { len: NAME_LEN }).in_section(S_CHARACTER),
    Field::new("sex",          "Sex",          0x24, enum1(SEX)).in_section(S_CHARACTER),
    Field::new("class",        "Class",        0x25, enum1(CLASS)).in_section(S_CHARACTER),
    Field::new("status",       "Status",       0x26, letter(STATUS)).in_section(S_CHARACTER),
    Field::new("strength",     "Strength",     0x06, u16le(99)).in_section(S_ATTRIBUTES),
    Field::new("dexterity",    "Dexterity",    0x08, u16le(99)).in_section(S_ATTRIBUTES),
    Field::new("intelligence", "Intelligence", 0x0A, u16le(99)).in_section(S_ATTRIBUTES),
    Field::new("hp",           "Hit Points",   0x00, u16le(9999)).in_section(S_VITALS),
    Field::new("hp_max",       "Max HP",       0x02, u16le(9999)).in_section(S_VITALS),
    Field::new("experience",   "Experience",   0x04, u16le(9999)).in_section(S_VITALS),
    Field::new("magic",        "Magic Points", 0x0C, u16le(99)).in_section(S_VITALS),
    Field::new("weapon",       "Weapon",       0x10, enum2(WEAPON)).in_section(S_EQUIPPED),
    Field::new("armor",        "Armor",        0x12, enum2(ARMOR)).in_section(S_EQUIPPED),
];

/// The party/game-state fields (absolute file offsets, applied at base 0).
#[rustfmt::skip]
const PARTY_FIELDS: &[Field] = &[
    Field::new("food",         "Food", 0x140, food(9999)).in_section(S_PARTY),
    Field::new("gold",         "Gold",        0x144, u16le(9999)).in_section(S_PARTY),
    Field::new("honesty",      "Honesty",      0x146, u16le(99)).in_section(S_VIRTUES),
    Field::new("compassion",   "Compassion",   0x148, u16le(99)).in_section(S_VIRTUES),
    Field::new("valor",        "Valor",        0x14A, u16le(99)).in_section(S_VIRTUES),
    Field::new("justice",      "Justice",      0x14C, u16le(99)).in_section(S_VIRTUES),
    Field::new("sacrifice",    "Sacrifice",    0x14E, u16le(99)).in_section(S_VIRTUES),
    Field::new("honor",        "Honor",        0x150, u16le(99)).in_section(S_VIRTUES),
    Field::new("spirituality", "Spirituality", 0x152, u16le(99)).in_section(S_VIRTUES),
    Field::new("humility",     "Humility",     0x154, u16le(99)).in_section(S_VIRTUES),
    Field::new("torches",  "Torches",  0x156, u16le(99)).in_section(S_INVENTORY),
    Field::new("gems",     "Gems",     0x158, u16le(99)).in_section(S_INVENTORY),
    Field::new("keys",     "Keys",     0x15A, u16le(99)).in_section(S_INVENTORY),
    Field::new("sextants", "Sextants", 0x15C, u16le(99)).in_section(S_INVENTORY),
    // Reagents (u16 counts), order per the format spec.
    Field::new("reagent_ash",       "Sulfurous Ash", 0x18E, u16le(99)).in_section(S_INVENTORY),
    Field::new("reagent_ginseng",   "Ginseng",       0x190, u16le(99)).in_section(S_INVENTORY),
    Field::new("reagent_garlic",    "Garlic",        0x192, u16le(99)).in_section(S_INVENTORY),
    Field::new("reagent_silk",      "Spider Silk",   0x194, u16le(99)).in_section(S_INVENTORY),
    Field::new("reagent_moss",      "Blood Moss",    0x196, u16le(99)).in_section(S_INVENTORY),
    Field::new("reagent_pearl",     "Black Pearl",   0x198, u16le(99)).in_section(S_INVENTORY),
    Field::new("reagent_nightshade","Nightshade",    0x19A, u16le(99)).in_section(S_INVENTORY),
    Field::new("reagent_mandrake",  "Mandrake Root", 0x19C, u16le(99)).in_section(S_INVENTORY),
    Field::new("x", "Map X", 0x1D4, FieldKind::Byte).in_section(S_LOCATION),
    Field::new("y", "Map Y", 0x1D5, FieldKind::Byte).in_section(S_LOCATION),
];

// --- Record helpers, parameterized by a base offset. ---

fn player_base(index: usize) -> usize {
    HEADER_LEN + index * RECORD_LEN
}

/// Whether a player slot holds a character (its name is non-empty).
fn player_is_occupied(buf: &[u8], index: usize) -> bool {
    buf[player_base(index) + 0x14] != 0
}

fn field_get(fields: &[Field], buf: &[u8], base: usize, key: &str) -> Option<String> {
    let field = fields.iter().find(|f| f.key == key)?;
    Some(schema::read_field(buf, base, field))
}

fn field_set(fields: &[Field], buf: &mut [u8], base: usize, key: &str, value: &str) -> Result<()> {
    let field = fields
        .iter()
        .find(|f| f.key == key)
        .ok_or_else(|| Error::Format(format!("unknown field '{key}'")))?;
    schema::write_field(buf, base, field, value)
}

fn field_inspect(
    fields: &[Field],
    buf: &[u8],
    base: usize,
) -> Vec<(&'static str, &'static str, String)> {
    fields
        .iter()
        .map(|f| {
            (
                f.section.unwrap_or_default(),
                f.label,
                schema::read_field(buf, base, f),
            )
        })
        .collect()
}

/// The player-record field table (for building editors).
pub fn player_fields() -> &'static [Field] {
    PLAYER_FIELDS
}

/// The party/game-state field table (for building editors).
pub fn party_fields() -> &'static [Field] {
    PARTY_FIELDS
}

/// A parsed Ultima IV `PARTY.SAV` file.
#[derive(Clone)]
pub struct Ultima4Save {
    bytes: Vec<u8>,
}

impl Ultima4Save {
    /// Wrap an in-memory byte buffer, validating its length.
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self> {
        if bytes.len() != SAVE_LEN {
            return Err(Error::Format(format!(
                "expected {SAVE_LEN} bytes, got {}",
                bytes.len()
            )));
        }
        Ok(Self { bytes })
    }

    /// Read and parse a save file from disk.
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        Self::from_bytes(std::fs::read(path)?)
    }

    /// The complete raw byte buffer.
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Write this save to `path` atomically. Callers are responsible for backups.
    pub fn write(&self, path: impl AsRef<Path>) -> Result<()> {
        crate::save::atomic_write(path, &self.bytes)
    }

    /// The 0-based indices of player slots that hold a character.
    pub fn occupied_players(&self) -> Vec<usize> {
        (0..PLAYER_COUNT)
            .filter(|&i| player_is_occupied(&self.bytes, i))
            .collect()
    }

    /// A one-line summary of a player (0-based index).
    pub fn player_summary(&self, index: usize) -> String {
        let g = |key| self.player_get(index, key).unwrap_or_default();
        format!(
            "{} — {}, HP {}/{} · {}",
            g("name"),
            g("class"),
            g("hp"),
            g("hp_max"),
            g("status"),
        )
    }

    /// Format a player field by key, or `None` for an unknown player/key.
    pub fn player_get(&self, index: usize, key: &str) -> Option<String> {
        if index >= PLAYER_COUNT {
            return None;
        }
        field_get(PLAYER_FIELDS, &self.bytes, player_base(index), key)
    }

    /// Set a player field by key, validating the value first.
    pub fn player_set(&mut self, index: usize, key: &str, value: &str) -> Result<()> {
        if index >= PLAYER_COUNT {
            return Err(Error::Format(format!(
                "player slot must be 1..={PLAYER_COUNT} (got {})",
                index + 1
            )));
        }
        field_set(
            PLAYER_FIELDS,
            &mut self.bytes,
            player_base(index),
            key,
            value,
        )
    }

    /// All known fields of a player as `(section, label, value)` tuples.
    pub fn player_inspect(&self, index: usize) -> Vec<(&'static str, &'static str, String)> {
        field_inspect(PLAYER_FIELDS, &self.bytes, player_base(index))
    }

    /// Format a party field by key, or `None` for an unknown key.
    pub fn party_get(&self, key: &str) -> Option<String> {
        field_get(PARTY_FIELDS, &self.bytes, 0, key)
    }

    /// Set a party field by key, validating the value first.
    pub fn party_set(&mut self, key: &str, value: &str) -> Result<()> {
        field_set(PARTY_FIELDS, &mut self.bytes, 0, key, value)
    }

    /// All known party/game-state fields as `(section, label, value)` tuples.
    pub fn party_inspect(&self) -> Vec<(&'static str, &'static str, String)> {
        field_inspect(PARTY_FIELDS, &self.bytes, 0)
    }

    /// The keys of all known player fields.
    pub fn player_field_keys() -> impl Iterator<Item = &'static str> {
        PLAYER_FIELDS.iter().map(|f| f.key)
    }

    /// The keys of all known party fields.
    pub fn party_field_keys() -> impl Iterator<Item = &'static str> {
        PARTY_FIELDS.iter().map(|f| f.key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A synthetic save with one occupied player (Mariah-like) and known party state.
    fn synthetic() -> Vec<u8> {
        let mut buf = vec![0u8; SAVE_LEN];
        // Player 0 at 0x08.
        let base = HEADER_LEN;
        buf[base..base + 2].copy_from_slice(&200u16.to_le_bytes()); // hp
        buf[base + 2..base + 4].copy_from_slice(&200u16.to_le_bytes()); // hpMax
        buf[base + 6..base + 8].copy_from_slice(&9u16.to_le_bytes()); // str
        buf[base + 0x0A..base + 0x0C].copy_from_slice(&20u16.to_le_bytes()); // int
        buf[base + 0x10..base + 0x12].copy_from_slice(&1u16.to_le_bytes()); // weapon = Staff
        buf[base + 0x12..base + 0x14].copy_from_slice(&1u16.to_le_bytes()); // armor = Cloth
        buf[base + 0x14..base + 0x1A].copy_from_slice(b"Mariah"); // name
        buf[base + 0x24] = 0x0C; // sex = Female
        buf[base + 0x25] = 0x00; // class = Mage
        buf[base + 0x26] = b'G'; // status = Good
                                 // Party state.
        buf[0x140..0x144].copy_from_slice(&30099u32.to_le_bytes()); // food
        buf[0x144..0x146].copy_from_slice(&200u16.to_le_bytes()); // gold
        buf[0x146..0x148].copy_from_slice(&50u16.to_le_bytes()); // honesty
        buf[0x1D4] = 86; // x
        buf[0x1D5] = 108; // y
        buf
    }

    #[test]
    fn parses_a_player_and_party_state() {
        let save = Ultima4Save::from_bytes(synthetic()).unwrap();
        assert_eq!(save.occupied_players(), vec![0]);
        assert_eq!(save.player_get(0, "name").as_deref(), Some("Mariah"));
        assert_eq!(save.player_get(0, "hp").as_deref(), Some("200"));
        assert_eq!(save.player_get(0, "class").as_deref(), Some("Mage"));
        assert_eq!(save.player_get(0, "sex").as_deref(), Some("Female"));
        assert_eq!(save.player_get(0, "status").as_deref(), Some("Good"));
        assert_eq!(save.player_get(0, "weapon").as_deref(), Some("Staff"));
        assert_eq!(save.party_get("gold").as_deref(), Some("200"));
        assert_eq!(save.party_get("food").as_deref(), Some("300"));
        assert_eq!(save.party_get("honesty").as_deref(), Some("50"));
        assert_eq!(save.party_get("x").as_deref(), Some("86"));
    }

    #[test]
    fn edits_round_trip_and_touch_only_target_bytes() {
        let mut save = Ultima4Save::from_bytes(synthetic()).unwrap();
        let before = save.as_bytes().to_vec();

        save.player_set(0, "class", "Fighter").unwrap();
        save.player_set(0, "hp", "999").unwrap();
        save.party_set("gold", "9999").unwrap();

        assert_eq!(save.player_get(0, "class").as_deref(), Some("Fighter"));
        assert_eq!(save.player_get(0, "hp").as_deref(), Some("999"));
        assert_eq!(save.party_get("gold").as_deref(), Some("9999"));

        // Only the edited bytes changed.
        let after = save.as_bytes();
        let changed: Vec<usize> = (0..SAVE_LEN).filter(|&i| before[i] != after[i]).collect();
        // class (1 byte @ 0x08+0x25), hp (2 @ 0x08), gold (2 @ 0x144).
        assert_eq!(changed, vec![0x08, 0x09, 0x2D, 0x144, 0x145]);
    }

    #[test]
    fn validation_rejects_bad_values() {
        let mut save = Ultima4Save::from_bytes(synthetic()).unwrap();
        assert!(save.player_set(0, "class", "Wizard").is_err()); // not a U4 class
        assert!(save.player_set(0, "strength", "999").is_err()); // over max 99
        assert!(save.party_set("gold", "banana").is_err());
        assert!(save.player_set(0, "bogus", "1").is_err()); // unknown key
    }

    #[test]
    fn write_then_load_is_identical() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("PARTY.SAV");
        let mut save = Ultima4Save::from_bytes(synthetic()).unwrap();
        save.player_set(0, "name", "Avatar").unwrap();
        save.write(&path).unwrap();
        let reloaded = Ultima4Save::load(&path).unwrap();
        assert_eq!(reloaded.player_get(0, "name").as_deref(), Some("Avatar"));
        assert_eq!(reloaded.as_bytes(), save.as_bytes());
    }
}
