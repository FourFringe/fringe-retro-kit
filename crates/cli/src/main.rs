//! `fringe-retro` — command-line entry point.
//!
//! Phase 1 exposes a small, permanent headless CLI over `fringe-retro-core`.
//! The subcommands below are stubbed until the Ultima I engine lands in the next step.

use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};

/// Fringe Retro Kit — inspect, edit, and back up classic game save files.
#[derive(Debug, Parser)]
#[command(name = "fringe-retro", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Show everything we understand about a save file.
    Inspect {
        /// Path to the save file (e.g. PLAYER1.U1).
        path: PathBuf,
    },
    /// Print a single field's value.
    Get {
        /// Path to the save file.
        path: PathBuf,
        /// Field name, e.g. `strength`.
        field: String,
    },
    /// Raw hex dump of the file (optionally a byte range).
    Dump {
        /// Path to the save file.
        path: PathBuf,
        /// Byte range like `0x18:0x24` (inclusive start, exclusive end).
        #[arg(long)]
        range: Option<String>,
    },
    /// Edit a field. Creates a backup first, then writes atomically.
    Set {
        /// Path to the save file.
        path: PathBuf,
        /// Field name, e.g. `gold`.
        field: String,
        /// New value (numbers, or enum names such as `aircar`).
        value: String,
    },
    /// Make a manual timestamped backup.
    Backup {
        /// Path to the save file.
        path: PathBuf,
    },
    /// List backups for a save file.
    Backups {
        /// Path to the save file.
        path: PathBuf,
    },
    /// Restore a chosen backup over the active save.
    Restore {
        /// Path to the active save file.
        path: PathBuf,
        /// Path to the backup to restore.
        backup: PathBuf,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    // All subcommands are stubbed until the Ultima I engine lands (next step).
    anyhow::bail!("command not yet implemented: {:?}", cli.command);
}
