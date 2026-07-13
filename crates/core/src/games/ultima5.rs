//! Hardcoded Ultima V (Warriors of Destiny) save support (`SAVED.GAM`).
//!
//! Format reference: the [Ultima Codex] "Ultima V internal formats" page (the
//! `SAVED.GAM and RAM` section), cross-checked byte-for-byte against a real 4192-byte
//! `SAVED.GAM`.
//!
//! [Ultima Codex]: https://wiki.ultimacodex.com/wiki/Ultima_V_internal_formats
//!
//! `SAVED.GAM` is a snapshot of the game's working RAM. The first `0x1060` bytes are
//! written to disk: a 2-byte header, then sixteen fixed 32-byte character records, then a
//! block of party/game state (provisions, inventory, reagents, date, karma, location).
//! Numbers are plain little-endian binary. Edits mutate only known offsets in place,
//! preserving unknown bytes.

use std::path::Path;

use crate::schema::{self, Endian, Field, FieldKind, Variants};
use crate::{Error, Result};

/// Total size of the on-disk portion of a `SAVED.GAM` file, in bytes (`0x1060`).
pub const SAVE_LEN: usize = 4192;
/// Bytes before the first character record.
const HEADER_LEN: usize = 2;
/// Size of one character record, in bytes.
const RECORD_LEN: usize = 0x20;
/// Number of character slots in the roster.
pub const CHARACTER_COUNT: usize = 16;
/// Length of a character's name field, including the null terminator.
const NAME_LEN: usize = 9;

// --- Enum tables (value -> label). ---

const SEX: Variants = &[(0x0B, "Male"), (0x0C, "Female")];
const CLASS: Variants = &[
    (b'A' as u32, "Avatar"),
    (b'B' as u32, "Bard"),
    (b'F' as u32, "Fighter"),
    (b'M' as u32, "Mage"),
];
const STATUS: Variants = &[
    (b'G' as u32, "Good"),
    (b'P' as u32, "Poisoned"),
    (b'C' as u32, "Charmed"),
    (b'S' as u32, "Asleep"),
    (b'D' as u32, "Dead"),
];

/// A little-endian `u16` field with an inclusive edit maximum.
const fn u16le(max: u32) -> FieldKind {
    FieldKind::Int {
        bytes: 2,
        endian: Endian::Little,
        max,
    }
}

/// A single-byte integer field with an inclusive edit maximum.
const fn u8m(max: u32) -> FieldKind {
    FieldKind::Int {
        bytes: 1,
        endian: Endian::Little,
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
const S_INVENTORY: &str = "Inventory";
const S_REAGENTS: &str = "Reagents";
const S_TIME: &str = "Date & Time";
const S_LOCATION: &str = "Location";

/// The fields of one character record (offsets are within the 32-byte record).
#[rustfmt::skip]
const CHARACTER_FIELDS: &[Field] = &[
    Field::new("name",         "Name",          0x00, FieldKind::Name { len: NAME_LEN }).in_section(S_CHARACTER),
    Field::new("sex",          "Sex",           0x09, enum1(SEX)).in_section(S_CHARACTER),
    Field::new("class",        "Class",         0x0A, letter(CLASS)).in_section(S_CHARACTER),
    Field::new("status",       "Status",        0x0B, letter(STATUS)).in_section(S_CHARACTER),
    Field::new("strength",     "Strength",      0x0C, u8m(30)).in_section(S_ATTRIBUTES),
    Field::new("dexterity",    "Dexterity",     0x0D, u8m(30)).in_section(S_ATTRIBUTES),
    Field::new("intelligence", "Intelligence",  0x0E, u8m(30)).in_section(S_ATTRIBUTES),
    Field::new("magic",        "Magic Points",  0x0F, u8m(99)).in_section(S_VITALS),
    Field::new("hp",           "Hit Points",    0x10, u16le(9999)).in_section(S_VITALS),
    Field::new("hp_max",       "Max HP",        0x12, u16le(9999)).in_section(S_VITALS),
    Field::new("experience",   "Experience",    0x14, u16le(9999)).in_section(S_VITALS),
    Field::new("level",        "Level",         0x16, u8m(8)).in_section(S_VITALS),
    Field::new("months_inn",   "Months at Inn", 0x17, u8m(99)).in_section(S_VITALS),
    Field::new("helmet",       "Helmet",        0x19, FieldKind::Byte).in_section(S_EQUIPPED),
    Field::new("armor",        "Armor",         0x1A, FieldKind::Byte).in_section(S_EQUIPPED),
    Field::new("weapon_left",  "Weapon (Left)", 0x1B, FieldKind::Byte).in_section(S_EQUIPPED),
    Field::new("weapon_right", "Weapon (Right)",0x1C, FieldKind::Byte).in_section(S_EQUIPPED),
    Field::new("ring",         "Ring",          0x1D, FieldKind::Byte).in_section(S_EQUIPPED),
    Field::new("amulet",       "Amulet",        0x1E, FieldKind::Byte).in_section(S_EQUIPPED),
];

/// The party/game-state fields (absolute file offsets, applied at base 0).
#[rustfmt::skip]
const PARTY_FIELDS: &[Field] = &[
    Field::new("food",    "Food",          0x202, u16le(9999)).in_section(S_PARTY),
    Field::new("gold",    "Gold",          0x204, u16le(9999)).in_section(S_PARTY),
    Field::new("members", "Party Members", 0x2B5, u8m(6)).in_section(S_PARTY),
    Field::new("keys",          "Keys",          0x206, u8m(99)).in_section(S_INVENTORY),
    Field::new("gems",          "Gems",          0x207, u8m(99)).in_section(S_INVENTORY),
    Field::new("torches",       "Torches",       0x208, u8m(99)).in_section(S_INVENTORY),
    Field::new("magic_carpets", "Magic Carpets", 0x20A, u8m(99)).in_section(S_INVENTORY),
    Field::new("skull_keys",    "Skull Keys",    0x20B, u8m(99)).in_section(S_INVENTORY),
    Field::new("sextants",      "Sextants",      0x216, u8m(99)).in_section(S_INVENTORY),
    Field::new("reagent_ash",        "Sulfurous Ash", 0x2AA, u8m(99)).in_section(S_REAGENTS),
    Field::new("reagent_ginseng",    "Ginseng",       0x2AB, u8m(99)).in_section(S_REAGENTS),
    Field::new("reagent_garlic",     "Garlic",        0x2AC, u8m(99)).in_section(S_REAGENTS),
    Field::new("reagent_silk",       "Spider Silk",   0x2AD, u8m(99)).in_section(S_REAGENTS),
    Field::new("reagent_moss",       "Blood Moss",    0x2AE, u8m(99)).in_section(S_REAGENTS),
    Field::new("reagent_pearl",      "Black Pearl",   0x2AF, u8m(99)).in_section(S_REAGENTS),
    Field::new("reagent_nightshade", "Nightshade",    0x2B0, u8m(99)).in_section(S_REAGENTS),
    Field::new("reagent_mandrake",   "Mandrake Root", 0x2B1, u8m(99)).in_section(S_REAGENTS),
    Field::new("year",   "Year",   0x2CE, u16le(9999)).in_section(S_TIME),
    Field::new("month",  "Month",  0x2D7, u8m(13)).in_section(S_TIME),
    Field::new("day",    "Day",    0x2D8, u8m(28)).in_section(S_TIME),
    Field::new("hour",   "Hour",   0x2D9, u8m(23)).in_section(S_TIME),
    Field::new("minute", "Minute", 0x2DB, u8m(59)).in_section(S_TIME),
    Field::new("karma",    "Karma",    0x2E2, FieldKind::Byte).in_section(S_LOCATION),
    Field::new("location", "Location", 0x2ED, FieldKind::Byte).in_section(S_LOCATION),
    Field::new("z", "Map Z", 0x2EF, FieldKind::Byte).in_section(S_LOCATION),
    Field::new("x", "Map X", 0x2F0, FieldKind::Byte).in_section(S_LOCATION),
    Field::new("y", "Map Y", 0x2F1, FieldKind::Byte).in_section(S_LOCATION),
];

// --- Record helpers, parameterized by a base offset. ---

fn character_base(index: usize) -> usize {
    HEADER_LEN + index * RECORD_LEN
}

/// Whether a character slot holds a character. Unused roster slots are zero-filled; a
/// real record always has a class letter, so a non-zero class byte marks an occupied slot
/// (the Avatar's name can be blank early in a game, so the name alone isn't reliable).
fn character_is_occupied(buf: &[u8], index: usize) -> bool {
    buf[character_base(index) + 0x0A] != 0
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

/// The character-record field table (for building editors).
pub fn character_fields() -> &'static [Field] {
    CHARACTER_FIELDS
}

/// The party/game-state field table (for building editors).
pub fn party_fields() -> &'static [Field] {
    PARTY_FIELDS
}

/// A parsed Ultima V `SAVED.GAM` file.
#[derive(Clone)]
pub struct Ultima5Save {
    bytes: Vec<u8>,
}

impl Ultima5Save {
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

    /// The 0-based indices of character slots that hold a character.
    pub fn occupied_characters(&self) -> Vec<usize> {
        (0..CHARACTER_COUNT)
            .filter(|&i| character_is_occupied(&self.bytes, i))
            .collect()
    }

    /// A one-line summary of a character (0-based index).
    pub fn character_summary(&self, index: usize) -> String {
        let g = |key| self.character_get(index, key).unwrap_or_default();
        format!(
            "{} — {}, HP {}/{} · {}",
            g("name"),
            g("class"),
            g("hp"),
            g("hp_max"),
            g("status"),
        )
    }

    /// Format a character field by key, or `None` for an unknown character/key.
    pub fn character_get(&self, index: usize, key: &str) -> Option<String> {
        if index >= CHARACTER_COUNT {
            return None;
        }
        field_get(CHARACTER_FIELDS, &self.bytes, character_base(index), key)
    }

    /// Set a character field by key, validating the value first.
    pub fn character_set(&mut self, index: usize, key: &str, value: &str) -> Result<()> {
        if index >= CHARACTER_COUNT {
            return Err(Error::Format(format!(
                "character slot must be 1..={CHARACTER_COUNT} (got {})",
                index + 1
            )));
        }
        field_set(
            CHARACTER_FIELDS,
            &mut self.bytes,
            character_base(index),
            key,
            value,
        )
    }

    /// All known fields of a character as `(section, label, value)` tuples.
    pub fn character_inspect(&self, index: usize) -> Vec<(&'static str, &'static str, String)> {
        field_inspect(CHARACTER_FIELDS, &self.bytes, character_base(index))
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

    /// The keys of all known character fields.
    pub fn character_field_keys() -> impl Iterator<Item = &'static str> {
        CHARACTER_FIELDS.iter().map(|f| f.key)
    }

    /// The keys of all known party fields.
    pub fn party_field_keys() -> impl Iterator<Item = &'static str> {
        PARTY_FIELDS.iter().map(|f| f.key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A synthetic save with one occupied character (Avatar-like) and known party state.
    fn synthetic() -> Vec<u8> {
        let mut buf = vec![0u8; SAVE_LEN];
        // Character 0 at 0x02.
        let base = HEADER_LEN;
        buf[base..base + 6].copy_from_slice(b"Avatar"); // name
        buf[base + 0x09] = 0x0B; // sex = Male
        buf[base + 0x0A] = b'A'; // class = Avatar
        buf[base + 0x0B] = b'G'; // status = Good
        buf[base + 0x0C] = 15; // str
        buf[base + 0x0D] = 16; // dex
        buf[base + 0x0E] = 17; // int
        buf[base + 0x0F] = 5; // MP
        buf[base + 0x10..base + 0x12].copy_from_slice(&60u16.to_le_bytes()); // hp
        buf[base + 0x12..base + 0x14].copy_from_slice(&60u16.to_le_bytes()); // hpMax
        buf[base + 0x14..base + 0x16].copy_from_slice(&150u16.to_le_bytes()); // exp
        buf[base + 0x16] = 2; // level
                              // Party state.
        buf[0x202..0x204].copy_from_slice(&63u16.to_le_bytes()); // food
        buf[0x204..0x206].copy_from_slice(&150u16.to_le_bytes()); // gold
        buf[0x206] = 2; // keys
        buf[0x208] = 4; // torches
        buf[0x2AA] = 10; // sulfurous ash
        buf[0x2B5] = 3; // party members
        buf[0x2E2] = 99; // karma
        buf[0x2F0] = 86; // x
        buf[0x2F1] = 108; // y
        buf
    }

    #[test]
    fn parses_a_character_and_party_state() {
        let save = Ultima5Save::from_bytes(synthetic()).unwrap();
        assert_eq!(save.occupied_characters(), vec![0]);
        assert_eq!(save.character_get(0, "name").as_deref(), Some("Avatar"));
        assert_eq!(save.character_get(0, "class").as_deref(), Some("Avatar"));
        assert_eq!(save.character_get(0, "sex").as_deref(), Some("Male"));
        assert_eq!(save.character_get(0, "status").as_deref(), Some("Good"));
        assert_eq!(save.character_get(0, "hp").as_deref(), Some("60"));
        assert_eq!(save.character_get(0, "level").as_deref(), Some("2"));
        assert_eq!(save.party_get("food").as_deref(), Some("63"));
        assert_eq!(save.party_get("gold").as_deref(), Some("150"));
        assert_eq!(save.party_get("keys").as_deref(), Some("2"));
        assert_eq!(save.party_get("torches").as_deref(), Some("4"));
        assert_eq!(save.party_get("reagent_ash").as_deref(), Some("10"));
        assert_eq!(save.party_get("members").as_deref(), Some("3"));
        assert_eq!(save.party_get("karma").as_deref(), Some("99"));
        assert_eq!(save.party_get("x").as_deref(), Some("86"));
        assert_eq!(save.party_get("y").as_deref(), Some("108"));
    }

    #[test]
    fn edits_round_trip_and_touch_only_target_bytes() {
        let mut save = Ultima5Save::from_bytes(synthetic()).unwrap();
        let before = save.as_bytes().to_vec();

        save.character_set(0, "class", "Mage").unwrap();
        save.character_set(0, "hp", "240").unwrap();
        save.party_set("gold", "9999").unwrap();

        assert_eq!(save.character_get(0, "class").as_deref(), Some("Mage"));
        assert_eq!(save.character_get(0, "hp").as_deref(), Some("240"));
        assert_eq!(save.party_get("gold").as_deref(), Some("9999"));

        // Only the edited bytes changed.
        let after = save.as_bytes();
        let changed: Vec<usize> = (0..SAVE_LEN).filter(|&i| before[i] != after[i]).collect();
        // class (1 byte @ 0x02+0x0A), hp low byte (@ 0x02+0x10), gold (2 @ 0x204).
        assert_eq!(changed, vec![0x0C, 0x12, 0x204, 0x205]);
    }

    #[test]
    fn validation_rejects_bad_values() {
        let mut save = Ultima5Save::from_bytes(synthetic()).unwrap();
        assert!(save.character_set(0, "class", "Tinker").is_err()); // not a U5 class
        assert!(save.character_set(0, "strength", "99").is_err()); // over max 30
        assert!(save.party_set("gold", "banana").is_err());
        assert!(save.character_set(0, "bogus", "1").is_err()); // unknown key
    }

    #[test]
    fn write_then_load_is_identical() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("SAVED.GAM");
        let mut save = Ultima5Save::from_bytes(synthetic()).unwrap();
        save.character_set(0, "name", "Dupre").unwrap();
        save.write(&path).unwrap();
        let reloaded = Ultima5Save::load(&path).unwrap();
        assert_eq!(reloaded.character_get(0, "name").as_deref(), Some("Dupre"));
        assert_eq!(reloaded.as_bytes(), save.as_bytes());
    }
}
