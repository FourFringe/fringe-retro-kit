//! Fringe Retro Kit — map exporter.
//!
//! Bakes classic-game world maps into web tile bundles (a `z/x/y` PNG pyramid + `manifest.json`).
//! First target: the Ultima I overworld. See ROADMAP.md, Phase 8.

mod bundle;
mod cga;
mod config;
mod ega;
mod huffman;
mod lzw;
mod serve;
mod tilemap;
mod ultima1;
mod ultima2;
mod ultima3;
mod ultima4;
mod ultima5;
mod wasteland;

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};

use bundle::World;
use config::Config;

#[derive(Parser)]
#[command(name = "fringe-retro-map", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Bake a game's world map into a tile bundle under `<out>/<game>/<world>/`.
    Export {
        /// Game identifier (`ultima1` or `ultima2`).
        #[arg(long, default_value = "ultima1")]
        game: String,
        /// Game data directory. Defaults to the game's `save_dir` from `config.toml`.
        #[arg(long)]
        input: Option<PathBuf>,
        /// Export root. Defaults to `[map] export_dir` from `config.toml`.
        #[arg(long, short)]
        out: Option<PathBuf>,
        /// Also write the first world's flat composite image to this path (handy for debugging).
        #[arg(long)]
        png: Option<PathBuf>,
    },
    /// Serve exported map bundles in a local web browser.
    Serve {
        /// Export root to serve. Defaults to `[map] export_dir` from `config.toml`.
        #[arg(long, short)]
        root: Option<PathBuf>,
        /// Port to listen on.
        #[arg(long, default_value_t = 8737)]
        port: u16,
        /// Open the map browser in your default browser once the server is up.
        #[arg(long)]
        open: bool,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let config = Config::load()?;
    match cli.command {
        Command::Export {
            game,
            input,
            out,
            png,
        } => {
            let input = input
                .or_else(|| config.game_input_dir(&game))
                .with_context(|| {
                    format!("no --input given and no save_dir for '{game}' in config.toml")
                })?;
            let out = out
                .or_else(|| config.export_dir())
                .context("no --out given and no [map] export_dir in config.toml")?;
            export(&game, &input, &out, png.as_deref())
        }
        Command::Serve { root, port, open } => {
            let root = root
                .or_else(|| config.export_dir())
                .context("no --root given and no [map] export_dir in config.toml")?;
            let runtime = tokio::runtime::Runtime::new().context("starting async runtime")?;
            runtime.block_on(serve::serve(root, port, open, config))
        }
    }
}

fn export(game: &str, input: &Path, out: &Path, png: Option<&Path>) -> Result<()> {
    let worlds: Vec<World> = match game {
        "ultima1" => ultima1::export_worlds(input)?,
        "ultima2" => ultima2::export_worlds(input)?,
        "ultima3" => ultima3::export_worlds(input)?,
        "ultima4" => ultima4::export_worlds(input)?,
        "ultima5" => ultima5::export_worlds(input)?,
        "wasteland" => wasteland::export_worlds(input)?,
        other => {
            bail!("unsupported game '{other}' (supported: 'ultima1', 'ultima2', 'ultima3', 'ultima4', 'ultima5', 'wasteland')")
        }
    };

    if let Some(path) = png {
        if let Some(first) = worlds.first() {
            first
                .image
                .save(path)
                .with_context(|| format!("writing {}", path.display()))?;
            println!("Wrote composite {}", path.display());
        }
    }

    // Clear this game's previously-exported bundles first, so worlds that are no longer produced
    // (e.g. maps we've since decided to skip, or renamed worlds) don't linger as stale entries.
    // Scoped to `<out>/<game>/` so other games' bundles in the shared export root are untouched.
    let game_root = out.join(game);
    if game_root.exists() {
        std::fs::remove_dir_all(&game_root)
            .with_context(|| format!("clearing old export at {}", game_root.display()))?;
    }

    for world in &worlds {
        let dir = bundle::write_bundle(out, world)?;
        println!(
            "Baked {} ({}×{} px, {} POIs) → {}",
            world.meta.title,
            world.image.width(),
            world.image.height(),
            world.pois.len(),
            dir.display()
        );
    }
    println!("Baked {} world(s) for {game}.", worlds.len());
    Ok(())
}
