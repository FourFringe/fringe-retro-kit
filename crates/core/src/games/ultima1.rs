//! Hardcoded Ultima I save-file support (`PLAYER*.U1`).
//!
//! Format reference: <https://moddingwiki.shikadi.net/wiki/Ultima_I_Save_Game_Format>
//! (reverse-engineered by TheAlmightyGuru and Daniel D'Agostino). All multi-byte values
//! are little-endian 16-bit integers.
//!
//! Parsing, inspection, and editing land in the next step.

/// Total size of an Ultima I save file, in bytes (`0x334`).
pub const SAVE_LEN: usize = 0x334;
