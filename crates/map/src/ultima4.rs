//! Ultima IV (Quest of the Avatar, DOS) world-map rendering.
//!
//! Overworld: `WORLD.MAP` — a 256×256 tile map (one byte per tile, the byte *is* the tile index)
//! stored as an **8×8 grid of 32×32-tile chunks** in chunk-major order, so it must be de-chunked
//! back into a linear grid before rendering.
//!
//! Towns, villages and castles are each their own `.ULT` file: a 32×32 tile grid in the first
//! 1024 bytes (the trailing 256 bytes are NPC data), named by file. Dungeons are first-person
//! (`.DNG`) and skipped.
//!
//! Tiles: `SHAPES.EGA` — 256 tiles of 16×16 EGA graphics in the same 4-plane, row-interleaved
//! layout as Ultima I, so the [`crate::ega`] decoder is reused directly — but with a gently
//! brightened palette (see [`BRIGHT_PALETTE`]), because Britannia's deep-water blue is so dark it
//! muddies to near-black when the map is zoomed out to fit.
//!
//! Overworld points of interest are read from the location table in `AVATAR.EXE`: two parallel
//! 32-byte arrays (all the X coordinates, then all the Y coordinates), one entry per location in
//! the game's map-index order — the sixteen cities, the eight dungeons, then the eight shrines.
//! Each coordinate lands on its matching landmark tile, and that full kind sequence is a strong
//! signature, so the table is found without hard-coding an offset.

use std::path::Path;

use anyhow::{ensure, Context, Result};
use image::{Rgb, RgbImage};

use crate::bundle::{Poi, World, WorldMeta};
use crate::ega::{self, TILE_SIZE};

/// Overworld edge length, in tiles.
const WORLD_W: usize = 256;
const WORLD_H: usize = 256;
/// The overworld is stored as an 8×8 grid of 32×32-tile chunks.
const CHUNK: usize = 32;
const CHUNKS_PER_ROW: usize = WORLD_W / CHUNK; // 8

/// Town/village/castle maps are 32×32; their grid is the first 1024 bytes of the `.ULT` file.
const TOWN_W: usize = 32;
const TOWN_H: usize = 32;
const TOWN_GRID_LEN: usize = TOWN_W * TOWN_H;

const TILESET_FILE: &str = "SHAPES.EGA";
const OVERWORLD_FILE: &str = "WORLD.MAP";
/// The saved game; holds the party's overworld position.
const SAVE_FILE: &str = "PARTY.SAV";
/// The game executable, which embeds the overworld location table.
const EXE_FILE: &str = "AVATAR.EXE";

/// Every world shares one region so the browser nests the towns under the overworld.
const GROUP: &str = "britannia";

/// A gently brightened EGA palette for the overworld. Ultima IV's tiles are a dark stipple: even
/// deep water and grass are roughly four-fifths *pure black* (EGA colour 0) with only sparse
/// coloured specks, so at fit-zoom every tile averages toward black and the ocean reads as dead
/// space. Two adjustments fix that without washing the map out: colour 0 is lifted from black to a
/// dark navy, giving the ocean (and the shadows under land) a legible blue floor; and the other
/// colours get a `≈0.6` gamma lift so the terrain specks stay distinct above that floor. Only the
/// colours change, not the pixels, so tiles stay accurate.
const BRIGHT_PALETTE: [Rgb<u8>; 16] = [
    Rgb([16, 18, 44]),    // black → dark navy floor (deep water, shadow)
    Rgb([0, 0, 200]),     // blue
    Rgb([0, 200, 0]),     // green
    Rgb([0, 200, 200]),   // cyan
    Rgb([200, 0, 0]),     // red
    Rgb([200, 0, 200]),   // magenta
    Rgb([200, 132, 0]),   // brown
    Rgb([200, 200, 200]), // light grey
    Rgb([132, 132, 132]), // dark grey
    Rgb([132, 132, 255]), // bright blue
    Rgb([132, 255, 132]), // bright green
    Rgb([132, 255, 255]), // bright cyan
    Rgb([255, 132, 132]), // bright red
    Rgb([255, 132, 255]), // bright magenta
    Rgb([255, 255, 132]), // yellow
    Rgb([255, 255, 255]), // white
];

/// The overworld locations named in the browser, **in the order of the `AVATAR.EXE` coordinate
/// table**: the sixteen cities (Lord British's Castle, the three other castles/abbeys, the eight
/// towns, and the four villages), the eight dungeons, then the eight shrines. Each entry is
/// `(label, kind)`, where `kind` is the landmark tile the coordinate lands on — that lets the
/// table be located by signature (see [`find_location_table`]). Magincia is a ruin (`ruins`) and
/// the Great Stygian Abyss sits on an ocean whirlpool (`abyss`); [`display_kind`] maps those onto
/// the marker styles the viewer understands.
const LOCATIONS: [(&str, &str); LOCATION_COUNT] = [
    ("Lord British's Castle", "castle"),
    ("The Lycaeum", "castle"),
    ("Empath Abbey", "castle"),
    ("Serpent's Hold", "castle"),
    ("Moonglow", "town"),
    ("Britain", "town"),
    ("Jhelom", "town"),
    ("Yew", "town"),
    ("Minoc", "town"),
    ("Trinsic", "town"),
    ("Skara Brae", "town"),
    ("Magincia", "ruins"),
    ("Paws", "village"),
    ("Cove", "village"),
    ("Buccaneer's Den", "village"),
    ("Vesper", "village"),
    ("Deceit", "dungeon"),
    ("Despise", "dungeon"),
    ("Destard", "dungeon"),
    ("Wrong", "dungeon"),
    ("Covetous", "dungeon"),
    ("Shame", "dungeon"),
    ("Hythloth", "dungeon"),
    ("The Abyss", "abyss"),
    ("Shrine of Honesty", "shrine"),
    ("Shrine of Compassion", "shrine"),
    ("Shrine of Valor", "shrine"),
    ("Shrine of Justice", "shrine"),
    ("Shrine of Sacrifice", "shrine"),
    ("Shrine of Honor", "shrine"),
    ("Shrine of Spirituality", "shrine"),
    ("Shrine of Humility", "shrine"),
];

/// Number of entries in the `AVATAR.EXE` location table, and the stride between its parallel X
/// and Y coordinate arrays.
const LOCATION_COUNT: usize = 32;

/// Named town, village and castle maps: `(filename, display title, kind)`, each a 32×32 grid.
/// Lord British's Castle spans two floors (`LCB_1`/`LCB_2`).
const TOWNS: &[(&str, &str, &str)] = &[
    ("LCB_1.ULT", "Lord British's Castle", "castle"),
    ("LCB_2.ULT", "Lord British's Castle (Upper)", "castle"),
    ("EMPATH.ULT", "Empath Abbey", "castle"),
    ("LYCAEUM.ULT", "The Lycaeum", "castle"),
    ("SERPENT.ULT", "Serpent's Hold", "castle"),
    ("BRITAIN.ULT", "Britain", "town"),
    ("YEW.ULT", "Yew", "town"),
    ("MINOC.ULT", "Minoc", "town"),
    ("TRINSIC.ULT", "Trinsic", "town"),
    ("JHELOM.ULT", "Jhelom", "town"),
    ("MOONGLOW.ULT", "Moonglow", "town"),
    ("SKARA.ULT", "Skara Brae", "town"),
    ("MAGINCIA.ULT", "Magincia", "town"),
    ("COVE.ULT", "Cove", "village"),
    ("PAWS.ULT", "Paws", "village"),
    ("VESPER.ULT", "Vesper", "village"),
    ("DEN.ULT", "Buccaneer's Den", "village"),
];

/// Render Ultima IV into its worlds: the Britannia overworld plus each named town and castle.
pub fn export_worlds(game_dir: &Path) -> Result<Vec<World>> {
    let tileset = std::fs::read(game_dir.join(TILESET_FILE))
        .with_context(|| format!("reading {TILESET_FILE} from {}", game_dir.display()))?;
    let tiles = ega::decode_tileset(&tileset, &BRIGHT_PALETTE);
    ensure!(!tiles.is_empty(), "{TILESET_FILE} contained no tiles");

    let mut worlds = Vec::with_capacity(1 + TOWNS.len());

    let overworld = read_overworld(game_dir)?;
    worlds.push(world(
        "britannia",
        "Ultima IV — Britannia",
        "overworld",
        overworld_pois(game_dir, &overworld),
        render(&overworld, WORLD_W, WORLD_H, &tiles),
    ));

    for (file, title, kind) in TOWNS {
        let path = game_dir.join(file);
        if !path.exists() {
            continue; // Not every install ships every map; skip missing ones.
        }
        let data = std::fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
        ensure!(
            data.len() >= TOWN_GRID_LEN,
            "{} is {} bytes; expected at least {TOWN_GRID_LEN}",
            path.display(),
            data.len()
        );
        let world_id = file.trim_end_matches(".ULT").to_ascii_lowercase();
        worlds.push(world(
            &world_id,
            &format!("Ultima IV — {title}"),
            kind,
            vec![],
            render(&data[..TOWN_GRID_LEN], TOWN_W, TOWN_H, &tiles),
        ));
    }

    Ok(worlds)
}

/// The party's overworld position (in tiles) from `PARTY.SAV`, or `None` if the party isn't on
/// Britannia. Map X/Y are at `0x1D4`/`0x1D5`; the location word at `0x1F4` is `0` on the
/// overworld (non-zero inside a town or dungeon, where X/Y are local).
pub fn player_position(game_dir: &Path) -> Result<Option<(u32, u32)>> {
    let path = game_dir.join(SAVE_FILE);
    if !path.exists() {
        return Ok(None);
    }
    let data = std::fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
    if data.len() < 0x1F6 {
        return Ok(None);
    }
    let location = u16::from_le_bytes([data[0x1F4], data[0x1F5]]);
    if location != 0 {
        return Ok(None);
    }
    Ok(Some((u32::from(data[0x1D4]), u32::from(data[0x1D5]))))
}

/// Read `WORLD.MAP` and de-chunk it into a linear 256×256 tile grid.
fn read_overworld(game_dir: &Path) -> Result<Vec<u8>> {
    let raw = std::fs::read(game_dir.join(OVERWORLD_FILE))
        .with_context(|| format!("reading {OVERWORLD_FILE} from {}", game_dir.display()))?;
    ensure!(
        raw.len() >= WORLD_W * WORLD_H,
        "{OVERWORLD_FILE} is {} bytes; expected at least {}",
        raw.len(),
        WORLD_W * WORLD_H
    );
    let mut grid = vec![0u8; WORLD_W * WORLD_H];
    for cy in 0..CHUNKS_PER_ROW {
        for cx in 0..CHUNKS_PER_ROW {
            let base = (cy * CHUNKS_PER_ROW + cx) * (CHUNK * CHUNK);
            for ty in 0..CHUNK {
                for tx in 0..CHUNK {
                    let x = cx * CHUNK + tx;
                    let y = cy * CHUNK + ty;
                    grid[y * WORLD_W + x] = raw[base + ty * CHUNK + tx];
                }
            }
        }
    }
    Ok(grid)
}

/// The location type of an overworld landmark tile, if it is one. From the standard Ultima IV
/// tileset: `9` dungeon, `10` town, `11` castle, `12` village, `14` the entrance to Lord British's
/// Castle (tiles `13`/`15` are its wings), and `29` the ruins of Magincia (shown as a town). Used
/// by the fallback tile scan when the location table can't be read.
fn landmark(tile_index: u8) -> Option<(&'static str, &'static str)> {
    match tile_index {
        9 => Some(("dungeon", "Dungeon")),
        10 => Some(("town", "Town")),
        11 | 14 => Some(("castle", "Castle")),
        12 => Some(("village", "Village")),
        29 => Some(("town", "Ruins")),
        _ => None,
    }
}

/// Whether tile `tile` is the landmark a location of the given [`LOCATIONS`] `kind` sits on. This
/// is what lets the `AVATAR.EXE` table be recognised: every entry's coordinate must land on the
/// tile its kind implies (dungeons on `9`, towns on `10`, castles on `11`/`14`, villages on `12`,
/// Magincia's ruins on `29`, shrines on `30`, and the Abyss on the `70` whirlpool).
fn tile_matches_kind(tile: u8, kind: &str) -> bool {
    match kind {
        "dungeon" => tile == 9,
        "town" => tile == 10,
        "castle" => tile == 11 || tile == 14,
        "village" => tile == 12,
        "ruins" => tile == 29,
        "shrine" => tile == 30,
        "abyss" => tile == 70,
        _ => false,
    }
}

/// Map a [`LOCATIONS`] `kind` onto the marker style the viewer renders, or `None` for a location
/// that shouldn't get its own marker. Ruined Magincia rides along as a `town`; the Abyss as a
/// `dungeon`; everything else keeps its kind.
fn display_kind(kind: &str) -> Option<&'static str> {
    match kind {
        "town" => Some("town"),
        "village" => Some("village"),
        "castle" => Some("castle"),
        "dungeon" => Some("dungeon"),
        "shrine" => Some("shrine"),
        "ruins" => Some("town"),
        "abyss" => Some("dungeon"),
        _ => None,
    }
}

/// Overworld points of interest for Britannia. Prefers authoritative, **named** markers read from
/// the `AVATAR.EXE` location table; if that can't be located (e.g. a different build), it falls
/// back to typed-but-unnamed markers scanned from the map itself.
fn overworld_pois(game_dir: &Path, grid: &[u8]) -> Vec<Poi> {
    named_pois(game_dir, grid).unwrap_or_else(|| tile_scan_pois(grid))
}

/// Build named POIs from the `AVATAR.EXE` location table, or `None` if it can't be located.
/// Locations whose coordinate is repeated later in the table are aliases with no real overworld
/// entrance (the Shrine of Spirituality shares the Shrine of Humility's tile), so they're skipped.
fn named_pois(game_dir: &Path, grid: &[u8]) -> Option<Vec<Poi>> {
    let exe = std::fs::read(game_dir.join(EXE_FILE)).ok()?;
    let start = find_location_table(&exe, grid)?;
    let coord = |i: usize| (exe[start + i], exe[start + LOCATION_COUNT + i]);
    let mut pois = Vec::new();
    for (i, (label, kind)) in LOCATIONS.iter().enumerate() {
        let (x, y) = coord(i);
        // Skip alias coordinates that reappear later as a real (mapped) location.
        if (i + 1..LOCATION_COUNT).any(|j| coord(j) == (x, y)) {
            continue;
        }
        if let Some(display) = display_kind(kind) {
            pois.push(Poi {
                px: u32::from(x) * TILE_SIZE + TILE_SIZE / 2,
                py: u32::from(y) * TILE_SIZE + TILE_SIZE / 2,
                kind: display.to_string(),
                label: (*label).to_string(),
            });
        }
    }
    Some(pois)
}

/// Locate the location table in `AVATAR.EXE`: the offset of the X-coordinate array whose
/// [`LOCATION_COUNT`] `(x, y)` pairs (the Y array following [`LOCATION_COUNT`] bytes later) each
/// land on the landmark tile their [`LOCATIONS`] kind implies. That full kind sequence is a strong
/// signature, so this finds the table without hard-coding an offset.
fn find_location_table(exe: &[u8], grid: &[u8]) -> Option<usize> {
    let span = LOCATION_COUNT * 2;
    if exe.len() < span {
        return None;
    }
    (0..=exe.len() - span).find(|&start| {
        LOCATIONS.iter().enumerate().all(|(i, (_, kind))| {
            let x = exe[start + i] as usize;
            let y = exe[start + LOCATION_COUNT + i] as usize;
            x < WORLD_W && y < WORLD_H && tile_matches_kind(grid[y * WORLD_W + x], kind)
        })
    })
}

/// Scan the overworld grid for landmark tiles and emit a typed but unnamed POI at each — the
/// fallback when the location table can't be read.
fn tile_scan_pois(grid: &[u8]) -> Vec<Poi> {
    let mut pois = Vec::new();
    for (i, &tile) in grid.iter().enumerate() {
        if let Some((kind, label)) = landmark(tile) {
            let x = (i % WORLD_W) as u32;
            let y = (i / WORLD_W) as u32;
            pois.push(Poi {
                px: x * TILE_SIZE + TILE_SIZE / 2,
                py: y * TILE_SIZE + TILE_SIZE / 2,
                kind: kind.to_string(),
                label: label.to_string(),
            });
        }
    }
    pois
}

/// Assemble a [`World`] with Ultima IV's shared game id and region group.
fn world(id: &str, title: &str, kind: &str, pois: Vec<Poi>, image: RgbImage) -> World {
    World {
        meta: WorldMeta {
            game: "ultima4".into(),
            world: id.into(),
            title: title.into(),
            kind: kind.into(),
            group: GROUP.into(),
        },
        image,
        pois,
    }
}

/// Composite a `w`×`h` tile grid into a full-resolution image.
fn render(grid: &[u8], w: usize, h: usize, tiles: &[RgbImage]) -> RgbImage {
    let mut image = RgbImage::new(w as u32 * TILE_SIZE, h as u32 * TILE_SIZE);
    for ty in 0..h {
        for tx in 0..w {
            let tile_index = grid[ty * w + tx] as usize;
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
    fn dechunk_places_chunk_tiles() {
        // Build a raw chunked buffer where each chunk is filled with a distinct value, then check
        // the de-chunked grid puts them at the right world coordinates.
        let mut raw = vec![0u8; WORLD_W * WORLD_H];
        for cy in 0..CHUNKS_PER_ROW {
            for cx in 0..CHUNKS_PER_ROW {
                let base = (cy * CHUNKS_PER_ROW + cx) * (CHUNK * CHUNK);
                let val = (cy * CHUNKS_PER_ROW + cx) as u8;
                for k in 0..CHUNK * CHUNK {
                    raw[base + k] = val;
                }
            }
        }
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(OVERWORLD_FILE), &raw).unwrap();
        let grid = read_overworld(dir.path()).unwrap();
        // Chunk (cx=2, cy=1) → value 1*8+2 = 10; covers world x 64..96, y 32..64.
        assert_eq!(grid[40 * WORLD_W + 70], 10);
        // Top-left chunk is 0; bottom-right chunk is 63.
        assert_eq!(grid[0], 0);
        assert_eq!(grid[(WORLD_H - 1) * WORLD_W + (WORLD_W - 1)], 63);
    }

    #[test]
    fn tile_scan_pois_type_landmarks() {
        let mut grid = vec![0u8; WORLD_W * WORLD_H];
        grid[0] = 9; // dungeon
        grid[1] = 10; // town
        grid[2] = 11; // castle
        grid[3] = 12; // village
        grid[4] = 14; // Lord British's Castle entrance → castle
        grid[5] = 29; // ruins of Magincia → town
        grid[6] = 13; // castle wing → not a landmark
        grid[7] = 4; // grass → not a landmark
        let pois = tile_scan_pois(&grid);
        let kinds: Vec<&str> = pois.iter().map(|p| p.kind.as_str()).collect();
        assert_eq!(pois.len(), 6);
        assert_eq!(kinds.iter().filter(|k| **k == "castle").count(), 2);
        assert_eq!(kinds.iter().filter(|k| **k == "town").count(), 2); // town + ruins
        assert!(kinds.contains(&"dungeon"));
        assert!(kinds.contains(&"village"));
    }

    #[test]
    fn locates_table_by_kind_signature_and_names_pois() {
        // Lay each location's landmark tile at a distinct spot, then build a synthetic AVATAR.EXE
        // whose parallel X/Y arrays point at them in LOCATIONS order. The kind sequence should be
        // unique enough to locate the table, and the Shrine of Spirituality (an alias of Humility's
        // coordinate) should be dropped.
        let mut grid = vec![0u8; WORLD_W * WORLD_H];
        let tile_for = |kind: &str| match kind {
            "dungeon" => 9u8,
            "town" => 10,
            "castle" => 11,
            "village" => 12,
            "ruins" => 29,
            "shrine" => 30,
            "abyss" => 70,
            _ => 0,
        };
        // Give Spirituality (index 30) and Humility (index 31) the same coordinate.
        let coord = |i: usize| {
            let idx = if i == 30 { 31 } else { i };
            ((idx % WORLD_W) as u8, 3u8 + (idx / WORLD_W) as u8)
        };
        for (i, (_, kind)) in LOCATIONS.iter().enumerate() {
            let (x, y) = coord(i);
            grid[y as usize * WORLD_W + x as usize] = tile_for(kind);
        }
        let mut exe = vec![0u8; LOCATION_COUNT]; // X array
        let mut ys = vec![0u8; LOCATION_COUNT]; // Y array
        for i in 0..LOCATION_COUNT {
            let (x, y) = coord(i);
            exe[i] = x;
            ys[i] = y;
        }
        exe.extend_from_slice(&ys);

        let start = find_location_table(&exe, &grid).expect("table located");
        assert_eq!(start, 0);

        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(EXE_FILE), &exe).unwrap();
        let pois = named_pois(dir.path(), &grid).expect("named pois");
        // 32 locations minus the Spirituality alias = 31 markers.
        assert_eq!(pois.len(), 31);
        assert!(pois
            .iter()
            .any(|p| p.label == "Britain" && p.kind == "town"));
        assert!(pois
            .iter()
            .any(|p| p.label == "Magincia" && p.kind == "town"));
        assert!(pois
            .iter()
            .any(|p| p.label == "The Abyss" && p.kind == "dungeon"));
        assert!(pois.iter().any(|p| p.label == "Shrine of Humility"));
        assert!(!pois.iter().any(|p| p.label == "Shrine of Spirituality"));
    }
}
