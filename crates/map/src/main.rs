//! Fringe Retro Kit — map exporter.
//!
//! Bakes classic-game world maps into images (and, later, web tile pyramids). First target:
//! the Ultima I overworld. See ROADMAP.md, Phase 8.

mod ega;
mod ultima1;

use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "fringe-retro-map", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Render a game's world map to a PNG.
    Export {
        /// Game identifier (currently only `ultima1`).
        #[arg(long)]
        game: String,
        /// Path to the game's data directory (e.g. the folder containing MAP.BIN).
        #[arg(long)]
        input: PathBuf,
        /// Output PNG path.
        #[arg(long, short)]
        out: PathBuf,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Export { game, input, out } => export(&game, &input, &out),
    }
}

fn export(game: &str, input: &std::path::Path, out: &std::path::Path) -> Result<()> {
    let world = match game {
        "ultima1" => ultima1::render_overworld(input)?,
        other => bail!("unsupported game '{other}' (currently only 'ultima1')"),
    };
    world
        .save(out)
        .with_context(|| format!("writing {}", out.display()))?;
    println!(
        "Wrote {} ({}×{} px)",
        out.display(),
        world.width(),
        world.height()
    );
    Ok(())
}
