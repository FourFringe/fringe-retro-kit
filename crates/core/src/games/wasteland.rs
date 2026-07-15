//! Wasteland (1988) save support.
//!
//! A Wasteland save is a *directory* (`GAME1` plus static `GAME2`/`MASTER*` data and
//! assets); the mutable state lives in `GAME1`. `GAME1` is a series of **MSQ blocks**, each
//! beginning with a 4-byte `msqN` header (`N` is the disk digit), followed by two seed
//! bytes and a ciphertext body encrypted with a "rotating XOR" stream cipher.
//!
//! The **first** block of `GAME1` (`msq0`) is the *savegame* block: a fixed 4608-byte body
//! holding party state and seven 256-byte character records. The remaining blocks are map
//! state and are left untouched when we save.
//!
//! The cipher and layout come from Klaus Reimer's `wlandsuite`
//! (<https://github.com/kayahr/wlandsuite>), cross-checked against a real `GAME1`: the
//! initial key is `seed0 ^ seed1`, each decrypted byte is `cipher ^ key`, and after every
//! byte the key advances by `0x1F` (wrapping at 256).

use std::path::Path;

use crate::schema::{self, Endian, Field, FieldKind, Variants};
use crate::{Error, Result};

/// The value added to the rolling XOR key after each byte.
const KEY_STEP: u8 = 0x1F;

/// Bytes of MSQ header (`msqN`) plus the two seed bytes, before the ciphertext.
const BLOCK_PREFIX_LEN: usize = 6;
/// Decrypted length of the savegame block body, in bytes (`0x1200`).
const SAVEGAME_BODY_LEN: usize = 0x1200;
/// Total on-disk length of the encrypted savegame block (`msq0` + seed + ciphertext).
const SAVEGAME_BLOCK_LEN: usize = BLOCK_PREFIX_LEN + SAVEGAME_BODY_LEN;
/// Offset of the first character record within the decrypted savegame body.
const CHARACTER_BASE: usize = 0x100;
/// Size of one character record, in bytes.
const RECORD_LEN: usize = 0x100;
/// Number of character slots in a savegame.
pub const CHARACTER_COUNT: usize = 7;
/// Length of a character's name field, including the null terminator.
const NAME_LEN: usize = 14;
/// Length of a character's rank string, including the null terminator.
const RANK_LEN: usize = 25;

// --- Enum tables (value -> label). ---

const GENDER: Variants = &[(0, "Male"), (1, "Female")];
const NATIONALITY: Variants = &[
    (0, "US"),
    (1, "Russian"),
    (2, "Mexican"),
    (3, "Indian"),
    (4, "Chinese"),
];
const YES_NO: Variants = &[(0, "No"), (1, "Yes")];

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

/// A little-endian 3-byte integer field with an inclusive edit maximum.
const fn u24le(max: u32) -> FieldKind {
    FieldKind::Int {
        bytes: 3,
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

/// The fields of one character record (offsets are within the 256-byte record).
#[rustfmt::skip]
const CHARACTER_FIELDS: &[Field] = &[
    Field::new("name",         "Name",         0x00, FieldKind::Name { len: NAME_LEN }).in_section(S_CHARACTER),
    Field::new("gender",       "Gender",       0x18, enum1(GENDER)).in_section(S_CHARACTER),
    Field::new("nationality",  "Nationality",  0x19, enum1(NATIONALITY)).in_section(S_CHARACTER),
    Field::new("npc",          "NPC",          0x29, enum1(YES_NO)).in_section(S_CHARACTER),
    Field::new("rank",         "Rank",         0x32, FieldKind::Name { len: RANK_LEN }).in_section(S_CHARACTER),
    Field::new("strength",     "Strength",     0x0E, u8m(255)).in_section(S_ATTRIBUTES),
    Field::new("iq",           "IQ",           0x0F, u8m(255)).in_section(S_ATTRIBUTES),
    Field::new("luck",         "Luck",         0x10, u8m(255)).in_section(S_ATTRIBUTES),
    Field::new("speed",        "Speed",        0x11, u8m(255)).in_section(S_ATTRIBUTES),
    Field::new("agility",      "Agility",      0x12, u8m(255)).in_section(S_ATTRIBUTES),
    Field::new("dexterity",    "Dexterity",    0x13, u8m(255)).in_section(S_ATTRIBUTES),
    Field::new("charisma",     "Charisma",     0x14, u8m(255)).in_section(S_ATTRIBUTES),
    Field::new("con",          "Constitution", 0x1D, u16le(65535)).in_section(S_VITALS),
    Field::new("max_con",      "Max CON",      0x1B, u16le(65535)).in_section(S_VITALS),
    Field::new("last_con",     "Last CON",     0x26, u16le(65535)).in_section(S_VITALS),
    Field::new("ac",           "Armor Class",  0x1A, FieldKind::Byte).in_section(S_VITALS),
    Field::new("level",        "Level",        0x24, u8m(255)).in_section(S_VITALS),
    Field::new("experience",   "Experience",   0x21, u24le(0xFF_FFFF)).in_section(S_VITALS),
    Field::new("skill_points", "Skill Points", 0x20, FieldKind::Byte).in_section(S_VITALS),
    Field::new("money",        "Money",        0x15, u24le(0xFF_FFFF)).in_section(S_VITALS),
    Field::new("afflictions",  "Afflictions",  0x28, FieldKind::Byte).in_section(S_VITALS),
];

// --- Skills ---

/// Offset of the skill list within a character record.
const SKILL_BASE: usize = 0x80;
/// Number of skill slots (each `id, level`).
const SKILL_SLOTS: usize = 30;

/// The game's skill list, indexed by the id stored in each skill slot (`wlandsuite` /
/// Wasteland manual). A character carries a subset, packed from the start of the list.
#[rustfmt::skip]
const SKILLS: &[(u8, &str)] = &[
    (1, "Brawling"),        (2, "Climb"),          (3, "Clip Pistol"),    (4, "Knife Fight"),
    (5, "Pugilism"),        (6, "Rifle"),          (7, "Swim"),           (8, "Knife Throw"),
    (9, "Perception"),      (10, "Assault Rifle"), (11, "AT Weapon"),     (12, "SMG"),
    (13, "Acrobat"),        (14, "Gamble"),        (15, "Picklock"),      (16, "Silent Move"),
    (17, "Combat Shooting"),(18, "Confidence"),    (19, "Sleight of Hand"),(20, "Demolitions"),
    (21, "Forgery"),        (22, "Alarm Disarm"),  (23, "Bureaucracy"),   (24, "Bomb Disarm"),
    (25, "Medic"),          (26, "Safecrack"),     (27, "Cryptology"),    (28, "Metallurgy"),
    (29, "Helicopter Piloting"), (30, "Electronics"), (31, "Toaster Repair"), (32, "Doctor"),
    (33, "Clone Tech"),     (34, "Energy Weapon"), (35, "Cyborg Tech"),
];

/// One of a character's learned skills.
pub struct CharacterSkill {
    /// The skill's numeric id.
    pub id: u8,
    /// The skill's display name.
    pub name: &'static str,
    /// The skill's level.
    pub level: u8,
}

/// The display name for a skill id, if known.
fn skill_name(id: u8) -> Option<&'static str> {
    SKILLS.iter().find(|(i, _)| *i == id).map(|(_, n)| *n)
}

/// Resolve a skill selector — a numeric id or a name (case- and punctuation-insensitive,
/// e.g. `medic`, `Alarm Disarm`, `alarmdisarm`, `22`) — to a skill id.
fn skill_id(selector: &str) -> Option<u8> {
    let norm = |s: &str| -> String {
        s.chars()
            .filter(char::is_ascii_alphanumeric)
            .map(|c| c.to_ascii_lowercase())
            .collect()
    };
    if let Ok(n) = selector.parse::<u8>() {
        if skill_name(n).is_some() {
            return Some(n);
        }
    }
    let target = norm(selector);
    SKILLS
        .iter()
        .find(|(_, n)| norm(n) == target)
        .map(|(i, _)| *i)
}

/// Every known skill name (for help text).
pub fn skill_names() -> impl Iterator<Item = &'static str> {
    SKILLS.iter().map(|(_, n)| *n)
}

/// Whether `selector` names a known skill (by name or numeric id). Useful for routing an
/// editor key to [`WastelandSave::skill_set`] vs. a plain character field.
pub fn is_skill(selector: &str) -> bool {
    skill_id(selector).is_some()
}

/// Decrypt a single MSQ block.
///
/// `block` must start with the 4-byte `msqN` header, then the two seed bytes, then the
/// ciphertext. Returns the decrypted body (everything after the seed bytes).
pub fn decrypt(block: &[u8]) -> Result<Vec<u8>> {
    if block.len() < BLOCK_PREFIX_LEN {
        return Err(Error::Format(format!(
            "MSQ block too short: {} bytes",
            block.len()
        )));
    }
    if &block[0..3] != b"msq" {
        return Err(Error::Format(
            "not an MSQ block (missing 'msq' header)".into(),
        ));
    }
    let mut key = block[4] ^ block[5];
    let mut out = Vec::with_capacity(block.len() - BLOCK_PREFIX_LEN);
    for &cipher in &block[BLOCK_PREFIX_LEN..] {
        out.push(cipher ^ key);
        key = key.wrapping_add(KEY_STEP);
    }
    Ok(out)
}

/// Encrypt a decrypted MSQ block `body` for the given `disk` (0 or 1), producing a full
/// block (`msqN` header, the two checksum-seed bytes, then the ciphertext).
///
/// The two seed bytes store the block checksum (see [`block_checksum`]); the initial key is
/// `seed_lo ^ seed_hi`, matching [`decrypt`].
pub fn encrypt(body: &[u8], disk: u8) -> Vec<u8> {
    let checksum = block_checksum(body);
    let seed_lo = (checksum & 0xFF) as u8;
    let seed_hi = (checksum >> 8) as u8;

    let mut out = Vec::with_capacity(BLOCK_PREFIX_LEN + body.len());
    out.extend_from_slice(b"msq");
    out.push(b'0' + disk);
    out.push(seed_lo);
    out.push(seed_hi);

    let mut key = seed_lo ^ seed_hi;
    for &plain in body {
        out.push(plain ^ key);
        key = key.wrapping_add(KEY_STEP);
    }
    out
}

/// Wasteland's block checksum, stored (little-endian) as the two seed bytes.
///
/// Bytes are summed into a 16-bit accumulator; on each 16-bit overflow the carry is folded
/// back as `+0x100` (an artifact of the original game's byte-wise add-with-carry). The seed
/// is the two's-complement negation, so that a reader subtracting each byte from zero ends
/// back at the stored value. (`wlandsuite` uses a plain negated sum and is therefore *not*
/// byte-faithful; this reproduces the original game's saves exactly.)
fn block_checksum(body: &[u8]) -> u16 {
    let mut acc: u32 = 0;
    for &b in body {
        acc += b as u32;
        if acc > 0xFFFF {
            acc = (acc & 0xFFFF) + 0x100;
        }
    }
    (0u32.wrapping_sub(acc) & 0xFFFF) as u16
}

fn character_base(index: usize) -> usize {
    CHARACTER_BASE + index * RECORD_LEN
}

/// Whether a character slot holds a character. Empty slots are zero-filled, so a non-zero
/// first name byte marks an occupied slot.
fn character_is_occupied(body: &[u8], index: usize) -> bool {
    body[character_base(index)] != 0
}

fn field_get(buf: &[u8], base: usize, key: &str) -> Option<String> {
    let field = CHARACTER_FIELDS.iter().find(|f| f.key == key)?;
    Some(schema::read_field(buf, base, field))
}

fn field_set(buf: &mut [u8], base: usize, key: &str, value: &str) -> Result<()> {
    let field = CHARACTER_FIELDS
        .iter()
        .find(|f| f.key == key)
        .ok_or_else(|| Error::Format(format!("unknown field '{key}'")))?;
    schema::write_field(buf, base, field, value)
}

/// The character-record field table (for building editors).
pub fn character_fields() -> &'static [Field] {
    CHARACTER_FIELDS
}

/// A located MSQ block: its byte offset in the file and total size (header + seed + cipher).
struct MsqBlock {
    offset: usize,
    size: usize,
}

/// Split a `GAMEn` file into its MSQ blocks by scanning for `msqN` headers (`N` is the
/// disk digit taken from the first header). Block boundaries are the header offsets; the
/// last block runs to end-of-file. This mirrors `wlandsuite`'s scanner and shares its one
/// caveat: a literal `msqN` appearing inside a block's ciphertext would split it (rare, and
/// the savegame block is additionally validated by size and party order).
fn msq_blocks(raw: &[u8]) -> Result<Vec<MsqBlock>> {
    if raw.len() < 4 || &raw[0..3] != b"msq" {
        return Err(Error::Format(
            "not a Wasteland game file (missing 'msq' header)".into(),
        ));
    }
    let sig = [b'm', b's', b'q', raw[3]];
    let mut blocks = Vec::new();
    let mut prev = 0usize;
    let mut i = 4usize;
    while i + 4 <= raw.len() {
        if raw[i..i + 4] == sig {
            blocks.push(MsqBlock {
                offset: prev,
                size: i - prev,
            });
            prev = i;
            i += 4;
        } else {
            i += 1;
        }
    }
    blocks.push(MsqBlock {
        offset: prev,
        size: raw.len() - prev,
    });
    Ok(blocks)
}

/// Whether a decrypted block body looks like the savegame: bytes 1..8 are the party
/// member order — each in `0..=7`, with non-zero values unique (per `wlandsuite`).
fn is_savegame_body(body: &[u8]) -> bool {
    if body.len() < 9 {
        return false;
    }
    let mut seen = [false; 8];
    for &b in &body[1..8] {
        if b > 7 {
            return false;
        }
        if b != 0 {
            if seen[b as usize] {
                return false;
            }
            seen[b as usize] = true;
        }
    }
    true
}

/// A parsed Wasteland `GAME1` save.
#[derive(Clone)]
pub struct WastelandSave {
    /// The complete `GAME1` file, verbatim (map blocks and all).
    raw: Vec<u8>,
    /// The decrypted savegame-block body (party state + character records).
    body: Vec<u8>,
    /// Byte offset of the savegame block within `raw`.
    block_offset: usize,
    /// The disk digit (0 for `GAME1`), used to re-encrypt on write.
    disk: u8,
}

impl WastelandSave {
    /// Wrap a complete `GAME1` byte buffer, locating and decrypting the savegame block.
    pub fn from_bytes(raw: Vec<u8>) -> Result<Self> {
        if raw.len() < 4 || &raw[0..4] != b"msq0" {
            return Err(Error::Format(
                "not a Wasteland GAME1 save (missing 'msq0' header)".into(),
            ));
        }
        let disk = raw[3] - b'0';
        // The savegame is the block of size 4614 whose decrypted body is a valid party.
        for block in msq_blocks(&raw)? {
            if block.size != SAVEGAME_BLOCK_LEN {
                continue;
            }
            let body = decrypt(&raw[block.offset..block.offset + block.size])?;
            if body.len() >= CHARACTER_BASE + CHARACTER_COUNT * RECORD_LEN
                && is_savegame_body(&body)
            {
                return Ok(Self {
                    raw,
                    body,
                    block_offset: block.offset,
                    disk,
                });
            }
        }
        Err(Error::Format(
            "no savegame block found in GAME1 (expected a 4614-byte MSQ block)".into(),
        ))
    }

    /// Read and parse a `GAME1` file from disk.
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        Self::from_bytes(std::fs::read(path)?)
    }

    /// Re-encrypt the savegame block and write the whole `GAME1` to `path` atomically.
    /// Only the savegame block changes; all other blocks are preserved byte-for-byte.
    /// Callers are responsible for backups.
    pub fn write(&self, path: impl AsRef<Path>) -> Result<()> {
        crate::save::atomic_write(path, &self.to_bytes())
    }

    /// The re-encrypted `GAME1` bytes (as [`write`](Self::write) would produce).
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut raw = self.raw.clone();
        let block = encrypt(&self.body, self.disk);
        raw[self.block_offset..self.block_offset + SAVEGAME_BLOCK_LEN].copy_from_slice(&block);
        raw
    }

    /// The 0-based indices of character slots that hold a character.
    pub fn occupied_characters(&self) -> Vec<usize> {
        (0..CHARACTER_COUNT)
            .filter(|&i| character_is_occupied(&self.body, i))
            .collect()
    }

    /// A one-line summary of a character (0-based index).
    pub fn character_summary(&self, index: usize) -> String {
        let g = |key| self.character_get(index, key).unwrap_or_default();
        format!(
            "{} — L{} {}, CON {}/{}",
            g("name"),
            g("level"),
            g("rank"),
            g("con"),
            g("max_con"),
        )
    }

    /// Format a character field by key, or `None` for an unknown character/key.
    pub fn character_get(&self, index: usize, key: &str) -> Option<String> {
        if index >= CHARACTER_COUNT {
            return None;
        }
        field_get(&self.body, character_base(index), key)
    }

    /// Set a character field by key, validating the value first.
    pub fn character_set(&mut self, index: usize, key: &str, value: &str) -> Result<()> {
        if index >= CHARACTER_COUNT {
            return Err(Error::Format(format!(
                "character slot must be 1..={CHARACTER_COUNT} (got {})",
                index + 1
            )));
        }
        field_set(&mut self.body, character_base(index), key, value)
    }

    /// All known fields of a character as `(section, label, value)` tuples.
    pub fn character_inspect(&self, index: usize) -> Vec<(&'static str, &'static str, String)> {
        let base = character_base(index);
        CHARACTER_FIELDS
            .iter()
            .map(|f| {
                (
                    f.section.unwrap_or_default(),
                    f.label,
                    schema::read_field(&self.body, base, f),
                )
            })
            .collect()
    }

    /// The keys of all known character fields.
    pub fn character_field_keys() -> impl Iterator<Item = &'static str> {
        CHARACTER_FIELDS.iter().map(|f| f.key)
    }

    /// A character's learned skills, in stored order.
    pub fn skills(&self, index: usize) -> Vec<CharacterSkill> {
        if index >= CHARACTER_COUNT {
            return Vec::new();
        }
        let base = character_base(index) + SKILL_BASE;
        (0..SKILL_SLOTS)
            .filter_map(|s| {
                let id = self.body[base + s * 2];
                (id != 0).then(|| CharacterSkill {
                    id,
                    name: skill_name(id).unwrap_or("Unknown"),
                    level: self.body[base + s * 2 + 1],
                })
            })
            .collect()
    }

    /// The level of a character's skill by selector (name or id), or `None` for an unknown
    /// selector. Returns `Some(0)` for a known skill the character hasn't learned.
    pub fn skill_get(&self, index: usize, selector: &str) -> Option<u8> {
        if index >= CHARACTER_COUNT {
            return None;
        }
        let id = skill_id(selector)?;
        let base = character_base(index) + SKILL_BASE;
        let level = (0..SKILL_SLOTS)
            .find(|&s| self.body[base + s * 2] == id)
            .map_or(0, |s| self.body[base + s * 2 + 1]);
        Some(level)
    }

    /// Set the level of a character's skill by selector (name or id). Updates the skill if
    /// the character already has it, otherwise adds it to a free slot. `level` must be
    /// `1..=255`; removing skills is not supported (it would disturb the on-disk order).
    pub fn skill_set(&mut self, index: usize, selector: &str, level: u8) -> Result<()> {
        if index >= CHARACTER_COUNT {
            return Err(Error::Format(format!(
                "character slot must be 1..={CHARACTER_COUNT} (got {})",
                index + 1
            )));
        }
        if level == 0 {
            return Err(Error::Format("skill level must be at least 1".into()));
        }
        let id = skill_id(selector)
            .ok_or_else(|| Error::Format(format!("unknown skill '{selector}'")))?;
        let base = character_base(index) + SKILL_BASE;
        // Update the skill in place if the character already has it.
        if let Some(s) = (0..SKILL_SLOTS).find(|&s| self.body[base + s * 2] == id) {
            self.body[base + s * 2 + 1] = level;
            return Ok(());
        }
        // Otherwise append it to the first free slot.
        if let Some(s) = (0..SKILL_SLOTS).find(|&s| self.body[base + s * 2] == 0) {
            self.body[base + s * 2] = id;
            self.body[base + s * 2 + 1] = level;
            return Ok(());
        }
        Err(Error::Format(format!(
            "character already has the maximum {SKILL_SLOTS} skills"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decrypts_rotating_xor() {
        // Verified against a real GAME1 save: seed bytes bf f0 decrypt the leading
        // ciphertext to a run of 0xBB (the save game's initial fill).
        let block = [
            b'm', b's', b'q', b'0', 0xbf, 0xf0, 0xf4, 0xd5, 0x36, 0x17, 0x70, 0x51,
        ];
        let out = decrypt(&block).unwrap();
        assert_eq!(out, vec![0xbb; 6]);
    }

    #[test]
    fn rejects_non_msq_block() {
        let block = [b'x', b'y', b'z', b'0', 0x00, 0x00, 0x11];
        assert!(decrypt(&block).is_err());
    }

    #[test]
    fn rejects_short_block() {
        assert!(decrypt(b"msq0").is_err());
    }

    #[test]
    fn encrypt_round_trips_through_decrypt() {
        let body: Vec<u8> = (0..SAVEGAME_BODY_LEN).map(|i| (i * 7 + 3) as u8).collect();
        let block = encrypt(&body, 0);
        assert_eq!(&block[0..4], b"msq0");
        assert_eq!(block.len(), SAVEGAME_BLOCK_LEN);
        assert_eq!(decrypt(&block).unwrap(), body);
    }

    #[test]
    fn block_checksum_matches_the_game() {
        // No 16-bit overflow: a plain negated sum. sum(1,2,3)=6 -> -6.
        assert_eq!(block_checksum(&[1, 2, 3]), 0xFFFA);
        // Overflows 16 bits, exercising the carry fold (golden value from a real GAME1).
        assert_eq!(block_checksum(&[0xFF; 300]), 0xD42C);
    }

    /// Build a synthetic GAME1 with one occupied character in slot 0.
    fn synthetic() -> Vec<u8> {
        let mut body = vec![0u8; SAVEGAME_BODY_LEN];
        let base = CHARACTER_BASE;
        body[base..base + 4].copy_from_slice(b"Ace\0"); // name (null-terminated)
        body[base + 0x0E] = 24; // strength
        body[base + 0x0F] = 18; // iq
        body[base + 0x18] = 1; // gender = Female
        body[base + 0x19] = 2; // nationality = Mexican
                               // money (3-byte LE) = 0x0186A0 = 100000
        body[base + 0x15] = 0xA0;
        body[base + 0x16] = 0x86;
        body[base + 0x17] = 0x01;
        body[base + 0x1B] = 20; // max_con lo
        body[base + 0x1D] = 15; // con lo
        body[base + 0x24] = 5; // level
                               // experience (3-byte LE) = 0x0003E8 = 1000
        body[base + 0x21] = 0xE8;
        body[base + 0x22] = 0x03;
        body[base + 0x32..base + 0x32 + 7].copy_from_slice(b"Captain"); // rank
                                                                        // Skills (id, level) packed from 0x80: Perception 2, Medic 1.
        body[base + 0x80] = 9;
        body[base + 0x81] = 2;
        body[base + 0x82] = 25;
        body[base + 0x83] = 1;

        // Wrap the body in a savegame block, then append a second (untouched) block that a
        // real GAME1 would carry, to prove we preserve it.
        let mut raw = encrypt(&body, 0);
        raw.extend_from_slice(b"msq0");
        raw.extend(std::iter::repeat_n(0u8, 32));
        raw
    }

    #[test]
    fn parses_and_edits_a_character() {
        let mut save = WastelandSave::from_bytes(synthetic()).unwrap();
        assert_eq!(save.occupied_characters(), vec![0]);
        assert_eq!(save.character_get(0, "name").as_deref(), Some("Ace"));
        assert_eq!(save.character_get(0, "strength").as_deref(), Some("24"));
        assert_eq!(save.character_get(0, "gender").as_deref(), Some("Female"));
        assert_eq!(
            save.character_get(0, "nationality").as_deref(),
            Some("Mexican")
        );
        assert_eq!(save.character_get(0, "money").as_deref(), Some("100000"));
        assert_eq!(save.character_get(0, "con").as_deref(), Some("15"));
        assert_eq!(save.character_get(0, "experience").as_deref(), Some("1000"));
        assert_eq!(save.character_get(0, "rank").as_deref(), Some("Captain"));

        save.character_set(0, "strength", "30").unwrap();
        save.character_set(0, "name", "Angela").unwrap();
        assert_eq!(save.character_get(0, "strength").as_deref(), Some("30"));
        assert_eq!(save.character_get(0, "name").as_deref(), Some("Angela"));
    }

    #[test]
    fn reads_and_edits_skills() {
        let mut save = WastelandSave::from_bytes(synthetic()).unwrap();

        let skills = save.skills(0);
        let listed: Vec<(&str, u8)> = skills.iter().map(|s| (s.name, s.level)).collect();
        assert_eq!(listed, vec![("Perception", 2), ("Medic", 1)]);

        // Read by name (case/space-insensitive) and by id; unlearned skill reads 0.
        assert_eq!(save.skill_get(0, "perception"), Some(2));
        assert_eq!(save.skill_get(0, "25"), Some(1)); // Medic by id
        assert_eq!(save.skill_get(0, "Clip Pistol"), Some(0));
        assert_eq!(save.skill_get(0, "not a skill"), None);

        // Update an existing skill, and add a new one to a free slot.
        save.skill_set(0, "medic", 4).unwrap();
        save.skill_set(0, "Clip Pistol", 3).unwrap();
        assert_eq!(save.skill_get(0, "medic"), Some(4));
        assert_eq!(save.skill_get(0, "clippistol"), Some(3));
        assert_eq!(save.skills(0).len(), 3);

        // Level 0 and unknown skills are rejected.
        assert!(save.skill_set(0, "medic", 0).is_err());
        assert!(save.skill_set(0, "nope", 1).is_err());
    }

    #[test]
    fn write_preserves_trailing_blocks_and_round_trips() {
        let original = synthetic();
        let mut save = WastelandSave::from_bytes(original.clone()).unwrap();
        save.character_set(0, "level", "9").unwrap();
        let written = save.to_bytes();

        // The trailing (non-savegame) bytes are untouched.
        assert_eq!(
            &written[SAVEGAME_BLOCK_LEN..],
            &original[SAVEGAME_BLOCK_LEN..]
        );
        // Re-reading the written bytes preserves the edit.
        let reread = WastelandSave::from_bytes(written).unwrap();
        assert_eq!(reread.character_get(0, "level").as_deref(), Some("9"));
    }

    #[test]
    fn rejects_non_game1() {
        let mut raw = vec![0u8; SAVEGAME_BLOCK_LEN];
        raw[0..4].copy_from_slice(b"msq1");
        assert!(WastelandSave::from_bytes(raw).is_err());
    }

    #[test]
    fn finds_savegame_after_a_leading_map_block() {
        // A leading, non-savegame block (size != 4614) that the scanner must skip.
        let leading = encrypt(&[0u8; 100], 0);
        let mut raw = leading.clone();
        raw.extend_from_slice(&synthetic());

        let save = WastelandSave::from_bytes(raw).unwrap();
        assert_eq!(save.character_get(0, "name").as_deref(), Some("Ace"));
        // The leading block is preserved byte-for-byte on write.
        let written = save.to_bytes();
        assert_eq!(&written[..leading.len()], &leading[..]);
    }
}
