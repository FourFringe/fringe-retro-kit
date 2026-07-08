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

use crate::{Error, Result};

/// Total size of an Ultima I save file, in bytes (`0x334`).
pub const SAVE_LEN: usize = 0x334;

/// Length of the name field (bytes `0x00..0x0F`), including the null terminator.
const NAME_LEN: usize = 15;

// --- Enum tables (value -> label). Data-driven so `inspect`, `get`, and `set` all share
// one source of truth. Values are decimal (the wiki uses hex). ---

type EnumTable = &'static [(u16, &'static str)];

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

/// How to interpret a field's bytes.
#[derive(Clone, Copy)]
enum Kind {
    /// The null-terminated character name at the start of the file.
    Name,
    /// An unsigned little-endian 16-bit integer with an inclusive maximum value.
    U16 { max: u16 },
    /// A little-endian 16-bit value with named variants.
    Enum(EnumTable),
}

/// A known field: a stable key, a display label, the section it belongs to, its byte
/// offset, and how to read it.
struct FieldDef {
    key: &'static str,
    label: &'static str,
    section: &'static str,
    offset: usize,
    kind: Kind,
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
/// and `set`. Order here is for display; each field carries its own byte offset, so the
/// array order is independent of the on-disk layout.
#[rustfmt::skip]
const FIELDS: &[FieldDef] = &[
    // Character
    FieldDef { key: "name",  label: "Name",  section: S_CHARACTER, offset: 0x00, kind: Kind::Name },
    FieldDef { key: "race",  label: "Race",  section: S_CHARACTER, offset: 0x10, kind: Kind::Enum(RACE) },
    FieldDef { key: "class", label: "Class", section: S_CHARACTER, offset: 0x12, kind: Kind::Enum(CLASS) },
    FieldDef { key: "sex",   label: "Sex",   section: S_CHARACTER, offset: 0x14, kind: Kind::Enum(SEX) },

    // Attributes
    FieldDef { key: "strength",     label: "Strength",     section: S_ATTRIBUTES, offset: 0x18, kind: Kind::U16 { max: 9999 } },
    FieldDef { key: "agility",      label: "Agility",      section: S_ATTRIBUTES, offset: 0x1A, kind: Kind::U16 { max: 9999 } },
    FieldDef { key: "stamina",      label: "Stamina",      section: S_ATTRIBUTES, offset: 0x1C, kind: Kind::U16 { max: 9999 } },
    FieldDef { key: "charisma",     label: "Charisma",     section: S_ATTRIBUTES, offset: 0x1E, kind: Kind::U16 { max: 9999 } },
    FieldDef { key: "wisdom",       label: "Wisdom",       section: S_ATTRIBUTES, offset: 0x20, kind: Kind::U16 { max: 9999 } },
    FieldDef { key: "intelligence", label: "Intelligence", section: S_ATTRIBUTES, offset: 0x22, kind: Kind::U16 { max: 9999 } },

    // Status
    FieldDef { key: "hits",       label: "Hits",       section: S_STATUS, offset: 0x16, kind: Kind::U16 { max: 9999 } },
    FieldDef { key: "gold",       label: "Gold",       section: S_STATUS, offset: 0x24, kind: Kind::U16 { max: 9999 } },
    FieldDef { key: "experience", label: "Experience", section: S_STATUS, offset: 0x26, kind: Kind::U16 { max: 9999 } },
    FieldDef { key: "food",       label: "Food",       section: S_STATUS, offset: 0x28, kind: Kind::U16 { max: 9999 } },

    // Equipped
    FieldDef { key: "weapon",    label: "Ready Weapon", section: S_EQUIPPED, offset: 0x2A, kind: Kind::Enum(WEAPON) },
    FieldDef { key: "spell",     label: "Ready Spell",  section: S_EQUIPPED, offset: 0x2C, kind: Kind::Enum(SPELL) },
    FieldDef { key: "armour",    label: "Ready Armour", section: S_EQUIPPED, offset: 0x2E, kind: Kind::Enum(ARMOUR) },
    FieldDef { key: "transport", label: "Transport",    section: S_EQUIPPED, offset: 0x30, kind: Kind::Enum(TRANSPORT) },

    // Location
    FieldDef { key: "x",             label: "Map X",         section: S_LOCATION, offset: 0x34, kind: Kind::U16 { max: u16::MAX } },
    FieldDef { key: "y",             label: "Map Y",         section: S_LOCATION, offset: 0x36, kind: Kind::U16 { max: u16::MAX } },
    FieldDef { key: "last_signpost", label: "Last Signpost", section: S_LOCATION, offset: 0xA8, kind: Kind::U16 { max: u16::MAX } },
    FieldDef { key: "steps",         label: "Steps",         section: S_LOCATION, offset: 0xAC, kind: Kind::U16 { max: u16::MAX } },

    // Inventory: Gems
    FieldDef { key: "gem_red",   label: "Red",   section: S_GEMS, offset: 0x4C, kind: Kind::U16 { max: 9999 } },
    FieldDef { key: "gem_green", label: "Green", section: S_GEMS, offset: 0x4E, kind: Kind::U16 { max: 9999 } },
    FieldDef { key: "gem_blue",  label: "Blue",  section: S_GEMS, offset: 0x50, kind: Kind::U16 { max: 9999 } },
    FieldDef { key: "gem_white", label: "White", section: S_GEMS, offset: 0x52, kind: Kind::U16 { max: 9999 } },

    // Inventory: Armour
    FieldDef { key: "armour_leather",      label: "Leather",      section: S_ARMOUR, offset: 0x56, kind: Kind::U16 { max: 9999 } },
    FieldDef { key: "armour_chain_mail",   label: "Chain Mail",   section: S_ARMOUR, offset: 0x58, kind: Kind::U16 { max: 9999 } },
    FieldDef { key: "armour_plate_mail",   label: "Plate Mail",   section: S_ARMOUR, offset: 0x5A, kind: Kind::U16 { max: 9999 } },
    FieldDef { key: "armour_vacuum_suit",  label: "Vacuum Suit",  section: S_ARMOUR, offset: 0x5C, kind: Kind::U16 { max: 9999 } },
    FieldDef { key: "armour_reflect_suit", label: "Reflect Suit", section: S_ARMOUR, offset: 0x5E, kind: Kind::U16 { max: 9999 } },

    // Inventory: Weapons
    FieldDef { key: "weapon_dagger",      label: "Dagger",        section: S_WEAPONS, offset: 0x62, kind: Kind::U16 { max: 9999 } },
    FieldDef { key: "weapon_mace",        label: "Mace",          section: S_WEAPONS, offset: 0x64, kind: Kind::U16 { max: 9999 } },
    FieldDef { key: "weapon_axe",         label: "Axe",           section: S_WEAPONS, offset: 0x66, kind: Kind::U16 { max: 9999 } },
    FieldDef { key: "weapon_rope_spikes", label: "Rope & Spikes", section: S_WEAPONS, offset: 0x68, kind: Kind::U16 { max: 9999 } },
    FieldDef { key: "weapon_sword",       label: "Sword",         section: S_WEAPONS, offset: 0x6A, kind: Kind::U16 { max: 9999 } },
    FieldDef { key: "weapon_great_sword", label: "Great Sword",   section: S_WEAPONS, offset: 0x6C, kind: Kind::U16 { max: 9999 } },
    FieldDef { key: "weapon_bow",         label: "Bow & Arrows",  section: S_WEAPONS, offset: 0x6E, kind: Kind::U16 { max: 9999 } },
    FieldDef { key: "weapon_amulet",      label: "Amulet",        section: S_WEAPONS, offset: 0x70, kind: Kind::U16 { max: 9999 } },
    FieldDef { key: "weapon_wand",        label: "Wand",          section: S_WEAPONS, offset: 0x72, kind: Kind::U16 { max: 9999 } },
    FieldDef { key: "weapon_staff",       label: "Staff",         section: S_WEAPONS, offset: 0x74, kind: Kind::U16 { max: 9999 } },
    FieldDef { key: "weapon_triangle",    label: "Triangle",      section: S_WEAPONS, offset: 0x76, kind: Kind::U16 { max: 9999 } },
    FieldDef { key: "weapon_pistol",      label: "Pistol",        section: S_WEAPONS, offset: 0x78, kind: Kind::U16 { max: 9999 } },
    FieldDef { key: "weapon_light_sword", label: "Light Sword",   section: S_WEAPONS, offset: 0x7A, kind: Kind::U16 { max: 9999 } },
    FieldDef { key: "weapon_phazor",      label: "Phazor",        section: S_WEAPONS, offset: 0x7C, kind: Kind::U16 { max: 9999 } },
    FieldDef { key: "weapon_blaster",     label: "Blaster",       section: S_WEAPONS, offset: 0x7E, kind: Kind::U16 { max: 9999 } },

    // Inventory: Spells
    FieldDef { key: "spell_open",          label: "Open",          section: S_SPELLS, offset: 0x82, kind: Kind::U16 { max: 9999 } },
    FieldDef { key: "spell_unlock",        label: "Unlock",        section: S_SPELLS, offset: 0x84, kind: Kind::U16 { max: 9999 } },
    FieldDef { key: "spell_magic_missile", label: "Magic Missile", section: S_SPELLS, offset: 0x86, kind: Kind::U16 { max: 9999 } },
    FieldDef { key: "spell_steal",         label: "Steal",         section: S_SPELLS, offset: 0x88, kind: Kind::U16 { max: 9999 } },
    FieldDef { key: "spell_ladder_down",   label: "Ladder Down",   section: S_SPELLS, offset: 0x8A, kind: Kind::U16 { max: 9999 } },
    FieldDef { key: "spell_ladder_up",     label: "Ladder Up",     section: S_SPELLS, offset: 0x8C, kind: Kind::U16 { max: 9999 } },
    FieldDef { key: "spell_blink",         label: "Blink",         section: S_SPELLS, offset: 0x8E, kind: Kind::U16 { max: 9999 } },
    FieldDef { key: "spell_create",        label: "Create",        section: S_SPELLS, offset: 0x90, kind: Kind::U16 { max: 9999 } },
    FieldDef { key: "spell_destroy",       label: "Destroy",       section: S_SPELLS, offset: 0x92, kind: Kind::U16 { max: 9999 } },
    FieldDef { key: "spell_kill",          label: "Kill",          section: S_SPELLS, offset: 0x94, kind: Kind::U16 { max: 9999 } },

    // Inventory: Transports
    FieldDef { key: "transport_horse",        label: "Horse",        section: S_TRANSPORTS, offset: 0x98, kind: Kind::U16 { max: 9999 } },
    FieldDef { key: "transport_cart",         label: "Cart",         section: S_TRANSPORTS, offset: 0x9A, kind: Kind::U16 { max: 9999 } },
    FieldDef { key: "transport_raft",         label: "Raft",         section: S_TRANSPORTS, offset: 0x9C, kind: Kind::U16 { max: 9999 } },
    FieldDef { key: "transport_frigate",      label: "Frigate",      section: S_TRANSPORTS, offset: 0x9E, kind: Kind::U16 { max: 9999 } },
    FieldDef { key: "transport_aircar",       label: "Aircar",       section: S_TRANSPORTS, offset: 0xA0, kind: Kind::U16 { max: 9999 } },
    FieldDef { key: "transport_shuttle",      label: "Shuttle",      section: S_TRANSPORTS, offset: 0xA2, kind: Kind::U16 { max: 9999 } },
    FieldDef { key: "transport_time_machine", label: "Time Machine", section: S_TRANSPORTS, offset: 0xA4, kind: Kind::U16 { max: 9999 } },
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
        Some(self.format_field(field))
    }

    /// All known fields as `(section, label, value)` triples, in display order.
    pub fn inspect(&self) -> Vec<(&'static str, &'static str, String)> {
        FIELDS
            .iter()
            .map(|f| (f.section, f.label, self.format_field(f)))
            .collect()
    }

    /// The keys of all known fields (for help and error messages).
    pub fn field_keys() -> impl Iterator<Item = &'static str> {
        FIELDS.iter().map(|f| f.key)
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
        match field.kind {
            Kind::Name => self.set_name(value)?,
            Kind::U16 { max } => {
                let n: u16 = value.parse().map_err(|_| {
                    Error::Format(format!("{} must be a number (got '{value}')", field.label))
                })?;
                if n > max {
                    return Err(Error::Format(format!(
                        "{} must be between 0 and {max} (got {n})",
                        field.label
                    )));
                }
                self.write_u16(field.offset, n);
            }
            Kind::Enum(table) => {
                let resolved = parse_enum(table, value).ok_or_else(|| {
                    let options: Vec<_> = table.iter().map(|(_, label)| *label).collect();
                    Error::Format(format!(
                        "'{value}' is not a valid {}. Options: {}",
                        field.label,
                        options.join(", ")
                    ))
                })?;
                self.write_u16(field.offset, resolved);
            }
        }
        Ok(())
    }

    /// Write this save to `path` atomically. Callers are responsible for backups.
    pub fn write(&self, path: impl AsRef<Path>) -> Result<()> {
        crate::save::atomic_write(path, &self.bytes)
    }

    fn write_u16(&mut self, offset: usize, value: u16) {
        self.bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }

    fn set_name(&mut self, name: &str) -> Result<()> {
        if !name.is_ascii() {
            return Err(Error::Format("name must be ASCII".into()));
        }
        let max = NAME_LEN - 1; // reserve one byte for the null terminator
        if name.len() > max {
            return Err(Error::Format(format!(
                "name must be at most {max} characters (got {})",
                name.len()
            )));
        }
        // Clear the whole field, then write the name; the remainder stays null-padded.
        self.bytes[0..NAME_LEN].fill(0);
        self.bytes[0..name.len()].copy_from_slice(name.as_bytes());
        Ok(())
    }

    fn read_u16(&self, offset: usize) -> u16 {
        u16::from_le_bytes([self.bytes[offset], self.bytes[offset + 1]])
    }

    fn format_field(&self, field: &FieldDef) -> String {
        match field.kind {
            Kind::Name => self.name(),
            Kind::U16 { .. } => self.read_u16(field.offset).to_string(),
            Kind::Enum(table) => {
                let value = self.read_u16(field.offset);
                match table.iter().find(|(v, _)| *v == value) {
                    Some((_, label)) => (*label).to_string(),
                    None => format!("Unknown ({value})"),
                }
            }
        }
    }
}

/// Resolve an enum input (a numeric value or a case-insensitive variant name) to its
/// stored `u16`, or `None` if it matches no variant.
fn parse_enum(table: EnumTable, value: &str) -> Option<u16> {
    let value = value.trim();
    if let Ok(n) = value.parse::<u16>() {
        return table.iter().find(|(v, _)| *v == n).map(|(v, _)| *v);
    }
    table
        .iter()
        .find(|(_, label)| label.eq_ignore_ascii_case(value))
        .map(|(v, _)| *v)
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
