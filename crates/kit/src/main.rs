//! `fringe-retro-kit` — reverse-engineering kit tools for classic-game formats.
//!
//! This binary is kept separate from the player-facing `fringe-retro` (save editor) and
//! `fringe-retro-map` (map browser) tools: it holds the workbench used to *understand* a
//! format in the first place. The first tool is the codec workbench (`codec`).

mod util;
mod workbench;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "fringe-retro-kit",
    version,
    about = "Reverse-engineering kit tools for classic-game formats."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Codec workbench: decode/round-trip encoded blobs and identify checksums.
    Codec {
        #[command(subcommand)]
        command: workbench::Command,
    },
}

fn main() -> anyhow::Result<()> {
    match Cli::parse().command {
        Command::Codec { command } => workbench::run(command),
    }
}
