//! Hardcoded Ultima I save-file support (`PLAYER*.U1`).
//!
//! Format reference: <https://moddingwiki.shikadi.net/wiki/Ultima_I_Save_Game_Format>
//! (reverse-engineered by TheAlmightyGuru and Daniel D'Agostino). All multi-byte values
//! are little-endian 16-bit integers.
//!
//! The parser keeps the complete raw byte buffer and reads known offsets from it. Edits
//! mutate only known offsets in place, so bytes we don't understand are always preserved.

use std::fmt::Write as _;
use std::path::Path;

use crate::schema::{self, Endian, Field, FieldKind, Variants};
use crate::{Error, Result};

/// Total size of an Ultima I save file, in bytes (`0x334`).
pub const SAVE_LEN: usize = 0x334;

/// Length of the name field (bytes `0x00..0x0F`), including the null terminator.
const NAME_LEN: usize = 15;

// --- Enum tables (value -> label). Data-driven so `inspect`, `get`, and `set` all share
// one source of truth. Values are decimal (the wiki uses hex). ---

type EnumTable = Variants;

const RACE: EnumTable = &[(0, "Human"), (1, "Elf"), (2, "Dwarf"), (3, "Bobbit")];
const CLASS: EnumTable = &[(0, "Fighter"), (1, "Cleric"), (2, "Wizard"), (3, "Thief")];
const SEX: EnumTable = &[(0, "Male"), (1, "Female")];
const WEAPON: EnumTable = &[
    (0, "None"),
    (1, "Dagger"),
    (2, "Mace"),
    (3, "Axe"),
    (4, "Rope & Spikes"),
    (5, "Sword"),
    (6, "Great Sword"),
    (7, "Bow & Arrows"),
    (8, "Amulet"),
    (9, "Wand"),
    (10, "Staff"),
    (11, "Triangle"),
    (12, "Pistol"),
    (13, "Light Sword"),
    (14, "Phazor"),
    (15, "Blaster"),
];
const SPELL: EnumTable = &[
    (0, "None"),
    (1, "Open"),
    (2, "Unlock"),
    (3, "Magic Missile"),
    (4, "Steal"),
    (5, "Ladder Down"),
    (6, "Ladder Up"),
    (7, "Blink"),
    (8, "Create"),
    (9, "Destroy"),
    (10, "Kill"),
];
const ARMOUR: EnumTable = &[
    (0, "None"),
    (1, "Leather"),
    (2, "Chain Mail"),
    (3, "Plate Mail"),
    (4, "Vacuum Suit"),
    (5, "Reflect Suit"),
];
const TRANSPORT: EnumTable = &[
    (0, "Walking"),
    (1, "Horse"),
    (2, "Cart"),
    (3, "Raft"),
    (4, "Frigate"),
    (5, "Aircar"),
];

/// An Ultima I integer field: a little-endian `u16` with an inclusive edit maximum.
const fn u16le(max: u32) -> FieldKind {
    FieldKind::Int {
        bytes: 2,
        endian: Endian::Little,
        max,
    }
}

/// An Ultima I enum field: a little-endian `u16` mapped to named variants.
const fn enum16(variants: Variants) -> FieldKind {
    FieldKind::Enum {
        bytes: 2,
        endian: Endian::Little,
        variants,
    }
}

// Display sections, used to group `inspect` output.
const S_CHARACTER: &str = "Character";
const S_ATTRIBUTES: &str = "Attributes";
const S_STATUS: &str = "Status";
const S_EQUIPPED: &str = "Equipped";
const S_LOCATION: &str = "Location";
const S_GEMS: &str = "Inventory: Gems";
const S_ARMOUR: &str = "Inventory: Armour";
const S_WEAPONS: &str = "Inventory: Weapons";
const S_SPELLS: &str = "Inventory: Spells";
const S_TRANSPORTS: &str = "Inventory: Transports";

/// Every field we understand, grouped by section. This table drives `inspect`, `get`,
/// and `set` through the generic [`schema`] engine. Order here is for display; each field
/// carries its own byte offset, so the array order is independent of the on-disk layout.
#[rustfmt::skip]
const FIELDS: &[Field] = &[
    // Character
    Field::new("name",  "Name",  0x00, FieldKind::Name { len: NAME_LEN }).in_section(S_CHARACTER),
    Field::new("race",  "Race",  0x10, enum16(RACE)).in_section(S_CHARACTER),
    Field::new("class", "Class", 0x12, enum16(CLASS)).in_section(S_CHARACTER),
    Field::new("sex",   "Sex",   0x14, enum16(SEX)).in_section(S_CHARACTER),

    // Attributes
    Field::new("strength",     "Strength",     0x18, u16le(9999)).in_section(S_ATTRIBUTES),
    Field::new("agility",      "Agility",      0x1A, u16le(9999)).in_section(S_ATTRIBUTES),
    Field::new("stamina",      "Stamina",      0x1C, u16le(9999)).in_section(S_ATTRIBUTES),
    Field::new("charisma",     "Charisma",     0x1E, u16le(9999)).in_section(S_ATTRIBUTES),
    Field::new("wisdom",       "Wisdom",       0x20, u16le(9999)).in_section(S_ATTRIBUTES),
    Field::new("intelligence", "Intelligence", 0x22, u16le(9999)).in_section(S_ATTRIBUTES),

    // Status
    Field::new("hits",       "Hits",       0x16, u16le(9999)).in_section(S_STATUS),
    Field::new("gold",       "Gold",       0x24, u16le(9999)).in_section(S_STATUS),
    Field::new("experience", "Experience", 0x26, u16le(9999)).in_section(S_STATUS),
    Field::new("food",       "Food",       0x28, u16le(9999)).in_section(S_STATUS),

    // Equipped
    Field::new("weapon",    "Ready Weapon", 0x2A, enum16(WEAPON)).in_section(S_EQUIPPED),
    Field::new("spell",     "Ready Spell",  0x2C, enum16(SPELL)).in_section(S_EQUIPPED),
    Field::new("armour",    "Ready Armour", 0x2E, enum16(ARMOUR)).in_section(S_EQUIPPED),
    Field::new("transport", "Transport",    0x30, enum16(TRANSPORT)).in_section(S_EQUIPPED),

    // Location
    Field::new("x",             "Map X",         0x34, u16le(u16::MAX as u32)).in_section(S_LOCATION),
    Field::new("y",             "Map Y",         0x36, u16le(u16::MAX as u32)).in_section(S_LOCATION),
    Field::new("last_signpost", "Last Signpost", 0xA8, u16le(u16::MAX as u32)).in_section(S_LOCATION),
    Field::new("steps",         "Steps",         0xAC, u16le(u16::MAX as u32)).in_section(S_LOCATION),

    // Inventory: Gems
    Field::new("gem_red",   "Red",   0x4C, u16le(9999)).in_section(S_GEMS),
    Field::new("gem_green", "Green", 0x4E, u16le(9999)).in_section(S_GEMS),
    Field::new("gem_blue",  "Blue",  0x50, u16le(9999)).in_section(S_GEMS),
    Field::new("gem_white", "White", 0x52, u16le(9999)).in_section(S_GEMS),

    // Inventory: Armour
    Field::new("armour_leather",      "Leather",      0x56, u16le(9999)).in_section(S_ARMOUR),
    Field::new("armour_chain_mail",   "Chain Mail",   0x58, u16le(9999)).in_section(S_ARMOUR),
    Field::new("armour_plate_mail",   "Plate Mail",   0x5A, u16le(9999)).in_section(S_ARMOUR),
    Field::new("armour_vacuum_suit",  "Vacuum Suit",  0x5C, u16le(9999)).in_section(S_ARMOUR),
    Field::new("armour_reflect_suit", "Reflect Suit", 0x5E, u16le(9999)).in_section(S_ARMOUR),

    // Inventory: Weapons
    Field::new("weapon_dagger",      "Dagger",        0x62, u16le(9999)).in_section(S_WEAPONS),
    Field::new("weapon_mace",        "Mace",          0x64, u16le(9999)).in_section(S_WEAPONS),
    Field::new("weapon_axe",         "Axe",           0x66, u16le(9999)).in_section(S_WEAPONS),
    Field::new("weapon_rope_spikes", "Rope & Spikes", 0x68, u16le(9999)).in_section(S_WEAPONS),
    Field::new("weapon_sword",       "Sword",         0x6A, u16le(9999)).in_section(S_WEAPONS),
    Field::new("weapon_great_sword", "Great Sword",   0x6C, u16le(9999)).in_section(S_WEAPONS),
    Field::new("weapon_bow",         "Bow & Arrows",  0x6E, u16le(9999)).in_section(S_WEAPONS),
    Field::new("weapon_amulet",      "Amulet",        0x70, u16le(9999)).in_section(S_WEAPONS),
    Field::new("weapon_wand",        "Wand",          0x72, u16le(9999)).in_section(S_WEAPONS),
    Field::new("weapon_staff",       "Staff",         0x74, u16le(9999)).in_section(S_WEAPONS),
    Field::new("weapon_triangle",    "Triangle",      0x76, u16le(9999)).in_section(S_WEAPONS),
    Field::new("weapon_pistol",      "Pistol",        0x78, u16le(9999)).in_section(S_WEAPONS),
    Field::new("weapon_light_sword", "Light Sword",   0x7A, u16le(9999)).in_section(S_WEAPONS),
    Field::new("weapon_phazor",      "Phazor",        0x7C, u16le(9999)).in_section(S_WEAPONS),
    Field::new("weapon_blaster",     "Blaster",       0x7E, u16le(9999)).in_section(S_WEAPONS),

    // Inventory: Spells
    Field::new("spell_open",          "Open",          0x82, u16le(9999)).in_section(S_SPELLS),
    Field::new("spell_unlock",        "Unlock",        0x84, u16le(9999)).in_section(S_SPELLS),
    Field::new("spell_magic_missile", "Magic Missile", 0x86, u16le(9999)).in_section(S_SPELLS),
    Field::new("spell_steal",         "Steal",         0x88, u16le(9999)).in_section(S_SPELLS),
    Field::new("spell_ladder_down",   "Ladder Down",   0x8A, u16le(9999)).in_section(S_SPELLS),
    Field::new("spell_ladder_up",     "Ladder Up",     0x8C, u16le(9999)).in_section(S_SPELLS),
    Field::new("spell_blink",         "Blink",         0x8E, u16le(9999)).in_section(S_SPELLS),
    Field::new("spell_create",        "Create",        0x90, u16le(9999)).in_section(S_SPELLS),
    Field::new("spell_destroy",       "Destroy",       0x92, u16le(9999)).in_section(S_SPELLS),
    Field::new("spell_kill",          "Kill",          0x94, u16le(9999)).in_section(S_SPELLS),

    // Inventory: Transports
    Field::new("transport_horse",        "Horse",        0x98, u16le(9999)).in_section(S_TRANSPORTS),
    Field::new("transport_cart",         "Cart",         0x9A, u16le(9999)).in_section(S_TRANSPORTS),
    Field::new("transport_raft",         "Raft",         0x9C, u16le(9999)).in_section(S_TRANSPORTS),
    Field::new("transport_frigate",      "Frigate",      0x9E, u16le(9999)).in_section(S_TRANSPORTS),
    Field::new("transport_aircar",       "Aircar",       0xA0, u16le(9999)).in_section(S_TRANSPORTS),
    Field::new("transport_shuttle",      "Shuttle",      0xA2, u16le(9999)).in_section(S_TRANSPORTS),
    Field::new("transport_time_machine", "Time Machine", 0xA4, u16le(9999)).in_section(S_TRANSPORTS),
];

/// A parsed Ultima I save file.
///
/// Holds the complete raw byte buffer. Reads and edits operate on known offsets, so
/// bytes we don't understand are always preserved.
#[derive(Clone)]
pub struct Ultima1Save {
    bytes: Vec<u8>,
}

impl Ultima1Save {
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
        let bytes = std::fs::read(path)?;
        Self::from_bytes(bytes)
    }

    /// The complete raw byte buffer.
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// The character's name (bytes `0x00..0x0F`, null-terminated).
    pub fn name(&self) -> String {
        let raw = &self.bytes[0..NAME_LEN];
        let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
        String::from_utf8_lossy(&raw[..end]).into_owned()
    }

    /// Format a single field by key (e.g. `"strength"`) for display.
    /// Returns `None` if the key is not a known field.
    pub fn get_field(&self, key: &str) -> Option<String> {
        let field = FIELDS.iter().find(|f| f.key == key)?;
        Some(schema::read_field(&self.bytes, 0, field))
    }

    /// All known fields as `(section, label, value)` triples, in display order.
    pub fn inspect(&self) -> Vec<(&'static str, &'static str, String)> {
        FIELDS
            .iter()
            .map(|f| {
                (
                    f.section.unwrap_or_default(),
                    f.label,
                    schema::read_field(&self.bytes, 0, f),
                )
            })
            .collect()
    }

    /// The keys of all known fields (for help and error messages).
    pub fn field_keys() -> impl Iterator<Item = &'static str> {
        FIELDS.iter().map(|f| f.key)
    }

    /// The schema field table (key, label, kind) for building editors.
    pub fn fields() -> &'static [Field] {
        FIELDS
    }

    /// Set a field by key from a string value, validating it first.
    ///
    /// Numbers accept decimal input; enum fields accept either the numeric value or a
    /// variant name (case-insensitive). Only the field's own bytes are modified, so
    /// every byte we don't understand is preserved.
    pub fn set_field(&mut self, key: &str, value: &str) -> Result<()> {
        let field = FIELDS
            .iter()
            .find(|f| f.key == key)
            .ok_or_else(|| Error::Format(format!("unknown field '{key}'")))?;
        schema::write_field(&mut self.bytes, 0, field, value)
    }

    /// Write this save to `path` atomically. Callers are responsible for backups.
    pub fn write(&self, path: impl AsRef<Path>) -> Result<()> {
        crate::save::atomic_write(path, &self.bytes)
    }
}

/// Render an `xxd`-style hex dump of `bytes[start..end]` (offset, hex, ASCII).
///
/// The output is aligned to 16-byte rows; `start` is shown in context even if it is
/// not itself 16-byte aligned. `end` is clamped to the buffer length.
pub fn hex_dump(bytes: &[u8], start: usize, end: usize) -> String {
    let end = end.min(bytes.len());
    let mut out = String::new();
    let mut addr = start & !0xF; // align down to the start of the row
    while addr < end {
        let _ = write!(out, "{addr:08x}  ");
        let mut ascii = String::new();
        for i in 0..16 {
            let pos = addr + i;
            if pos >= start && pos < end {
                let b = bytes[pos];
                let _ = write!(out, "{b:02x} ");
                ascii.push(if (0x20..0x7f).contains(&b) {
                    b as char
                } else {
                    '.'
                });
            } else {
                out.push_str("   ");
                ascii.push(' ');
            }
            if i == 7 {
                out.push(' ');
            }
        }
        let _ = writeln!(out, " |{ascii}|");
        addr += 16;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put_u16(buf: &mut [u8], off: usize, val: u16) {
        buf[off..off + 2].copy_from_slice(&val.to_le_bytes());
    }

    /// A synthetic 820-byte save with known field values (the "Enki" test character).
    fn synthetic() -> Vec<u8> {
        let mut buf = vec![0u8; SAVE_LEN];
        buf[0..4].copy_from_slice(b"Enki"); // name (already null-padded)
        put_u16(&mut buf, 0x10, 0); // race: Human
        put_u16(&mut buf, 0x12, 2); // class: Wizard
        put_u16(&mut buf, 0x14, 0); // sex: Male
        put_u16(&mut buf, 0x16, 150); // hits
        put_u16(&mut buf, 0x18, 12); // strength
        put_u16(&mut buf, 0x1A, 13); // agility
        put_u16(&mut buf, 0x1C, 14); // stamina
        put_u16(&mut buf, 0x1E, 15); // charisma
        put_u16(&mut buf, 0x20, 16); // wisdom
        put_u16(&mut buf, 0x22, 35); // intelligence
        put_u16(&mut buf, 0x24, 100); // gold
        put_u16(&mut buf, 0x28, 200); // food
        put_u16(&mut buf, 0x2A, 1); // weapon: Dagger
        put_u16(&mut buf, 0x2E, 1); // armour: Leather
        put_u16(&mut buf, 0x30, 0); // transport: Walking
        put_u16(&mut buf, 0x34, 49); // map x
        put_u16(&mut buf, 0x36, 40); // map y
        buf
    }

    #[test]
    fn parses_known_fields() {
        let save = Ultima1Save::from_bytes(synthetic()).unwrap();
        assert_eq!(save.name(), "Enki");
        assert_eq!(save.get_field("race").unwrap(), "Human");
        assert_eq!(save.get_field("class").unwrap(), "Wizard");
        assert_eq!(save.get_field("strength").unwrap(), "12");
        assert_eq!(save.get_field("intelligence").unwrap(), "35");
        assert_eq!(save.get_field("gold").unwrap(), "100");
        assert_eq!(save.get_field("food").unwrap(), "200");
        assert_eq!(save.get_field("weapon").unwrap(), "Dagger");
        assert_eq!(save.get_field("armour").unwrap(), "Leather");
        assert_eq!(save.get_field("transport").unwrap(), "Walking");
    }

    #[test]
    fn round_trip_preserves_all_bytes() {
        let original = synthetic();
        let save = Ultima1Save::from_bytes(original.clone()).unwrap();
        assert_eq!(save.as_bytes(), original.as_slice());
    }

    #[test]
    fn rejects_wrong_length() {
        assert!(Ultima1Save::from_bytes(vec![0u8; 100]).is_err());
    }

    #[test]
    fn unknown_field_returns_none() {
        let save = Ultima1Save::from_bytes(synthetic()).unwrap();
        assert!(save.get_field("nonexistent").is_none());
    }

    #[test]
    fn unknown_enum_value_is_labeled() {
        let mut buf = synthetic();
        put_u16(&mut buf, 0x30, 99); // not a valid transport
        let save = Ultima1Save::from_bytes(buf).unwrap();
        assert_eq!(save.get_field("transport").unwrap(), "Unknown (99)");
    }

    #[test]
    fn hex_dump_covers_requested_range() {
        let save = Ultima1Save::from_bytes(synthetic()).unwrap();
        let dump = hex_dump(save.as_bytes(), 0x00, 0x08);
        assert!(dump.contains("45 6e 6b 69")); // "Enki"
        assert!(dump.contains("|Enki"));
    }

    #[test]
    fn set_number_updates_value() {
        let mut save = Ultima1Save::from_bytes(synthetic()).unwrap();
        save.set_field("gold", "500").unwrap();
        assert_eq!(save.get_field("gold").unwrap(), "500");
    }

    #[test]
    fn set_enum_by_name_and_number() {
        let mut save = Ultima1Save::from_bytes(synthetic()).unwrap();
        save.set_field("transport", "aircar").unwrap();
        assert_eq!(save.get_field("transport").unwrap(), "Aircar");
        save.set_field("transport", "1").unwrap();
        assert_eq!(save.get_field("transport").unwrap(), "Horse");
    }

    #[test]
    fn set_changes_only_target_bytes() {
        let mut save = Ultima1Save::from_bytes(synthetic()).unwrap();
        let before = save.as_bytes().to_vec();
        save.set_field("gold", "500").unwrap();
        for (i, (a, b)) in before.iter().zip(save.as_bytes()).enumerate() {
            if i == 0x24 || i == 0x25 {
                continue; // the two bytes of the gold field
            }
            assert_eq!(a, b, "byte {i:#04x} changed unexpectedly");
        }
    }

    #[test]
    fn set_rejects_invalid_input() {
        let mut save = Ultima1Save::from_bytes(synthetic()).unwrap();
        assert!(save.set_field("gold", "banana").is_err()); // not a number
        assert!(save.set_field("gold", "10000").is_err()); // over max
        assert!(save.set_field("transport", "spaceship").is_err()); // bad enum
        assert!(save.set_field("nope", "1").is_err()); // unknown field
    }

    #[test]
    fn set_name_updates_and_pads() {
        let mut save = Ultima1Save::from_bytes(synthetic()).unwrap();
        save.set_field("name", "Mondain").unwrap();
        assert_eq!(save.name(), "Mondain");
        // Everything after the 7-char name must be null-padded.
        assert_eq!(&save.as_bytes()[7..NAME_LEN], &[0u8; NAME_LEN - 7]);
        assert!(save.set_field("name", "ThisNameIsWayTooLong").is_err());
    }

    #[test]
    fn set_inventory_fields() {
        let mut save = Ultima1Save::from_bytes(synthetic()).unwrap();

        save.set_field("transport_time_machine", "1").unwrap();
        assert_eq!(save.get_field("transport_time_machine").unwrap(), "1");
        assert_eq!(&save.as_bytes()[0xA4..0xA6], &1u16.to_le_bytes());

        save.set_field("weapon_blaster", "5").unwrap();
        assert_eq!(save.get_field("weapon_blaster").unwrap(), "5");
        assert_eq!(&save.as_bytes()[0x7E..0x80], &5u16.to_le_bytes());

        save.set_field("gem_white", "3").unwrap();
        assert_eq!(&save.as_bytes()[0x52..0x54], &3u16.to_le_bytes());
    }

    #[test]
    fn inspect_is_grouped_into_sections() {
        let save = Ultima1Save::from_bytes(synthetic()).unwrap();
        let rows = save.inspect();
        assert!(rows.iter().any(|(s, _, _)| *s == "Character"));
        assert!(rows
            .iter()
            .any(|(s, l, _)| *s == "Inventory: Transports" && *l == "Time Machine"));
    }

    #[test]
    fn write_then_load_is_identical() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("PLAYER1.U1");
        let save = Ultima1Save::from_bytes(synthetic()).unwrap();
        save.write(&path).unwrap();
        let reloaded = Ultima1Save::load(&path).unwrap();
        assert_eq!(reloaded.as_bytes(), save.as_bytes());
    }
}
