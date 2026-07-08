//! `fringe-retro` — command-line entry point.
//!
//! Phase 1 exposes a small, permanent headless CLI over `fringe-retro-core`.
//! The subcommands below are stubbed until the Ultima I engine lands in the next step.

use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};
use fringe_retro_core::backup;
use fringe_retro_core::games::ultima1::{self, Ultima1Save};

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
    match cli.command {
        Command::Inspect { path } => {
            let save = Ultima1Save::load(&path)?;
            for (label, value) in save.inspect() {
                println!("{label:<14} {value}");
            }
        }
        Command::Get { path, field } => {
            let save = Ultima1Save::load(&path)?;
            match save.get_field(&field) {
                Some(value) => println!("{value}"),
                None => {
                    let keys: Vec<_> = Ultima1Save::field_keys().collect();
                    anyhow::bail!("unknown field '{field}'. Known fields: {}", keys.join(", "));
                }
            }
        }
        Command::Dump { path, range } => {
            let save = Ultima1Save::load(&path)?;
            let (start, end) = match range {
                Some(r) => parse_range(&r)?,
                None => (0, save.as_bytes().len()),
            };
            print!("{}", ultima1::hex_dump(save.as_bytes(), start, end));
        }
        Command::Set { path, field, value } => {
            let mut save = Ultima1Save::load(&path)?;
            let old = save.get_field(&field).unwrap_or_else(|| "?".to_string());
            save.set_field(&field, &value)?;
            let new = save.get_field(&field).unwrap_or_else(|| "?".to_string());
            // Back up the original on disk before overwriting it.
            let backup_path = backup::create(&path)?;
            save.write(&path)?;
            println!("{field}: {old} -> {new}");
            println!("backup: {}", backup_path.display());
        }
        Command::Backup { path } => {
            let backup_path = backup::create(&path)?;
            println!("{}", backup_path.display());
        }
        Command::Backups { path } => {
            let backups = backup::list(&path)?;
            if backups.is_empty() {
                println!("(no backups)");
            } else {
                for entry in backups {
                    println!("{}", entry.display());
                }
            }
        }
        Command::Restore {
            path,
            backup: backup_path,
        } => {
            let pre_restore = backup::restore(&backup_path, &path)?;
            println!("restored {} -> {}", backup_path.display(), path.display());
            println!("previous save backed up to {}", pre_restore.display());
        }
    }
    Ok(())
}

/// Parse a `START:END` byte range like `0x18:0x24` (hex or decimal), end exclusive.
fn parse_range(s: &str) -> Result<(usize, usize)> {
    let (a, b) = s
        .split_once(':')
        .ok_or_else(|| anyhow::anyhow!("range must look like START:END, e.g. 0x18:0x24"))?;
    let start = parse_num(a)?;
    let end = parse_num(b)?;
    if end < start {
        anyhow::bail!("range end ({end}) must be >= start ({start})");
    }
    Ok((start, end))
}

/// Parse a `usize` in either hexadecimal (`0x` prefix) or decimal.
fn parse_num(s: &str) -> Result<usize> {
    let s = s.trim();
    let parsed = match s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        Some(hex) => usize::from_str_radix(hex, 16),
        None => s.parse::<usize>(),
    };
    parsed.map_err(|_| anyhow::anyhow!("invalid number: '{s}'"))
}
