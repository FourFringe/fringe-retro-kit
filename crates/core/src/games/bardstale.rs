//! The Bard's Tale Trilogy (Krome's 2018 remaster) save support.
//!
//! Unlike the DOS-era games, this Unity remaster serialises its saves with .NET's
//! `BinaryFormatter` (the [MS-NRBF] format), so the file is a self-describing object graph
//! rather than a fixed byte layout. We lean on [`crate::codec::nrbf`] to parse that graph and
//! to **patch integers in place**: an edit rewrites exactly the bytes of one value and keeps
//! the file otherwise byte-for-byte identical, side-stepping `BinaryFormatter`'s finicky
//! re-encoding entirely.
//!
//! A save holds a party of [`Character`](https://en.wikipedia.org/wiki/The_Bard%27s_Tale)
//! objects (`BardsTale.Character`) plus party-wide game state. We expose the numeric
//! character stats (attributes, hit/spell points, level, experience, thief and bard bonuses)
//! and the pooled party gold as editable fields; identity fields such as name, class and race
//! are read-only (changing them would need a full re-serialise and can corrupt the save).
//!
//! Edits must be made with **Steam fully closed** so its cloud sync doesn't clobber the file;
//! the CLI's normal timestamped backup still applies.

use std::path::Path;

use crate::codec::nrbf::{Document, Value};
use crate::save::atomic_write;
use crate::schema::{Endian, Field, FieldKind, Variants};
use crate::{Error, Result};

/// The .NET class of a party/roster character.
const CHARACTER_CLASS: &str = "BardsTale.Character";

/// An editable Bard's Tale Trilogy save, backed by its parsed NRBF object graph.
pub struct BardsTaleSave {
    doc: Document,
    /// Object ids of the characters, in id order (their in-file order).
    char_ids: Vec<i32>,
}

impl BardsTaleSave {
    /// Parse a save's bytes into an editable object graph.
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self> {
        let doc = Document::parse(&bytes)?;
        let char_ids: Vec<i32> = doc
            .objects_of_class(CHARACTER_CLASS)
            .map(|o| o.id)
            .collect();
        Ok(BardsTaleSave { doc, char_ids })
    }

    /// The (possibly patched) save bytes.
    pub fn as_bytes(&self) -> &[u8] {
        self.doc.bytes()
    }

    /// Write the save back to disk atomically.
    pub fn write(&self, path: impl AsRef<Path>) -> Result<()> {
        atomic_write(path, self.doc.bytes())
    }

    /// The indices of the characters present in this save (all of them).
    pub fn occupied_characters(&self) -> Vec<usize> {
        (0..self.char_ids.len()).collect()
    }

    /// A one-line summary of a character (name and level), for the entity picker.
    pub fn character_summary(&self, index: usize) -> String {
        let Some(&id) = self.char_ids.get(index) else {
            return String::new();
        };
        let Some(obj) = self.doc.object(id) else {
            return String::new();
        };
        let name = self.doc.member_str(obj, "m_name").unwrap_or("(unnamed)");
        let level = obj.int("m_level").unwrap_or(0);
        format!("{name} (Lvl {level})")
    }

    /// The current value of a character field, if present in this save.
    pub fn character_get(&self, index: usize, key: &str) -> Option<String> {
        // Class/race/gender are boxed .NET enums referenced by the character; resolve to a name.
        if let Some((member, variants)) = char_enum(key) {
            let (_, value) = self.enum_ref(index, member)?;
            return Some(variant_name(variants, value));
        }
        let member = char_member(key)?;
        let id = *self.char_ids.get(index)?;
        let obj = self.doc.object(id)?;
        obj.int(member).map(|v| v.to_string())
    }

    /// Set a character field to a new integer value, validated against the field's storage width.
    pub fn character_set(&mut self, index: usize, key: &str, value: &str) -> Result<()> {
        // An enum field patches the `value__` of the character's boxed enum object.
        if let Some((member, variants)) = char_enum(key) {
            let (ref_id, _) = self.enum_ref(index, member).ok_or_else(|| {
                Error::Format(format!("character #{index} has no `{key}` to set"))
            })?;
            let v = variant_value(variants, value)
                .ok_or_else(|| Error::Format(format!("`{key}` must be one of its named values")))?;
            return self.doc.patch_int(ref_id, "value__", v);
        }
        let member =
            char_member(key).ok_or_else(|| Error::Format(format!("unknown field `{key}`")))?;
        let id = *self
            .char_ids
            .get(index)
            .ok_or_else(|| Error::Format(format!("no character #{index}")))?;
        let v = parse_int(key, value)?;
        self.doc.patch_int(id, member, v)
    }

    /// Resolve a character's boxed-enum member (`m_class`/`m_race`/`m_gender`) to the boxed
    /// object's id and its current `value__`.
    fn enum_ref(&self, index: usize, member: &str) -> Option<(i32, i64)> {
        let id = *self.char_ids.get(index)?;
        let ch = self.doc.object(id)?;
        let Value::Ref(ref_id) = ch.member(member)?.value else {
            return None;
        };
        let value = self.doc.object(ref_id)?.int("value__")?;
        Some((ref_id, value))
    }

    /// The current value of a party-wide field, if present in this save.
    pub fn party_get(&self, key: &str) -> Option<String> {
        let (class, member) = party_target(key)?;
        let obj = self.doc.objects_of_class(class).next()?;
        obj.int(member).map(|v| v.to_string())
    }

    /// Set a party-wide field to a new integer value.
    pub fn party_set(&mut self, key: &str, value: &str) -> Result<()> {
        let (class, member) =
            party_target(key).ok_or_else(|| Error::Format(format!("unknown field `{key}`")))?;
        let v = parse_int(key, value)?;
        let id = self
            .doc
            .objects_of_class(class)
            .next()
            .map(|o| o.id)
            .ok_or_else(|| Error::Format(format!("this save has no {class}")))?;
        self.doc.patch_int(id, member, v)
    }

    /// The editable party-wide fields (entity 0).
    pub fn party_fields() -> &'static [Field] {
        PARTY_FIELDS
    }

    /// The editable per-character fields.
    pub fn character_fields() -> &'static [Field] {
        CHAR_FIELDS
    }
}

/// Quickly test whether some bytes look like a Bard's Tale (NRBF `BinaryFormatter`) save: a
/// `SerializationHeaderRecord` (tag `0`) with major version 1 and minor version 0.
pub fn looks_like(bytes: &[u8]) -> bool {
    bytes.len() > 17
        && bytes[0] == 0
        && bytes[9..13] == [1, 0, 0, 0]
        && bytes[13..17] == [0, 0, 0, 0]
}

/// A short human description of a save slot — the party's name and current location — for the
/// save-file picker. Returns `None` if the bytes don't parse or hold neither detail.
pub fn describe(bytes: &[u8]) -> Option<String> {
    let doc = Document::parse(bytes).ok()?;
    let party = doc.objects_of_class("BardsTale.SaveableParty").next();
    let name = party.and_then(|o| doc.member_str(o, "m_name"));
    let header = doc.objects_of_class("BardsTale.GameSaveHeader").next();
    let map = header.and_then(|o| doc.member_str(o, "m_map"));
    match (
        name.filter(|s| !s.is_empty()),
        map.filter(|s| !s.is_empty()),
    ) {
        (Some(name), Some(map)) => Some(format!("{name} · {map}")),
        (Some(name), None) => Some(name.to_string()),
        (None, Some(map)) => Some(map.to_string()),
        (None, None) => None,
    }
}

/// Parse an edit value as an integer, with a field-specific error message.
fn parse_int(key: &str, value: &str) -> Result<i64> {
    value
        .trim()
        .parse::<i64>()
        .map_err(|_| Error::Format(format!("`{key}` must be a whole number")))
}

/// Map a character field key to its .NET member name.
fn char_member(key: &str) -> Option<&'static str> {
    Some(match key {
        "strength" => "m_strength",
        "intelligence" => "m_intelligence",
        "dexterity" => "m_dexterity",
        "constitution" => "m_constitution",
        "luck" => "m_luck",
        "hitpoints" => "m_hitpoints",
        "max_hitpoints" => "m_maxHitpoints",
        "spellpoints" => "m_spellpoints",
        "max_spellpoints" => "m_maxSpellpoints",
        "level" => "m_level",
        "experience" => "m_experience",
        "condition" => "m_condition",
        "disarm_trap" => "m_disarmTrapBonus",
        "identify" => "m_identifyBonus",
        "hide_in_shadows" => "m_hideInShadowsBonus",
        "songs_remaining" => "m_songsRemaining",
        _ => return None,
    })
}

/// Map a party field key to the `(class, member)` that holds it.
fn party_target(key: &str) -> Option<(&'static str, &'static str)> {
    match key {
        // Party gold is pooled in the game-state object (each character's own `m_gold` is 0).
        "gold" => Some(("BardsTale.GameStateData", "m_gold")),
        _ => None,
    }
}

/// Map an enum field key to the character's boxed-enum member and its named values.
fn char_enum(key: &str) -> Option<(&'static str, Variants)> {
    Some(match key {
        "class" => ("m_class", CLASS),
        "race" => ("m_race", RACE),
        "gender" => ("m_gender", GENDER),
        _ => return None,
    })
}

/// The display name of an enum value, or the raw number if it isn't a known variant.
fn variant_name(variants: Variants, value: i64) -> String {
    variants
        .iter()
        .find(|(k, _)| i64::from(*k) == value)
        .map(|(_, name)| (*name).to_string())
        .unwrap_or_else(|| value.to_string())
}

/// Parse an enum edit: a variant name (case-insensitive) or a raw integer.
fn variant_value(variants: Variants, input: &str) -> Option<i64> {
    let input = input.trim();
    if let Some((k, _)) = variants.iter().find(|(_, n)| n.eq_ignore_ascii_case(input)) {
        return Some(i64::from(*k));
    }
    input.parse::<i64>().ok()
}

/// Character classes, in the game's enum order (values confirmed against a real party).
const CLASS: Variants = &[
    (0, "Warrior"),
    (1, "Paladin"),
    (2, "Rogue"),
    (3, "Bard"),
    (4, "Hunter"),
    (5, "Monk"),
    (6, "Conjurer"),
    (7, "Magician"),
    (8, "Sorcerer"),
    (9, "Wizard"),
    (10, "Archmage"),
];

/// Character races, in the game's enum order.
const RACE: Variants = &[
    (0, "Human"),
    (1, "Elf"),
    (2, "Dwarf"),
    (3, "Hobbit"),
    (4, "Half-Elf"),
    (5, "Half-Orc"),
    (6, "Gnome"),
];

/// Character genders, in the game's enum order.
const GENDER: Variants = &[(0, "Male"), (1, "Female")];

/// A little-endian integer field of the given inclusive maximum. The width/endian are only
/// display metadata: edits go through [`Document::patch_int`], which validates against the
/// value's real storage width.
const fn int(max: u32) -> FieldKind {
    FieldKind::Int {
        bytes: 4,
        endian: Endian::Little,
        max,
    }
}

/// A named-variant field. The width/endian are display metadata; edits patch the boxed enum's
/// `value__` via [`Document::patch_int`].
const fn enum_field(variants: Variants) -> FieldKind {
    FieldKind::Enum {
        bytes: 4,
        endian: Endian::Little,
        variants,
    }
}

const PARTY_FIELDS: &[Field] =
    &[Field::new("gold", "Gold", 0, int(i32::MAX as u32)).in_section("Party")];

const CHAR_FIELDS: &[Field] = &[
    Field::new("class", "Class", 0, enum_field(CLASS)).in_section("Identity"),
    Field::new("race", "Race", 0, enum_field(RACE)).in_section("Identity"),
    Field::new("gender", "Gender", 0, enum_field(GENDER)).in_section("Identity"),
    Field::new("strength", "Strength", 0, int(999)).in_section("Attributes"),
    Field::new("intelligence", "Intelligence", 0, int(999)).in_section("Attributes"),
    Field::new("dexterity", "Dexterity", 0, int(999)).in_section("Attributes"),
    Field::new("constitution", "Constitution", 0, int(999)).in_section("Attributes"),
    Field::new("luck", "Luck", 0, int(999)).in_section("Attributes"),
    Field::new("hitpoints", "Hit Points", 0, int(65535)).in_section("Vitals"),
    Field::new("max_hitpoints", "Max Hit Points", 0, int(65535)).in_section("Vitals"),
    Field::new("spellpoints", "Spell Points", 0, int(65535)).in_section("Vitals"),
    Field::new("max_spellpoints", "Max Spell Points", 0, int(65535)).in_section("Vitals"),
    Field::new("level", "Level", 0, int(255)).in_section("Progress"),
    Field::new("experience", "Experience", 0, int(i32::MAX as u32)).in_section("Progress"),
    Field::new("condition", "Condition", 0, int(255)).in_section("Status"),
    Field::new("disarm_trap", "Disarm Trap Bonus", 0, int(255)).in_section("Thief"),
    Field::new("identify", "Identify Bonus", 0, int(255)).in_section("Thief"),
    Field::new("hide_in_shadows", "Hide in Shadows Bonus", 0, int(255)).in_section("Thief"),
    Field::new("songs_remaining", "Bard Songs Remaining", 0, int(255)).in_section("Bard"),
];

#[cfg(test)]
mod tests {
    use super::*;

    /// A length-prefixed string (single-byte length, adequate for short test strings).
    fn lp(s: &str) -> Vec<u8> {
        let mut v = vec![s.len() as u8];
        v.extend_from_slice(s.as_bytes());
        v
    }

    /// The 17-byte `SerializationHeaderRecord` that opens every NRBF stream.
    fn header(root_id: i32) -> Vec<u8> {
        let mut v = vec![0]; // SerializationHeaderRecord
        v.extend_from_slice(&root_id.to_le_bytes()); // RootId
        v.extend_from_slice(&(-1i32).to_le_bytes()); // HeaderId
        v.extend_from_slice(&1i32.to_le_bytes()); // MajorVersion
        v.extend_from_slice(&0i32.to_le_bytes()); // MinorVersion
        v
    }

    /// A `ClassWithMembersAndTypes` object whose members are a leading string (`m_name`) plus
    /// several Int32s. `members` is `(member_name, value)`; the object id is `id`.
    fn class_obj(id: i32, class: &str, name: &str, members: &[(&str, i32)]) -> Vec<u8> {
        let mut v = vec![5]; // ClassWithMembersAndTypes
        v.extend_from_slice(&id.to_le_bytes());
        v.extend(lp(class));
        v.extend_from_slice(&((members.len() + 1) as i32).to_le_bytes()); // + m_name
        v.extend(lp("m_name"));
        for (name, _) in members {
            v.extend(lp(name));
        }
        // BinaryTypeEnumeration: all PRIMITIVE (0).
        v.extend(vec![0u8; members.len() + 1]);
        // Primitive types: String (18) for m_name, Int32 (8) for the rest.
        v.push(18);
        v.extend(vec![8u8; members.len()]);
        v.extend_from_slice(&7i32.to_le_bytes()); // LibraryId
        v.extend(lp(name)); // m_name value
        for (_, value) in members {
            v.extend_from_slice(&value.to_le_bytes());
        }
        v
    }

    /// A `ClassWithMembersAndTypes` object whose members are all strings. `members` is
    /// `(member_name, value)`; the object id is `id`.
    fn str_obj(id: i32, class: &str, members: &[(&str, &str)]) -> Vec<u8> {
        let mut v = vec![5]; // ClassWithMembersAndTypes
        v.extend_from_slice(&id.to_le_bytes());
        v.extend(lp(class));
        v.extend_from_slice(&(members.len() as i32).to_le_bytes());
        for (name, _) in members {
            v.extend(lp(name));
        }
        v.extend(vec![0u8; members.len()]); // all PRIMITIVE
        v.extend(vec![18u8; members.len()]); // all String
        v.extend_from_slice(&7i32.to_le_bytes()); // LibraryId
        for (_, value) in members {
            v.extend(lp(value));
        }
        v
    }

    /// A one-character party save with a `GameStats` holding party gold.
    fn save_bytes() -> Vec<u8> {
        let mut v = header(1);
        v.extend(class_obj(
            1,
            CHARACTER_CLASS,
            "Brian",
            &[
                ("m_strength", 12),
                ("m_hitpoints", 30),
                ("m_maxHitpoints", 30),
                ("m_level", 2),
                ("m_experience", 1015),
            ],
        ));
        v.extend(class_obj(
            2,
            "BardsTale.GameStateData",
            "",
            &[("m_gold", 1200)],
        ));
        v.push(11); // MessageEnd
        v
    }

    #[test]
    fn detects_nrbf_saves() {
        assert!(looks_like(&save_bytes()));
        assert!(!looks_like(b"PLAYER1.U1 is not NRBF at all"));
        assert!(!looks_like(&[0, 1, 2]));
    }

    #[test]
    fn reads_character_stats() {
        let save = BardsTaleSave::from_bytes(save_bytes()).unwrap();
        assert_eq!(save.occupied_characters(), vec![0]);
        assert_eq!(save.character_get(0, "strength"), Some("12".to_string()));
        assert_eq!(save.character_get(0, "hitpoints"), Some("30".to_string()));
        assert_eq!(
            save.character_get(0, "experience"),
            Some("1015".to_string())
        );
        assert!(save.character_summary(0).contains("Brian"));
        assert!(save.character_summary(0).contains("Lvl 2"));
    }

    #[test]
    fn edits_a_character_in_place() {
        let before = save_bytes().len();
        let mut save = BardsTaleSave::from_bytes(save_bytes()).unwrap();
        save.character_set(0, "strength", "18").unwrap();
        assert_eq!(save.character_get(0, "strength"), Some("18".to_string()));
        // The edit keeps the file the same size (patched in place).
        assert_eq!(save.as_bytes().len(), before);
    }

    #[test]
    fn reads_and_edits_party_gold() {
        let mut save = BardsTaleSave::from_bytes(save_bytes()).unwrap();
        assert_eq!(save.party_get("gold"), Some("1200".to_string()));
        save.party_set("gold", "999").unwrap();
        assert_eq!(save.party_get("gold"), Some("999".to_string()));
    }

    #[test]
    fn rejects_bad_edits() {
        let mut save = BardsTaleSave::from_bytes(save_bytes()).unwrap();
        assert!(save.character_set(0, "strength", "not a number").is_err());
        assert!(save.character_set(0, "no_such_field", "1").is_err());
        assert!(save.character_set(9, "strength", "1").is_err());
    }

    #[test]
    fn reads_and_edits_a_boxed_enum_class() {
        let mut v = header(1);
        // Boxed enum object #10: BardsTale.Character+Class { value__ = 3 (Bard) }.
        v.push(5); // ClassWithMembersAndTypes
        v.extend_from_slice(&10i32.to_le_bytes());
        v.extend(lp("BardsTale.Character+Class"));
        v.extend_from_slice(&1i32.to_le_bytes()); // 1 member
        v.extend(lp("value__"));
        v.push(0); // PRIMITIVE
        v.push(8); // Int32
        v.extend_from_slice(&7i32.to_le_bytes()); // LibraryId
        v.extend_from_slice(&3i32.to_le_bytes()); // value__ = 3

        // Character #1 with m_name (string), m_class (reference -> #10), m_level (int).
        v.push(5);
        v.extend_from_slice(&1i32.to_le_bytes());
        v.extend(lp(CHARACTER_CLASS));
        v.extend_from_slice(&3i32.to_le_bytes()); // 3 members
        v.extend(lp("m_name"));
        v.extend(lp("m_class"));
        v.extend(lp("m_level"));
        v.extend_from_slice(&[0u8, 3u8, 0u8]); // PRIMITIVE, SYSTEM_CLASS, PRIMITIVE
        v.push(18); // m_name primitive = String
        v.extend(lp("BardsTale.Character+Class")); // SYSTEM_CLASS type name
        v.push(8); // m_level primitive = Int32
        v.extend_from_slice(&7i32.to_le_bytes()); // LibraryId
        v.extend(lp("Merlin")); // m_name value
        v.push(9); // MemberReference
        v.extend_from_slice(&10i32.to_le_bytes()); // m_class -> #10
        v.extend_from_slice(&5i32.to_le_bytes()); // m_level = 5
        v.push(11); // MessageEnd

        let mut save = BardsTaleSave::from_bytes(v).unwrap();
        assert_eq!(save.character_get(0, "class").as_deref(), Some("Bard"));
        // Set by name, and by raw number.
        save.character_set(0, "class", "Wizard").unwrap();
        assert_eq!(save.character_get(0, "class").as_deref(), Some("Wizard"));
        save.character_set(0, "class", "10").unwrap();
        assert_eq!(save.character_get(0, "class").as_deref(), Some("Archmage"));
        // An unknown variant name is rejected.
        assert!(save.character_set(0, "class", "Wombat").is_err());
    }

    #[test]
    fn describes_a_slot() {
        let mut v = header(1);
        v.extend(str_obj(
            1,
            "BardsTale.SaveableParty",
            &[("m_name", "The A-TEAM")],
        ));
        v.extend(str_obj(
            2,
            "BardsTale.GameSaveHeader",
            &[("m_map", "Skara Brae")],
        ));
        v.push(11); // MessageEnd
        assert_eq!(describe(&v).as_deref(), Some("The A-TEAM · Skara Brae"));
        // Bytes that don't parse yield no description rather than an error.
        assert_eq!(describe(b"not an nrbf stream"), None);
    }
}
