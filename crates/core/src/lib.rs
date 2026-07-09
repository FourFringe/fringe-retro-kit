//! Core library for Fringe Retro Kit: save-file parsing, editing, and backups.
//!
//! Phase 1 hardcodes Ultima I support (see [`games::ultima1`]). The public surface
//! is intentionally small and UI-agnostic so that both the CLI and a future TUI can
//! build on the same engine.

pub mod backup;
pub mod diff;
pub mod games;
pub mod save;

use thiserror::Error;

/// Errors that can arise while loading, editing, or saving a game save file.
#[derive(Debug, Error)]
pub enum Error {
    /// An underlying filesystem error.
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),

    /// The bytes on disk did not match the expected save-file format.
    #[error("save format error: {0}")]
    Format(String),
}

/// Result alias used throughout the crate.
pub type Result<T> = std::result::Result<T, Error>;
