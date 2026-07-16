//! Fringe Retro Kit — map exporter.
//!
//! Bakes classic-game world maps into web tile bundles (a `z/x/y` PNG pyramid + `manifest.json`).
//! First target: the Ultima I overworld. See ROADMAP.md, Phase 8.

mod bundle;
mod config;
mod ega;
mod serve;
mod ultima1;

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};

use bundle::WorldMeta;
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
        /// Game identifier (currently only `ultima1`).
        #[arg(long, default_value = "ultima1")]
        game: String,
        /// Game data directory. Defaults to the game's `save_dir` from `config.toml`.
        #[arg(long)]
        input: Option<PathBuf>,
        /// Export root. Defaults to `[map] export_dir` from `config.toml`.
        #[arg(long, short)]
        out: Option<PathBuf>,
        /// Also write the flat composite image to this path (handy for debugging).
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
            runtime.block_on(serve::serve(root, port, open))
        }
    }
}

fn export(game: &str, input: &Path, out: &Path, png: Option<&Path>) -> Result<()> {
    let (world, meta) = match game {
        "ultima1" => (
            ultima1::render_overworld(input)?,
            WorldMeta {
                game: "ultima1".into(),
                world: "overworld".into(),
                title: "Ultima I — Sosaria".into(),
            },
        ),
        other => bail!("unsupported game '{other}' (currently only 'ultima1')"),
    };

    if let Some(path) = png {
        world
            .save(path)
            .with_context(|| format!("writing {}", path.display()))?;
        println!("Wrote composite {}", path.display());
    }

    let dir = bundle::write_bundle(&world, out, &meta)?;
    println!(
        "Baked {} ({}×{} px) → {}",
        meta.title,
        world.width(),
        world.height(),
        dir.display()
    );
    Ok(())
}
