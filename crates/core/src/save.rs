//! Generic helpers for treating a save file as a raw byte buffer.
//!
//! The editing model is: load the entire file into memory, mutate only the offsets
//! we understand, and write the buffer back. This preserves every byte we don't
//! understand, by construction.
//!
//! Filled in when we implement inspection and editing.
