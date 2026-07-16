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

// Ultima I keeps a table of named overworld locations in `OUT.EXE`: a block of
// null-terminated names, plus two parallel arrays (all X then all Y, one byte each) of their
// tile positions. We anchor the name block by its first few entries and locate the coordinate
// arrays by validating that the positions land on overworld landmark tiles.
const LOCATIONS_EXE: &str = "OUT.EXE";
const NAME_ANCHOR: &[u8] = b"Moon\0Fawn\0Paws\0Montor\0";
/// Named overworld locations: 31 towns, 8 castles, 8 monuments, 37 dungeons.
const LOCATION_COUNT: usize = 84;

/// A rendered overworld: the composite image plus the named locations found on it.
pub struct Overworld {
    pub image: RgbImage,
    pub pois: Vec<Poi>,
}

/// Render the full Ultima I overworld and collect its named locations as POIs.
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

    let mut world = RgbImage::new(OVERWORLD_W * TILE_SIZE, OVERWORLD_H * TILE_SIZE);
    let mut grid = vec![0u8; (OVERWORLD_W * OVERWORLD_H) as usize];
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
                grid[tile_y as usize * OVERWORLD_W as usize + tile_x as usize] = tile_index;
            }
        }
    }

    // POIs come from the game's own location table; if it can't be read, the map still renders.
    let pois = read_locations(game_dir, &grid).unwrap_or_default();
    Ok(Overworld { image: world, pois })
}

/// Read Ultima I's named-location table from `OUT.EXE` into POIs. Returns `None` if the file
/// or its tables aren't found (the map then renders without labels).
fn read_locations(game_dir: &Path, grid: &[u8]) -> Option<Vec<Poi>> {
    let exe = std::fs::read(game_dir.join(LOCATIONS_EXE)).ok()?;
    let names_start = find_subslice(&exe, NAME_ANCHOR)?;
    let names = read_names(&exe, names_start, LOCATION_COUNT);
    if names.len() != LOCATION_COUNT {
        return None;
    }
    let coords = locate_coordinate_table(&exe, grid)?;
    let xs = &exe[coords..coords + LOCATION_COUNT];
    let ys = &exe[coords + LOCATION_COUNT..coords + 2 * LOCATION_COUNT];

    let half = TILE_SIZE / 2;
    let pois = (0..LOCATION_COUNT)
        .map(|i| Poi {
            px: u32::from(xs[i]) * TILE_SIZE + half,
            py: u32::from(ys[i]) * TILE_SIZE + half,
            kind: location_kind(i).to_string(),
            label: names[i].clone(),
        })
        .collect();
    Some(pois)
}

/// The category of a location by its index in the table.
fn location_kind(index: usize) -> &'static str {
    match index {
        0..=30 => "town",
        31..=38 => "castle",
        39..=46 => "monument",
        _ => "dungeon",
    }
}

/// Read `count` null-terminated names starting at `start`.
fn read_names(exe: &[u8], start: usize, count: usize) -> Vec<String> {
    let mut names = Vec::with_capacity(count);
    let mut pos = start;
    for _ in 0..count {
        let Some(rel) = exe[pos..].iter().position(|&b| b == 0) else {
            break;
        };
        names.push(String::from_utf8_lossy(&exe[pos..pos + rel]).into_owned());
        pos += rel + 1;
    }
    names
}

/// Locate the coordinate table: two consecutive `LOCATION_COUNT`-byte arrays (all X, then all
/// Y) whose `(x, y)` pairs land on overworld landmark tiles. Returns the X-array offset.
fn locate_coordinate_table(exe: &[u8], grid: &[u8]) -> Option<usize> {
    let n = LOCATION_COUNT;
    if exe.len() < 2 * n {
        return None;
    }
    let (mut best_off, mut best_score) = (None, 0usize);
    for off in 0..=exe.len() - 2 * n {
        let xs = &exe[off..off + n];
        if xs.iter().any(|&b| u32::from(b) >= OVERWORLD_W) {
            continue;
        }
        let ys = &exe[off + n..off + 2 * n];
        if ys.iter().any(|&b| u32::from(b) >= OVERWORLD_H) {
            continue;
        }
        let score = (0..n)
            .filter(|&i| {
                let idx = ys[i] as usize * OVERWORLD_W as usize + xs[i] as usize;
                matches!(grid.get(idx), Some(4..=7))
            })
            .count();
        if score > best_score {
            best_score = score;
            best_off = Some(off);
        }
    }
    // Require a strong majority on landmark tiles so we don't lock onto a coincidental run.
    if best_score >= n * 9 / 10 {
        best_off
    } else {
        None
    }
}

/// Find the first offset of `needle` within `haystack`.
fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_by_table_index() {
        assert_eq!(location_kind(0), "town");
        assert_eq!(location_kind(30), "town");
        assert_eq!(location_kind(31), "castle");
        assert_eq!(location_kind(38), "castle");
        assert_eq!(location_kind(39), "monument");
        assert_eq!(location_kind(46), "monument");
        assert_eq!(location_kind(47), "dungeon");
        assert_eq!(location_kind(83), "dungeon");
    }

    #[test]
    fn reads_null_terminated_names() {
        let blob = b"Moon\0Fawn\0Paws\0";
        assert_eq!(read_names(blob, 0, 3), vec!["Moon", "Fawn", "Paws"]);
    }

    #[test]
    fn finds_subslice() {
        assert_eq!(find_subslice(b"xxMoon\0yy", NAME_ANCHOR), None);
        assert_eq!(find_subslice(b"..needle..", b"needle"), Some(2));
        assert_eq!(find_subslice(b"abc", b"z"), None);
    }
}
