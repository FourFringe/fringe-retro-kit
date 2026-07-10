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

use crate::schema::{self, Endian, Field, FieldKind, Variants};
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

type LetterTable = Variants;

const RACE: LetterTable = &[
    (b'H' as u32, "Human"),
    (b'E' as u32, "Elf"),
    (b'D' as u32, "Dwarf"),
    (b'F' as u32, "Fuzzy"),
    (b'B' as u32, "Bobbit"),
];
const CLASS: LetterTable = &[
    (b'F' as u32, "Fighter"),
    (b'C' as u32, "Cleric"),
    (b'W' as u32, "Wizard"),
    (b'T' as u32, "Thief"),
    (b'P' as u32, "Paladin"),
    (b'B' as u32, "Barbarian"),
    (b'L' as u32, "Lark"),
    (b'I' as u32, "Illusionist"),
    (b'A' as u32, "Alchemist"),
    (b'D' as u32, "Druid"),
    (b'R' as u32, "Ranger"),
];
const GENDER: LetterTable = &[
    (b'M' as u32, "Male"),
    (b'F' as u32, "Female"),
    (b'O' as u32, "Other"),
];
const STATUS: LetterTable = &[
    (b'G' as u32, "Good"),
    (b'P' as u32, "Poisoned"),
    (b'D' as u32, "Dead"),
    (b'A' as u32, "Ashes"),
];

/// Flag names for the marks/cards bitfield at offset `0x0E`, bit 0 (LSB) first.
const MARKS_CARDS: &[&str] = &[
    "Love", "Sol", "Moon", "Death", "Force", "Fire", "Snake", "Kings",
];

/// An Ultima III little-endian BCD field of `bytes` bytes.
const fn bcd(bytes: usize) -> FieldKind {
    FieldKind::Bcd {
        bytes,
        endian: Endian::Little,
    }
}

/// An Ultima III ASCII-letter enum field.
const fn letter(variants: Variants) -> FieldKind {
    FieldKind::Letter { variants }
}

/// Every character-record field we understand (offsets are within a 64-byte record).
#[rustfmt::skip]
const FIELDS: &[Field] = &[
    Field::new("name",         "Name",         0x00, FieldKind::Name { len: NAME_LEN }),
    Field::new("marks_cards",  "Marks/Cards",  0x0E, FieldKind::Bitfield { flags: MARKS_CARDS }),
    Field::new("torches",      "Torches",      0x0F, bcd(1)),
    Field::new("in_party",     "In Party",     0x10, FieldKind::Bool),
    Field::new("status",       "Status",       0x11, letter(STATUS)),
    Field::new("strength",     "Strength",     0x12, bcd(1)),
    Field::new("dexterity",    "Dexterity",    0x13, bcd(1)),
    Field::new("intelligence", "Intelligence", 0x14, bcd(1)),
    Field::new("wisdom",       "Wisdom",       0x15, bcd(1)),
    Field::new("race",         "Race",         0x16, letter(RACE)),
    Field::new("class",        "Class",        0x17, letter(CLASS)),
    Field::new("gender",       "Gender",       0x18, letter(GENDER)),
    Field::new("magic",        "Magic Points", 0x19, bcd(1)),
    Field::new("hits",         "Hit Points",   0x1A, bcd(2)),
    Field::new("max_hits",     "Max Hits",     0x1C, bcd(2)),
    Field::new("experience",   "Experience",   0x1E, bcd(2)),
    Field::new("food_frac",    "Food (frac)",  0x20, bcd(1)),
    Field::new("food",         "Food",         0x21, bcd(2)),
    Field::new("gold",         "Gold",         0x23, bcd(2)),
    Field::new("gems",         "Gems",         0x25, bcd(1)),
    Field::new("keys",         "Keys",         0x26, bcd(1)),
    Field::new("powders",      "Powders",      0x27, bcd(1)),
    Field::new("worn_armor",   "Worn Armor",   0x28, FieldKind::Byte),
    // Armor owned (BCD counts, letters B..H in the format spec).
    Field::new("armor_cloth",        "Armor: Cloth",     0x29, bcd(1)),
    Field::new("armor_leather",      "Armor: Leather",   0x2A, bcd(1)),
    Field::new("armor_chain",        "Armor: Chain",     0x2B, bcd(1)),
    Field::new("armor_plate",        "Armor: Plate",     0x2C, bcd(1)),
    Field::new("armor_chain_plus2",  "Armor: +2 Chain",  0x2D, bcd(1)),
    Field::new("armor_plate_plus2",  "Armor: +2 Plate",  0x2E, bcd(1)),
    Field::new("armor_exotic",       "Armor: Exotic",    0x2F, bcd(1)),
    Field::new("weapon",       "Ready Weapon", 0x30, FieldKind::Byte),
    // Weapons owned (BCD counts, letters B..P in the format spec).
    Field::new("weapon_dagger",      "Weapon: Dagger",   0x31, bcd(1)),
    Field::new("weapon_mace",        "Weapon: Mace",     0x32, bcd(1)),
    Field::new("weapon_sling",       "Weapon: Sling",    0x33, bcd(1)),
    Field::new("weapon_axe",         "Weapon: Axe",      0x34, bcd(1)),
    Field::new("weapon_bow",         "Weapon: Bow",      0x35, bcd(1)),
    Field::new("weapon_sword",       "Weapon: Sword",    0x36, bcd(1)),
    Field::new("weapon_2h_sword",    "Weapon: 2H Sword", 0x37, bcd(1)),
    Field::new("weapon_axe_plus2",   "Weapon: +2 Axe",   0x38, bcd(1)),
    Field::new("weapon_bow_plus2",   "Weapon: +2 Bow",   0x39, bcd(1)),
    Field::new("weapon_sword_plus2", "Weapon: +2 Sword", 0x3A, bcd(1)),
    Field::new("weapon_gloves",      "Weapon: Gloves",   0x3B, bcd(1)),
    Field::new("weapon_axe_plus4",   "Weapon: +4 Axe",   0x3C, bcd(1)),
    Field::new("weapon_bow_plus4",   "Weapon: +4 Bow",   0x3D, bcd(1)),
    Field::new("weapon_sword_plus4", "Weapon: +4 Sword", 0x3E, bcd(1)),
    Field::new("weapon_exotic",      "Weapon: Exotic",   0x3F, bcd(1)),
];

// --- Shared character-record helpers, parameterized by the record's base offset. ---

/// Whether the record at `base` holds a character (its name is non-empty).
fn record_is_occupied(buf: &[u8], base: usize) -> bool {
    buf[base] != 0
}

/// Format a single field of the record at `base`, or `None` for an unknown key.
fn record_get(buf: &[u8], base: usize, key: &str) -> Option<String> {
    let field = FIELDS.iter().find(|f| f.key == key)?;
    Some(schema::read_field(buf, base, field))
}

/// All known fields of the record at `base` as `(label, value)` pairs.
fn record_inspect(buf: &[u8], base: usize) -> Vec<(&'static str, String)> {
    FIELDS
        .iter()
        .map(|f| (f.label, schema::read_field(buf, base, f)))
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
    schema::write_field(buf, base, field, value)
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
            (
                "Moves",
                schema::read_field(
                    &self.bytes,
                    0,
                    &Field::new(
                        "moves",
                        "Moves",
                        0x03,
                        FieldKind::Bcd {
                            bytes: 4,
                            endian: Endian::Little,
                        },
                    ),
                ),
            ),
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
    fn set_inventory_counts() {
        let mut roster = Ultima3Roster::from_bytes(synthetic_roster()).unwrap();
        roster.set_field(0, "weapon_sword", "3").unwrap();
        assert_eq!(roster.get_field(0, "weapon_sword").unwrap(), "3");
        assert_eq!(roster.as_bytes()[0x36], 0x03); // BCD 3 at the Sword count offset
        roster.set_field(0, "armor_plate", "2").unwrap();
        assert_eq!(roster.get_field(0, "armor_plate").unwrap(), "2");
        assert_eq!(roster.as_bytes()[0x2C], 0x02);
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
