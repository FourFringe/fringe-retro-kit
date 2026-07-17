//! Ultima III (Exodus, DOS) world-map rendering.
//!
//! Overworld: `SOSARIA.ULT` — its first 4096 bytes are a 64×64 tile grid (tile index =
//! `byte >> 2`, the same packing as Ultima II); the remaining bytes hold moon-phase and
//! whirlpool state. Towns and castles are each their own named 4648-byte `.ULT` file
//! (`BRITISH`, `FAWN`, `YEW`, …) with the same 64×64 grid, so their names come straight from the
//! filename. Dungeons are first-person (a different 2192-byte format) and are skipped.
//!
//! Tiles: `SHAPES.ULT` — 80 tiles of 16×16 CGA 2-bpp graphics (64 bytes each, no header), so the
//! [`crate::cga`] decoder is reused directly (with a gently muted palette — see [`SOFT_PALETTE`]).
//!
//! Overworld points of interest are read from the location table in `EXODUS.BIN`: a run of 19
//! `(x, y)` byte pairs for the overworld entrances, ordered as the two castles, then the ten
//! towns, then the seven dungeons. Cross-referencing those coordinates with the map (and known
//! adjacencies like Britain beside Lord British's Castle, and Montor East/West) pins each to its
//! real name.

use std::path::Path;

use anyhow::{ensure, Context, Result};
use image::{Rgb, RgbImage};

use crate::bundle::{Poi, World, WorldMeta};
use crate::cga::{self, TILE_SIZE};

/// Map edge length, in tiles.
const MAP_W: usize = 64;
const MAP_H: usize = 64;
/// Bytes of tile grid at the start of every `.ULT` map file (one byte per tile).
const MAP_GRID_LEN: usize = MAP_W * MAP_H;

const TILESET_FILE: &str = "SHAPES.ULT";
const OVERWORLD_FILE: &str = "SOSARIA.ULT";
/// The active-party save; holds the overworld position.
const SAVE_FILE: &str = "PARTY.ULT";
/// The game executable, which embeds the overworld location table.
const EXE_FILE: &str = "EXODUS.BIN";

/// A gently muted CGA palette 1. Pure black/cyan/magenta/white is eye-searing on Ultima III's
/// mostly-black Sosaria, so we dim and desaturate it a touch for a more readable map. This only
/// changes the colours, not the pixels, so tiles stay accurate.
const SOFT_PALETTE: [Rgb<u8>; 4] = [
    Rgb([0x14, 0x16, 0x22]), // near-black, faintly blue (ocean)
    Rgb([0x5f, 0xac, 0xac]), // muted cyan
    Rgb([0xb0, 0x74, 0xb0]), // muted magenta
    Rgb([0xd2, 0xd2, 0xde]), // soft off-white
];

/// Every world shares one region so the browser nests the towns under the overworld.
const GROUP: &str = "sosaria";

/// The overworld locations named in the browser, **in the order of the `EXODUS.BIN` coordinate
/// table**: the two castles, then the ten towns, then the seven dungeons. (Ambrosia is reached
/// by whirlpool and has no overworld entrance, so it isn't here.) The dungeons keep a generic
/// label — their table order isn't independently verified, and Ultima III's dungeons are
/// first-person, so we don't render them.
const LOCATIONS: [(&str, &str); 19] = [
    ("Lord British's Castle", "castle"),
    ("Castle of Exodus", "castle"),
    ("Britain", "town"),
    ("Moon", "town"),
    ("Yew", "town"),
    ("Montor East", "town"),
    ("Montor West", "town"),
    ("Grey", "town"),
    ("Dawn", "town"),
    ("Devil Guard", "town"),
    ("Fawn", "town"),
    ("Death Gulch", "town"),
    ("Dungeon", "dungeon"),
    ("Dungeon", "dungeon"),
    ("Dungeon", "dungeon"),
    ("Dungeon", "dungeon"),
    ("Dungeon", "dungeon"),
    ("Dungeon", "dungeon"),
    ("Dungeon", "dungeon"),
];

/// Named town and castle maps: `(filename, display title, kind)`. Each is a 64×64 `.ULT` grid in
/// the same format as the overworld. Dungeons (first-person) and combat arenas are omitted.
const TOWNS: &[(&str, &str, &str)] = &[
    ("LCB.ULT", "Lord British's Castle", "castle"),
    ("EXODUS.ULT", "Castle of Exodus", "castle"),
    ("BRITISH.ULT", "Britain", "town"),
    ("YEW.ULT", "Yew", "town"),
    ("MOON.ULT", "Moon", "town"),
    ("GREY.ULT", "Grey", "town"),
    ("DAWN.ULT", "Dawn", "town"),
    ("DEVIL.ULT", "Devil Guard", "town"),
    ("DEATH.ULT", "Death Gulch", "town"),
    ("FAWN.ULT", "Fawn", "town"),
    ("MONTOR_E.ULT", "Montor East", "town"),
    ("MONTOR_W.ULT", "Montor West", "town"),
    ("AMBROSIA.ULT", "Ambrosia", "town"),
];

/// Render Ultima III into its worlds: the Sosaria overworld plus each named town and castle.
pub fn export_worlds(game_dir: &Path) -> Result<Vec<World>> {
    let tiles = read_tileset(game_dir)?;
    ensure!(!tiles.is_empty(), "{TILESET_FILE} contained no tiles");

    let mut worlds = Vec::with_capacity(1 + TOWNS.len());

    let overworld = read_grid(&game_dir.join(OVERWORLD_FILE))?;
    worlds.push(world(
        "sosaria",
        "Ultima III — Sosaria",
        "overworld",
        overworld_pois(game_dir, &overworld),
        render(&overworld, &tiles),
    ));

    for (file, title, kind) in TOWNS {
        let path = game_dir.join(file);
        if !path.exists() {
            continue; // Not every install ships every map; skip missing ones.
        }
        let grid = read_grid(&path)?;
        let world_id = file.trim_end_matches(".ULT").to_ascii_lowercase();
        worlds.push(world(
            &world_id,
            &format!("Ultima III — {title}"),
            kind,
            vec![],
            render(&grid, &tiles),
        ));
    }

    Ok(worlds)
}

/// The party's overworld position (in tiles) from `PARTY.ULT`, or `None` if the party isn't on
/// the overworld. Byte `0x02` is the location (0 = Sosaria); `0x08`/`0x09` are the map X/Y.
pub fn player_position(game_dir: &Path) -> Result<Option<(u32, u32)>> {
    let path = game_dir.join(SAVE_FILE);
    if !path.exists() {
        return Ok(None);
    }
    let data = std::fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
    // Only report a position while the party is on the overworld; in a town the X/Y are local.
    if data.len() < 0x0A || data[0x02] != 0 {
        return Ok(None);
    }
    Ok(Some((u32::from(data[0x08]), u32::from(data[0x09]))))
}

/// Assemble a [`World`] with Ultima III's shared game id and region group.
fn world(id: &str, title: &str, kind: &str, pois: Vec<Poi>, image: RgbImage) -> World {
    World {
        meta: WorldMeta {
            game: "ultima3".into(),
            world: id.into(),
            title: title.into(),
            kind: kind.into(),
            group: GROUP.into(),
        },
        image,
        pois,
    }
}

/// The location type of a Sosaria landmark tile, if it is one. Derived authoritatively from the
/// `EXODUS.BIN` location table: its 19 overworld entrances land on tile `5` (the 7 dungeons),
/// tile `6` (towns), and tile `7` (the 2 castles). The town of Dawn's entrance sits on plain
/// grass (tile `3`), the way it only appears at the new moons.
fn landmark(tile_index: u8) -> Option<&'static str> {
    match tile_index {
        5 => Some("dungeon"),
        3 | 6 => Some("town"),
        7 => Some("castle"),
        _ => None,
    }
}

/// Overworld points of interest for Sosaria. Prefers authoritative, **named** markers read from
/// the `EXODUS.BIN` location table; if that can't be located (e.g. a different build), it falls
/// back to typed-but-unnamed markers scanned from the map itself.
fn overworld_pois(game_dir: &Path, grid: &[u8]) -> Vec<Poi> {
    named_pois(game_dir, grid).unwrap_or_else(|| tile_scan_pois(grid))
}

/// Build named POIs from the `EXODUS.BIN` location table, or `None` if it can't be located.
fn named_pois(game_dir: &Path, grid: &[u8]) -> Option<Vec<Poi>> {
    let exe = std::fs::read(game_dir.join(EXE_FILE)).ok()?;
    let start = find_location_table(&exe, grid)?;
    let pois = LOCATIONS
        .iter()
        .enumerate()
        .map(|(i, (label, kind))| {
            let x = u32::from(exe[start + 2 * i]);
            let y = u32::from(exe[start + 2 * i + 1]);
            Poi {
                px: x * TILE_SIZE + TILE_SIZE / 2,
                py: y * TILE_SIZE + TILE_SIZE / 2,
                kind: (*kind).to_string(),
                label: (*label).to_string(),
            }
        })
        .collect();
    Some(pois)
}

/// Locate the location table in `EXODUS.BIN`: the run of [`LOCATIONS`]`.len()` `(x, y)` byte
/// pairs whose coordinates each land on a landmark tile matching that location's expected kind.
/// That full kind sequence (castle, castle, ten towns, seven dungeons) is a strong signature, so
/// this finds the table without hard-coding an offset.
fn find_location_table(exe: &[u8], grid: &[u8]) -> Option<usize> {
    let span = LOCATIONS.len() * 2;
    if exe.len() < span {
        return None;
    }
    (0..=exe.len() - span).find(|&start| {
        LOCATIONS.iter().enumerate().all(|(i, (_, kind))| {
            let x = exe[start + 2 * i] as usize;
            let y = exe[start + 2 * i + 1] as usize;
            x < MAP_W && y < MAP_H && landmark(grid[y * MAP_W + x] >> 2) == Some(*kind)
        })
    })
}

/// Scan the overworld grid for landmark tiles and emit a typed but unnamed POI at each — the
/// fallback when the location table can't be read.
fn tile_scan_pois(grid: &[u8]) -> Vec<Poi> {
    let label = |kind: &str| match kind {
        "castle" => "Castle",
        "dungeon" => "Dungeon",
        _ => "Town",
    };
    let mut pois = Vec::new();
    for (i, &byte) in grid.iter().enumerate() {
        if let Some(kind) = landmark(byte >> 2) {
            let x = (i % MAP_W) as u32;
            let y = (i / MAP_W) as u32;
            pois.push(Poi {
                px: x * TILE_SIZE + TILE_SIZE / 2,
                py: y * TILE_SIZE + TILE_SIZE / 2,
                kind: kind.to_string(),
                label: label(kind).to_string(),
            });
        }
    }
    pois
}

/// Decode `SHAPES.ULT` into its 80 CGA tiles.
fn read_tileset(game_dir: &Path) -> Result<Vec<RgbImage>> {
    let data = std::fs::read(game_dir.join(TILESET_FILE))
        .with_context(|| format!("reading {TILESET_FILE} from {}", game_dir.display()))?;
    Ok(data
        .chunks_exact(cga::BYTES_PER_TILE)
        .map(|tile| cga::decode_tile(tile, &SOFT_PALETTE))
        .collect())
}

/// Read the 64×64 tile grid from the front of a `.ULT` map file.
fn read_grid(path: &Path) -> Result<Vec<u8>> {
    let data = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    ensure!(
        data.len() >= MAP_GRID_LEN,
        "{} is {} bytes; expected at least {MAP_GRID_LEN}",
        path.display(),
        data.len()
    );
    Ok(data[..MAP_GRID_LEN].to_vec())
}

/// Composite a 64×64 tile grid into a full-resolution image.
fn render(grid: &[u8], tiles: &[RgbImage]) -> RgbImage {
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
    image
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_composites_grid_at_tile_size() {
        let tiles: Vec<RgbImage> = (0..4)
            .map(|i| RgbImage::from_pixel(TILE_SIZE, TILE_SIZE, image::Rgb([i as u8, 0, 0])))
            .collect();
        let grid = vec![0u8; MAP_GRID_LEN]; // all tile 0
        let img = render(&grid, &tiles);
        assert_eq!(img.width(), MAP_W as u32 * TILE_SIZE);
        assert_eq!(img.height(), MAP_H as u32 * TILE_SIZE);
        assert_eq!(*img.get_pixel(0, 0), image::Rgb([0, 0, 0]));
    }

    #[test]
    fn grid_uses_high_six_bits_as_tile_index() {
        // Byte 0x84 (132) → tile 33, the way SOSARIA.ULT encodes a landmark.
        let mut tiles: Vec<RgbImage> = (0..34)
            .map(|_| RgbImage::from_pixel(TILE_SIZE, TILE_SIZE, image::Rgb([0, 0, 0])))
            .collect();
        tiles[33] = RgbImage::from_pixel(TILE_SIZE, TILE_SIZE, image::Rgb([9, 9, 9]));
        let mut grid = vec![0u8; MAP_GRID_LEN];
        grid[0] = 132;
        let img = render(&grid, &tiles);
        assert_eq!(*img.get_pixel(0, 0), image::Rgb([9, 9, 9]));
    }

    #[test]
    fn tile_scan_pois_type_landmarks() {
        let mut grid = vec![0u8; MAP_GRID_LEN];
        grid[0] = 5 << 2; // dungeon at (0,0)
        grid[1] = 6 << 2; // town at (1,0)
        grid[2] = 7 << 2; // castle at (2,0)
        grid[3] = 4 << 2; // mountains (terrain) — not a landmark
        let pois = tile_scan_pois(&grid);
        let kinds: Vec<&str> = pois.iter().map(|p| p.kind.as_str()).collect();
        assert_eq!(pois.len(), 3);
        assert!(kinds.contains(&"dungeon"));
        assert!(kinds.contains(&"town"));
        assert!(kinds.contains(&"castle"));
        // The town POI sits at the centre of tile (1,0).
        let town = pois.iter().find(|p| p.kind == "town").unwrap();
        assert_eq!(
            (town.px, town.py),
            (TILE_SIZE + TILE_SIZE / 2, TILE_SIZE / 2)
        );
    }

    #[test]
    fn locates_table_by_kind_signature() {
        // A 64×64 grid with landmark tiles placed so exactly one 19-entry (x,y) run in the
        // synthetic "exe" matches the LOCATIONS kind sequence (castle, castle, 10 towns, 7 dungeons).
        let mut grid = vec![0u8; MAP_GRID_LEN];
        let mut exe = Vec::new();
        for (i, (_, kind)) in LOCATIONS.iter().enumerate() {
            let x = i; // distinct positions along row 0
            let y = 1;
            let tile = match *kind {
                "castle" => 7u8,
                "dungeon" => 5,
                _ => 6,
            };
            grid[y * MAP_W + x] = tile << 2;
            exe.push(x as u8);
            exe.push(y as u8);
        }
        let start = find_location_table(&exe, &grid).expect("table located");
        assert_eq!(start, 0);
    }
}
