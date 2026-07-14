//! `fringe-retro` — command-line entry point.
//!
//! Phase 1 exposes a small, permanent headless CLI over `fringe-retro-core`.

mod config;
mod edit;
mod inspect;
mod resources;
mod templates;
mod tui;

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::Result;
use clap::{Parser, Subcommand};
use fringe_retro_core::backup;
use fringe_retro_core::diff::diff_bytes;
use fringe_retro_core::games::ultima1::{self, Ultima1Save};
use fringe_retro_core::games::ultima2::{self, Ultima2Save};
use fringe_retro_core::games::ultima3::{self, Ultima3Party, Ultima3Roster};
use fringe_retro_core::games::ultima4::{self, Ultima4Save};
use fringe_retro_core::games::ultima5::{self, Ultima5Save};
use fringe_retro_core::games::GameKind;

use crate::config::Config;
use crate::templates::{Template, TemplateSet};

/// Fringe Retro Kit — inspect, edit, and back up classic game save files.
#[derive(Debug, Parser)]
#[command(name = "fringe-retro", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
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
    /// Watch a save file and print byte-level changes as they happen (Ctrl-C to stop).
    Watch {
        /// Path to the save file to watch.
        path: PathBuf,
        /// Poll interval in milliseconds.
        #[arg(long, default_value_t = 500)]
        interval: u64,
    },
    /// List the games configured in your library manifest.
    Games,
    /// List character templates and whether each one is valid for its game.
    Templates,
    /// List curated web resources for a game, or open one in your browser.
    Resources {
        /// Game id (e.g. `ultima4`). Omit to list resources for every game.
        game: Option<String>,
        /// Open link number N (as shown in the list) in your default browser.
        #[arg(long, value_name = "N")]
        open: Option<usize>,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let config = Config::load()?;
    // With no subcommand, launch the interactive terminal UI.
    let Some(command) = cli.command else {
        return tui::run(config);
    };
    match command {
        Command::Inspect { path } => {
            let path = config.resolve_save_path(&path)?;
            let bytes = std::fs::read(&path)?;
            for line in inspect::inspect_lines(&bytes)? {
                println!("{line}");
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
            } else if bytes.len() == ultima4::SAVE_LEN {
                let save = Ultima4Save::from_bytes(bytes)?;
                // Party/game-state fields, or a player's field via `--slot`.
                let member = roster_index(slot)?;
                match save
                    .party_get(&field)
                    .or_else(|| save.player_get(member, &field))
                {
                    Some(value) => println!("{value}"),
                    None => {
                        let party: Vec<_> = Ultima4Save::party_field_keys().collect();
                        let player: Vec<_> = Ultima4Save::player_field_keys().collect();
                        anyhow::bail!(
                            "unknown field '{field}'.\n  Party fields: {}\n  Player fields (use --slot): {}",
                            party.join(", "),
                            player.join(", ")
                        );
                    }
                }
            } else if bytes.len() == ultima5::SAVE_LEN {
                let save = Ultima5Save::from_bytes(bytes)?;
                // Party/game-state fields, or a character's field via `--slot`.
                let member = roster_index(slot)?;
                match save
                    .party_get(&field)
                    .or_else(|| save.character_get(member, &field))
                {
                    Some(value) => println!("{value}"),
                    None => {
                        let party: Vec<_> = Ultima5Save::party_field_keys().collect();
                        let character: Vec<_> = Ultima5Save::character_field_keys().collect();
                        anyhow::bail!(
                            "unknown field '{field}'.\n  Party fields: {}\n  Character fields (use --slot): {}",
                            party.join(", "),
                            character.join(", ")
                        );
                    }
                }
            } else if bytes.len() == ultima2::SAVE_LEN {
                let save = Ultima2Save::from_bytes(bytes)?;
                match save.get_field(&field) {
                    Some(value) => println!("{value}"),
                    None => {
                        let keys: Vec<_> = Ultima2Save::field_keys().collect();
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
            } else if bytes.len() == ultima4::SAVE_LEN {
                let mut save = Ultima4Save::from_bytes(bytes)?;
                let member = roster_index(slot)?;
                let is_party = Ultima4Save::party_field_keys().any(|k| k == field);
                let read = |s: &Ultima4Save| {
                    if is_party {
                        s.party_get(&field)
                    } else {
                        s.player_get(member, &field)
                    }
                    .unwrap_or_else(|| "?".to_string())
                };
                let old = read(&save);
                if is_party {
                    save.party_set(&field, &value)?;
                } else {
                    save.player_set(member, &field, &value)?;
                }
                let new = read(&save);
                let backup_path = backup::create(&path)?;
                save.write(&path)?;
                if is_party {
                    println!("{field}: {old} -> {new}");
                } else {
                    println!("slot {slot} {field}: {old} -> {new}");
                }
                println!("backup: {}", backup_path.display());
            } else if bytes.len() == ultima5::SAVE_LEN {
                let mut save = Ultima5Save::from_bytes(bytes)?;
                let member = roster_index(slot)?;
                let is_party = Ultima5Save::party_field_keys().any(|k| k == field);
                let read = |s: &Ultima5Save| {
                    if is_party {
                        s.party_get(&field)
                    } else {
                        s.character_get(member, &field)
                    }
                    .unwrap_or_else(|| "?".to_string())
                };
                let old = read(&save);
                if is_party {
                    save.party_set(&field, &value)?;
                } else {
                    save.character_set(member, &field, &value)?;
                }
                let new = read(&save);
                let backup_path = backup::create(&path)?;
                save.write(&path)?;
                if is_party {
                    println!("{field}: {old} -> {new}");
                } else {
                    println!("slot {slot} {field}: {old} -> {new}");
                }
                println!("backup: {}", backup_path.display());
            } else if bytes.len() == ultima2::SAVE_LEN {
                let mut save = Ultima2Save::from_bytes(bytes)?;
                let old = save.get_field(&field).unwrap_or_else(|| "?".to_string());
                save.set_field(&field, &value)?;
                let new = save.get_field(&field).unwrap_or_else(|| "?".to_string());
                let backup_path = backup::create(&path)?;
                save.write(&path)?;
                println!("{field}: {old} -> {new}");
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
            match backup::restore(&backup_path, &path)? {
                Some(pre_restore) => {
                    println!("restored {} -> {}", backup_path.display(), path.display());
                    println!("previous save backed up to {}", pre_restore.display());
                }
                None => {
                    println!(
                        "{} already matches {}; nothing to restore",
                        path.display(),
                        backup_path.display()
                    );
                }
            }
        }
        Command::Watch { path, interval } => {
            let path = config.resolve_save_path(&path)?;
            watch_file(&path, interval)?;
        }
        Command::Games => {
            print_games(&config)?;
        }
        Command::Templates => {
            let set = TemplateSet::load()?;
            print_templates(&set);
        }
        Command::Resources { game, open } => {
            run_resources(game.as_deref(), open)?;
        }
    }
    Ok(())
}

/// Print every template with its target game, field count, and validity.
fn print_templates(set: &TemplateSet) {
    if set.all().is_empty() {
        println!("(no templates — see templates.example.toml)");
        return;
    }
    for t in set.all() {
        let status = match validate_template(t) {
            Ok(()) => "ok".to_string(),
            Err(e) => format!("ERROR: {e}"),
        };
        println!(
            "{:<10} {:<24} {} field(s)  [{status}]",
            t.game,
            t.name,
            t.fields.len()
        );
    }
}

/// Validate a template by dry-running its edits against an empty scratch save.
fn validate_template(t: &Template) -> std::result::Result<(), String> {
    let Some(kind) = GameKind::from_id(&t.game) else {
        return Err(format!("unknown game '{}'", t.game));
    };
    let Some(mut scratch) = edit::Session::scratch(kind) else {
        return Err(format!("game '{}' is not editable", t.game));
    };
    scratch.apply(0, &t.fields).map_err(|e| e.to_string())
}

/// Convert a 1-based character slot into a 0-based roster index.
fn roster_index(slot: usize) -> Result<usize> {
    anyhow::ensure!(slot >= 1, "slot must be >= 1");
    Ok(slot - 1)
}

/// List curated web resources, or open a chosen one in the browser.
fn run_resources(game: Option<&str>, open: Option<usize>) -> Result<()> {
    let all = resources::Resources::load()?;

    // Opening a link requires a specific game and a 1-based index into its list.
    if let Some(n) = open {
        let game = game.ok_or_else(|| {
            anyhow::anyhow!("`--open` needs a game, e.g. `resources ultima4 --open 1`")
        })?;
        let links = all.for_game(game);
        anyhow::ensure!(!links.is_empty(), "no resources for '{game}'");
        let link = links.get(n.wrapping_sub(1)).ok_or_else(|| {
            anyhow::anyhow!("no link {n} for '{game}' (have 1..={})", links.len())
        })?;
        println!("Opening {} — {}", link.title, link.url);
        return resources::open_url(&link.url);
    }

    // Otherwise, print a numbered listing (one game, or all).
    let games: Vec<String> = match game {
        Some(g) => vec![g.to_string()],
        None => all.games().map(str::to_owned).collect(),
    };
    let mut printed = false;
    for id in games {
        let links = all.for_game(&id);
        if links.is_empty() {
            if game.is_some() {
                anyhow::bail!("no resources for '{id}'");
            }
            continue;
        }
        if printed {
            println!();
        }
        printed = true;
        let title = GameKind::from_id(&id).map(|k| k.title()).unwrap_or(&id);
        println!("{title} ({id}):");
        for (i, link) in links.iter().enumerate() {
            println!("  {:>2}. [{}] {}", i + 1, link.category, link.title);
            println!("      {}", link.url);
        }
    }
    if !printed {
        println!("No resources configured.");
    }
    Ok(())
}

/// Print the games configured in the library manifest and whether their saves are present.
fn print_games(config: &Config) -> Result<()> {
    let games = config.games()?;
    if games.is_empty() {
        println!("No games configured. Copy config.example.toml to config.toml to get started.");
        return Ok(());
    }
    for game in games {
        let default_save = game
            .save_dir
            .as_ref()
            .map(|dir| dir.join(game.kind.default_save_file()));
        let status = match &default_save {
            Some(path) if path.exists() => "found",
            Some(_) => "missing",
            None => "no save_dir",
        };
        let note = if game.kind.is_inspectable() {
            ""
        } else {
            "  (inspect not yet supported)"
        };
        println!("{:<14} {}{}  [{status}]", game.id, game.kind.title(), note);
        if let Some(path) = &default_save {
            println!("    save:     {}", path.display());
        }
        if let Some(platform) = &game.platform {
            println!("    platform: {platform}");
        }
        if let Some(dir) = &game.install_dir {
            println!("    install:  {}", dir.display());
        }
    }
    Ok(())
}

/// Poll `path` and print byte-level changes until interrupted.
fn watch_file(path: &Path, interval_ms: u64) -> Result<()> {
    println!("Watching {} — press Ctrl-C to stop.", path.display());
    let mut previous = std::fs::read(path).ok();
    match &previous {
        Some(p) => println!("Initial size: {} bytes.", p.len()),
        None => println!("(file does not exist yet; waiting for it to appear)"),
    }
    loop {
        std::thread::sleep(Duration::from_millis(interval_ms));
        let current = match std::fs::read(path) {
            Ok(c) => c,
            Err(_) => continue, // e.g. momentarily absent during an atomic replace
        };
        match previous {
            Some(ref prev) if *prev == current => {}
            Some(ref prev) => print_changes(prev, &current),
            None => println!(
                "[{}] file appeared: {} bytes",
                chrono::Local::now().format("%H:%M:%S"),
                current.len()
            ),
        }
        previous = Some(current);
    }
}

/// Print the byte-level differences between two versions of a file.
fn print_changes(old: &[u8], new: &[u8]) {
    let ts = chrono::Local::now().format("%H:%M:%S");
    if old.len() != new.len() {
        println!("[{ts}] size changed: {} -> {} bytes", old.len(), new.len());
    }
    let changes = diff_bytes(old, new);
    if changes.is_empty() {
        return;
    }
    println!("[{ts}] {} byte(s) changed:", changes.len());
    for c in &changes {
        println!(
            "  0x{:04X}: {:02X} -> {:02X}   ({:>3} -> {:>3})   '{}' -> '{}'",
            c.offset,
            c.old,
            c.new,
            c.old,
            c.new,
            printable(c.old),
            printable(c.new),
        );
    }
}

/// Render a byte as a printable ASCII character, or `.` if it isn't one.
fn printable(b: u8) -> char {
    if (0x20..0x7f).contains(&b) {
        b as char
    } else {
        '.'
    }
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
