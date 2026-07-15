//! Hardcoded Ultima VI (The False Prophet) party support (`OBJLIST`).
//!
//! Unlike the earlier Ultimas, a U6 save is a *directory* (`SAVEGAME/`) of object files.
//! Most are map-object blocks (`OBJBLK*`, some LZW-compressed) — but the party's character
//! sheets live in **`OBJLIST`**, which is **uncompressed** and laid out as flat, fixed
//! arrays. So editing character stats needs no decompression or object-graph parsing.
//!
//! `OBJLIST` stores per-actor stats as column arrays indexed by *actor number* (0..255):
//! strength at `0x900 + n`, dexterity at `0xA00 + n`, and so on. The party is a short list
//! of actor numbers at `0xFE0`, with each member's **name** stored by *party position* in a
//! 14-byte slot at `0xF00`. Layout cross-checked against the Nuvie reimplementation and a
//! real `OBJLIST` (the Avatar/Dupre/Shamino/Iolo starting party).

use std::path::Path;

use crate::schema::{self, Endian, Field, FieldKind, Variants};
use crate::{Error, Result};

/// Size of a U6 `OBJLIST` file, in bytes (`0x1D73`).
pub const OBJLIST_LEN: usize = 7539;

/// Offset of the party-name slots (indexed by party position).
const NAME_BASE: usize = 0xF00;
/// Bytes per party-name slot (13 chars + null terminator).
const NAME_LEN: usize = 14;
/// Offset of the party roster (actor number of each member).
const ROSTER_BASE: usize = 0xFE0;
/// Offset of the party-size byte.
const NUM_IN_PARTY: usize = 0xFF0;
/// Maximum number of party members the roster/name area can hold.
const PARTY_MAX: usize = 16;

// --- Enum tables (value -> label). ---

const GENDER: Variants = &[(0, "Male"), (1, "Female")];

/// A single-byte integer field with an inclusive edit maximum.
const fn u8m(max: u32) -> FieldKind {
    FieldKind::Int {
        bytes: 1,
        endian: Endian::Little,
        max,
    }
}

/// A little-endian `u16` field with an inclusive edit maximum.
const fn u16le(max: u32) -> FieldKind {
    FieldKind::Int {
        bytes: 2,
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

// Display sections.
const S_CHARACTER: &str = "Character";
const S_ATTRIBUTES: &str = "Attributes";
const S_VITALS: &str = "Vitals";
const S_PARTY: &str = "Party";

/// Party-member fields. `name` is addressed by party position; the rest are per-actor stat
/// arrays whose `offset` is the **array base** (real address `base + actor_num * width`).
#[rustfmt::skip]
const CHARACTER_FIELDS: &[Field] = &[
    Field::new("name",         "Name",         NAME_BASE, FieldKind::Name { len: NAME_LEN }).in_section(S_CHARACTER),
    Field::new("strength",     "Strength",     0x0900, u8m(30)).in_section(S_ATTRIBUTES),
    Field::new("dexterity",    "Dexterity",    0x0A00, u8m(30)).in_section(S_ATTRIBUTES),
    Field::new("intelligence", "Intelligence", 0x0B00, u8m(30)).in_section(S_ATTRIBUTES),
    Field::new("experience",   "Experience",   0x0C00, u16le(9999)).in_section(S_VITALS),
    Field::new("hp",           "Hit Points",   0x0E00, u8m(255)).in_section(S_VITALS),
    Field::new("level",        "Level",        0x0FF1, u8m(8)).in_section(S_VITALS),
    Field::new("magic",        "Magic Points", 0x13F1, u8m(255)).in_section(S_VITALS),
];

/// Player/party-wide fields (single values, not per-actor arrays).
#[rustfmt::skip]
const PARTY_FIELDS: &[Field] = &[
    Field::new("karma",  "Karma",  0x1BF9, u8m(99)).in_section(S_PARTY),
    Field::new("gender", "Gender", 0x1C71, enum1(GENDER)).in_section(S_PARTY),
];

/// The byte width of a field's stored value (its array stride).
fn field_width(kind: &FieldKind) -> usize {
    match kind {
        FieldKind::Int { bytes, .. }
        | FieldKind::Scaled { bytes, .. }
        | FieldKind::Bcd { bytes, .. }
        | FieldKind::Enum { bytes, .. } => *bytes,
        _ => 1,
    }
}

/// A `Field` relocated to an absolute address (so schema ops apply it at base 0).
fn at(field: &Field, address: usize) -> Field {
    Field::new(field.key, field.label, address, field.kind).in_section(field.section.unwrap_or(""))
}

/// The party-member field table (for building editors).
pub fn character_fields() -> &'static [Field] {
    CHARACTER_FIELDS
}

/// The party-wide field table (for building editors).
pub fn party_fields() -> &'static [Field] {
    PARTY_FIELDS
}

/// A parsed Ultima VI `OBJLIST` file.
#[derive(Clone)]
pub struct Ultima6Save {
    bytes: Vec<u8>,
}

impl Ultima6Save {
    /// Wrap an in-memory byte buffer, validating its length.
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self> {
        if bytes.len() != OBJLIST_LEN {
            return Err(Error::Format(format!(
                "expected {OBJLIST_LEN} bytes, got {}",
                bytes.len()
            )));
        }
        Ok(Self { bytes })
    }

    /// Read and parse an `OBJLIST` file from disk.
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

    /// The number of party members.
    pub fn num_in_party(&self) -> usize {
        (self.bytes[NUM_IN_PARTY] as usize).min(PARTY_MAX)
    }

    /// The 0-based positions of party members (0..num_in_party).
    pub fn occupied_characters(&self) -> Vec<usize> {
        (0..self.num_in_party()).collect()
    }

    /// The actor number backing party position `member`.
    fn actor_num(&self, member: usize) -> usize {
        self.bytes[ROSTER_BASE + member] as usize
    }

    /// The absolute address of a member's field: `name` is by party position, the per-actor
    /// stat arrays are `array_base + actor_num * width`.
    fn field_address(&self, field: &Field, member: usize) -> usize {
        if field.key == "name" {
            NAME_BASE + member * NAME_LEN
        } else {
            field.offset + self.actor_num(member) * field_width(&field.kind)
        }
    }

    /// A one-line summary of a party member.
    pub fn character_summary(&self, member: usize) -> String {
        let g = |key| self.character_get(member, key).unwrap_or_default();
        format!(
            "{} — L{} STR {} DEX {} INT {} HP {}",
            g("name"),
            g("level"),
            g("strength"),
            g("dexterity"),
            g("intelligence"),
            g("hp"),
        )
    }

    /// Format a party member's field by key, or `None` for an unknown member/key.
    pub fn character_get(&self, member: usize, key: &str) -> Option<String> {
        if member >= self.num_in_party() {
            return None;
        }
        let field = CHARACTER_FIELDS.iter().find(|f| f.key == key)?;
        let address = self.field_address(field, member);
        Some(schema::read_field(&self.bytes, 0, &at(field, address)))
    }

    /// Set a party member's field by key, validating the value first.
    pub fn character_set(&mut self, member: usize, key: &str, value: &str) -> Result<()> {
        if member >= self.num_in_party() {
            return Err(Error::Format(format!(
                "party member must be 1..={} (got {})",
                self.num_in_party(),
                member + 1
            )));
        }
        let field = CHARACTER_FIELDS
            .iter()
            .find(|f| f.key == key)
            .ok_or_else(|| Error::Format(format!("unknown field '{key}'")))?;
        let address = self.field_address(field, member);
        schema::write_field(&mut self.bytes, 0, &at(field, address), value)
    }

    /// All known fields of a party member as `(section, label, value)` tuples.
    pub fn character_inspect(&self, member: usize) -> Vec<(&'static str, &'static str, String)> {
        CHARACTER_FIELDS
            .iter()
            .map(|f| {
                let address = self.field_address(f, member);
                (
                    f.section.unwrap_or_default(),
                    f.label,
                    schema::read_field(&self.bytes, 0, &at(f, address)),
                )
            })
            .collect()
    }

    /// Format a party-wide field by key, or `None` for an unknown key.
    pub fn party_get(&self, key: &str) -> Option<String> {
        let field = PARTY_FIELDS.iter().find(|f| f.key == key)?;
        Some(schema::read_field(&self.bytes, 0, field))
    }

    /// Set a party-wide field by key, validating the value first.
    pub fn party_set(&mut self, key: &str, value: &str) -> Result<()> {
        let field = PARTY_FIELDS
            .iter()
            .find(|f| f.key == key)
            .ok_or_else(|| Error::Format(format!("unknown field '{key}'")))?;
        schema::write_field(&mut self.bytes, 0, field, value)
    }

    /// All known party-wide fields as `(section, label, value)` tuples.
    pub fn party_inspect(&self) -> Vec<(&'static str, &'static str, String)> {
        PARTY_FIELDS
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

    /// The keys of all known party-member fields (name first).
    pub fn character_field_keys() -> impl Iterator<Item = &'static str> {
        CHARACTER_FIELDS.iter().map(|f| f.key)
    }

    /// The keys of all known party-wide fields.
    pub fn party_field_keys() -> impl Iterator<Item = &'static str> {
        PARTY_FIELDS.iter().map(|f| f.key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A synthetic OBJLIST with a 2-member party: Avatar (actor 1) and Dupre (actor 2).
    fn synthetic() -> Vec<u8> {
        let mut buf = vec![0u8; OBJLIST_LEN];
        buf[NUM_IN_PARTY] = 2;
        buf[ROSTER_BASE] = 1; // party pos 0 -> actor 1
        buf[ROSTER_BASE + 1] = 2; // party pos 1 -> actor 2
        buf[NAME_BASE..NAME_BASE + 6].copy_from_slice(b"Avatar");
        buf[NAME_BASE + NAME_LEN..NAME_BASE + NAME_LEN + 5].copy_from_slice(b"Dupre");
        // Avatar (actor 1) stats.
        buf[0x900 + 1] = 15; // strength
        buf[0xA00 + 1] = 16; // dexterity
        buf[0xB00 + 1] = 17; // intelligence
        buf[0xC00 + 2..0xC00 + 4].copy_from_slice(&9999u16.to_le_bytes()); // exp (actor 1)
        buf[0xE00 + 1] = 90; // hp
        buf[0xFF1 + 1] = 8; // level
        buf[0x13F1 + 1] = 30; // magic
                              // Dupre (actor 2) strength.
        buf[0x900 + 2] = 26;
        // Party-wide.
        buf[0x1BF9] = 5; // karma
        buf[0x1C71] = 1; // gender = Female
        buf
    }

    #[test]
    fn rejects_wrong_size() {
        assert!(Ultima6Save::from_bytes(vec![0u8; 100]).is_err());
        assert!(Ultima6Save::from_bytes(vec![0u8; OBJLIST_LEN]).is_ok());
    }

    #[test]
    fn reads_party_members() {
        let save = Ultima6Save::from_bytes(synthetic()).unwrap();
        assert_eq!(save.occupied_characters(), vec![0, 1]);
        assert_eq!(save.character_get(0, "name").as_deref(), Some("Avatar"));
        assert_eq!(save.character_get(0, "strength").as_deref(), Some("15"));
        assert_eq!(save.character_get(0, "dexterity").as_deref(), Some("16"));
        assert_eq!(save.character_get(0, "experience").as_deref(), Some("9999"));
        assert_eq!(save.character_get(0, "level").as_deref(), Some("8"));
        assert_eq!(save.character_get(0, "magic").as_deref(), Some("30"));
        // Second member resolves its own actor number.
        assert_eq!(save.character_get(1, "name").as_deref(), Some("Dupre"));
        assert_eq!(save.character_get(1, "strength").as_deref(), Some("26"));
    }

    #[test]
    fn edits_are_validated_and_touch_only_their_bytes() {
        let mut save = Ultima6Save::from_bytes(synthetic()).unwrap();
        save.character_set(0, "strength", "25").unwrap();
        save.character_set(0, "name", "Lord British").unwrap();
        assert_eq!(save.character_get(0, "strength").as_deref(), Some("25"));
        assert_eq!(
            save.character_get(0, "name").as_deref(),
            Some("Lord British")
        );
        // Dupre's strength (a neighbouring array cell) is untouched.
        assert_eq!(save.character_get(1, "strength").as_deref(), Some("26"));

        // Out-of-range values are rejected (strength caps at 30).
        assert!(save.character_set(0, "strength", "99").is_err());
        assert!(save.character_set(0, "level", "50").is_err());
        assert!(save.character_set(0, "bogus", "1").is_err());
    }

    #[test]
    fn reads_and_edits_party_wide_fields() {
        let mut save = Ultima6Save::from_bytes(synthetic()).unwrap();
        assert_eq!(save.party_get("karma").as_deref(), Some("5"));
        assert_eq!(save.party_get("gender").as_deref(), Some("Female"));
        save.party_set("karma", "50").unwrap();
        save.party_set("gender", "Male").unwrap();
        assert_eq!(save.party_get("karma").as_deref(), Some("50"));
        assert_eq!(save.party_get("gender").as_deref(), Some("Male"));
    }
}
