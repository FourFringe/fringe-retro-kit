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

use crate::schema::{self, Endian, Field, FieldKind, Variants};
use crate::{Error, Result};

/// Total size of an Ultima II `PLAYER` save file, in bytes (`0x180`).
pub const SAVE_LEN: usize = 0x180;

/// Length of the name field (bytes `0x00..0x10`), including the null terminator.
const NAME_LEN: usize = 0x10;

type LetterTable = Variants;
type EnumTable = Variants;

const SEX: LetterTable = &[(b'M' as u32, "Male"), (b'F' as u32, "Female")];

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

/// An Ultima II big-endian BCD field of `bytes` bytes.
const fn bcd(bytes: usize) -> FieldKind {
    FieldKind::Bcd {
        bytes,
        endian: Endian::Big,
    }
}

/// An Ultima II single-byte numeric enum field.
const fn enum1(variants: Variants) -> FieldKind {
    FieldKind::Enum {
        bytes: 1,
        endian: Endian::Little,
        variants,
    }
}

/// An Ultima II ASCII-letter enum field.
const fn letter(variants: Variants) -> FieldKind {
    FieldKind::Letter { variants }
}

// Display sections, used to group the interactive editor's field list.
const S_CHARACTER: &str = "Character";
const S_ATTRIBUTES: &str = "Attributes";
const S_STATUS: &str = "Status";
const S_LOCATION: &str = "Location";
const S_EQUIPPED: &str = "Equipped";
const S_WEAPONS: &str = "Inventory: Weapons";
const S_ARMOUR: &str = "Inventory: Armour";

/// Fields mapped so far. Confirmed against a snapshot of HP 14, Food 289, XP 0, Gold 400.
#[rustfmt::skip]
const FIELDS: &[Field] = &[
    Field::new("name",  "Name",  0x00, FieldKind::Name { len: NAME_LEN }).in_section(S_CHARACTER),
    Field::new("sex",   "Sex",   0x10, letter(SEX)).in_section(S_CHARACTER),
    Field::new("class", "Class", 0x11, enum1(CLASS)).in_section(S_CHARACTER),
    Field::new("race",  "Race",  0x12, enum1(RACE)).in_section(S_CHARACTER),
    // Attributes: six 1-byte BCD values at 0x15..0x1A, stored *adjusted* (after race/class/gender
    // bonuses). Order and encoding confirmed with the FRINGE character (all-distinct values).
    Field::new("strength",     "Strength",     0x15, bcd(1)).in_section(S_ATTRIBUTES),
    Field::new("agility",      "Agility",      0x16, bcd(1)).in_section(S_ATTRIBUTES),
    Field::new("stamina",      "Stamina",      0x17, bcd(1)).in_section(S_ATTRIBUTES),
    Field::new("charisma",     "Charisma",     0x18, bcd(1)).in_section(S_ATTRIBUTES),
    Field::new("wisdom",       "Wisdom",       0x19, bcd(1)).in_section(S_ATTRIBUTES),
    Field::new("intelligence", "Intelligence", 0x1A, bcd(1)).in_section(S_ATTRIBUTES),
    Field::new("hits", "Hits", 0x1B, bcd(2)).in_section(S_STATUS),
    Field::new("food", "Food", 0x1D, bcd(2)).in_section(S_STATUS),
    Field::new("experience", "Experience", 0x20, bcd(2)).in_section(S_STATUS),
    Field::new("gold", "Gold", 0x22, bcd(2)).in_section(S_STATUS),
    // Map position (raw binary bytes), confirmed by moving left/right (X) and up/down (Y).
    Field::new("x", "Map X", 0x24, FieldKind::Byte).in_section(S_LOCATION),
    Field::new("y", "Map Y", 0x25, FieldKind::Byte).in_section(S_LOCATION),
    // Readied weapon (0x2B) and worn armour (0x2C), single index into the item order (0 = none).
    // Confirmed by Wield/Wear diffs (Dagger..Bow -> 1..4; Cloth/Leather -> 1/2).
    Field::new("weapon", "Weapon (readied)", 0x2B, enum1(WEAPON)).in_section(S_EQUIPPED),
    Field::new("armor",  "Armour (worn)",    0x2C, enum1(ARMOR)).in_section(S_EQUIPPED),
    // Weapons owned (counts), 0x41..0x49 in weapon order (Dagger..Quicksword); array base 0x40 is
    // Hands. Dagger..Sword confirmed by purchase diffs (5/4/3/2/1). Encoding assumed 1-byte BCD to
    // match the rest of U2 — verify with a count >= 10 (10 would read as 0x10).
    Field::new("weapon_dagger",     "Daggers",      0x41, bcd(1)).in_section(S_WEAPONS).tentative(),
    Field::new("weapon_mace",       "Maces",        0x42, bcd(1)).in_section(S_WEAPONS).tentative(),
    Field::new("weapon_axe",        "Axes",         0x43, bcd(1)).in_section(S_WEAPONS).tentative(),
    Field::new("weapon_bow",        "Bows",         0x44, bcd(1)).in_section(S_WEAPONS).tentative(),
    Field::new("weapon_sword",      "Swords",       0x45, bcd(1)).in_section(S_WEAPONS).tentative(),
    Field::new("weapon_greatsword", "Great swords", 0x46, bcd(1)).in_section(S_WEAPONS).tentative(),
    Field::new("weapon_lightsword", "Light swords", 0x47, bcd(1)).in_section(S_WEAPONS).tentative(),
    Field::new("weapon_phaser",     "Phasers",      0x48, bcd(1)).in_section(S_WEAPONS).tentative(),
    Field::new("weapon_quicksword", "Quickswords",  0x49, bcd(1)).in_section(S_WEAPONS).tentative(),
    // Armour owned (counts), 0x61..0x66 in armour order (Cloth..Power); array base 0x60 is Skin.
    // Cloth..Plate confirmed by purchase diffs. Same BCD-vs-binary caveat as weapons.
    Field::new("armor_cloth",   "Cloth armour",   0x61, bcd(1)).in_section(S_ARMOUR).tentative(),
    Field::new("armor_leather", "Leather armour", 0x62, bcd(1)).in_section(S_ARMOUR).tentative(),
    Field::new("armor_chain",   "Chain armour",   0x63, bcd(1)).in_section(S_ARMOUR).tentative(),
    Field::new("armor_plate",   "Plate armour",   0x64, bcd(1)).in_section(S_ARMOUR).tentative(),
    Field::new("armor_reflect", "Reflect armour", 0x65, bcd(1)).in_section(S_ARMOUR).tentative(),
    Field::new("armor_power",   "Power armour",   0x66, bcd(1)).in_section(S_ARMOUR).tentative(),
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
        Some(schema::read_field(&self.bytes, 0, field))
    }

    /// All known fields as `(section, label, value, tentative)` tuples.
    pub fn inspect(&self) -> Vec<(&'static str, &'static str, String, bool)> {
        FIELDS
            .iter()
            .map(|f| {
                (
                    f.section.unwrap_or_default(),
                    f.label,
                    schema::read_field(&self.bytes, 0, f),
                    f.tentative,
                )
            })
            .collect()
    }

    /// The keys of all known fields.
    pub fn field_keys() -> impl Iterator<Item = &'static str> {
        FIELDS.iter().map(|f| f.key)
    }

    /// The schema field table (key, label, kind) for building editors.
    pub fn fields() -> &'static [Field] {
        FIELDS
    }

    /// Set a field of the character by key, validating the value first.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_field_has_a_section() {
        assert!(FIELDS.iter().all(|f| f.section.is_some()));
    }

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
