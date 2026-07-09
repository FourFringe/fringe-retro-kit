//! Hardcoded Ultima III (Exodus) character support (`ROSTER.ULT` and `PARTY.ULT`).
//!
//! Format reference: <https://wiki.ultimacodex.com/wiki/Ultima_III_internal_formats>
//! (the Codex of Ultima Wisdom). Both files store the same 64-byte character record:
//! `ROSTER.ULT` is an array of 20 of them; `PARTY.ULT` has an 18-byte party header
//! followed by copies of the four active party members' records.
//!
//! Ultima III stores numbers as **BCD** (binary-coded decimal), race/class/sex/status as
//! **ASCII letters**, and marks/cards as a **bitfield**. Edits mutate only known offsets
//! in place, preserving unknown bytes. The shared record helpers below are parameterized
//! by a base offset so the roster and the party reuse them.

use std::path::Path;

use crate::{Error, Result};

/// Size of one character record, in bytes.
pub const RECORD_LEN: usize = 0x40;
/// Number of character slots in a roster.
pub const RECORD_COUNT: usize = 20;
/// Total size of `ROSTER.ULT`, in bytes (20 × 64).
pub const ROSTER_LEN: usize = RECORD_LEN * RECORD_COUNT;

/// Size of the `PARTY.ULT` header preceding the character records.
const PARTY_HEADER_LEN: usize = 0x12;
/// Number of active characters in a party.
pub const PARTY_MEMBER_COUNT: usize = 4;
/// Total size of `PARTY.ULT`, in bytes (header + 4 × 64).
pub const PARTY_LEN: usize = PARTY_HEADER_LEN + PARTY_MEMBER_COUNT * RECORD_LEN;

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

// --- Shared character-record helpers, parameterized by the record's base offset. ---

/// Whether the record at `base` holds a character (its name is non-empty).
fn record_is_occupied(buf: &[u8], base: usize) -> bool {
    buf[base] != 0
}

/// Format a single field of the record at `base`, or `None` for an unknown key.
fn record_get(buf: &[u8], base: usize, key: &str) -> Option<String> {
    let field = FIELDS.iter().find(|f| f.key == key)?;
    Some(format_field(buf, base, field))
}

/// All known fields of the record at `base` as `(label, value)` pairs.
fn record_inspect(buf: &[u8], base: usize) -> Vec<(&'static str, String)> {
    FIELDS
        .iter()
        .map(|f| (f.label, format_field(buf, base, f)))
        .collect()
}

/// A one-line summary of the record at `base`.
fn record_summary(buf: &[u8], base: usize) -> String {
    let g = |key| record_get(buf, base, key).unwrap_or_default();
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

/// Set a field of the record at `base`, validating the value first.
fn record_set(buf: &mut [u8], base: usize, key: &str, value: &str) -> Result<()> {
    let field = FIELDS
        .iter()
        .find(|f| f.key == key)
        .ok_or_else(|| Error::Format(format!("unknown field '{key}'")))?;
    let at = base + field.offset;
    match field.kind {
        Kind::Name { len } => set_name(buf, base, len, value)?,
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
            write_bcd(buf, at, bytes, n);
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
            buf[at] = letter;
        }
        Kind::Byte => {
            let n: u8 = value.parse().map_err(|_| {
                Error::Format(format!("{} must be 0..=255 (got '{value}')", field.label))
            })?;
            buf[at] = n;
        }
        Kind::Bool => {
            buf[at] = if parse_bool(value)? { 0xFF } else { 0x00 };
        }
        Kind::Bitfield(_) => {
            let n: u8 = value.parse().map_err(|_| {
                Error::Format(format!(
                    "{} takes a raw 0..=255 value for now (got '{value}')",
                    field.label
                ))
            })?;
            buf[at] = n;
        }
    }
    Ok(())
}

fn set_name(buf: &mut [u8], base: usize, len: usize, name: &str) -> Result<()> {
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
    buf[base..base + len].fill(0);
    buf[base..base + name.len()].copy_from_slice(name.as_bytes());
    Ok(())
}

fn format_field(buf: &[u8], base: usize, field: &FieldDef) -> String {
    let at = base + field.offset;
    match field.kind {
        Kind::Name { len } => {
            let raw = &buf[at..at + len];
            let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
            String::from_utf8_lossy(&raw[..end]).into_owned()
        }
        Kind::Bcd { bytes } => read_bcd(buf, at, bytes).to_string(),
        Kind::Letter(table) => {
            let b = buf[at];
            match table.iter().find(|(letter, _)| *letter == b) {
                Some((_, name)) => (*name).to_string(),
                None if b.is_ascii_graphic() => format!("Unknown ('{}')", b as char),
                None => format!("Unknown (0x{b:02X})"),
            }
        }
        Kind::Byte => buf[at].to_string(),
        Kind::Bool => match buf[at] {
            0x00 => "no".to_string(),
            0xFF => "yes".to_string(),
            other => format!("0x{other:02X}"),
        },
        Kind::Bitfield(flags) => {
            let b = buf[at];
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

/// The keys of all known character-record fields (for help and error messages).
fn record_field_keys() -> impl Iterator<Item = &'static str> {
    FIELDS.iter().map(|f| f.key)
}

/// A parsed Ultima III roster: 20 fixed 64-byte character records.
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
        Self::from_bytes(std::fs::read(path)?)
    }

    /// The complete raw byte buffer.
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Whether a slot holds a character.
    pub fn is_occupied(&self, index: usize) -> bool {
        index < RECORD_COUNT && record_is_occupied(&self.bytes, index * RECORD_LEN)
    }

    /// The 0-based indices of occupied slots.
    pub fn occupied_slots(&self) -> Vec<usize> {
        (0..RECORD_COUNT).filter(|&i| self.is_occupied(i)).collect()
    }

    /// A one-line summary of the character in a slot.
    pub fn summary(&self, index: usize) -> String {
        record_summary(&self.bytes, index * RECORD_LEN)
    }

    /// Format a single field of a character, or `None` for an unknown slot/key.
    pub fn get_field(&self, index: usize, key: &str) -> Option<String> {
        if index >= RECORD_COUNT {
            return None;
        }
        record_get(&self.bytes, index * RECORD_LEN, key)
    }

    /// All known fields of a character as `(label, value)` pairs.
    pub fn inspect(&self, index: usize) -> Vec<(&'static str, String)> {
        record_inspect(&self.bytes, index * RECORD_LEN)
    }

    /// The keys of all known character fields.
    pub fn field_keys() -> impl Iterator<Item = &'static str> {
        record_field_keys()
    }

    /// Set a field of a character by key, validating the value first.
    pub fn set_field(&mut self, index: usize, key: &str, value: &str) -> Result<()> {
        if index >= RECORD_COUNT {
            return Err(Error::Format(format!(
                "slot must be 1..={RECORD_COUNT} (got {})",
                index + 1
            )));
        }
        record_set(&mut self.bytes, index * RECORD_LEN, key, value)
    }

    /// Write this roster to `path` atomically. Callers are responsible for backups.
    pub fn write(&self, path: impl AsRef<Path>) -> Result<()> {
        crate::save::atomic_write(path, &self.bytes)
    }
}

/// A parsed Ultima III party (`PARTY.ULT`): an 18-byte header plus copies of the four
/// active party members' records (same format as roster records).
#[derive(Clone)]
pub struct Ultima3Party {
    bytes: Vec<u8>,
}

impl Ultima3Party {
    /// Wrap an in-memory byte buffer, validating its length.
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self> {
        if bytes.len() != PARTY_LEN {
            return Err(Error::Format(format!(
                "expected {PARTY_LEN} bytes, got {}",
                bytes.len()
            )));
        }
        Ok(Self { bytes })
    }

    /// Read and parse a party file from disk.
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        Self::from_bytes(std::fs::read(path)?)
    }

    /// The complete raw byte buffer.
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// The number of active characters in the party (header byte `0x07`).
    pub fn party_size(&self) -> usize {
        self.bytes[0x07] as usize
    }

    /// The roster slot number (1-based) of each of the four party positions.
    pub fn party_order(&self) -> [u8; PARTY_MEMBER_COUNT] {
        let mut order = [0u8; PARTY_MEMBER_COUNT];
        order.copy_from_slice(&self.bytes[0x0A..0x0A + PARTY_MEMBER_COUNT]);
        order
    }

    /// Party-header fields (transport, moves, size, position, order) as `(label, value)`.
    pub fn header_inspect(&self) -> Vec<(&'static str, String)> {
        let transport = match self.bytes[0x00] {
            0x3F => "On Foot".to_string(),
            0x0A => "Horse".to_string(),
            0x0B => "Ship".to_string(),
            other => format!("Unknown (0x{other:02X})"),
        };
        let order = self
            .party_order()
            .iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        vec![
            ("Transport", transport),
            ("Moves", read_bcd(&self.bytes, 0x03, 4).to_string()),
            ("Party Size", self.bytes[0x07].to_string()),
            (
                "Position",
                format!("({}, {})", self.bytes[0x08], self.bytes[0x09]),
            ),
            ("Order (roster slots)", order),
        ]
    }

    /// A one-line summary of a party member (0-based position).
    pub fn summary(&self, member: usize) -> String {
        record_summary(&self.bytes, self.member_base(member))
    }

    /// Format a single field of a party member, or `None` for an unknown member/key.
    pub fn get_field(&self, member: usize, key: &str) -> Option<String> {
        if member >= PARTY_MEMBER_COUNT {
            return None;
        }
        record_get(&self.bytes, self.member_base(member), key)
    }

    /// All known fields of a party member as `(label, value)` pairs.
    pub fn inspect(&self, member: usize) -> Vec<(&'static str, String)> {
        record_inspect(&self.bytes, self.member_base(member))
    }

    /// The keys of all known character fields.
    pub fn field_keys() -> impl Iterator<Item = &'static str> {
        record_field_keys()
    }

    /// Set a field of a party member by key, validating the value first.
    pub fn set_field(&mut self, member: usize, key: &str, value: &str) -> Result<()> {
        if member >= PARTY_MEMBER_COUNT {
            return Err(Error::Format(format!(
                "party slot must be 1..={PARTY_MEMBER_COUNT} (got {})",
                member + 1
            )));
        }
        let base = self.member_base(member);
        record_set(&mut self.bytes, base, key, value)
    }

    /// Write this party to `path` atomically. Callers are responsible for backups.
    pub fn write(&self, path: impl AsRef<Path>) -> Result<()> {
        crate::save::atomic_write(path, &self.bytes)
    }

    fn member_base(&self, member: usize) -> usize {
        PARTY_HEADER_LEN + member * RECORD_LEN
    }
}

/// Read a little-endian BCD number of `n` bytes (each byte holds two decimal digits).
fn read_bcd(buf: &[u8], offset: usize, n: usize) -> u32 {
    let mut value = 0u32;
    let mut place = 1u32;
    for i in 0..n {
        let b = buf[offset + i];
        let digits = (b >> 4) as u32 * 10 + (b & 0x0F) as u32;
        value += digits * place;
        place *= 100;
    }
    value
}

/// Write a decimal `value` as a little-endian BCD number of `n` bytes.
fn write_bcd(buf: &mut [u8], offset: usize, n: usize, mut value: u32) {
    for i in 0..n {
        let pair = (value % 100) as u8;
        buf[offset + i] = ((pair / 10) << 4) | (pair % 10);
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

    /// A roster whose first slot is the real "Enkiii" character (from a live save).
    fn synthetic_roster() -> Vec<u8> {
        let mut buf = vec![0u8; ROSTER_LEN];
        write_enkiii(&mut buf, 0);
        buf
    }

    /// Write the "Enkiii" test character into the record starting at `base`.
    fn write_enkiii(buf: &mut [u8], base: usize) {
        buf[base..base + 6].copy_from_slice(b"Enkiii"); // name
        buf[base + 0x11] = b'G'; // status: Good
        buf[base + 0x12] = 0x14; // strength (BCD 14)
        buf[base + 0x13] = 0x12; // dexterity (BCD 12)
        buf[base + 0x14] = 0x12; // intelligence
        buf[base + 0x15] = 0x12; // wisdom
        buf[base + 0x16] = b'H'; // race: Human
        buf[base + 0x17] = b'F'; // class: Fighter
        buf[base + 0x18] = b'M'; // gender: Male
        buf[base + 0x1A] = 0x50; // hits (BCD, low pair) -> 150
        buf[base + 0x1B] = 0x01;
        buf[base + 0x1C] = 0x50; // max hits -> 150
        buf[base + 0x1D] = 0x01;
        buf[base + 0x23] = 0x50; // gold -> 150
        buf[base + 0x24] = 0x01;
    }

    #[test]
    fn parses_real_character() {
        let roster = Ultima3Roster::from_bytes(synthetic_roster()).unwrap();
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
        let roster = Ultima3Roster::from_bytes(synthetic_roster()).unwrap();
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
        let mut roster = Ultima3Roster::from_bytes(synthetic_roster()).unwrap();
        roster.set_field(0, "gold", "1234").unwrap();
        assert_eq!(roster.get_field(0, "gold").unwrap(), "1234");
        // 1234 -> low pair 0x34, high pair 0x12.
        assert_eq!(&roster.as_bytes()[0x23..0x25], &[0x34, 0x12]);
    }

    #[test]
    fn set_letter_enum_by_name_and_letter() {
        let mut roster = Ultima3Roster::from_bytes(synthetic_roster()).unwrap();
        roster.set_field(0, "class", "Wizard").unwrap();
        assert_eq!(roster.as_bytes()[0x17], b'W');
        roster.set_field(0, "race", "E").unwrap();
        assert_eq!(roster.get_field(0, "race").unwrap(), "Elf");
    }

    #[test]
    fn set_bool_in_party() {
        let mut roster = Ultima3Roster::from_bytes(synthetic_roster()).unwrap();
        roster.set_field(0, "in_party", "yes").unwrap();
        assert_eq!(roster.as_bytes()[0x10], 0xFF);
        roster.set_field(0, "in_party", "no").unwrap();
        assert_eq!(roster.as_bytes()[0x10], 0x00);
    }

    #[test]
    fn set_changes_only_target_bytes() {
        let mut roster = Ultima3Roster::from_bytes(synthetic_roster()).unwrap();
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
        let mut roster = Ultima3Roster::from_bytes(synthetic_roster()).unwrap();
        assert!(roster.set_field(0, "strength", "abc").is_err());
        assert!(roster.set_field(0, "strength", "100").is_err()); // > 99 (1-byte BCD)
        assert!(roster.set_field(0, "race", "Klingon").is_err());
        assert!(roster.set_field(99, "strength", "10").is_err()); // bad slot
    }

    #[test]
    fn roster_write_then_load_is_identical() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ROSTER.ULT");
        let roster = Ultima3Roster::from_bytes(synthetic_roster()).unwrap();
        roster.write(&path).unwrap();
        let reloaded = Ultima3Roster::load(&path).unwrap();
        assert_eq!(reloaded.as_bytes(), roster.as_bytes());
    }

    /// A party with a header and one member ("Enkiii") in position 0.
    fn synthetic_party() -> Vec<u8> {
        let mut buf = vec![0u8; PARTY_LEN];
        buf[0x00] = 0x3F; // transport: on foot
        buf[0x03] = 0x52; // moves (BCD) -> 52
        buf[0x07] = 0x01; // one member
        buf[0x08] = 45; // x
        buf[0x09] = 19; // y
        buf[0x0A] = 2; // party order: roster slots 2, 4, 5, 1
        buf[0x0B] = 4;
        buf[0x0C] = 5;
        buf[0x0D] = 1;
        write_enkiii(&mut buf, PARTY_HEADER_LEN); // member 0
        buf
    }

    #[test]
    fn party_parses_header_and_member() {
        let party = Ultima3Party::from_bytes(synthetic_party()).unwrap();
        assert_eq!(party.party_size(), 1);
        assert_eq!(party.party_order(), [2, 4, 5, 1]);
        assert_eq!(party.get_field(0, "name").unwrap(), "Enkiii");
        assert_eq!(party.get_field(0, "class").unwrap(), "Fighter");
        assert_eq!(party.get_field(0, "hits").unwrap(), "150");
        let header = party.header_inspect();
        assert!(header.iter().any(|(l, v)| *l == "Moves" && v == "52"));
        assert!(header
            .iter()
            .any(|(l, v)| *l == "Transport" && v == "On Foot"));
    }

    #[test]
    fn party_set_member_field() {
        let mut party = Ultima3Party::from_bytes(synthetic_party()).unwrap();
        party.set_field(0, "gold", "999").unwrap();
        assert_eq!(party.get_field(0, "gold").unwrap(), "999");
        // The edit must land inside member 0's record, not the header.
        assert!(party.set_field(9, "gold", "1").is_err()); // bad party slot
    }

    #[test]
    fn party_rejects_wrong_length() {
        assert!(Ultima3Party::from_bytes(vec![0u8; 100]).is_err());
    }

    #[test]
    fn party_write_then_load_is_identical() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("PARTY.ULT");
        let party = Ultima3Party::from_bytes(synthetic_party()).unwrap();
        party.write(&path).unwrap();
        let reloaded = Ultima3Party::load(&path).unwrap();
        assert_eq!(reloaded.as_bytes(), party.as_bytes());
    }
}
