//! Generic field-schema engine (Phase 3).
//!
//! The three hardcoded Ultima games all describe a save as *fields at fixed offsets over a
//! byte buffer*, differing only in the primitive encodings (little- vs big-endian integers
//! and BCD, numeric vs ASCII-letter enums, bitfields). This module factors that common
//! shape out so a game can be expressed as **data** — a table of [`Field`]s — while the
//! container/codec concerns stay in per-game code.
//!
//! Everything operates on a `&[u8]` buffer at a `base` offset, so a single record (Ultima
//! I/II), an array of records (Ultima III's roster), and a header-plus-members file (Ultima
//! III's party) all reuse the same [`Record`]. Reads never fail (they format whatever is
//! there); writes validate first and mutate only the field's own bytes, so unknown bytes
//! are preserved by construction.

use crate::{Error, Result};

/// Byte order for multi-byte numbers (both plain binary and BCD).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Endian {
    Little,
    Big,
}

/// A lookup table mapping a stored key to a display name.
///
/// For [`FieldKind::Enum`] the key is the numeric value; for [`FieldKind::Letter`] the key
/// is the ASCII byte of the letter (e.g. `(b'H' as u32, "Human")`).
pub type Variants = &'static [(u32, &'static str)];

/// How to interpret and edit a field's bytes.
#[derive(Clone, Copy)]
pub enum FieldKind {
    /// Null-terminated ASCII name occupying `len` bytes.
    Name { len: usize },
    /// A plain binary integer of `bytes` width, with an inclusive maximum for edits.
    Int {
        bytes: usize,
        endian: Endian,
        max: u32,
    },
    /// Binary-coded decimal of `bytes` width (each nibble is a decimal digit).
    Bcd { bytes: usize, endian: Endian },
    /// A raw single byte (0..=255).
    Byte,
    /// A boolean stored as `0x00` (false) / `0xFF` (true).
    Bool,
    /// A numeric value of `bytes` width mapped to named variants.
    Enum {
        bytes: usize,
        endian: Endian,
        variants: Variants,
    },
    /// A single ASCII letter mapped to named variants (variant keys are letter bytes).
    Letter { variants: Variants },
    /// Up to eight named bit flags packed into one byte (bit 0 = first flag).
    Bitfield { flags: &'static [&'static str] },
}

/// A single known field: a stable key, a display label, a byte offset (relative to the
/// record base), how to read it, and optional grouping/confidence metadata.
#[derive(Clone, Copy)]
pub struct Field {
    pub key: &'static str,
    pub label: &'static str,
    pub offset: usize,
    pub kind: FieldKind,
    pub section: Option<&'static str>,
    pub tentative: bool,
}

impl Field {
    /// A field with no section and confirmed (non-tentative) status.
    pub const fn new(
        key: &'static str,
        label: &'static str,
        offset: usize,
        kind: FieldKind,
    ) -> Self {
        Field {
            key,
            label,
            offset,
            kind,
            section: None,
            tentative: false,
        }
    }

    /// Assign this field to a display section (for grouped `inspect` output).
    pub const fn in_section(mut self, section: &'static str) -> Self {
        self.section = Some(section);
        self
    }

    /// Mark this field as tentative (mapped but not yet confirmed in-game).
    pub const fn tentative(mut self) -> Self {
        self.tentative = true;
        self
    }
}

/// A formatted field, ready for display.
pub struct FieldView {
    pub section: Option<&'static str>,
    pub label: &'static str,
    pub value: String,
    pub tentative: bool,
}

/// A record layout: a table of fields plus the record's byte length. Games hold one of
/// these and apply it at whatever `base` offset a given record lives.
pub struct Record {
    pub fields: &'static [Field],
    pub len: usize,
}

impl Record {
    /// The field with the given key, if known.
    pub fn field(&self, key: &str) -> Option<&Field> {
        self.fields.iter().find(|f| f.key == key)
    }

    /// The keys of every field in this record.
    pub fn keys(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.fields.iter().map(|f| f.key)
    }

    /// Format one field by key, reading from `buf` at `base`.
    pub fn get(&self, buf: &[u8], base: usize, key: &str) -> Option<String> {
        self.field(key).map(|f| read_field(buf, base, f))
    }

    /// Format every field, in table order.
    pub fn inspect(&self, buf: &[u8], base: usize) -> Vec<FieldView> {
        self.fields
            .iter()
            .map(|f| FieldView {
                section: f.section,
                label: f.label,
                value: read_field(buf, base, f),
                tentative: f.tentative,
            })
            .collect()
    }

    /// Validate and write one field by key into `buf` at `base`.
    pub fn set(&self, buf: &mut [u8], base: usize, key: &str, value: &str) -> Result<()> {
        let field = self
            .field(key)
            .ok_or_else(|| Error::Format(format!("unknown field '{key}'")))?;
        write_field(buf, base, field, value)
    }
}

/// Format a single field to a display string. Reads never fail.
pub fn read_field(buf: &[u8], base: usize, field: &Field) -> String {
    let at = base + field.offset;
    match field.kind {
        FieldKind::Name { len } => read_name(buf, at, len),
        FieldKind::Int { bytes, endian, .. } => read_int(buf, at, bytes, endian).to_string(),
        FieldKind::Bcd { bytes, endian } => read_bcd(buf, at, bytes, endian).to_string(),
        FieldKind::Byte => buf[at].to_string(),
        FieldKind::Bool => match buf[at] {
            0x00 => "no".to_string(),
            0xFF => "yes".to_string(),
            other => format!("0x{other:02X}"),
        },
        FieldKind::Enum {
            bytes,
            endian,
            variants,
        } => {
            let key = read_int(buf, at, bytes, endian);
            match variant_name(variants, key) {
                Some(name) => name.to_string(),
                None => format!("Unknown ({key})"),
            }
        }
        FieldKind::Letter { variants } => {
            let byte = buf[at];
            match variant_name(variants, byte as u32) {
                Some(name) => name.to_string(),
                None if byte.is_ascii_graphic() => format!("Unknown ('{}')", byte as char),
                None => format!("Unknown (0x{byte:02X})"),
            }
        }
        FieldKind::Bitfield { flags } => read_bitfield(buf, at, flags),
    }
}

/// Validate `value` and write it into `buf` at `base + field.offset`, touching only this
/// field's bytes.
pub fn write_field(buf: &mut [u8], base: usize, field: &Field, value: &str) -> Result<()> {
    let at = base + field.offset;
    match field.kind {
        FieldKind::Name { len } => write_name(buf, at, len, value, field.label)?,
        FieldKind::Int { bytes, endian, max } => {
            let n = parse_number(value, field.label)?;
            if n > max {
                return Err(range_error(field.label, max, n));
            }
            write_int(buf, at, bytes, endian, n);
        }
        FieldKind::Bcd { bytes, endian } => {
            let max = 10u32.pow(2 * bytes as u32) - 1;
            let n = parse_number(value, field.label)?;
            if n > max {
                return Err(range_error(field.label, max, n));
            }
            write_bcd(buf, at, bytes, endian, n);
        }
        FieldKind::Byte => {
            let n = parse_number(value, field.label)?;
            if n > u8::MAX as u32 {
                return Err(range_error(field.label, u8::MAX as u32, n));
            }
            buf[at] = n as u8;
        }
        FieldKind::Bool => {
            buf[at] = if parse_bool(value, field.label)? {
                0xFF
            } else {
                0x00
            }
        }
        FieldKind::Enum {
            bytes,
            endian,
            variants,
        } => {
            let key = parse_variant(variants, value)
                .ok_or_else(|| variant_error(field.label, variants, value))?;
            write_int(buf, at, bytes, endian, key);
        }
        FieldKind::Letter { variants } => {
            let key = parse_variant(variants, value)
                .ok_or_else(|| variant_error(field.label, variants, value))?;
            buf[at] = key as u8;
        }
        FieldKind::Bitfield { .. } => {
            let n = parse_number(value, field.label)?;
            if n > u8::MAX as u32 {
                return Err(range_error(field.label, u8::MAX as u32, n));
            }
            buf[at] = n as u8;
        }
    }
    Ok(())
}

// --- primitive readers/writers -------------------------------------------------------

fn read_int(buf: &[u8], off: usize, bytes: usize, endian: Endian) -> u32 {
    let mut value = 0u32;
    match endian {
        Endian::Little => {
            for i in (0..bytes).rev() {
                value = (value << 8) | buf[off + i] as u32;
            }
        }
        Endian::Big => {
            for i in 0..bytes {
                value = (value << 8) | buf[off + i] as u32;
            }
        }
    }
    value
}

fn write_int(buf: &mut [u8], off: usize, bytes: usize, endian: Endian, value: u32) {
    for i in 0..bytes {
        let byte = (value >> (8 * i)) as u8;
        match endian {
            Endian::Little => buf[off + i] = byte,
            Endian::Big => buf[off + bytes - 1 - i] = byte,
        }
    }
}

fn read_bcd(buf: &[u8], off: usize, bytes: usize, endian: Endian) -> u32 {
    let mut value = 0u32;
    let mut push = |b: u8| value = value * 100 + (b >> 4) as u32 * 10 + (b & 0x0F) as u32;
    match endian {
        // Most-significant digit-pair at the lowest address.
        Endian::Big => (0..bytes).for_each(|i| push(buf[off + i])),
        // Least-significant digit-pair at the lowest address.
        Endian::Little => (0..bytes).rev().for_each(|i| push(buf[off + i])),
    }
    value
}

fn write_bcd(buf: &mut [u8], off: usize, bytes: usize, endian: Endian, mut value: u32) {
    let mut place = |i: usize, v: &mut u32| {
        let pair = (*v % 100) as u8;
        buf[off + i] = ((pair / 10) << 4) | (pair % 10);
        *v /= 100;
    };
    match endian {
        Endian::Big => (0..bytes).rev().for_each(|i| place(i, &mut value)),
        Endian::Little => (0..bytes).for_each(|i| place(i, &mut value)),
    }
}

fn read_name(buf: &[u8], off: usize, len: usize) -> String {
    let raw = &buf[off..off + len];
    let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
    String::from_utf8_lossy(&raw[..end]).into_owned()
}

fn write_name(buf: &mut [u8], off: usize, len: usize, name: &str, label: &str) -> Result<()> {
    if !name.is_ascii() {
        return Err(Error::Format(format!("{label} must be ASCII")));
    }
    let max = len - 1;
    if name.len() > max {
        return Err(Error::Format(format!(
            "{label} must be at most {max} characters (got {})",
            name.len()
        )));
    }
    buf[off..off + len].fill(0);
    buf[off..off + name.len()].copy_from_slice(name.as_bytes());
    Ok(())
}

fn read_bitfield(buf: &[u8], off: usize, flags: &[&str]) -> String {
    let byte = buf[off];
    let active: Vec<_> = flags
        .iter()
        .enumerate()
        .filter(|(i, _)| byte & (1 << i) != 0)
        .map(|(_, name)| *name)
        .collect();
    if active.is_empty() {
        "(none)".to_string()
    } else {
        active.join(", ")
    }
}

// --- parsing helpers -----------------------------------------------------------------

fn variant_name(variants: Variants, key: u32) -> Option<&'static str> {
    variants
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, name)| *name)
}

/// Resolve an enum/letter input: a variant name (case-insensitive), a single letter that
/// matches a letter-keyed variant, or a numeric key that names a known variant.
fn parse_variant(variants: Variants, value: &str) -> Option<u32> {
    let value = value.trim();
    if let Some((key, _)) = variants
        .iter()
        .find(|(_, name)| name.eq_ignore_ascii_case(value))
    {
        return Some(*key);
    }
    if value.len() == 1 {
        let c = value.as_bytes()[0].to_ascii_uppercase() as u32;
        if variants.iter().any(|(k, _)| *k == c) {
            return Some(c);
        }
    }
    let n: u32 = value.parse().ok()?;
    variants.iter().any(|(k, _)| *k == n).then_some(n)
}

fn parse_number(value: &str, label: &str) -> Result<u32> {
    value
        .trim()
        .parse()
        .map_err(|_| Error::Format(format!("{label} must be a number (got '{value}')")))
}

fn parse_bool(value: &str, label: &str) -> Result<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "yes" | "true" | "y" | "1" | "on" => Ok(true),
        "no" | "false" | "n" | "0" | "off" => Ok(false),
        _ => Err(Error::Format(format!(
            "{label} must be yes/no (got '{value}')"
        ))),
    }
}

fn range_error(label: &str, max: u32, got: u32) -> Error {
    Error::Format(format!("{label} must be between 0 and {max} (got {got})"))
}

fn variant_error(label: &str, variants: Variants, value: &str) -> Error {
    let options: Vec<_> = variants.iter().map(|(_, name)| *name).collect();
    Error::Format(format!(
        "'{value}' is not a valid {label}. Options: {}",
        options.join(", ")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    const RACE: Variants = &[(b'H' as u32, "Human"), (b'E' as u32, "Elf")];
    const TRANSPORT: Variants = &[(0, "Walking"), (5, "Aircar")];
    const CLASS_NUM: Variants = &[(0, "Fighter"), (2, "Wizard")];
    const MARKS: &[&str] = &["Love", "Sol", "Moon", "Death"];

    #[test]
    fn int_little_endian_round_trips() {
        // Ultima I stores a u16 LE, e.g. 525 = 0D 02.
        let mut buf = vec![0u8; 4];
        write_int(&mut buf, 0, 2, Endian::Little, 525);
        assert_eq!(&buf[0..2], &[0x0D, 0x02]);
        assert_eq!(read_int(&buf, 0, 2, Endian::Little), 525);
    }

    #[test]
    fn bcd_big_endian_matches_ultima2() {
        // Ultima II gold 3000 = 30 00 (big-endian BCD).
        let mut buf = vec![0u8; 2];
        write_bcd(&mut buf, 0, 2, Endian::Big, 3000);
        assert_eq!(&buf, &[0x30, 0x00]);
        assert_eq!(read_bcd(&buf, 0, 2, Endian::Big), 3000);
    }

    #[test]
    fn bcd_little_endian_matches_ultima3() {
        // Ultima III HP 150 = 50 01 (little-endian BCD).
        let mut buf = vec![0u8; 2];
        write_bcd(&mut buf, 0, 2, Endian::Little, 150);
        assert_eq!(&buf, &[0x50, 0x01]);
        assert_eq!(read_bcd(&buf, 0, 2, Endian::Little), 150);
    }

    fn sample_record() -> Record {
        static FIELDS: &[Field] = &[
            Field::new("name", "Name", 0x00, FieldKind::Name { len: 4 }),
            Field::new(
                "transport",
                "Transport",
                0x04,
                FieldKind::Enum {
                    bytes: 2,
                    endian: Endian::Little,
                    variants: TRANSPORT,
                },
            ),
            Field::new("race", "Race", 0x06, FieldKind::Letter { variants: RACE }),
            Field::new("in_party", "In Party", 0x07, FieldKind::Bool),
            Field::new(
                "gold",
                "Gold",
                0x08,
                FieldKind::Bcd {
                    bytes: 2,
                    endian: Endian::Big,
                },
            ),
            Field::new("marks", "Marks", 0x0A, FieldKind::Bitfield { flags: MARKS }),
        ];
        Record {
            fields: FIELDS,
            len: 0x0B,
        }
    }

    #[test]
    fn record_reads_and_writes_all_kinds() {
        let rec = sample_record();
        let mut buf = vec![0u8; rec.len];

        rec.set(&mut buf, 0, "name", "ABE").unwrap();
        rec.set(&mut buf, 0, "transport", "Aircar").unwrap();
        rec.set(&mut buf, 0, "race", "Elf").unwrap();
        rec.set(&mut buf, 0, "in_party", "yes").unwrap();
        rec.set(&mut buf, 0, "gold", "1234").unwrap();
        rec.set(&mut buf, 0, "marks", "5").unwrap(); // Love + Moon

        assert_eq!(rec.get(&buf, 0, "name").unwrap(), "ABE");
        assert_eq!(rec.get(&buf, 0, "transport").unwrap(), "Aircar");
        assert_eq!(buf[0x04..0x06], [5, 0]); // enum stored little-endian
        assert_eq!(rec.get(&buf, 0, "race").unwrap(), "Elf");
        assert_eq!(buf[0x06], b'E');
        assert_eq!(rec.get(&buf, 0, "in_party").unwrap(), "yes");
        assert_eq!(buf[0x07], 0xFF);
        assert_eq!(rec.get(&buf, 0, "gold").unwrap(), "1234");
        assert_eq!(&buf[0x08..0x0A], &[0x12, 0x34]);
        assert_eq!(rec.get(&buf, 0, "marks").unwrap(), "Love, Moon");
    }

    #[test]
    fn set_touches_only_the_field_bytes() {
        let rec = sample_record();
        let mut buf = vec![0xAAu8; rec.len];
        rec.set(&mut buf, 0, "gold", "9999").unwrap();
        for (i, &b) in buf.iter().enumerate() {
            if i == 0x08 || i == 0x09 {
                continue;
            }
            assert_eq!(b, 0xAA, "byte {i:#04x} changed unexpectedly");
        }
    }

    #[test]
    fn record_array_uses_base_offset() {
        // Two records back-to-back (like an Ultima III roster).
        let rec = sample_record();
        let mut buf = vec![0u8; rec.len * 2];
        rec.set(&mut buf, 0, "name", "ONE").unwrap();
        rec.set(&mut buf, rec.len, "name", "TWO").unwrap();
        assert_eq!(rec.get(&buf, 0, "name").unwrap(), "ONE");
        assert_eq!(rec.get(&buf, rec.len, "name").unwrap(), "TWO");
    }

    #[test]
    fn enum_accepts_name_and_number() {
        static FIELDS: &[Field] = &[Field::new(
            "class",
            "Class",
            0,
            FieldKind::Enum {
                bytes: 1,
                endian: Endian::Little,
                variants: CLASS_NUM,
            },
        )];
        let rec = Record {
            fields: FIELDS,
            len: 1,
        };
        let mut buf = vec![0u8; 1];
        rec.set(&mut buf, 0, "class", "Wizard").unwrap();
        assert_eq!(buf[0], 2);
        rec.set(&mut buf, 0, "class", "0").unwrap();
        assert_eq!(rec.get(&buf, 0, "class").unwrap(), "Fighter");
        assert!(rec.set(&mut buf, 0, "class", "Ranger").is_err());
    }

    #[test]
    fn validation_rejects_out_of_range_and_bad_input() {
        static FIELDS: &[Field] = &[
            Field::new(
                "hp",
                "HP",
                0,
                FieldKind::Int {
                    bytes: 2,
                    endian: Endian::Little,
                    max: 999,
                },
            ),
            Field::new(
                "food",
                "Food",
                2,
                FieldKind::Bcd {
                    bytes: 1,
                    endian: Endian::Big,
                },
            ),
        ];
        let rec = Record {
            fields: FIELDS,
            len: 3,
        };
        let mut buf = vec![0u8; 3];
        assert!(rec.set(&mut buf, 0, "hp", "1000").is_err()); // over max
        assert!(rec.set(&mut buf, 0, "hp", "abc").is_err()); // not a number
        assert!(rec.set(&mut buf, 0, "food", "100").is_err()); // 1-byte BCD max is 99
        assert!(rec.set(&mut buf, 0, "missing", "1").is_err()); // unknown field
    }

    #[test]
    fn inspect_reports_sections_and_tentative() {
        static FIELDS: &[Field] = &[
            Field::new("str", "Strength", 0, FieldKind::Byte).in_section("Attributes"),
            Field::new("xp", "Experience", 1, FieldKind::Byte).tentative(),
        ];
        let rec = Record {
            fields: FIELDS,
            len: 2,
        };
        let buf = vec![7u8, 0u8];
        let views = rec.inspect(&buf, 0);
        assert_eq!(views[0].section, Some("Attributes"));
        assert_eq!(views[0].value, "7");
        assert!(!views[0].tentative);
        assert!(views[1].tentative);
    }
}
