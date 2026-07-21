//! `fringe-retro-kit` — reverse-engineering kit tools for classic-game formats.
//!
//! This binary is kept separate from the player-facing `fringe-retro` (save editor) and
//! `fringe-retro-map` (map browser) tools: it holds the workbench used to *understand* a
//! format in the first place. The tools so far are the codec workbench (`codec`), the
//! string ripper (`strings`), and the schema explorer (`schema`).

mod explorer;
mod ripper;
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
    /// String ripper: extract ASCII or Wasteland 5-bit packed strings.
    Strings {
        #[command(subcommand)]
        command: ripper::Command,
    },
    /// Schema explorer: find values, diff saves, and detect record strides.
    Schema {
        #[command(subcommand)]
        command: explorer::Command,
    },
}

fn main() -> anyhow::Result<()> {
    match Cli::parse().command {
        Command::Codec { command } => workbench::run(command),
        Command::Strings { command } => ripper::run(command),
        Command::Schema { command } => explorer::run(command),
    }
}
