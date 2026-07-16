//! Ultima II (DOS) world-map rendering.
//!
//! Ultima II's overworlds and towns each live in their own file, named `MAP[XG]NN` (mixed case on
//! disk). Every map is a 64×64 grid of one byte per tile; the tile index is the top 6 bits
//! (`byte >> 2`), giving 0–63 — a direct index into the 64-entry tileset. Some map files carry a
//! 128-byte NPC/monster header before the grid, so we read the **last** 4096 bytes.
//!
//! The tileset itself is embedded in `ULTIMAII.EXE` at offset `0x7C40`: 64 entries of 66 bytes
//! each (a 2-byte header followed by 64 bytes of CGA 2-bpp pixel data). See [`crate::cga`].

use std::path::Path;

use anyhow::{ensure, Context, Result};
use image::RgbImage;

use crate::bundle::{World, WorldMeta};
use crate::cga::{self, CGA_PALETTE1, TILE_SIZE};

/// Map edge length, in tiles.
const MAP_W: usize = 64;
const MAP_H: usize = 64;
/// Bytes of tile grid in a map file (one byte per tile).
const MAP_GRID_LEN: usize = MAP_W * MAP_H;

const EXE_FILE: &str = "ULTIMAII.EXE";
/// The shipped DOS executable is exactly this size; the tileset offset below assumes it.
const EXE_LEN: usize = 37344;
/// Offset of the embedded tileset within `ULTIMAII.EXE`.
const TILESET_OFFSET: usize = 0x7C40;
const TILE_COUNT: usize = 64;
/// Each tileset entry: a 2-byte header followed by the CGA pixel data.
const TILE_HEADER: usize = 2;
const TILE_STRIDE: usize = TILE_HEADER + cga::BYTES_PER_TILE;

/// Render every Ultima II map file in `game_dir` into its own world.
pub fn export_worlds(game_dir: &Path) -> Result<Vec<World>> {
    let tiles = read_tileset(game_dir)?;

    let mut names = discover_maps(game_dir)
        .with_context(|| format!("scanning {} for map files", game_dir.display()))?;
    names.sort();
    ensure!(
        !names.is_empty(),
        "no Ultima II map files (MAP[XG]NN) found in {}",
        game_dir.display()
    );

    let mut worlds = Vec::with_capacity(names.len());
    for name in names {
        let data =
            std::fs::read(game_dir.join(&name)).with_context(|| format!("reading map {name}"))?;
        if data.len() < MAP_GRID_LEN {
            continue; // Not a full map grid; skip rather than fail the whole export.
        }
        // Some maps prepend a 128-byte NPC header; the grid is always the final 4096 bytes.
        let grid = &data[data.len() - MAP_GRID_LEN..];

        let mut image = RgbImage::new((MAP_W as u32) * TILE_SIZE, (MAP_H as u32) * TILE_SIZE);
        for ty in 0..MAP_H {
            for tx in 0..MAP_W {
                let tile_index = (grid[ty * MAP_W + tx] >> 2) as usize;
                let tile = tiles.get(tile_index).unwrap_or(&tiles[0]);
                image::imageops::replace(
                    &mut image,
                    tile,
                    (tx as u32 * TILE_SIZE) as i64,
                    (ty as u32 * TILE_SIZE) as i64,
                );
            }
        }

        let world_id = name.to_ascii_lowercase();
        worlds.push(World {
            meta: WorldMeta {
                game: "ultima2".into(),
                title: format!("Ultima II — {}", name.to_ascii_uppercase()),
                world: world_id,
            },
            image,
            pois: vec![],
        });
    }

    ensure!(
        !worlds.is_empty(),
        "found Ultima II map files but none contained a full 64×64 grid"
    );
    Ok(worlds)
}

/// Extract the 64-entry tileset embedded in `ULTIMAII.EXE`.
fn read_tileset(game_dir: &Path) -> Result<Vec<RgbImage>> {
    let exe = std::fs::read(game_dir.join(EXE_FILE))
        .with_context(|| format!("reading {EXE_FILE} from {}", game_dir.display()))?;
    ensure!(
        exe.len() == EXE_LEN,
        "{EXE_FILE} is {} bytes; expected {EXE_LEN} (the tileset offset assumes the shipped DOS build)",
        exe.len()
    );
    let end = TILESET_OFFSET + TILE_COUNT * TILE_STRIDE;
    ensure!(
        exe.len() >= end,
        "{EXE_FILE} is too short to contain the tileset at {TILESET_OFFSET:#x}"
    );

    let tiles = (0..TILE_COUNT)
        .map(|i| {
            let pixels = TILESET_OFFSET + i * TILE_STRIDE + TILE_HEADER;
            cga::decode_tile(&exe[pixels..pixels + cga::BYTES_PER_TILE], &CGA_PALETTE1)
        })
        .collect();
    Ok(tiles)
}

/// List the names of all `MAP[XG]NN` files in `game_dir` (case-insensitive on disk).
fn discover_maps(game_dir: &Path) -> Result<Vec<String>> {
    let mut names = Vec::new();
    for entry in std::fs::read_dir(game_dir)? {
        let name = entry?.file_name().to_string_lossy().into_owned();
        if is_map_name(&name) {
            names.push(name);
        }
    }
    Ok(names)
}

/// Whether `name` matches the Ultima II map naming scheme `MAP[XG]NN` (any case).
fn is_map_name(name: &str) -> bool {
    let b = name.as_bytes();
    b.len() == 6
        && name[..3].eq_ignore_ascii_case("MAP")
        && matches!(b[3].to_ascii_uppercase(), b'X' | b'G')
        && b[4].is_ascii_digit()
        && b[5].is_ascii_digit()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_map_names() {
        assert!(is_map_name("MAPX00"));
        assert!(is_map_name("MAPG44"));
        assert!(is_map_name("mapg10")); // lowercase on disk
        assert!(is_map_name("MapX07"));
    }

    #[test]
    fn rejects_non_map_names() {
        assert!(!is_map_name("MON00")); // monster/NPC companion
        assert!(!is_map_name("TLK00")); // dialogue companion
        assert!(!is_map_name("MAPX0")); // too short
        assert!(!is_map_name("MAPX000")); // too long
        assert!(!is_map_name("MAPZ00")); // wrong world letter
        assert!(!is_map_name("MAPXAB")); // non-digit index
    }
}
