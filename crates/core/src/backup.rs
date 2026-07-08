//! Automatic, timestamped backups and atomic writes.
//!
//! Before the first write to a save file we copy it to a timestamped backup, and all
//! writes go to a temp file that is atomically renamed over the target. Combined, these
//! make data loss extremely unlikely.
//!
//! Filled in when we implement editing.
