//! Hardcoded Ultima III (Exodus) roster support (`ROSTER.ULT`).
//!
//! Format reference: <https://wiki.ultimacodex.com/wiki/Ultima_III_internal_formats>
//! (the Codex of Ultima Wisdom). `ROSTER.ULT` is an array of 20 fixed 64-byte character
//! records. Unlike Ultima I, Ultima III stores numbers as **BCD** (binary-coded decimal),
//! and race/class/sex/status as **ASCII letters**, and the marks/cards as a **bitfield**.
//!
//! This module reads and edits individual records in place, preserving unknown bytes.
//! (The active party lives in a separate `PARTY.ULT` file, not handled here yet.)

use std::path::Path;

use crate::{Error, Result};

/// Size of one character record, in bytes.
pub const RECORD_LEN: usize = 0x40;
/// Number of character slots in a roster.
pub const RECORD_COUNT: usize = 20;
/// Total size of `ROSTER.ULT`, in bytes (20 × 64).
pub const ROSTER_LEN: usize = RECORD_LEN * RECORD_COUNT;

/// Length of the name field (bytes `0x00..0x0A`), including the null terminator.
const NAME_LEN: usize = 0x0A;

// --- Letter tables (ASCII byte -> label). Ultima III stores these fields as a single
// ASCII character. ---

type LetterTable = &'static [(u8, &'static str)];

const RACE: LetterTable = &[
    (b'H', "Human"),
    (b'E', "Elf"),
    (b'D', "Dwarf"),
    (b'F', "Fuzzy"),
    (b'B', "Bobbit"),
];
const CLASS: LetterTable = &[
    (b'F', "Fighter"),
    (b'C', "Cleric"),
    (b'W', "Wizard"),
    (b'T', "Thief"),
    (b'P', "Paladin"),
    (b'B', "Barbarian"),
    (b'L', "Lark"),
    (b'I', "Illusionist"),
    (b'A', "Alchemist"),
    (b'D', "Druid"),
    (b'R', "Ranger"),
];
const GENDER: LetterTable = &[(b'M', "Male"), (b'F', "Female"), (b'O', "Other")];
const STATUS: LetterTable = &[
    (b'G', "Good"),
    (b'P', "Poisoned"),
    (b'D', "Dead"),
    (b'A', "Ashes"),
];

/// Flag names for the marks/cards bitfield at offset `0x0E`, bit 0 (LSB) first.
const MARKS_CARDS: &[&str] = &[
    "Love", "Sol", "Moon", "Death", "Force", "Fire", "Snake", "Kings",
];

/// How to interpret a field's bytes within a character record.
#[derive(Clone, Copy)]
enum Kind {
    /// Null-terminated ASCII name of the given length.
    Name { len: usize },
    /// Binary-coded decimal number occupying `bytes` bytes (low pair first).
    Bcd { bytes: usize },
    /// A single ASCII letter mapped to a named variant.
    Letter(LetterTable),
    /// A raw unsigned byte.
    Byte,
    /// A boolean stored as `0x00` (no) / `0xFF` (yes).
    Bool,
    /// A byte of independent flags (bit 0 = first label).
    Bitfield(&'static [&'static str]),
}

/// A known field within a character record: a stable key, a display label, its byte
/// offset (relative to the record start), and how to read it.
struct FieldDef {
    key: &'static str,
    label: &'static str,
    offset: usize,
    kind: Kind,
}

/// Every character-record field we understand (offsets are within a 64-byte record).
#[rustfmt::skip]
const FIELDS: &[FieldDef] = &[
    FieldDef { key: "name",         label: "Name",         offset: 0x00, kind: Kind::Name { len: NAME_LEN } },
    FieldDef { key: "marks_cards",  label: "Marks/Cards",  offset: 0x0E, kind: Kind::Bitfield(MARKS_CARDS) },
    FieldDef { key: "torches",      label: "Torches",      offset: 0x0F, kind: Kind::Bcd { bytes: 1 } },
    FieldDef { key: "in_party",     label: "In Party",     offset: 0x10, kind: Kind::Bool },
    FieldDef { key: "status",       label: "Status",       offset: 0x11, kind: Kind::Letter(STATUS) },
    FieldDef { key: "strength",     label: "Strength",     offset: 0x12, kind: Kind::Bcd { bytes: 1 } },
    FieldDef { key: "dexterity",    label: "Dexterity",    offset: 0x13, kind: Kind::Bcd { bytes: 1 } },
    FieldDef { key: "intelligence", label: "Intelligence", offset: 0x14, kind: Kind::Bcd { bytes: 1 } },
    FieldDef { key: "wisdom",       label: "Wisdom",       offset: 0x15, kind: Kind::Bcd { bytes: 1 } },
    FieldDef { key: "race",         label: "Race",         offset: 0x16, kind: Kind::Letter(RACE) },
    FieldDef { key: "class",        label: "Class",        offset: 0x17, kind: Kind::Letter(CLASS) },
    FieldDef { key: "gender",       label: "Gender",       offset: 0x18, kind: Kind::Letter(GENDER) },
    FieldDef { key: "magic",        label: "Magic Points", offset: 0x19, kind: Kind::Bcd { bytes: 1 } },
    FieldDef { key: "hits",         label: "Hit Points",   offset: 0x1A, kind: Kind::Bcd { bytes: 2 } },
    FieldDef { key: "max_hits",     label: "Max Hits",     offset: 0x1C, kind: Kind::Bcd { bytes: 2 } },
    FieldDef { key: "experience",   label: "Experience",   offset: 0x1E, kind: Kind::Bcd { bytes: 2 } },
    FieldDef { key: "food_frac",    label: "Food (frac)",  offset: 0x20, kind: Kind::Bcd { bytes: 1 } },
    FieldDef { key: "food",         label: "Food",         offset: 0x21, kind: Kind::Bcd { bytes: 2 } },
    FieldDef { key: "gold",         label: "Gold",         offset: 0x23, kind: Kind::Bcd { bytes: 2 } },
    FieldDef { key: "gems",         label: "Gems",         offset: 0x25, kind: Kind::Bcd { bytes: 1 } },
    FieldDef { key: "keys",         label: "Keys",         offset: 0x26, kind: Kind::Bcd { bytes: 1 } },
    FieldDef { key: "powders",      label: "Powders",      offset: 0x27, kind: Kind::Bcd { bytes: 1 } },
    FieldDef { key: "worn_armor",   label: "Worn Armor",   offset: 0x28, kind: Kind::Byte },
    FieldDef { key: "weapon",       label: "Ready Weapon", offset: 0x30, kind: Kind::Byte },
];

/// A parsed Ultima III roster: 20 fixed 64-byte character records.
///
/// Holds the complete raw byte buffer. Reads and edits operate on known offsets within a
/// record, so bytes we don't understand are always preserved.
#[derive(Clone)]
pub struct Ultima3Roster {
    bytes: Vec<u8>,
}

impl Ultima3Roster {
    /// Wrap an in-memory byte buffer, validating its length.
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self> {
        if bytes.len() != ROSTER_LEN {
            return Err(Error::Format(format!(
                "expected {ROSTER_LEN} bytes, got {}",
                bytes.len()
            )));
        }
        Ok(Self { bytes })
    }

    /// Read and parse a roster file from disk.
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let bytes = std::fs::read(path)?;
        Self::from_bytes(bytes)
    }

    /// The complete raw byte buffer.
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Whether a slot holds a character (its name is non-empty).
    pub fn is_occupied(&self, index: usize) -> bool {
        index < RECORD_COUNT && self.bytes[index * RECORD_LEN] != 0
    }

    /// The 0-based indices of occupied slots.
    pub fn occupied_slots(&self) -> Vec<usize> {
        (0..RECORD_COUNT).filter(|&i| self.is_occupied(i)).collect()
    }

    /// A one-line summary of the character in a slot.
    pub fn summary(&self, index: usize) -> String {
        let g = |key| self.get_field(index, key).unwrap_or_default();
        format!(
            "{} — {} {}, {} · HP {}/{} · {}",
            g("name"),
            g("race"),
            g("class"),
            g("gender"),
            g("hits"),
            g("max_hits"),
            g("status"),
        )
    }

    /// Format a single field of a character, or `None` for an unknown slot/key.
    pub fn get_field(&self, index: usize, key: &str) -> Option<String> {
        if index >= RECORD_COUNT {
            return None;
        }
        let field = FIELDS.iter().find(|f| f.key == key)?;
        Some(self.format_field(index, field))
    }

    /// All known fields of a character as `(label, value)` pairs.
    pub fn inspect(&self, index: usize) -> Vec<(&'static str, String)> {
        FIELDS
            .iter()
            .map(|f| (f.label, self.format_field(index, f)))
            .collect()
    }

    /// The keys of all known fields (for help and error messages).
    pub fn field_keys() -> impl Iterator<Item = &'static str> {
        FIELDS.iter().map(|f| f.key)
    }

    /// Set a field of a character by key, validating the value first. Only the field's
    /// own bytes are modified.
    pub fn set_field(&mut self, index: usize, key: &str, value: &str) -> Result<()> {
        if index >= RECORD_COUNT {
            return Err(Error::Format(format!(
                "slot must be 1..={RECORD_COUNT} (got {})",
                index + 1
            )));
        }
        let field = FIELDS
            .iter()
            .find(|f| f.key == key)
            .ok_or_else(|| Error::Format(format!("unknown field '{key}'")))?;
        let at = index * RECORD_LEN + field.offset;
        match field.kind {
            Kind::Name { len } => self.set_name(index * RECORD_LEN, len, value)?,
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
                write_bcd(&mut self.bytes, at, bytes, n);
            }
            Kind::Letter(table) => {
                let letter = parse_letter(table, value).ok_or_else(|| {
                    let options: Vec<_> = table.iter().map(|(_, name)| *name).collect();
                    Error::Format(format!(
                        "'{value}' is not a valid {}. Options: {}",
                        field.label,
                        options.join(", ")
                    ))
                })?;
                self.bytes[at] = letter;
            }
            Kind::Byte => {
                let n: u8 = value.parse().map_err(|_| {
                    Error::Format(format!("{} must be 0..=255 (got '{value}')", field.label))
                })?;
                self.bytes[at] = n;
            }
            Kind::Bool => {
                self.bytes[at] = if parse_bool(value)? { 0xFF } else { 0x00 };
            }
            Kind::Bitfield(_) => {
                // Editing individual flags isn't supported yet; accept a raw byte value.
                let n: u8 = value.parse().map_err(|_| {
                    Error::Format(format!(
                        "{} takes a raw 0..=255 value for now (got '{value}')",
                        field.label
                    ))
                })?;
                self.bytes[at] = n;
            }
        }
        Ok(())
    }

    /// Write this roster to `path` atomically. Callers are responsible for backups.
    pub fn write(&self, path: impl AsRef<Path>) -> Result<()> {
        crate::save::atomic_write(path, &self.bytes)
    }

    fn set_name(&mut self, record_base: usize, len: usize, name: &str) -> Result<()> {
        if !name.is_ascii() {
            return Err(Error::Format("name must be ASCII".into()));
        }
        let max = len - 1; // reserve one byte for the null terminator
        if name.len() > max {
            return Err(Error::Format(format!(
                "name must be at most {max} characters (got {})",
                name.len()
            )));
        }
        self.bytes[record_base..record_base + len].fill(0);
        self.bytes[record_base..record_base + name.len()].copy_from_slice(name.as_bytes());
        Ok(())
    }

    fn format_field(&self, index: usize, field: &FieldDef) -> String {
        let at = index * RECORD_LEN + field.offset;
        match field.kind {
            Kind::Name { len } => {
                let raw = &self.bytes[at..at + len];
                let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
                String::from_utf8_lossy(&raw[..end]).into_owned()
            }
            Kind::Bcd { bytes } => read_bcd(&self.bytes, at, bytes).to_string(),
            Kind::Letter(table) => {
                let b = self.bytes[at];
                match table.iter().find(|(letter, _)| *letter == b) {
                    Some((_, name)) => (*name).to_string(),
                    None if b.is_ascii_graphic() => format!("Unknown ('{}')", b as char),
                    None => format!("Unknown (0x{b:02X})"),
                }
            }
            Kind::Byte => self.bytes[at].to_string(),
            Kind::Bool => match self.bytes[at] {
                0x00 => "no".to_string(),
                0xFF => "yes".to_string(),
                other => format!("0x{other:02X}"),
            },
            Kind::Bitfield(flags) => {
                let b = self.bytes[at];
                let set: Vec<_> = flags
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| b & (1 << i) != 0)
                    .map(|(_, name)| *name)
                    .collect();
                if set.is_empty() {
                    "(none)".to_string()
                } else {
                    set.join(", ")
                }
            }
        }
    }
}

/// Read a little-endian BCD number of `n` bytes (each byte holds two decimal digits).
fn read_bcd(bytes: &[u8], offset: usize, n: usize) -> u32 {
    let mut value = 0u32;
    let mut place = 1u32;
    for i in 0..n {
        let b = bytes[offset + i];
        let digits = (b >> 4) as u32 * 10 + (b & 0x0F) as u32;
        value += digits * place;
        place *= 100;
    }
    value
}

/// Write a decimal `value` as a little-endian BCD number of `n` bytes.
fn write_bcd(bytes: &mut [u8], offset: usize, n: usize, mut value: u32) {
    for i in 0..n {
        let pair = (value % 100) as u8;
        bytes[offset + i] = ((pair / 10) << 4) | (pair % 10);
        value /= 100;
    }
}

/// Resolve a letter-enum input (a full name or a single letter, case-insensitive) to its
/// stored ASCII byte.
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

/// Parse a boolean from common spellings.
fn parse_bool(value: &str) -> Result<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "yes" | "y" | "true" | "1" | "on" => Ok(true),
        "no" | "n" | "false" | "0" | "off" => Ok(false),
        other => Err(Error::Format(format!("expected yes/no (got '{other}')"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a roster whose first slot is the real "Enkiii" character (from a live save).
    fn synthetic() -> Vec<u8> {
        let mut buf = vec![0u8; ROSTER_LEN];
        let r = &mut buf[0..RECORD_LEN];
        r[0..6].copy_from_slice(b"Enkiii"); // name
        r[0x11] = b'G'; // status: Good
        r[0x12] = 0x14; // strength (BCD 14)
        r[0x13] = 0x12; // dexterity (BCD 12)
        r[0x14] = 0x12; // intelligence
        r[0x15] = 0x12; // wisdom
        r[0x16] = b'H'; // race: Human
        r[0x17] = b'F'; // class: Fighter
        r[0x18] = b'M'; // gender: Male
        r[0x1A] = 0x50; // hits (BCD, low pair) -> 150
        r[0x1B] = 0x01;
        r[0x1C] = 0x50; // max hits -> 150
        r[0x1D] = 0x01;
        r[0x21] = 0x50; // food -> 150
        r[0x22] = 0x01;
        r[0x23] = 0x50; // gold -> 150
        r[0x24] = 0x01;
        buf
    }

    #[test]
    fn parses_real_character() {
        let roster = Ultima3Roster::from_bytes(synthetic()).unwrap();
        assert_eq!(roster.get_field(0, "name").unwrap(), "Enkiii");
        assert_eq!(roster.get_field(0, "race").unwrap(), "Human");
        assert_eq!(roster.get_field(0, "class").unwrap(), "Fighter");
        assert_eq!(roster.get_field(0, "gender").unwrap(), "Male");
        assert_eq!(roster.get_field(0, "status").unwrap(), "Good");
        assert_eq!(roster.get_field(0, "strength").unwrap(), "14");
        assert_eq!(roster.get_field(0, "dexterity").unwrap(), "12");
        assert_eq!(roster.get_field(0, "hits").unwrap(), "150");
        assert_eq!(roster.get_field(0, "max_hits").unwrap(), "150");
        assert_eq!(roster.get_field(0, "gold").unwrap(), "150");
        assert_eq!(roster.get_field(0, "in_party").unwrap(), "no");
    }

    #[test]
    fn occupancy() {
        let roster = Ultima3Roster::from_bytes(synthetic()).unwrap();
        assert_eq!(roster.occupied_slots(), vec![0]);
        assert!(roster.is_occupied(0));
        assert!(!roster.is_occupied(1));
    }

    #[test]
    fn rejects_wrong_length() {
        assert!(Ultima3Roster::from_bytes(vec![0u8; 100]).is_err());
    }

    #[test]
    fn bcd_round_trips_through_set() {
        let mut roster = Ultima3Roster::from_bytes(synthetic()).unwrap();
        roster.set_field(0, "gold", "1234").unwrap();
        assert_eq!(roster.get_field(0, "gold").unwrap(), "1234");
        // 1234 -> low pair 0x34, high pair 0x12.
        assert_eq!(&roster.as_bytes()[0x23..0x25], &[0x34, 0x12]);
    }

    #[test]
    fn set_letter_enum_by_name_and_letter() {
        let mut roster = Ultima3Roster::from_bytes(synthetic()).unwrap();
        roster.set_field(0, "class", "Wizard").unwrap();
        assert_eq!(roster.as_bytes()[0x17], b'W');
        roster.set_field(0, "race", "E").unwrap();
        assert_eq!(roster.get_field(0, "race").unwrap(), "Elf");
    }

    #[test]
    fn set_bool_in_party() {
        let mut roster = Ultima3Roster::from_bytes(synthetic()).unwrap();
        roster.set_field(0, "in_party", "yes").unwrap();
        assert_eq!(roster.as_bytes()[0x10], 0xFF);
        roster.set_field(0, "in_party", "no").unwrap();
        assert_eq!(roster.as_bytes()[0x10], 0x00);
    }

    #[test]
    fn set_changes_only_target_bytes() {
        let mut roster = Ultima3Roster::from_bytes(synthetic()).unwrap();
        let before = roster.as_bytes().to_vec();
        roster.set_field(0, "strength", "50").unwrap();
        for (i, (a, b)) in before.iter().zip(roster.as_bytes()).enumerate() {
            if i == 0x12 {
                continue; // strength byte
            }
            assert_eq!(a, b, "byte {i:#04x} changed unexpectedly");
        }
        assert_eq!(roster.get_field(0, "strength").unwrap(), "50");
    }

    #[test]
    fn set_rejects_invalid() {
        let mut roster = Ultima3Roster::from_bytes(synthetic()).unwrap();
        assert!(roster.set_field(0, "strength", "abc").is_err());
        assert!(roster.set_field(0, "strength", "100").is_err()); // > 99 (1-byte BCD)
        assert!(roster.set_field(0, "race", "Klingon").is_err());
        assert!(roster.set_field(99, "strength", "10").is_err()); // bad slot
    }

    #[test]
    fn write_then_load_is_identical() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ROSTER.ULT");
        let roster = Ultima3Roster::from_bytes(synthetic()).unwrap();
        roster.write(&path).unwrap();
        let reloaded = Ultima3Roster::load(&path).unwrap();
        assert_eq!(reloaded.as_bytes(), roster.as_bytes());
    }
}
