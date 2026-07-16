//! Fringe Retro Kit — map exporter.
//!
//! Bakes classic-game world maps into web tile bundles (a `z/x/y` PNG pyramid + `manifest.json`).
//! First target: the Ultima I overworld. See ROADMAP.md, Phase 8.

mod bundle;
mod ega;
mod serve;
mod ultima1;

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};

use bundle::WorldMeta;

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
        #[arg(long)]
        game: String,
        /// Path to the game's data directory (e.g. the folder containing MAP.BIN).
        #[arg(long)]
        input: PathBuf,
        /// Export root; the bundle is written to `<out>/<game>/<world>/`.
        #[arg(long, short)]
        out: PathBuf,
        /// Also write the flat composite image to this path (handy for debugging).
        #[arg(long)]
        png: Option<PathBuf>,
    },
    /// Serve exported map bundles in a local web browser.
    Serve {
        /// Export root to serve (the directory that holds `<game>/<world>/` bundles).
        #[arg(long, short)]
        root: PathBuf,
        /// Port to listen on.
        #[arg(long, default_value_t = 8737)]
        port: u16,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Export {
            game,
            input,
            out,
            png,
        } => export(&game, &input, &out, png.as_deref()),
        Command::Serve { root, port } => {
            let runtime = tokio::runtime::Runtime::new().context("starting async runtime")?;
            runtime.block_on(serve::serve(root, port))
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
