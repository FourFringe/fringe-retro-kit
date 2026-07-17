//! Ultima V (Warriors of Destiny, DOS) world-map rendering.
//!
//! Two full 256×256 tile worlds, each built from **16×16-tile chunks** (256 chunks in a 16×16
//! grid):
//!
//! - **Britannia** (`BRIT.DAT`) stores only its 205 non-water chunks, concatenated 256 bytes
//!   each. A 256-entry layout table in `DATA.OVL` maps each grid cell to a chunk index, or `0xFF`
//!   for open ocean (rendered as deep water). The table is a bijection over the stored chunks, so
//!   it's found by signature rather than a hard-coded offset (see [`find_chunk_layout`]).
//! - **The Underworld** (`UNDER.DAT`) stores all 256 chunks in grid order, so it assembles
//!   linearly.
//!
//! In both, a chunk's bytes are tile indices directly.
//!
//! Towns, dwellings, castles and keeps are 32×32-tile maps stored 16-to-a-file in `TOWNE.DAT`,
//! `DWELLING.DAT`, `CASTLE.DAT` and `KEEP.DAT`. `DATA.OVL` holds, per file, the first map index of
//! each of its eight locations (so multi-floor places like Lord British's Castle can be split into
//! levels), plus a 40-entry table of every enterable location's overworld `(x, y)` — used both to
//! place named points of interest and to tell whether a location sits on the surface or in the
//! Underworld. The eight dungeons are first-person, so they only contribute a marker.
//!
//! Tiles: `TILES.16` — 512 tiles of 16×16, 4-bit EGA graphics (two pixels per byte, high nibble
//! first), **LZW-compressed** with the Ultima 6-style codec (see [`crate::lzw`]).

use std::path::Path;

use anyhow::{ensure, Context, Result};
use image::RgbImage;

use crate::bundle::{Poi, World};
use crate::ega::EGA_PALETTE;
use crate::lzw;
use crate::tilemap::{self, TILE_SIZE};

/// World edge length, in tiles (both Britannia and the Underworld are 256×256).
const WORLD_W: usize = 256;
const WORLD_H: usize = 256;
/// Worlds are a 16×16 grid of 16×16-tile chunks; each chunk is 256 bytes (one per tile).
const CHUNK: usize = 16;
const CHUNKS_PER_ROW: usize = WORLD_W / CHUNK; // 16
const CHUNK_BYTES: usize = CHUNK * CHUNK; // 256
/// The layout table has one entry per grid cell.
const LAYOUT_LEN: usize = CHUNKS_PER_ROW * CHUNKS_PER_ROW; // 256
/// Layout entry for an open-ocean cell (no stored chunk).
const WATER_CHUNK: u8 = 0xFF;
/// Deep-water tile, used to fill open-ocean cells.
const WATER_TILE: u8 = 1;

/// One tile is 16×16 pixels at 4 bits per pixel: 128 bytes.
const TILE_BYTES: usize = 128;

/// Town/dwelling/castle/keep maps are 32×32 tiles, one byte each (1024 bytes per floor).
const LOC_MAP: usize = 32;
const LOC_MAP_BYTES: usize = LOC_MAP * LOC_MAP;
/// Each location file holds 16 floors, grouped into eight locations.
const LOCS_PER_FILE: usize = 8;
const MAPS_PER_FILE: usize = 16;

/// The four top-down location files, in the order their `DATA.OVL` tables are laid out.
const LOC_FILES: [&str; 4] = ["TOWNE.DAT", "DWELLING.DAT", "CASTLE.DAT", "KEEP.DAT"];

/// `DATA.OVL` offsets (standard DOS build). For each location file, an 8-byte array of the first
/// map index of each location; and two 40-byte arrays of every enterable location's overworld
/// `(x, y)`. The 40 entries are in Party-Location order (see [`LOCATIONS`]).
const START_OFFS: [usize; 4] = [0x1E2A, 0x1E32, 0x1E3A, 0x1E42];
const LOC_X_OFF: usize = 0x1E9A;
const LOC_Y_OFF: usize = 0x1EC2;
const LOC_COUNT: usize = 40;

const TILESET_FILE: &str = "TILES.16";
const OVERWORLD_FILE: &str = "BRIT.DAT";
const UNDERWORLD_FILE: &str = "UNDER.DAT";
/// Holds the Britannia chunk-layout table.
const DATA_FILE: &str = "DATA.OVL";
/// The saved game; holds the party's world position.
const SAVE_FILE: &str = "SAVED.GAM";

/// This game's identifier, shared by every world it exports.
const GAME: &str = "ultima5";

/// The 40 enterable locations in Party-Location order — the order of the `DATA.OVL` coordinate and
/// first-map-index tables: 8 towns, 8 dwellings, 8 castles/villages, 8 keeps, then 8 dungeons. Each
/// is `(name, kind)`, where `kind` is the marker/badge style. The first 32 have top-down maps (the
/// four location files, eight each); the eight dungeons are first-person and only get a marker.
const LOCATIONS: [(&str, &str); LOC_COUNT] = [
    ("Moonglow", "town"),
    ("Britain", "town"),
    ("Jhelom", "town"),
    ("Yew", "town"),
    ("Minoc", "town"),
    ("Trinsic", "town"),
    ("Skara Brae", "town"),
    ("New Magincia", "town"),
    ("Fogsbane", "village"),
    ("Stormcrow", "village"),
    ("Greyhaven", "village"),
    ("Waveguide", "village"),
    ("Iolo's Hut", "village"),
    ("Sutek's Hut", "village"),
    ("Sin'Vraal's Hut", "village"),
    ("Grendel's Hut", "village"),
    ("Lord British's Castle", "castle"),
    ("Palace of Blackthorn", "castle"),
    ("West Britanny", "village"),
    ("North Britanny", "village"),
    ("East Britanny", "village"),
    ("Paws", "village"),
    ("Cove", "village"),
    ("Buccaneer's Den", "town"),
    ("Ararat", "castle"),
    ("Bordermarch", "castle"),
    ("Farthing", "castle"),
    ("Windemere", "castle"),
    ("Stonegate", "castle"),
    ("The Lycaeum", "castle"),
    ("Empath Abbey", "castle"),
    ("Serpent's Hold", "castle"),
    ("Deceit", "dungeon"),
    ("Despise", "dungeon"),
    ("Destard", "dungeon"),
    ("Wrong", "dungeon"),
    ("Covetous", "dungeon"),
    ("Shame", "dungeon"),
    ("Hythloth", "dungeon"),
    ("Doom", "dungeon"),
];

/// Render Ultima V into its worlds: the Britannia surface, the Underworld, and every town,
/// dwelling, castle and keep (one map per floor).
pub fn export_worlds(game_dir: &Path) -> Result<Vec<World>> {
    let tiles = read_tileset(game_dir)?;
    let data = std::fs::read(game_dir.join(DATA_FILE))
        .with_context(|| format!("reading {DATA_FILE} from {}", game_dir.display()))?;

    let britannia = read_britannia(game_dir, &data)?;
    let underworld = read_underworld(game_dir)?;
    let (brit_pois, under_pois) = location_pois(&data, &britannia);

    let mut worlds = vec![
        tilemap::world(
            GAME,
            "britannia",
            "Ultima V — Britannia",
            "overworld",
            "britannia",
            brit_pois,
            tilemap::render(&britannia, WORLD_W, WORLD_H, &tiles),
        ),
        tilemap::world(
            GAME,
            "underworld",
            "Ultima V — The Underworld",
            "overworld",
            "underworld",
            under_pois,
            tilemap::render(&underworld, WORLD_W, WORLD_H, &tiles),
        ),
    ];
    worlds.extend(location_worlds(game_dir, &data, &tiles)?);
    Ok(worlds)
}

/// The party's world position from `SAVED.GAM`, or `None` if the party isn't on a world map. The
/// bool is `true` for the Underworld. Location `0x2ED` is `0` on the world map; the Z-coordinate
/// `0x2EF` is `0` on the surface and `0xFF` in the Underworld (`1`–`7` inside a dungeon); X/Y are
/// at `0x2F0`/`0x2F1`.
pub fn player_position(game_dir: &Path) -> Result<Option<(bool, u32, u32)>> {
    let path = game_dir.join(SAVE_FILE);
    if !path.exists() {
        return Ok(None);
    }
    let data = std::fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
    if data.len() < 0x2F2 || data[0x2ED] != 0 {
        return Ok(None);
    }
    let underworld = match data[0x2EF] {
        0 => false,
        0xFF => true,
        _ => return Ok(None), // inside a dungeon; X/Y are local
    };
    Ok(Some((
        underworld,
        u32::from(data[0x2F0]),
        u32::from(data[0x2F1]),
    )))
}

/// Decode `TILES.16` (LZW-compressed) into its 512 tiles.
fn read_tileset(game_dir: &Path) -> Result<Vec<RgbImage>> {
    let packed = std::fs::read(game_dir.join(TILESET_FILE))
        .with_context(|| format!("reading {TILESET_FILE} from {}", game_dir.display()))?;
    let raw = lzw::decompress(&packed).with_context(|| format!("decompressing {TILESET_FILE}"))?;
    let tiles: Vec<RgbImage> = raw.chunks_exact(TILE_BYTES).map(decode_tile).collect();
    ensure!(!tiles.is_empty(), "{TILESET_FILE} contained no tiles");
    Ok(tiles)
}

/// Decode one 128-byte, 4-bit tile (two pixels per byte, high nibble = left pixel) into a 16×16
/// image using the standard EGA palette.
fn decode_tile(bytes: &[u8]) -> RgbImage {
    let mut img = RgbImage::new(TILE_SIZE, TILE_SIZE);
    for y in 0..TILE_SIZE {
        for x in 0..(TILE_SIZE / 2) {
            let byte = bytes[(y * (TILE_SIZE / 2) + x) as usize];
            img.put_pixel(x * 2, y, EGA_PALETTE[usize::from(byte >> 4)]);
            img.put_pixel(x * 2 + 1, y, EGA_PALETTE[usize::from(byte & 0x0F)]);
        }
    }
    img
}

/// Assemble the Britannia surface: its stored chunks placed by the `DATA.OVL` layout table, with
/// open-ocean cells filled with deep water.
fn read_britannia(game_dir: &Path, data: &[u8]) -> Result<Vec<u8>> {
    let chunks = std::fs::read(game_dir.join(OVERWORLD_FILE))
        .with_context(|| format!("reading {OVERWORLD_FILE} from {}", game_dir.display()))?;
    let chunk_count = chunks.len() / CHUNK_BYTES;
    ensure!(chunk_count > 0, "{OVERWORLD_FILE} contained no chunks");

    let layout_off = find_chunk_layout(data, chunk_count)
        .with_context(|| format!("locating the Britannia chunk layout in {DATA_FILE}"))?;
    let layout = &data[layout_off..layout_off + LAYOUT_LEN];

    let mut grid = vec![WATER_TILE; WORLD_W * WORLD_H];
    for gy in 0..CHUNKS_PER_ROW {
        for gx in 0..CHUNKS_PER_ROW {
            let entry = layout[gy * CHUNKS_PER_ROW + gx];
            if entry == WATER_CHUNK {
                continue; // open ocean; grid is already deep water
            }
            place_chunk(&mut grid, &chunks[entry as usize * CHUNK_BYTES..], gx, gy);
        }
    }
    Ok(grid)
}

/// Assemble the Underworld: all 256 chunks are stored in grid order, one after another.
fn read_underworld(game_dir: &Path) -> Result<Vec<u8>> {
    let chunks = std::fs::read(game_dir.join(UNDERWORLD_FILE))
        .with_context(|| format!("reading {UNDERWORLD_FILE} from {}", game_dir.display()))?;
    ensure!(
        chunks.len() >= LAYOUT_LEN * CHUNK_BYTES,
        "{UNDERWORLD_FILE} is {} bytes; expected at least {}",
        chunks.len(),
        LAYOUT_LEN * CHUNK_BYTES
    );
    let mut grid = vec![0u8; WORLD_W * WORLD_H];
    for gy in 0..CHUNKS_PER_ROW {
        for gx in 0..CHUNKS_PER_ROW {
            let ci = gy * CHUNKS_PER_ROW + gx;
            place_chunk(&mut grid, &chunks[ci * CHUNK_BYTES..], gx, gy);
        }
    }
    Ok(grid)
}

/// Copy one 16×16-tile chunk into the world grid at chunk cell `(gx, gy)`.
fn place_chunk(grid: &mut [u8], chunk: &[u8], gx: usize, gy: usize) {
    for ty in 0..CHUNK {
        for tx in 0..CHUNK {
            let x = gx * CHUNK + tx;
            let y = gy * CHUNK + ty;
            grid[y * WORLD_W + x] = chunk[ty * CHUNK + tx];
        }
    }
}

/// Locate the Britannia chunk-layout table in `DATA.OVL`: the [`LAYOUT_LEN`]-byte window whose
/// non-`0xFF` bytes are a bijection over the `chunk_count` stored chunks (each index `0..count`
/// appears exactly once). That's a strong, unique signature, so no offset is hard-coded.
fn find_chunk_layout(data: &[u8], chunk_count: usize) -> Option<usize> {
    if data.len() < LAYOUT_LEN {
        return None;
    }
    (0..=data.len() - LAYOUT_LEN)
        .find(|&off| is_chunk_layout(&data[off..off + LAYOUT_LEN], chunk_count))
}

/// Whether `window`'s non-`0xFF` bytes are exactly the chunk indices `0..chunk_count`, once each.
fn is_chunk_layout(window: &[u8], chunk_count: usize) -> bool {
    let mut seen = [false; 256];
    let mut count = 0usize;
    for &b in window {
        if b == WATER_CHUNK {
            continue;
        }
        let i = usize::from(b);
        if i >= chunk_count || seen[i] {
            return false;
        }
        seen[i] = true;
        count += 1;
    }
    count == chunk_count
}

/// Overworld points of interest from the `DATA.OVL` location table, split into the two worlds. A
/// location's `(x, y)` is looked up on Britannia; if that tile is open water it belongs to the
/// Underworld instead (only Doom does). Returns `(britannia, underworld)`.
fn location_pois(data: &[u8], britannia: &[u8]) -> (Vec<Poi>, Vec<Poi>) {
    let mut brit = Vec::new();
    let mut under = Vec::new();
    if data.len() < LOC_X_OFF + LOC_COUNT || data.len() < LOC_Y_OFF + LOC_COUNT {
        return (brit, under); // unexpected build; ship the maps without markers
    }
    for (i, (name, kind)) in LOCATIONS.iter().enumerate() {
        let x = data[LOC_X_OFF + i];
        let y = data[LOC_Y_OFF + i];
        let marker = tilemap::poi(u32::from(x), u32::from(y), kind, name);
        if britannia[usize::from(y) * WORLD_W + usize::from(x)] == WATER_TILE {
            under.push(marker);
        } else {
            brit.push(marker);
        }
    }
    (brit, under)
}

/// Build a [`World`] for every floor of every town, dwelling, castle and keep. Each location file
/// holds 16 floors; the `DATA.OVL` first-map-index array partitions them among the file's eight
/// locations, so a location's floors are `[start[i], start[i + 1])`.
fn location_worlds(game_dir: &Path, data: &[u8], tiles: &[RgbImage]) -> Result<Vec<World>> {
    let mut worlds = Vec::new();
    for (fi, file) in LOC_FILES.iter().enumerate() {
        let path = game_dir.join(file);
        if !path.exists() {
            continue; // not every install ships every file
        }
        let bytes = std::fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
        let start_off = START_OFFS[fi];
        if bytes.len() < MAPS_PER_FILE * LOC_MAP_BYTES || data.len() < start_off + LOCS_PER_FILE {
            continue;
        }
        let starts = &data[start_off..start_off + LOCS_PER_FILE];
        for loc in 0..LOCS_PER_FILE {
            let (name, kind) = LOCATIONS[fi * LOCS_PER_FILE + loc];
            let first = usize::from(starts[loc]);
            let end = starts
                .get(loc + 1)
                .map_or(MAPS_PER_FILE, |&s| usize::from(s));
            if first >= end || end > MAPS_PER_FILE {
                continue; // malformed range; skip this location
            }
            let floors = end - first;
            for floor in 0..floors {
                let map = &bytes[(first + floor) * LOC_MAP_BYTES..][..LOC_MAP_BYTES];
                let (id, title) = if floors > 1 {
                    (
                        format!("{}-l{}", slug(name), floor + 1),
                        format!("Ultima V — {name} (level {})", floor + 1),
                    )
                } else {
                    (slug(name), format!("Ultima V — {name}"))
                };
                worlds.push(tilemap::world(
                    GAME,
                    &id,
                    &title,
                    kind,
                    "britannia",
                    Vec::new(),
                    tilemap::render(map, LOC_MAP, LOC_MAP, tiles),
                ));
            }
        }
    }
    Ok(worlds)
}

/// A URL-safe slug of a location name (lowercase, non-alphanumeric runs collapsed to `-`).
fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    out.trim_end_matches('-').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locates_bijection_layout() {
        // A DATA.OVL-like buffer: some filler, then a layout window using chunks 0..count once
        // (with water fill), then more filler. find_chunk_layout should return the window offset.
        let count = 10usize;
        let mut window = vec![WATER_CHUNK; LAYOUT_LEN];
        for (i, slot) in window.iter_mut().take(count).enumerate() {
            *slot = i as u8;
        }
        let mut data = vec![0u8; 50]; // filler of valid-looking small indices
        let off = data.len();
        data.extend_from_slice(&window);
        data.extend_from_slice(&[0u8; 30]);
        assert_eq!(find_chunk_layout(&data, count), Some(off));
    }

    #[test]
    fn rejects_non_bijection_window() {
        // A window that reuses a chunk index isn't a valid layout.
        let mut window = vec![WATER_CHUNK; LAYOUT_LEN];
        window[0] = 0;
        window[1] = 0; // duplicate
        assert!(!is_chunk_layout(&window, 2));
    }

    #[test]
    fn place_chunk_positions_tiles() {
        let mut grid = vec![0u8; WORLD_W * WORLD_H];
        let chunk: Vec<u8> = (0..CHUNK_BYTES as u16).map(|v| (v % 256) as u8).collect();
        // Place the chunk at cell (2, 1): world x 32..48, y 16..32.
        place_chunk(&mut grid, &chunk, 2, 1);
        // Top-left tile of the chunk lands at (32, 16); its value is chunk[0] = 0.
        assert_eq!(grid[16 * WORLD_W + 32], 0);
        // Chunk tile (tx=3, ty=1) = chunk[19] lands at world (35, 17).
        assert_eq!(grid[17 * WORLD_W + 35], 19);
    }

    #[test]
    fn decode_tile_splits_nibbles() {
        // A tile whose first byte is 0x1F: left pixel colour 1, right pixel colour 15.
        let mut bytes = [0u8; TILE_BYTES];
        bytes[0] = 0x1F;
        let img = decode_tile(&bytes);
        assert_eq!(*img.get_pixel(0, 0), EGA_PALETTE[1]);
        assert_eq!(*img.get_pixel(1, 0), EGA_PALETTE[15]);
    }

    #[test]
    fn location_table_matches_count() {
        assert_eq!(LOCATIONS.len(), LOC_COUNT);
    }

    #[test]
    fn slug_is_url_safe() {
        assert_eq!(slug("Lord British's Castle"), "lord-british-s-castle");
        assert_eq!(slug("New Magincia"), "new-magincia");
        assert_eq!(slug("Yew"), "yew");
    }

    #[test]
    fn location_pois_split_by_world() {
        // A minimal DATA.OVL: the X/Y arrays place location 0 on land and the last on water.
        let mut data = vec![0u8; LOC_Y_OFF + LOC_COUNT];
        for i in 0..LOC_COUNT {
            data[LOC_X_OFF + i] = i as u8;
            data[LOC_Y_OFF + i] = 0;
        }
        let mut britannia = vec![5u8; WORLD_W * WORLD_H]; // all land
                                                          // Put the last location's tile (x = LOC_COUNT-1, y = 0) on deep water → Underworld.
        britannia[LOC_COUNT - 1] = WATER_TILE;
        let (brit, under) = location_pois(&data, &britannia);
        assert_eq!(brit.len(), LOC_COUNT - 1);
        assert_eq!(under.len(), 1);
        assert_eq!(under[0].label, LOCATIONS[LOC_COUNT - 1].0);
    }
}
