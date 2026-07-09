//! `fringe-retro` — command-line entry point.
//!
//! Phase 1 exposes a small, permanent headless CLI over `fringe-retro-core`.

mod config;

use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};
use fringe_retro_core::backup;
use fringe_retro_core::games::ultima1::{self, Ultima1Save};
use fringe_retro_core::games::ultima3::{self, Ultima3Party, Ultima3Roster};

use crate::config::Config;

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
        /// Character slot for multi-character rosters (Ultima III), 1-based.
        #[arg(long, default_value_t = 1)]
        slot: usize,
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
        /// Character slot for multi-character rosters (Ultima III), 1-based.
        #[arg(long, default_value_t = 1)]
        slot: usize,
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
    let config = Config::load()?;
    match cli.command {
        Command::Inspect { path } => {
            let path = config.resolve_save_path(&path)?;
            let bytes = std::fs::read(&path)?;
            if bytes.len() == ultima3::PARTY_LEN {
                // Ultima III active party.
                let party = Ultima3Party::from_bytes(bytes)?;
                println!("Party:");
                for (label, value) in party.header_inspect() {
                    println!("  {label:<20} {value}");
                }
                let order = party.party_order();
                let members = party.party_size().min(ultima3::PARTY_MEMBER_COUNT);
                for (member, slot) in order.iter().enumerate().take(members) {
                    println!(
                        "\nMember {} (roster slot {}): {}",
                        member + 1,
                        slot,
                        party.summary(member)
                    );
                    for (label, value) in party.inspect(member) {
                        println!("  {label:<16} {value}");
                    }
                }
            } else if bytes.len() == ultima3::ROSTER_LEN {
                // Ultima III roster: 20 character slots.
                let roster = Ultima3Roster::from_bytes(bytes)?;
                let occupied = roster.occupied_slots();
                if occupied.is_empty() {
                    println!("(empty roster)");
                }
                for slot in occupied {
                    println!("\nSlot {}: {}", slot + 1, roster.summary(slot));
                    for (label, value) in roster.inspect(slot) {
                        println!("  {label:<16} {value}");
                    }
                }
            } else {
                // Ultima I single-character save.
                let save = Ultima1Save::from_bytes(bytes)?;
                let mut current_section = "";
                for (section, label, value) in save.inspect() {
                    if section != current_section {
                        println!("\n{section}:");
                        current_section = section;
                    }
                    println!("  {label:<16} {value}");
                }
            }
        }
        Command::Get { path, field, slot } => {
            let path = config.resolve_save_path(&path)?;
            let bytes = std::fs::read(&path)?;
            if bytes.len() == ultima3::PARTY_LEN {
                let party = Ultima3Party::from_bytes(bytes)?;
                let member = roster_index(slot)?;
                match party.get_field(member, &field) {
                    Some(value) => println!("{value}"),
                    None => {
                        let keys: Vec<_> = Ultima3Party::field_keys().collect();
                        anyhow::bail!("unknown field '{field}'. Known fields: {}", keys.join(", "));
                    }
                }
            } else if bytes.len() == ultima3::ROSTER_LEN {
                let roster = Ultima3Roster::from_bytes(bytes)?;
                let index = roster_index(slot)?;
                match roster.get_field(index, &field) {
                    Some(value) => println!("{value}"),
                    None => {
                        let keys: Vec<_> = Ultima3Roster::field_keys().collect();
                        anyhow::bail!("unknown field '{field}'. Known fields: {}", keys.join(", "));
                    }
                }
            } else {
                let save = Ultima1Save::from_bytes(bytes)?;
                match save.get_field(&field) {
                    Some(value) => println!("{value}"),
                    None => {
                        let keys: Vec<_> = Ultima1Save::field_keys().collect();
                        anyhow::bail!("unknown field '{field}'. Known fields: {}", keys.join(", "));
                    }
                }
            }
        }
        Command::Dump { path, range } => {
            let path = config.resolve_save_path(&path)?;
            let bytes = std::fs::read(&path)?;
            let (start, end) = match range {
                Some(r) => parse_range(&r)?,
                None => (0, bytes.len()),
            };
            print!("{}", ultima1::hex_dump(&bytes, start, end));
        }
        Command::Set {
            path,
            field,
            value,
            slot,
        } => {
            let path = config.resolve_save_path(&path)?;
            let bytes = std::fs::read(&path)?;
            if bytes.len() == ultima3::PARTY_LEN {
                let mut party = Ultima3Party::from_bytes(bytes)?;
                let member = roster_index(slot)?;
                let old = party
                    .get_field(member, &field)
                    .unwrap_or_else(|| "?".to_string());
                party.set_field(member, &field, &value)?;
                let new = party
                    .get_field(member, &field)
                    .unwrap_or_else(|| "?".to_string());
                let backup_path = backup::create(&path)?;
                party.write(&path)?;
                println!("slot {slot} {field}: {old} -> {new}");
                println!("backup: {}", backup_path.display());
            } else if bytes.len() == ultima3::ROSTER_LEN {
                let mut roster = Ultima3Roster::from_bytes(bytes)?;
                let index = roster_index(slot)?;
                let old = roster
                    .get_field(index, &field)
                    .unwrap_or_else(|| "?".to_string());
                roster.set_field(index, &field, &value)?;
                let new = roster
                    .get_field(index, &field)
                    .unwrap_or_else(|| "?".to_string());
                let backup_path = backup::create(&path)?;
                roster.write(&path)?;
                println!("slot {slot} {field}: {old} -> {new}");
                println!("backup: {}", backup_path.display());
            } else {
                let mut save = Ultima1Save::from_bytes(bytes)?;
                let old = save.get_field(&field).unwrap_or_else(|| "?".to_string());
                save.set_field(&field, &value)?;
                let new = save.get_field(&field).unwrap_or_else(|| "?".to_string());
                // Back up the original on disk before overwriting it.
                let backup_path = backup::create(&path)?;
                save.write(&path)?;
                println!("{field}: {old} -> {new}");
                println!("backup: {}", backup_path.display());
            }
        }
        Command::Backup { path } => {
            let path = config.resolve_save_path(&path)?;
            let backup_path = backup::create(&path)?;
            println!("{}", backup_path.display());
        }
        Command::Backups { path } => {
            let path = config.resolve_save_path(&path)?;
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
            let path = config.resolve_save_path(&path)?;
            let backup_path = config.resolve_save_path(&backup_path)?;
            let pre_restore = backup::restore(&backup_path, &path)?;
            println!("restored {} -> {}", backup_path.display(), path.display());
            println!("previous save backed up to {}", pre_restore.display());
        }
    }
    Ok(())
}

/// Convert a 1-based character slot into a 0-based roster index.
fn roster_index(slot: usize) -> Result<usize> {
    anyhow::ensure!(slot >= 1, "slot must be >= 1");
    Ok(slot - 1)
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
