//! Reusable low-level **codecs** for retro save/data formats: ciphers, decompressors, and
//! checksums shared by the game modules, the map exporter, and the reverse-engineering kit tools.
//!
//! Each codec is a small, self-contained transform with a symmetric or verifiable inverse, so a
//! caller can decode a blob, re-encode it, and confirm a byte-for-byte round-trip. Game-specific
//! framing (locating a block, reading its seed) stays in the game modules; only the raw
//! algorithms live here.

pub mod checksum;
pub mod exepack;
pub mod huffman;
pub mod xor;
