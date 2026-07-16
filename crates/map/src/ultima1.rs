//! Ultima I (DOS) world-map rendering.
//!
//! The overworld lives in `MAP.BIN`: a 168×156 grid of tiles, **nibble-packed** two tiles per
//! byte (high nibble = left tile, low nibble = right tile), 84 bytes per row. Each nibble is a
//! direct index into the `EGATILES.BIN` tileset (0 = water, 1 = grass, 2 = forest,
//! 3 = mountains, plus sparse landmark tiles for towns/castles/dungeons).

use std::path::Path;

use anyhow::{ensure, Context, Result};
use image::RgbImage;

use crate::bundle::Poi;
use crate::ega::{self, TILE_SIZE};

/// Overworld width and height, in tiles.
pub const OVERWORLD_W: u32 = 168;
pub const OVERWORLD_H: u32 = 156;

const BYTES_PER_ROW: usize = OVERWORLD_W as usize / 2; // two tiles per byte
const MAP_LEN: usize = BYTES_PER_ROW * OVERWORLD_H as usize;

const TILESET_FILE: &str = "EGATILES.BIN";
const MAP_FILE: &str = "MAP.BIN";
const SAVE_FILE: &str = "PLAYER1.U1";

/// A rendered overworld: the composite image plus the landmarks found on it.
pub struct Overworld {
    pub image: RgbImage,
    pub pois: Vec<Poi>,
}

/// The landmark kind for an overworld tile index, if any (`kind`, display `label`).
fn landmark(tile: u8) -> Option<(&'static str, &'static str)> {
    match tile {
        4 | 5 => Some(("castle", "Castle")),
        6 => Some(("signpost", "Signpost")),
        7 => Some(("town", "Town")),
        _ => None,
    }
}

/// Render the full Ultima I overworld and collect its landmark POIs.
pub fn render_overworld(game_dir: &Path) -> Result<Overworld> {
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

    let half = TILE_SIZE / 2;
    let mut world = RgbImage::new(OVERWORLD_W * TILE_SIZE, OVERWORLD_H * TILE_SIZE);
    let mut pois = Vec::new();
    for row in 0..OVERWORLD_H as usize {
        for bx in 0..BYTES_PER_ROW {
            let byte = map[row * BYTES_PER_ROW + bx];
            for (nibble, tile_index) in [byte >> 4, byte & 0x0F].into_iter().enumerate() {
                let tile_x = (bx * 2 + nibble) as u32;
                let tile_y = row as u32;
                let tile = tiles.get(tile_index as usize).unwrap_or(&tiles[0]);
                image::imageops::replace(
                    &mut world,
                    tile,
                    (tile_x * TILE_SIZE) as i64,
                    (tile_y * TILE_SIZE) as i64,
                );
                if let Some((kind, label)) = landmark(tile_index) {
                    pois.push(Poi {
                        px: tile_x * TILE_SIZE + half,
                        py: tile_y * TILE_SIZE + half,
                        kind: kind.to_string(),
                        label: label.to_string(),
                    });
                }
            }
        }
    }
    Ok(Overworld { image: world, pois })
}

/// The party's current overworld position (in tiles), read from the save, or `None` if there
/// is no save file. Uses the Ultima I parser in `fringe-retro-core`.
pub fn player_position(game_dir: &Path) -> Result<Option<(u32, u32)>> {
    let save = game_dir.join(SAVE_FILE);
    if !save.exists() {
        return Ok(None);
    }
    let parsed = fringe_retro_core::games::ultima1::Ultima1Save::load(&save)
        .with_context(|| format!("reading {}", save.display()))?;
    let x = parsed.get_field("x").and_then(|v| v.parse::<u32>().ok());
    let y = parsed.get_field("y").and_then(|v| v.parse::<u32>().ok());
    Ok(x.zip(y))
}
