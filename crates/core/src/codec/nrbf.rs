//! MS-NRBF (.NET **BinaryFormatter**) reader.
//!
//! Some modern remasters of classic RPGs (built in Unity/Mono) serialise their saves with .NET's
//! `BinaryFormatter`, whose on-the-wire shape is the **[MS-NRBF]** ".NET Remoting Binary Format".
//! Unlike the fixed byte layouts of the DOS-era games, an NRBF stream is **self-describing**: every
//! object records its class name, its member names, and their types inline. That makes a save fully
//! navigable by name — no offset hunting.
//!
//! This reader parses a stream (or several concatenated streams — the games often write a small
//! header stream followed by the game-state stream) into an object graph addressed by object id.
//! It also remembers the byte offset of every inline **primitive** member, so an integer field
//! (a stat, gold, experience) can be **patched in place** without re-serialising — the edit keeps
//! the file byte-for-byte identical except for the one value, which side-steps `BinaryFormatter`'s
//! finicky re-encoding entirely.
//!
//! Only the subset of [MS-NRBF] that appears in these saves is implemented. Note that *parsing*
//! the structure here is safe; this never *deserialises* into live objects (the vector behind
//! `BinaryFormatter`'s insecurity), so untrusted saves can be inspected without risk.

use std::collections::BTreeMap;

use crate::{Error, Result};

/// Build an [`Error::Format`] with a formatted message.
macro_rules! err {
    ($($a:tt)*) => {
        Error::Format(format!($($a)*))
    };
}

/// Record type tags, a subset of the [MS-NRBF] `RecordTypeEnumeration`.
mod rec {
    pub const HEADER: u8 = 0;
    pub const CLASS_WITH_ID: u8 = 1;
    pub const SYSTEM_CLASS_WITH_MEMBERS_AND_TYPES: u8 = 4;
    pub const CLASS_WITH_MEMBERS_AND_TYPES: u8 = 5;
    pub const BINARY_OBJECT_STRING: u8 = 6;
    pub const BINARY_ARRAY: u8 = 7;
    pub const MEMBER_PRIMITIVE_TYPED: u8 = 8;
    pub const MEMBER_REFERENCE: u8 = 9;
    pub const OBJECT_NULL: u8 = 10;
    pub const MESSAGE_END: u8 = 11;
    pub const BINARY_LIBRARY: u8 = 12;
    pub const OBJECT_NULL_MULTIPLE_256: u8 = 13;
    pub const OBJECT_NULL_MULTIPLE: u8 = 14;
    pub const ARRAY_SINGLE_PRIMITIVE: u8 = 15;
    pub const ARRAY_SINGLE_OBJECT: u8 = 16;
    pub const ARRAY_SINGLE_STRING: u8 = 17;
}

/// Binary member type tags ([MS-NRBF] `BinaryTypeEnumeration`). Only `PRIMITIVE` members are
/// written inline; every other kind is a nested record.
mod bt {
    pub const PRIMITIVE: u8 = 0;
    pub const SYSTEM_CLASS: u8 = 3;
    pub const CLASS: u8 = 4;
    pub const PRIMITIVE_ARRAY: u8 = 7;
}

/// A decoded member value. Objects, strings and arrays are stored by id in the [`Document`] and
/// referenced here as [`Value::Ref`]; resolve them with [`Document::object`] / [`Document::string`]
/// / [`Document::array`].
#[derive(Debug, Clone)]
pub enum Value {
    Null,
    Bool(bool),
    /// A signed integer of any width (also byte/char-code/DateTime ticks).
    Int(i64),
    /// An unsigned integer of any width.
    UInt(u64),
    Float(f64),
    /// An inline primitive string (rare; most strings are referenced objects).
    Str(String),
    /// A reference to an object, string or array by id.
    Ref(i32),
    // --- internal signals, never stored in a member or array ---
    #[doc(hidden)]
    End,
    #[doc(hidden)]
    Nulls(usize),
}

/// One member of an [`Object`]: its name, decoded value, and — for inline primitives — where its
/// bytes live so the value can be patched in place.
#[derive(Debug, Clone)]
pub struct Member {
    pub name: String,
    pub value: Value,
    scalar: Option<Scalar>,
}

#[derive(Debug, Clone, Copy)]
struct Scalar {
    offset: usize,
    prim: u8,
}

/// A decoded object: its id, .NET class name (e.g. `BardsTale.Character`), and ordered members.
#[derive(Debug, Clone)]
pub struct Object {
    pub id: i32,
    pub class: String,
    pub members: Vec<Member>,
}

impl Object {
    /// The named member, if present.
    pub fn member(&self, name: &str) -> Option<&Member> {
        self.members.iter().find(|m| m.name == name)
    }

    /// The named member as an integer, if it is one.
    pub fn int(&self, name: &str) -> Option<i64> {
        match self.member(name)?.value {
            Value::Int(v) => Some(v),
            Value::UInt(v) => Some(v as i64),
            Value::Bool(b) => Some(b as i64),
            _ => None,
        }
    }
}

/// A class layout, kept so a later `ClassWithId` record can reuse it.
#[derive(Clone)]
struct Layout {
    class: String,
    names: Vec<String>,
    btypes: Vec<u8>,
    prims: Vec<u8>, // parallel to `btypes`; the primitive type where `btypes[i] == PRIMITIVE`
}

/// A parsed NRBF document: the object graph plus the original bytes, so scalars can be patched.
pub struct Document {
    data: Vec<u8>,
    objects: BTreeMap<i32, Object>,
    strings: BTreeMap<i32, String>,
    arrays: BTreeMap<i32, Vec<Value>>,
    roots: Vec<i32>,
}

impl Document {
    /// Parse one or more concatenated NRBF streams.
    pub fn parse(bytes: &[u8]) -> Result<Document> {
        Parser::new(bytes).run()
    }

    /// The (possibly patched) bytes, ready to write back to disk.
    pub fn bytes(&self) -> &[u8] {
        &self.data
    }

    /// The object with this id, if any.
    pub fn object(&self, id: i32) -> Option<&Object> {
        self.objects.get(&id)
    }

    /// The string with this id, if any.
    pub fn string(&self, id: i32) -> Option<&str> {
        self.strings.get(&id).map(String::as_str)
    }

    /// The array with this id, if any.
    pub fn array(&self, id: i32) -> Option<&[Value]> {
        self.arrays.get(&id).map(Vec::as_slice)
    }

    /// The root object id of each stream, in order.
    pub fn roots(&self) -> &[i32] {
        &self.roots
    }

    /// Every object, in id order.
    pub fn objects(&self) -> impl Iterator<Item = &Object> {
        self.objects.values()
    }

    /// Every object of a given .NET class (e.g. `BardsTale.Character`).
    pub fn objects_of_class<'a>(&'a self, class: &'a str) -> impl Iterator<Item = &'a Object> + 'a {
        self.objects.values().filter(move |o| o.class == class)
    }

    /// Resolve a member that holds a string (inline or by reference).
    pub fn member_str<'a>(&'a self, obj: &'a Object, name: &str) -> Option<&'a str> {
        match &obj.member(name)?.value {
            Value::Str(s) => Some(s),
            Value::Ref(id) => self.string(*id),
            _ => None,
        }
    }

    /// Patch an inline integer member in place, keeping the file the same size. Fails if the member
    /// isn't an inline integer scalar or the value doesn't fit its width.
    pub fn patch_int(&mut self, obj_id: i32, member: &str, value: i64) -> Result<()> {
        let (offset, prim) = {
            let obj = self
                .objects
                .get(&obj_id)
                .ok_or_else(|| err!("no object #{obj_id}"))?;
            let m = obj
                .member(member)
                .ok_or_else(|| err!("object #{obj_id} has no member `{member}`"))?;
            let sc = m
                .scalar
                .ok_or_else(|| err!("member `{member}` is not an inline scalar"))?;
            (sc.offset, sc.prim)
        };
        let width = int_width(prim).ok_or_else(|| err!("member `{member}` is not an integer"))?;
        if !int_fits(value, prim) {
            return Err(err!(
                "value {value} does not fit member `{member}` (a {width}-byte integer)"
            ));
        }
        self.data[offset..offset + width].copy_from_slice(&value.to_le_bytes()[..width]);
        if let Some(m) = self
            .objects
            .get_mut(&obj_id)
            .and_then(|o| o.members.iter_mut().find(|m| m.name == member))
        {
            m.value = if is_unsigned(prim) {
                Value::UInt(value as u64)
            } else {
                Value::Int(value)
            };
        }
        Ok(())
    }
}

/// Byte width of an integer primitive type, or `None` if it isn't a patchable integer.
fn int_width(prim: u8) -> Option<usize> {
    match prim {
        1 | 2 | 10 => Some(1),       // Bool, Byte, SByte
        7 | 14 => Some(2),           // Int16, UInt16
        8 | 15 => Some(4),           // Int32, UInt32
        9 | 16 | 12 | 13 => Some(8), // Int64, UInt64, TimeSpan, DateTime
        _ => None,
    }
}

fn is_unsigned(prim: u8) -> bool {
    matches!(prim, 2 | 14 | 15 | 16 | 1)
}

fn int_fits(value: i64, prim: u8) -> bool {
    match prim {
        1 => value == 0 || value == 1,
        2 => (0..=255).contains(&value),
        10 => (-128..=127).contains(&value),
        7 => (i16::MIN as i64..=i16::MAX as i64).contains(&value),
        14 => (0..=u16::MAX as i64).contains(&value),
        8 => (i32::MIN as i64..=i32::MAX as i64).contains(&value),
        15 => (0..=u32::MAX as i64).contains(&value),
        9 | 12 | 13 => true,
        16 => value >= 0,
        _ => false,
    }
}

struct Parser<'a> {
    d: &'a [u8],
    pos: usize,
    /// Object ids are scoped to a single stream; a file may hold several concatenated streams that
    /// reuse ids, so each stream after the first adds [`STREAM_STEP`] to keep ids globally unique.
    base: i32,
    seen_header: bool,
    objects: BTreeMap<i32, Object>,
    strings: BTreeMap<i32, String>,
    arrays: BTreeMap<i32, Vec<Value>>,
    layouts: BTreeMap<i32, Layout>,
    roots: Vec<i32>,
}

/// Per-stream id offset — larger than any object count we expect in one stream.
const STREAM_STEP: i32 = 1_000_000;

impl<'a> Parser<'a> {
    fn new(d: &'a [u8]) -> Self {
        Parser {
            d,
            pos: 0,
            base: 0,
            seen_header: false,
            objects: BTreeMap::new(),
            strings: BTreeMap::new(),
            arrays: BTreeMap::new(),
            layouts: BTreeMap::new(),
            roots: Vec::new(),
        }
    }

    fn run(mut self) -> Result<Document> {
        while self.pos < self.d.len() {
            match self.read_record()? {
                Value::End => continue,
                Value::Ref(id) => self.roots.push(id),
                _ => {}
            }
        }
        Ok(Document {
            data: self.d.to_vec(),
            objects: self.objects,
            strings: self.strings,
            arrays: self.arrays,
            roots: self.roots,
        })
    }

    // --- cursor primitives ---
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        if self.pos + n > self.d.len() {
            return Err(err!("unexpected end of NRBF stream"));
        }
        let s = &self.d[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    fn i32(&mut self) -> Result<i32> {
        Ok(i32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    /// Read an object id and scope it to the current stream.
    fn id(&mut self) -> Result<i32> {
        Ok(self.i32()? + self.base)
    }
    fn len7(&mut self) -> Result<usize> {
        let (mut out, mut shift) = (0usize, 0u32);
        loop {
            let b = self.u8()?;
            out |= ((b & 0x7f) as usize) << shift;
            if b & 0x80 == 0 {
                break;
            }
            shift += 7;
            if shift >= 35 {
                return Err(err!("over-long NRBF length prefix"));
            }
        }
        Ok(out)
    }
    fn string(&mut self) -> Result<String> {
        let n = self.len7()?;
        Ok(String::from_utf8_lossy(self.take(n)?).into_owned())
    }

    fn read_prim(&mut self, prim: u8) -> Result<Value> {
        Ok(match prim {
            1 => Value::Bool(self.u8()? != 0),
            2 => Value::Int(self.u8()? as i64),
            3 => {
                // Char: 1–4 UTF-8 bytes.
                let b0 = *self
                    .d
                    .get(self.pos)
                    .ok_or_else(|| err!("unexpected end reading Char"))?;
                let n = if b0 < 0x80 {
                    1
                } else if b0 < 0xE0 {
                    2
                } else if b0 < 0xF0 {
                    3
                } else {
                    4
                };
                Value::Str(String::from_utf8_lossy(self.take(n)?).into_owned())
            }
            5 => Value::Str(self.string()?), // Decimal (as string)
            6 => Value::Float(f64::from_le_bytes(self.take(8)?.try_into().unwrap())),
            7 => Value::Int(i16::from_le_bytes(self.take(2)?.try_into().unwrap()) as i64),
            8 => Value::Int(i32::from_le_bytes(self.take(4)?.try_into().unwrap()) as i64),
            9 => Value::Int(i64::from_le_bytes(self.take(8)?.try_into().unwrap())),
            10 => Value::Int(self.u8()? as i8 as i64),
            11 => Value::Float(f32::from_le_bytes(self.take(4)?.try_into().unwrap()) as f64),
            12 | 13 => Value::Int(i64::from_le_bytes(self.take(8)?.try_into().unwrap())),
            14 => Value::UInt(u16::from_le_bytes(self.take(2)?.try_into().unwrap()) as u64),
            15 => Value::UInt(u32::from_le_bytes(self.take(4)?.try_into().unwrap()) as u64),
            16 => Value::UInt(u64::from_le_bytes(self.take(8)?.try_into().unwrap())),
            18 => Value::Str(self.string()?),
            other => return Err(err!("unsupported NRBF primitive type {other}")),
        })
    }

    /// Read a class's member-type metadata: the per-member binary type, plus (for `PRIMITIVE`
    /// members) the primitive type needed to read the inline value.
    fn read_member_type_info(&mut self, count: usize) -> Result<(Vec<u8>, Vec<u8>)> {
        let btypes: Vec<u8> = (0..count).map(|_| self.u8()).collect::<Result<_>>()?;
        let mut prims = vec![0u8; count];
        for (i, &b) in btypes.iter().enumerate() {
            match b {
                bt::PRIMITIVE | bt::PRIMITIVE_ARRAY => prims[i] = self.u8()?,
                bt::SYSTEM_CLASS => {
                    self.string()?;
                }
                bt::CLASS => {
                    self.string()?;
                    self.i32()?;
                }
                _ => {}
            }
        }
        Ok((btypes, prims))
    }

    fn read_class_body(&mut self, id: i32, layout: Layout) -> Result<Value> {
        let mut members = Vec::with_capacity(layout.names.len());
        for i in 0..layout.names.len() {
            let (value, scalar) = if layout.btypes[i] == bt::PRIMITIVE {
                let offset = self.pos;
                let prim = layout.prims[i];
                (self.read_prim(prim)?, Some(Scalar { offset, prim }))
            } else {
                (self.read_record()?, None)
            };
            members.push(Member {
                name: layout.names[i].clone(),
                value,
                scalar,
            });
        }
        self.objects.insert(
            id,
            Object {
                id,
                class: layout.class,
                members,
            },
        );
        Ok(Value::Ref(id))
    }

    fn read_elements(&mut self, len: usize) -> Result<Vec<Value>> {
        let mut items = Vec::with_capacity(len);
        while items.len() < len {
            match self.read_record()? {
                Value::Nulls(n) => items.resize(items.len() + n, Value::Null),
                other => items.push(other),
            }
        }
        Ok(items)
    }

    fn read_record(&mut self) -> Result<Value> {
        let rt = self.u8()?;
        Ok(match rt {
            rec::HEADER => {
                if self.seen_header {
                    self.base += STREAM_STEP;
                } else {
                    self.seen_header = true;
                }
                self.take(16)?; // RootId, HeaderId, MajorVersion, MinorVersion
                self.read_record()?
            }
            rec::BINARY_LIBRARY => {
                self.i32()?;
                self.string()?;
                self.read_record()?
            }
            rec::CLASS_WITH_MEMBERS_AND_TYPES | rec::SYSTEM_CLASS_WITH_MEMBERS_AND_TYPES => {
                let id = self.id()?;
                let class = self.string()?;
                let count = self.i32()? as usize;
                let names: Vec<String> =
                    (0..count).map(|_| self.string()).collect::<Result<_>>()?;
                let (btypes, prims) = self.read_member_type_info(count)?;
                if rt == rec::CLASS_WITH_MEMBERS_AND_TYPES {
                    self.i32()?; // LibraryId
                }
                let layout = Layout {
                    class,
                    names,
                    btypes,
                    prims,
                };
                self.layouts.insert(id, layout.clone());
                self.read_class_body(id, layout)?
            }
            rec::CLASS_WITH_ID => {
                let id = self.id()?;
                let meta = self.id()?;
                let layout = self
                    .layouts
                    .get(&meta)
                    .ok_or_else(|| err!("ClassWithId references unknown layout #{meta}"))?
                    .clone();
                self.read_class_body(id, layout)?
            }
            rec::BINARY_OBJECT_STRING => {
                let id = self.id()?;
                let s = self.string()?;
                self.strings.insert(id, s);
                Value::Ref(id)
            }
            rec::MEMBER_REFERENCE => Value::Ref(self.id()?),
            rec::OBJECT_NULL => Value::Null,
            rec::OBJECT_NULL_MULTIPLE_256 => Value::Nulls(self.u8()? as usize),
            rec::OBJECT_NULL_MULTIPLE => Value::Nulls(self.i32()? as usize),
            rec::MEMBER_PRIMITIVE_TYPED => {
                let prim = self.u8()?;
                self.read_prim(prim)?
            }
            rec::ARRAY_SINGLE_PRIMITIVE => {
                let id = self.id()?;
                let len = self.i32()? as usize;
                let prim = self.u8()?;
                let items = (0..len)
                    .map(|_| self.read_prim(prim))
                    .collect::<Result<_>>()?;
                self.arrays.insert(id, items);
                Value::Ref(id)
            }
            rec::ARRAY_SINGLE_OBJECT | rec::ARRAY_SINGLE_STRING => {
                let id = self.id()?;
                let len = self.i32()? as usize;
                let items = self.read_elements(len)?;
                self.arrays.insert(id, items);
                Value::Ref(id)
            }
            rec::BINARY_ARRAY => {
                let id = self.id()?;
                let atype = self.u8()?;
                let rank = self.i32()? as usize;
                let lengths: Vec<usize> = (0..rank)
                    .map(|_| Ok(self.i32()? as usize))
                    .collect::<Result<_>>()?;
                if matches!(atype, 3..=5) {
                    for _ in 0..rank {
                        self.i32()?; // lower bounds
                    }
                }
                let elem_bt = self.u8()?;
                let elem_prim = match elem_bt {
                    bt::PRIMITIVE | bt::PRIMITIVE_ARRAY => self.u8()?,
                    bt::SYSTEM_CLASS => {
                        self.string()?;
                        0
                    }
                    bt::CLASS => {
                        self.string()?;
                        self.i32()?;
                        0
                    }
                    _ => 0,
                };
                let total: usize = lengths.iter().product();
                let mut items = Vec::with_capacity(total);
                while items.len() < total {
                    if elem_bt == bt::PRIMITIVE {
                        items.push(self.read_prim(elem_prim)?);
                    } else {
                        match self.read_record()? {
                            Value::Nulls(n) => items.resize(items.len() + n, Value::Null),
                            other => items.push(other),
                        }
                    }
                }
                self.arrays.insert(id, items);
                Value::Ref(id)
            }
            rec::MESSAGE_END => Value::End,
            other => {
                return Err(err!(
                    "unknown NRBF record type {other} at offset {}",
                    self.pos - 1
                ))
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builder for a length-prefixed string.
    fn lp(s: &str) -> Vec<u8> {
        let mut v = vec![s.len() as u8];
        v.extend_from_slice(s.as_bytes());
        v
    }

    /// A minimal single-stream document: a `SystemClassWithMembersAndTypes` (`0x04`, no library)
    /// named `Foo` with two Int32 members `a`, `b`, followed by `MessageEnd`.
    fn foo_stream(a: i32, b: i32) -> Vec<u8> {
        let mut v = vec![0]; // SerializationHeaderRecord
        v.extend_from_slice(&1i32.to_le_bytes()); // RootId
        v.extend_from_slice(&(-1i32).to_le_bytes()); // HeaderId
        v.extend_from_slice(&1i32.to_le_bytes()); // MajorVersion
        v.extend_from_slice(&0i32.to_le_bytes()); // MinorVersion
        v.push(4); // SystemClassWithMembersAndTypes
        v.extend_from_slice(&1i32.to_le_bytes()); // ObjectId
        v.extend(lp("Foo"));
        v.extend_from_slice(&2i32.to_le_bytes()); // member count
        v.extend(lp("a"));
        v.extend(lp("b"));
        v.extend_from_slice(&[bt::PRIMITIVE, bt::PRIMITIVE]); // binary types
        v.extend_from_slice(&[8, 8]); // Int32, Int32
        v.extend_from_slice(&a.to_le_bytes());
        v.extend_from_slice(&b.to_le_bytes());
        v.push(11); // MessageEnd
        v
    }

    #[test]
    fn reads_class_members() {
        let doc = Document::parse(&foo_stream(7, 42)).unwrap();
        assert_eq!(doc.roots(), &[1]);
        let foo = doc.object(1).unwrap();
        assert_eq!(foo.class, "Foo");
        assert_eq!(foo.int("a"), Some(7));
        assert_eq!(foo.int("b"), Some(42));
    }

    #[test]
    fn patches_a_scalar_in_place() {
        let original = foo_stream(7, 42);
        let mut doc = Document::parse(&original).unwrap();
        doc.patch_int(1, "a", 99).unwrap();
        // Same size, and only the one field changed on re-parse.
        assert_eq!(doc.bytes().len(), original.len());
        let reparsed = Document::parse(doc.bytes()).unwrap();
        assert_eq!(reparsed.object(1).unwrap().int("a"), Some(99));
        assert_eq!(reparsed.object(1).unwrap().int("b"), Some(42));
    }

    #[test]
    fn rejects_out_of_range_and_missing() {
        let mut doc = Document::parse(&foo_stream(1, 2)).unwrap();
        // 4-byte Int32 can't hold 1<<40.
        assert!(doc.patch_int(1, "a", 1 << 40).is_err());
        assert!(doc.patch_int(1, "missing", 1).is_err());
        assert!(doc.patch_int(999, "a", 1).is_err());
    }

    #[test]
    fn resolves_a_string_member() {
        // Class `Bar` with one String member `s` whose value is a BinaryObjectString.
        let mut v = vec![0];
        v.extend_from_slice(&1i32.to_le_bytes());
        v.extend_from_slice(&(-1i32).to_le_bytes());
        v.extend_from_slice(&1i32.to_le_bytes());
        v.extend_from_slice(&0i32.to_le_bytes());
        v.push(4); // SystemClassWithMembersAndTypes
        v.extend_from_slice(&1i32.to_le_bytes());
        v.extend(lp("Bar"));
        v.extend_from_slice(&1i32.to_le_bytes()); // one member
        v.extend(lp("s"));
        v.push(1); // BinaryType String
                   // member value: BinaryObjectString #2 = "hey"
        v.push(6);
        v.extend_from_slice(&2i32.to_le_bytes());
        v.extend(lp("hey"));
        v.push(11);
        let doc = Document::parse(&v).unwrap();
        let bar = doc.object(1).unwrap();
        assert_eq!(doc.member_str(bar, "s"), Some("hey"));
    }
}
