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

use std::collections::HashMap;
use std::path::Path;

use anyhow::{ensure, Context, Result};
use image::{Rgb, RgbImage};

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

/// The overworld tile that marks a Shrine of the Virtues.
const SHRINE_TILE: u8 = 25;

/// The Shrines of the Virtues by overworld tile position, `(name, x, y)`. Ultima V draws a shrine
/// tile ([`SHRINE_TILE`]) at each but stores no name for them, so the names come from
/// cross-referencing Ultima IV's shrine table (Britannia's geography is shared, and the tile
/// positions match it exactly). Seven virtues have a surface shrine; Spirituality's is the Codex.
const SHRINES: [(&str, u8, u8); 7] = [
    ("Shrine of Justice", 73, 11),
    ("Shrine of Sacrifice", 205, 45),
    ("Shrine of Honesty", 233, 66),
    ("Shrine of Compassion", 128, 92),
    ("Shrine of Honor", 81, 207),
    ("Shrine of Humility", 231, 216),
    ("Shrine of Valor", 36, 229),
];

/// Render Ultima V into its worlds: the Britannia surface, the Underworld, and every town,
/// dwelling, castle and keep (one map per floor).
pub fn export_worlds(game_dir: &Path) -> Result<Vec<World>> {
    let tiles = read_tileset(game_dir)?;
    let data = std::fs::read(game_dir.join(DATA_FILE))
        .with_context(|| format!("reading {DATA_FILE} from {}", game_dir.display()))?;

    let britannia = read_britannia(game_dir, &data)?;
    let underworld = read_underworld(game_dir)?;
    let (loc_worlds, mut targets) = location_worlds(game_dir, &data, &tiles)?;
    let dungeons = dungeon_worlds(game_dir, &mut targets)?;
    let (mut brit_pois, under_pois) = location_pois(&data, &britannia, &targets);
    brit_pois.extend(shrine_pois(&britannia));

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
    worlds.extend(loc_worlds);
    worlds.extend(dungeons);
    Ok(worlds)
}

/// Where the party is, from `SAVED.GAM`: the location code `0x2ED`, the Z/floor `0x2EF`, and the
/// tile `0x2F0`/`0x2F1`. Location `0` is a world map (Z `0` = surface, `0xFF` = Underworld, `1`–`7`
/// = a dungeon, which has no top-down map); `1..=32` is a top-down location — `index = code - 1`
/// into [`LOCATIONS`] (Party-Location order) — with the party on floor Z. Codes `33`–`40` are the
/// first-person dungeons.
#[derive(Debug, PartialEq)]
enum PartyPos {
    World {
        underworld: bool,
        x: u32,
        y: u32,
    },
    Location {
        index: usize,
        floor: u32,
        x: u32,
        y: u32,
    },
}

/// Read the party position from `SAVED.GAM`, or `None` when there's no save or the party is
/// somewhere without a rendered map (a dungeon).
fn read_party_pos(game_dir: &Path) -> Result<Option<PartyPos>> {
    let path = game_dir.join(SAVE_FILE);
    if !path.exists() {
        return Ok(None);
    }
    let data = std::fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
    if data.len() < 0x2F2 {
        return Ok(None);
    }
    let (loc, z) = (data[0x2ED], data[0x2EF]);
    let (x, y) = (u32::from(data[0x2F0]), u32::from(data[0x2F1]));
    Ok(match loc {
        0 => match z {
            0 => Some(PartyPos::World {
                underworld: false,
                x,
                y,
            }),
            0xFF => Some(PartyPos::World {
                underworld: true,
                x,
                y,
            }),
            _ => None, // inside a dungeon; X/Y are local and there's no top-down map
        },
        1..=32 => Some(PartyPos::Location {
            index: usize::from(loc) - 1,
            floor: u32::from(z),
            x,
            y,
        }),
        _ => None, // dungeon-entrance codes; no top-down map
    })
}

/// The bundle slug of top-down location `index`'s floor `floor` (0-based), matching the ids
/// [`location_worlds`] assigns: a single-floor place is `slug(name)`, a multi-floor one is
/// `slug(name)-l{floor+1}`. `None` if the floor wasn't exported.
fn location_floor_slug(data: &[u8], index: usize, floor: u32) -> Option<String> {
    let name = LOCATIONS.get(index)?.0;
    let fi = index / LOCS_PER_FILE;
    let loc = index % LOCS_PER_FILE;
    let start_off = *START_OFFS.get(fi)?;
    let starts = data.get(start_off..start_off + LOCS_PER_FILE)?;
    let first = usize::from(starts[loc]);
    let end = starts
        .get(loc + 1)
        .map_or(MAPS_PER_FILE, |&s| usize::from(s));
    let floors = end.checked_sub(first).filter(|&f| f > 0)?;
    if floor as usize >= floors {
        return None;
    }
    Some(if floors > 1 {
        format!("{}-l{}", slug(name), floor + 1)
    } else {
        slug(name)
    })
}

/// The party's "you are here" marker for the world identified by `world_slug`, in **tile**
/// coordinates, or `None` if the marker doesn't belong on that world.
///
/// This gives the dual marker: when the party is inside a top-down location, the marker shows on
/// that location's own sub-map (the party's current tile) **and** on the parent overworld at the
/// location's entrance (the tile the party will step back out onto). On a world map it's the
/// single position, on the surface or Underworld as appropriate.
pub fn marker_position(game_dir: &Path, world_slug: &str) -> Result<Option<(u32, u32)>> {
    let Some(pos) = read_party_pos(game_dir)? else {
        return Ok(None);
    };
    match pos {
        PartyPos::World { underworld, x, y } => {
            let want = if underworld {
                "underworld"
            } else {
                "britannia"
            };
            Ok((world_slug == want).then_some((x, y)))
        }
        PartyPos::Location { index, floor, x, y } => {
            let data = std::fs::read(game_dir.join(DATA_FILE))
                .with_context(|| format!("reading {DATA_FILE}"))?;
            // On the location's own sub-map, mark the party's current tile.
            if location_floor_slug(&data, index, floor).as_deref() == Some(world_slug) {
                return Ok(Some((x, y)));
            }
            // Otherwise, mark the entrance on the parent overworld (surface or Underworld).
            let (Some(&ex), Some(&ey)) = (data.get(LOC_X_OFF + index), data.get(LOC_Y_OFF + index))
            else {
                return Ok(None);
            };
            let britannia = read_britannia(game_dir, &data)?;
            let on_underworld = britannia
                .get(usize::from(ey) * WORLD_W + usize::from(ex))
                .is_some_and(|&t| t == WATER_TILE);
            let overworld = if on_underworld {
                "underworld"
            } else {
                "britannia"
            };
            Ok((world_slug == overworld).then_some((u32::from(ex), u32::from(ey))))
        }
    }
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
fn location_pois(
    data: &[u8],
    britannia: &[u8],
    targets: &HashMap<&str, String>,
) -> (Vec<Poi>, Vec<Poi>) {
    let mut brit = Vec::new();
    let mut under = Vec::new();
    if data.len() < LOC_X_OFF + LOC_COUNT || data.len() < LOC_Y_OFF + LOC_COUNT {
        return (brit, under); // unexpected build; ship the maps without markers
    }
    for (i, (name, kind)) in LOCATIONS.iter().enumerate() {
        let x = data[LOC_X_OFF + i];
        let y = data[LOC_Y_OFF + i];
        let mut marker = tilemap::poi(u32::from(x), u32::from(y), kind, name);
        // Link to the location's entrance floor, when it has a top-down map (dungeons don't).
        marker.target = targets.get(name).cloned();
        if britannia[usize::from(y) * WORLD_W + usize::from(x)] == WATER_TILE {
            under.push(marker);
        } else {
            brit.push(marker);
        }
    }
    (brit, under)
}

/// Shrine markers for the Britannia surface: each [`SHRINES`] entry is emitted only when the grid
/// actually carries a [`SHRINE_TILE`] at its coordinate, so a differing build never gets a dead
/// marker. Shrines are label-only (there is no shrine sub-map to open).
fn shrine_pois(britannia: &[u8]) -> Vec<Poi> {
    SHRINES
        .iter()
        .filter(|&&(_, x, y)| {
            britannia.get(usize::from(y) * WORLD_W + usize::from(x)) == Some(&SHRINE_TILE)
        })
        .map(|&(name, x, y)| tilemap::poi(u32::from(x), u32::from(y), "shrine", name))
        .collect()
}

/// Build a [`World`] for every floor of every town, dwelling, castle and keep. Each location file
/// holds 16 floors; the `DATA.OVL` first-map-index array partitions them among the file's eight
/// locations, so a location's floors are `[start[i], start[i + 1])`.
fn location_worlds(
    game_dir: &Path,
    data: &[u8],
    tiles: &[RgbImage],
) -> Result<(Vec<World>, HashMap<&'static str, String>)> {
    let mut worlds = Vec::new();
    let mut targets: HashMap<&'static str, String> = HashMap::new();
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
            // The overworld POI links here — the ground floor (`-l1` when the place has levels).
            let entrance = if floors > 1 {
                format!("{}-l1", slug(name))
            } else {
                slug(name)
            };
            targets.insert(name, format!("/{GAME}/{entrance}"));
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
    Ok((worlds, targets))
}

// --- Dungeons -------------------------------------------------------------------------------
//
// Ultima V's eight dungeons are first-person, so the game ships no top-down art for them. But the
// maze itself is stored in `DUNGEON.DAT` as plain tile grids (8 dungeons × 8 levels × 8×8, one
// byte per tile), so we reconstruct a top-down "graph-paper" map by synthesising a tile image for
// each cell type. Each tile byte's high nibble is the type; the low nibble is a detail.

/// `DUNGEON.DAT`: 8 dungeons × 8 levels × 8×8 tiles, one byte per tile.
const DUNGEON_FILE: &str = "DUNGEON.DAT";
const DUNGEON_COUNT: usize = 8;
const DUNGEON_LEVELS: usize = 8;
const DUNGEON_EDGE: usize = 8;
const DUNGEON_LEVEL_BYTES: usize = DUNGEON_EDGE * DUNGEON_EDGE; // 64
const DUNGEON_BYTES: usize = DUNGEON_COUNT * DUNGEON_LEVELS * DUNGEON_LEVEL_BYTES; // 4096
/// The eight dungeons sit at the end of [`LOCATIONS`], in `DUNGEON.DAT` order.
const DUNGEON_FIRST_LOCATION: usize = 32;
/// Edge, in pixels, of a synthesised dungeon-cell image.
const DTILE: u32 = 32;

const D_WALL: Rgb<u8> = Rgb([54, 52, 64]);
const D_WALL_ALT: Rgb<u8> = Rgb([80, 54, 54]);
const D_WALL_EDGE: Rgb<u8> = Rgb([32, 30, 40]);
const D_FLOOR: Rgb<u8> = Rgb([206, 198, 176]);
const D_GRID: Rgb<u8> = Rgb([176, 168, 148]);
const D_DOOR: Rgb<u8> = Rgb([148, 96, 42]);
const D_SECRET: Rgb<u8> = Rgb([170, 60, 150]);
const D_LADDER_UP: Rgb<u8> = Rgb([232, 202, 44]);
const D_LADDER_DOWN: Rgb<u8> = Rgb([228, 138, 40]);
const D_CHEST: Rgb<u8> = Rgb([150, 102, 40]);
const D_FOUNTAIN: Rgb<u8> = Rgb([56, 120, 216]);
const D_TRAP: Rgb<u8> = Rgb([196, 44, 44]);
const D_ROOM: Rgb<u8> = Rgb([150, 70, 180]);

/// Build a [`World`] for every level of every dungeon, and register each dungeon's first level as
/// the target its overworld entrance POI links to.
fn dungeon_worlds(
    game_dir: &Path,
    targets: &mut HashMap<&'static str, String>,
) -> Result<Vec<World>> {
    let path = game_dir.join(DUNGEON_FILE);
    if !path.exists() {
        return Ok(Vec::new()); // not every install ships it
    }
    let data = std::fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
    if data.len() < DUNGEON_BYTES {
        return Ok(Vec::new()); // unexpected build; ship the rest of the maps without dungeons
    }
    let tiles = dungeon_tileset();
    let mut worlds = Vec::with_capacity(DUNGEON_COUNT * DUNGEON_LEVELS);
    for di in 0..DUNGEON_COUNT {
        let (name, _) = LOCATIONS[DUNGEON_FIRST_LOCATION + di];
        let s = slug(name);
        targets.insert(name, format!("/{GAME}/{s}-l1"));
        for level in 0..DUNGEON_LEVELS {
            let off = (di * DUNGEON_LEVELS + level) * DUNGEON_LEVEL_BYTES;
            let grid = &data[off..off + DUNGEON_LEVEL_BYTES];
            worlds.push(tilemap::world(
                GAME,
                &format!("{s}-l{}", level + 1),
                &format!("Ultima V — {name} (level {})", level + 1),
                "dungeon",
                &s,
                Vec::new(),
                tilemap::render(grid, DUNGEON_EDGE, DUNGEON_EDGE, &tiles),
            ));
        }
    }
    Ok(worlds)
}

/// The 256 synthesised dungeon-cell images, indexed by the raw `DUNGEON.DAT` byte.
fn dungeon_tileset() -> Vec<RgbImage> {
    (0..=u8::MAX).map(dungeon_tile).collect()
}

/// Synthesise the top-down image for one dungeon cell. The high nibble is the cell type; the low
/// nibble a detail (energy-field colour, etc.).
fn dungeon_tile(byte: u8) -> RgbImage {
    let (hi, lo) = (byte >> 4, byte & 0x0F);
    match hi {
        0xB => return wall_tile(D_WALL, false),     // normal wall
        0xC => return wall_tile(D_WALL_ALT, false), // alternate wall (skeleton in manacles)
        0xD => return wall_tile(D_WALL, true),      // secret door — a wall with a faint seam
        _ => {}
    }
    let mut img = floor_tile();
    match hi {
        0xE => fill_rect(&mut img, 6, 13, 26, 19, D_DOOR), // door
        0x1 => fill_rect(&mut img, 9, 9, 23, 16, D_LADDER_UP), // ladder up
        0x2 => fill_rect(&mut img, 9, 16, 23, 23, D_LADDER_DOWN), // ladder down
        0x3 => {
            fill_rect(&mut img, 9, 9, 23, 16, D_LADDER_UP);
            fill_rect(&mut img, 9, 16, 23, 23, D_LADDER_DOWN);
        }
        0x4 | 0x7 => fill_rect(&mut img, 9, 10, 23, 22, D_CHEST), // chest (closed / open)
        0x5 => fill_rect(&mut img, 9, 9, 23, 23, D_FOUNTAIN),     // fountain
        0x6 => fill_rect(&mut img, 10, 10, 22, 22, D_TRAP),       // trap / pit
        0x8 => fill_rect(&mut img, 5, 5, 27, 27, field_color(lo)), // energy field
        0xF => {
            // A room (a separate combat map): a bold frame on the floor cell.
            fill_rect(&mut img, 5, 5, 27, 7, D_ROOM);
            fill_rect(&mut img, 5, 25, 27, 27, D_ROOM);
            fill_rect(&mut img, 5, 5, 7, 27, D_ROOM);
            fill_rect(&mut img, 25, 5, 27, 27, D_ROOM);
        }
        _ => {} // 0x0 open hallway (plain floor); 0x9 / 0xA unused
    }
    img
}

/// The colour of an energy field, from the low nibble's bottom two bits.
fn field_color(lo: u8) -> Rgb<u8> {
    match lo & 0x3 {
        0 => Rgb([150, 80, 200]), // sleep — purple
        1 => Rgb([70, 180, 70]),  // poison — green
        2 => Rgb([232, 96, 32]),  // fire — orange
        _ => Rgb([232, 220, 44]), // energy — yellow
    }
}

/// A parchment floor cell with a faint grid border.
fn floor_tile() -> RgbImage {
    let mut img = RgbImage::from_pixel(DTILE, DTILE, D_FLOOR);
    border(&mut img, D_GRID);
    img
}

/// A solid wall cell; `secret` adds a faint seam hinting at a hidden door.
fn wall_tile(fill: Rgb<u8>, secret: bool) -> RgbImage {
    let mut img = RgbImage::from_pixel(DTILE, DTILE, fill);
    border(&mut img, D_WALL_EDGE);
    if secret {
        fill_rect(
            &mut img,
            DTILE / 2 - 1,
            6,
            DTILE / 2 + 1,
            DTILE - 6,
            D_SECRET,
        );
    }
    img
}

/// Fill the half-open rectangle `[x0, x1) × [y0, y1)` with `c`, clipped to the image.
fn fill_rect(img: &mut RgbImage, x0: u32, y0: u32, x1: u32, y1: u32, c: Rgb<u8>) {
    for y in y0..y1.min(img.height()) {
        for x in x0..x1.min(img.width()) {
            img.put_pixel(x, y, c);
        }
    }
}

/// Draw a one-pixel border around the image.
fn border(img: &mut RgbImage, c: Rgb<u8>) {
    let (w, h) = (img.width(), img.height());
    for x in 0..w {
        img.put_pixel(x, 0, c);
        img.put_pixel(x, h - 1, c);
    }
    for y in 0..h {
        img.put_pixel(0, y, c);
        img.put_pixel(w - 1, y, c);
    }
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
    fn dungeon_tiles_categorise_by_high_nibble() {
        assert_eq!(dungeon_tileset().len(), 256);
        let mid = DTILE / 2;
        // Normal (0xB) and alternate (0xC) walls fill the cell with their colours.
        assert_eq!(*dungeon_tile(0xB0).get_pixel(mid, mid), D_WALL);
        assert_eq!(*dungeon_tile(0xC0).get_pixel(mid, mid), D_WALL_ALT);
        // Open hallway (0x00) is floor; a door (0xE0) draws a bar across it.
        assert_eq!(*dungeon_tile(0x00).get_pixel(mid, mid), D_FLOOR);
        assert_eq!(*dungeon_tile(0xE0).get_pixel(mid, 15), D_DOOR);
        // The energy-field colour comes from the low nibble's bottom two bits.
        assert_eq!(field_color(1), Rgb([70, 180, 70]));
        assert_eq!(field_color(2), Rgb([232, 96, 32]));
    }

    #[test]
    fn shrine_pois_land_on_shrine_tiles() {
        // A grid with a shrine tile at Honesty's coordinate and open water elsewhere yields exactly
        // that one shrine marker, centred on its tile.
        let mut grid = vec![WATER_TILE; WORLD_W * WORLD_H];
        let (_, x, y) = SHRINES[2];
        grid[usize::from(y) * WORLD_W + usize::from(x)] = SHRINE_TILE;
        let pois = shrine_pois(&grid);
        assert_eq!(pois.len(), 1);
        assert_eq!(pois[0].label, "Shrine of Honesty");
        assert_eq!(pois[0].kind, "shrine");
        assert!(pois[0].target.is_none());
        assert_eq!(
            (pois[0].px, pois[0].py),
            (
                u32::from(x) * TILE_SIZE + TILE_SIZE / 2,
                u32::from(y) * TILE_SIZE + TILE_SIZE / 2
            )
        );
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
        let targets: HashMap<&str, String> = [("Moonglow", "/ultima5/moonglow".to_string())]
            .into_iter()
            .collect();
        let (brit, under) = location_pois(&data, &britannia, &targets);
        assert_eq!(brit.len(), LOC_COUNT - 1);
        assert_eq!(under.len(), 1);
        assert_eq!(under[0].label, LOCATIONS[LOC_COUNT - 1].0);
        // The linked location carries its entrance target; unlinked ones don't.
        assert_eq!(
            brit.iter()
                .find(|p| p.label == "Moonglow")
                .unwrap()
                .target
                .as_deref(),
            Some("/ultima5/moonglow")
        );
        assert!(brit
            .iter()
            .find(|p| p.label == "Britain")
            .unwrap()
            .target
            .is_none());
    }

    #[test]
    fn read_party_pos_decodes_world_and_location() {
        let dir = tempfile::tempdir().unwrap();
        let write = |save: &[u8]| std::fs::write(dir.path().join(SAVE_FILE), save).unwrap();
        let mut save = vec![0u8; 0x2F2];

        // Location 0, Z 0 → the Britannia surface at (10, 20).
        save[0x2ED] = 0;
        save[0x2EF] = 0;
        save[0x2F0] = 10;
        save[0x2F1] = 20;
        write(&save);
        assert_eq!(
            read_party_pos(dir.path()).unwrap(),
            Some(PartyPos::World {
                underworld: false,
                x: 10,
                y: 20
            })
        );

        // Location 13 (Iolo's Hut = LOCATIONS[12]), Z 0, tile (15, 15) — the real save's state.
        save[0x2ED] = 13;
        save[0x2F0] = 15;
        save[0x2F1] = 15;
        write(&save);
        assert_eq!(
            read_party_pos(dir.path()).unwrap(),
            Some(PartyPos::Location {
                index: 12,
                floor: 0,
                x: 15,
                y: 15
            })
        );

        // Location 0, Z 3 → inside a dungeon: no top-down map.
        save[0x2ED] = 0;
        save[0x2EF] = 3;
        write(&save);
        assert_eq!(read_party_pos(dir.path()).unwrap(), None);
    }

    #[test]
    fn location_floor_slug_maps_floors() {
        // A DATA.OVL whose TOWNE.DAT first-map-index array makes location 0 single-floor and
        // location 1 three floors ([1, 4)).
        let mut data = vec![0u8; START_OFFS[3] + LOCS_PER_FILE];
        let s0 = START_OFFS[0];
        data[s0..s0 + LOCS_PER_FILE].copy_from_slice(&[0, 1, 4, 5, 6, 7, 8, 9]);

        // index 0 = Moonglow, single floor → the bare slug.
        assert_eq!(
            location_floor_slug(&data, 0, 0).as_deref(),
            Some("moonglow")
        );
        // index 1 = Britain, floor 2 (0-based) → the `-l3` sub-map.
        assert_eq!(
            location_floor_slug(&data, 1, 2).as_deref(),
            Some("britain-l3")
        );
        // A floor past the location's floor count isn't a real sub-map.
        assert_eq!(location_floor_slug(&data, 0, 1), None);
    }
}
