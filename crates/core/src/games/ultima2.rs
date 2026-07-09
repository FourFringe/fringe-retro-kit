//! Hardcoded Ultima II (The Revenge of the Enchantress) save support (`PLAYER`).
//!
//! **Work in progress / partially reverse-engineered.** Ultima II has no published format
//! spec, so these offsets were mapped by diffing a live save (see the project notes). Only
//! fields confirmed against a known in-game snapshot are exposed here.
//!
//! Numbers are **BCD** (binary-coded decimal), stored **big-endian** (the most significant
//! digit-pair is at the lower offset) — e.g. food `02 89` decodes to 289. This differs
//! from Ultima III, which uses little-endian BCD.

use std::path::Path;

use crate::{Error, Result};

/// Total size of an Ultima II `PLAYER` save file, in bytes (`0x180`).
pub const SAVE_LEN: usize = 0x180;

/// Length of the name field (bytes `0x00..0x10`), including the null terminator.
const NAME_LEN: usize = 0x10;

type LetterTable = &'static [(u8, &'static str)];
type EnumTable = &'static [(u8, &'static str)];

const SEX: LetterTable = &[(b'M', "Male"), (b'F', "Female")];

// Class and race are stored as small numeric indices (not ASCII letters as in Ultima III).
// Order confirmed against the game manual and the ENKII/FRINGE snapshots.
const CLASS: EnumTable = &[(0, "Fighter"), (1, "Cleric"), (2, "Wizard"), (3, "Thief")];
const RACE: EnumTable = &[(0, "Human"), (1, "Elf"), (2, "Dwarf"), (3, "Hobbit")];

// Readied weapon / worn armour are stored as a single index into the item order (0 = none).
// Index order is the same as the owned-count arrays, confirmed by purchase diffs.
const WEAPON: EnumTable = &[
    (0, "None"),
    (1, "Dagger"),
    (2, "Mace"),
    (3, "Axe"),
    (4, "Bow"),
    (5, "Sword"),
    (6, "Great sword"),
    (7, "Light sword"),
    (8, "Phaser"),
    (9, "Quicksword"),
];
const ARMOR: EnumTable = &[
    (0, "None"),
    (1, "Cloth"),
    (2, "Leather"),
    (3, "Chain"),
    (4, "Plate"),
    (5, "Reflect"),
    (6, "Power"),
];

/// How to interpret a field's bytes.
#[derive(Clone, Copy)]
enum Kind {
    /// Null-terminated ASCII name of the given length.
    Name { len: usize },
    /// A single ASCII letter mapped to a named variant.
    Letter(LetterTable),
    /// A single byte holding a numeric index mapped to a named variant.
    Enum(EnumTable),
    /// A raw single byte interpreted as a plain (binary) 0..=255 number.
    Byte,
    /// A big-endian binary-coded decimal number occupying `bytes` bytes.
    Bcd { bytes: usize },
}

/// A known field: a stable key, a display label, its byte offset, and how to read it.
struct FieldDef {
    key: &'static str,
    label: &'static str,
    offset: usize,
    kind: Kind,
    /// Whether this field is still tentative (mapped but not yet confirmed in-game).
    tentative: bool,
}

/// Fields mapped so far. Confirmed against a snapshot of HP 14, Food 289, XP 0, Gold 400.
#[rustfmt::skip]
const FIELDS: &[FieldDef] = &[
    FieldDef { key: "name", label: "Name", offset: 0x00, kind: Kind::Name { len: NAME_LEN }, tentative: false },
    FieldDef { key: "sex",   label: "Sex",   offset: 0x10, kind: Kind::Letter(SEX),  tentative: false },
    FieldDef { key: "class", label: "Class", offset: 0x11, kind: Kind::Enum(CLASS),  tentative: false },
    FieldDef { key: "race",  label: "Race",  offset: 0x12, kind: Kind::Enum(RACE),   tentative: false },
    // Attributes: six 1-byte BCD values at 0x15..0x1A, stored *adjusted* (after race/class/gender
    // bonuses). Order and encoding confirmed with the FRINGE character (all-distinct values).
    FieldDef { key: "strength",     label: "Strength",     offset: 0x15, kind: Kind::Bcd { bytes: 1 }, tentative: false },
    FieldDef { key: "agility",      label: "Agility",      offset: 0x16, kind: Kind::Bcd { bytes: 1 }, tentative: false },
    FieldDef { key: "stamina",      label: "Stamina",      offset: 0x17, kind: Kind::Bcd { bytes: 1 }, tentative: false },
    FieldDef { key: "charisma",     label: "Charisma",     offset: 0x18, kind: Kind::Bcd { bytes: 1 }, tentative: false },
    FieldDef { key: "wisdom",       label: "Wisdom",       offset: 0x19, kind: Kind::Bcd { bytes: 1 }, tentative: false },
    FieldDef { key: "intelligence", label: "Intelligence", offset: 0x1A, kind: Kind::Bcd { bytes: 1 }, tentative: false },
    FieldDef { key: "hits", label: "Hits", offset: 0x1B, kind: Kind::Bcd { bytes: 2 },       tentative: false },
    FieldDef { key: "food", label: "Food", offset: 0x1D, kind: Kind::Bcd { bytes: 2 },       tentative: false },
    FieldDef { key: "experience", label: "Experience", offset: 0x20, kind: Kind::Bcd { bytes: 2 }, tentative: false },
    FieldDef { key: "gold", label: "Gold", offset: 0x22, kind: Kind::Bcd { bytes: 2 },       tentative: false },
    // Map position (raw binary bytes), confirmed by moving left/right (X) and up/down (Y).
    FieldDef { key: "x", label: "Map X", offset: 0x24, kind: Kind::Byte, tentative: false },
    FieldDef { key: "y", label: "Map Y", offset: 0x25, kind: Kind::Byte, tentative: false },
    // Readied weapon (0x2B) and worn armour (0x2C), single index into the item order (0 = none).
    // Confirmed by Wield/Wear diffs (Dagger..Bow -> 1..4; Cloth/Leather -> 1/2).
    FieldDef { key: "weapon", label: "Weapon (readied)", offset: 0x2B, kind: Kind::Enum(WEAPON), tentative: false },
    FieldDef { key: "armor",  label: "Armour (worn)",    offset: 0x2C, kind: Kind::Enum(ARMOR),  tentative: false },
    // Weapons owned (counts), 0x41..0x49 in weapon order (Dagger..Quicksword); array base 0x40 is
    // Hands. Dagger..Sword confirmed by purchase diffs (5/4/3/2/1). Encoding assumed 1-byte BCD to
    // match the rest of U2 — verify with a count >= 10 (10 would read as 0x10).
    FieldDef { key: "weapon_dagger",     label: "Daggers",      offset: 0x41, kind: Kind::Bcd { bytes: 1 }, tentative: true },
    FieldDef { key: "weapon_mace",       label: "Maces",        offset: 0x42, kind: Kind::Bcd { bytes: 1 }, tentative: true },
    FieldDef { key: "weapon_axe",        label: "Axes",         offset: 0x43, kind: Kind::Bcd { bytes: 1 }, tentative: true },
    FieldDef { key: "weapon_bow",        label: "Bows",         offset: 0x44, kind: Kind::Bcd { bytes: 1 }, tentative: true },
    FieldDef { key: "weapon_sword",      label: "Swords",       offset: 0x45, kind: Kind::Bcd { bytes: 1 }, tentative: true },
    FieldDef { key: "weapon_greatsword", label: "Great swords", offset: 0x46, kind: Kind::Bcd { bytes: 1 }, tentative: true },
    FieldDef { key: "weapon_lightsword", label: "Light swords", offset: 0x47, kind: Kind::Bcd { bytes: 1 }, tentative: true },
    FieldDef { key: "weapon_phaser",     label: "Phasers",      offset: 0x48, kind: Kind::Bcd { bytes: 1 }, tentative: true },
    FieldDef { key: "weapon_quicksword", label: "Quickswords",  offset: 0x49, kind: Kind::Bcd { bytes: 1 }, tentative: true },
    // Armour owned (counts), 0x61..0x66 in armour order (Cloth..Power); array base 0x60 is Skin.
    // Cloth..Plate confirmed by purchase diffs. Same BCD-vs-binary caveat as weapons.
    FieldDef { key: "armor_cloth",   label: "Cloth armour",   offset: 0x61, kind: Kind::Bcd { bytes: 1 }, tentative: true },
    FieldDef { key: "armor_leather", label: "Leather armour", offset: 0x62, kind: Kind::Bcd { bytes: 1 }, tentative: true },
    FieldDef { key: "armor_chain",   label: "Chain armour",   offset: 0x63, kind: Kind::Bcd { bytes: 1 }, tentative: true },
    FieldDef { key: "armor_plate",   label: "Plate armour",   offset: 0x64, kind: Kind::Bcd { bytes: 1 }, tentative: true },
    FieldDef { key: "armor_reflect", label: "Reflect armour", offset: 0x65, kind: Kind::Bcd { bytes: 1 }, tentative: true },
    FieldDef { key: "armor_power",   label: "Power armour",   offset: 0x66, kind: Kind::Bcd { bytes: 1 }, tentative: true },
];

/// A parsed Ultima II `PLAYER` save file.
#[derive(Clone)]
pub struct Ultima2Save {
    bytes: Vec<u8>,
}

impl Ultima2Save {
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

    /// Format a single field by key, or `None` for an unknown key.
    pub fn get_field(&self, key: &str) -> Option<String> {
        let field = FIELDS.iter().find(|f| f.key == key)?;
        Some(self.format_field(field))
    }

    /// All known fields as `(label, value, tentative)` triples.
    pub fn inspect(&self) -> Vec<(&'static str, String, bool)> {
        FIELDS
            .iter()
            .map(|f| (f.label, self.format_field(f), f.tentative))
            .collect()
    }

    /// The keys of all known fields.
    pub fn field_keys() -> impl Iterator<Item = &'static str> {
        FIELDS.iter().map(|f| f.key)
    }

    /// Set a field of the character by key, validating the value first.
    pub fn set_field(&mut self, key: &str, value: &str) -> Result<()> {
        let field = FIELDS
            .iter()
            .find(|f| f.key == key)
            .ok_or_else(|| Error::Format(format!("unknown field '{key}'")))?;
        match field.kind {
            Kind::Name { len } => self.set_name(field.offset, len, value)?,
            Kind::Letter(table) => {
                let letter = parse_letter(table, value).ok_or_else(|| {
                    let options: Vec<_> = table.iter().map(|(_, name)| *name).collect();
                    Error::Format(format!(
                        "'{value}' is not a valid {}. Options: {}",
                        field.label,
                        options.join(", ")
                    ))
                })?;
                self.bytes[field.offset] = letter;
            }
            Kind::Enum(table) => {
                let byte = parse_enum(table, value).ok_or_else(|| {
                    let options: Vec<_> = table.iter().map(|(_, name)| *name).collect();
                    Error::Format(format!(
                        "'{value}' is not a valid {}. Options: {}",
                        field.label,
                        options.join(", ")
                    ))
                })?;
                self.bytes[field.offset] = byte;
            }
            Kind::Byte => {
                let n: u8 = value.parse().map_err(|_| {
                    Error::Format(format!(
                        "{} must be a number 0-255 (got '{value}')",
                        field.label
                    ))
                })?;
                self.bytes[field.offset] = n;
            }
            Kind::Bcd { bytes } => {
                let max = 10u32.pow(2 * bytes as u32) - 1;
                let n: u32 = value.parse().map_err(|_| {
                    Error::Format(format!("{} must be a number (got '{value}')", field.label))
                })?;
                if n > max {
                    return Err(Error::Format(format!(
                        "{} must be between 0 and {max} (got {n})",
                        field.label
                    )));
                }
                write_bcd_be(&mut self.bytes, field.offset, bytes, n);
            }
        }
        Ok(())
    }

    /// Write this save to `path` atomically. Callers are responsible for backups.
    pub fn write(&self, path: impl AsRef<Path>) -> Result<()> {
        crate::save::atomic_write(path, &self.bytes)
    }

    fn set_name(&mut self, offset: usize, len: usize, name: &str) -> Result<()> {
        if !name.is_ascii() {
            return Err(Error::Format("name must be ASCII".into()));
        }
        let max = len - 1;
        if name.len() > max {
            return Err(Error::Format(format!(
                "name must be at most {max} characters (got {})",
                name.len()
            )));
        }
        self.bytes[offset..offset + len].fill(0);
        self.bytes[offset..offset + name.len()].copy_from_slice(name.as_bytes());
        Ok(())
    }

    fn format_field(&self, field: &FieldDef) -> String {
        let at = field.offset;
        match field.kind {
            Kind::Name { len } => {
                let raw = &self.bytes[at..at + len];
                let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
                String::from_utf8_lossy(&raw[..end]).into_owned()
            }
            Kind::Letter(table) => {
                let b = self.bytes[at];
                match table.iter().find(|(letter, _)| *letter == b) {
                    Some((_, name)) => (*name).to_string(),
                    None if b.is_ascii_graphic() => format!("Unknown ('{}')", b as char),
                    None => format!("Unknown (0x{b:02X})"),
                }
            }
            Kind::Enum(table) => {
                let b = self.bytes[at];
                match table.iter().find(|(index, _)| *index == b) {
                    Some((_, name)) => (*name).to_string(),
                    None => format!("Unknown ({b})"),
                }
            }
            Kind::Byte => self.bytes[at].to_string(),
            Kind::Bcd { bytes } => read_bcd_be(&self.bytes, at, bytes).to_string(),
        }
    }
}

/// Read a big-endian BCD number of `n` bytes (most significant digit-pair first).
fn read_bcd_be(buf: &[u8], offset: usize, n: usize) -> u32 {
    let mut value = 0u32;
    for i in 0..n {
        let b = buf[offset + i];
        value = value * 100 + (b >> 4) as u32 * 10 + (b & 0x0F) as u32;
    }
    value
}

/// Write a decimal `value` as a big-endian BCD number of `n` bytes.
fn write_bcd_be(buf: &mut [u8], offset: usize, n: usize, mut value: u32) {
    for i in (0..n).rev() {
        let pair = (value % 100) as u8;
        buf[offset + i] = ((pair / 10) << 4) | (pair % 10);
        value /= 100;
    }
}

/// Resolve a letter input (a full name or a single letter, case-insensitive) to its byte.
fn parse_letter(table: LetterTable, value: &str) -> Option<u8> {
    let value = value.trim();
    if let Some((letter, _)) = table
        .iter()
        .find(|(_, name)| name.eq_ignore_ascii_case(value))
    {
        return Some(*letter);
    }
    if value.len() == 1 {
        let c = value.as_bytes()[0].to_ascii_uppercase();
        if table.iter().any(|(letter, _)| *letter == c) {
            return Some(c);
        }
    }
    None
}

/// Resolve an enum input (a variant name, case-insensitive, or a numeric index) to its byte.
fn parse_enum(table: EnumTable, value: &str) -> Option<u8> {
    let value = value.trim();
    if let Some((index, _)) = table
        .iter()
        .find(|(_, name)| name.eq_ignore_ascii_case(value))
    {
        return Some(*index);
    }
    // Accept a raw numeric index, but only if it names a known variant.
    let n: u8 = value.parse().ok()?;
    table.iter().any(|(index, _)| *index == n).then_some(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A synthetic save matching a real snapshot: HP 14, Food 289, XP 0, Gold 400.
    /// A synthetic save matching the confirmed pristine FRINGE snapshot:
    /// Male Elf Wizard, Str 15 / Agi 25 / Sta 12 / Cha 14 / Wis 16 / Int 28, HP/Food/Gold 400.
    fn synthetic() -> Vec<u8> {
        let mut buf = vec![0u8; SAVE_LEN];
        buf[0x00..0x06].copy_from_slice(b"FRINGE"); // name
        buf[0x10] = b'M'; // sex
        buf[0x11] = 0x02; // class = Wizard
        buf[0x12] = 0x01; // race = Elf
        buf[0x15..0x1B].copy_from_slice(&[0x15, 0x25, 0x12, 0x14, 0x16, 0x28]); // Str..Int (BCD)
        buf[0x1B] = 0x04; // hits (BCD BE) -> 0400 = 400
        buf[0x1C] = 0x00;
        buf[0x1D] = 0x04; // food (BCD BE) -> 0400 = 400
        buf[0x1E] = 0x00;
        buf[0x22] = 0x04; // gold (BCD BE) -> 0400 = 400
        buf[0x23] = 0x00;
        buf
    }

    #[test]
    fn decodes_known_snapshot() {
        let save = Ultima2Save::from_bytes(synthetic()).unwrap();
        assert_eq!(save.get_field("name").unwrap(), "FRINGE");
        assert_eq!(save.get_field("sex").unwrap(), "Male");
        assert_eq!(save.get_field("class").unwrap(), "Wizard");
        assert_eq!(save.get_field("race").unwrap(), "Elf");
        assert_eq!(save.get_field("strength").unwrap(), "15");
        assert_eq!(save.get_field("agility").unwrap(), "25");
        assert_eq!(save.get_field("stamina").unwrap(), "12");
        assert_eq!(save.get_field("charisma").unwrap(), "14");
        assert_eq!(save.get_field("wisdom").unwrap(), "16");
        assert_eq!(save.get_field("intelligence").unwrap(), "28");
        assert_eq!(save.get_field("hits").unwrap(), "400");
        assert_eq!(save.get_field("food").unwrap(), "400");
        assert_eq!(save.get_field("experience").unwrap(), "0");
        assert_eq!(save.get_field("gold").unwrap(), "400");
    }

    #[test]
    fn decodes_and_sets_equipped_slots() {
        let mut buf = synthetic();
        buf[0x2B] = 3; // readied weapon = Axe
        buf[0x2C] = 2; // worn armour = Leather
        let mut save = Ultima2Save::from_bytes(buf).unwrap();
        assert_eq!(save.get_field("weapon").unwrap(), "Axe");
        assert_eq!(save.get_field("armor").unwrap(), "Leather");
        save.set_field("weapon", "Bow").unwrap();
        assert_eq!(save.as_bytes()[0x2B], 4);
        save.set_field("armor", "None").unwrap();
        assert_eq!(save.as_bytes()[0x2C], 0);
    }

    #[test]
    fn decodes_inventory_counts() {
        let mut buf = synthetic();
        // Weapons at 0x41..0x45 = 5,4,3,2,1; armour at 0x61..0x64 = 3,2,1,1.
        buf[0x41..0x46].copy_from_slice(&[0x05, 0x04, 0x03, 0x02, 0x01]);
        buf[0x61..0x65].copy_from_slice(&[0x03, 0x02, 0x01, 0x01]);
        let save = Ultima2Save::from_bytes(buf).unwrap();
        assert_eq!(save.get_field("weapon_dagger").unwrap(), "5");
        assert_eq!(save.get_field("weapon_sword").unwrap(), "1");
        assert_eq!(save.get_field("armor_cloth").unwrap(), "3");
        assert_eq!(save.get_field("armor_plate").unwrap(), "1");
        assert_eq!(save.get_field("armor_power").unwrap(), "0");
    }

    #[test]
    fn sets_enum_by_name_and_index() {
        let mut save = Ultima2Save::from_bytes(synthetic()).unwrap();
        save.set_field("class", "Fighter").unwrap();
        assert_eq!(save.get_field("class").unwrap(), "Fighter");
        assert_eq!(save.as_bytes()[0x11], 0);
        save.set_field("race", "2").unwrap(); // numeric index -> Dwarf
        assert_eq!(save.get_field("race").unwrap(), "Dwarf");
        assert_eq!(save.as_bytes()[0x12], 2);
        assert!(save.set_field("class", "Ranger").is_err());
    }

    #[test]
    fn rejects_wrong_length() {
        assert!(Ultima2Save::from_bytes(vec![0u8; 100]).is_err());
    }

    #[test]
    fn bcd_be_round_trips_through_set() {
        let mut save = Ultima2Save::from_bytes(synthetic()).unwrap();
        save.set_field("gold", "1234").unwrap();
        assert_eq!(save.get_field("gold").unwrap(), "1234");
        // Big-endian BCD: high pair 0x12 first, low pair 0x34 second.
        assert_eq!(&save.as_bytes()[0x22..0x24], &[0x12, 0x34]);
    }

    #[test]
    fn set_changes_only_target_bytes() {
        let mut save = Ultima2Save::from_bytes(synthetic()).unwrap();
        let before = save.as_bytes().to_vec();
        save.set_field("food", "500").unwrap();
        for (i, (a, b)) in before.iter().zip(save.as_bytes()).enumerate() {
            if i == 0x1D || i == 0x1E {
                continue; // the two food bytes
            }
            assert_eq!(a, b, "byte {i:#04x} changed unexpectedly");
        }
        assert_eq!(save.get_field("food").unwrap(), "500");
    }
}
