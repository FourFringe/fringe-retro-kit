//! Ultima I (DOS) world-map rendering.
//!
//! The overworld lives in `MAP.BIN`: a 168×156 grid of tiles, **nibble-packed** two tiles per
//! byte (high nibble = left tile, low nibble = right tile), 84 bytes per row. Each nibble is a
//! direct index into the `EGATILES.BIN` tileset (0 = water, 1 = grass, 2 = forest,
//! 3 = mountains, plus sparse landmark tiles for towns/castles/dungeons).

use std::path::Path;

use anyhow::{ensure, Context, Result};
use image::RgbImage;

use crate::ega::{self, TILE_SIZE};

/// Overworld width and height, in tiles.
pub const OVERWORLD_W: u32 = 168;
pub const OVERWORLD_H: u32 = 156;

const BYTES_PER_ROW: usize = OVERWORLD_W as usize / 2; // two tiles per byte
const MAP_LEN: usize = BYTES_PER_ROW * OVERWORLD_H as usize;

const TILESET_FILE: &str = "EGATILES.BIN";
const MAP_FILE: &str = "MAP.BIN";

/// Render the full Ultima I overworld into a single `(168*16) × (156*16)` image.
pub fn render_overworld(game_dir: &Path) -> Result<RgbImage> {
    let tileset = std::fs::read(game_dir.join(TILESET_FILE))
        .with_context(|| format!("reading {TILESET_FILE} from {}", game_dir.display()))?;
    let tiles = ega::decode_tileset(&tileset);
    ensure!(!tiles.is_empty(), "{TILESET_FILE} contained no tiles");

    let map = std::fs::read(game_dir.join(MAP_FILE))
        .with_context(|| format!("reading {MAP_FILE} from {}", game_dir.display()))?;
    ensure!(
        map.len() >= MAP_LEN,
        "{MAP_FILE} is {} bytes; expected at least {MAP_LEN}",
        map.len()
    );

    let mut world = RgbImage::new(OVERWORLD_W * TILE_SIZE, OVERWORLD_H * TILE_SIZE);
    for row in 0..OVERWORLD_H as usize {
        for bx in 0..BYTES_PER_ROW {
            let byte = map[row * BYTES_PER_ROW + bx];
            for (half, tile_index) in [byte >> 4, byte & 0x0F].into_iter().enumerate() {
                let tile = tiles.get(tile_index as usize).unwrap_or(&tiles[0]);
                let x = (bx * 2 + half) as u32 * TILE_SIZE;
                let y = row as u32 * TILE_SIZE;
                image::imageops::replace(&mut world, tile, x as i64, y as i64);
            }
        }
    }
    Ok(world)
}
