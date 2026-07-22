//! Ultima II (DOS) world-map rendering.
//!
//! Ultima II's overworlds and towns each live in their own file, named `MAP[XG]NN` (mixed case on
//! disk). Every map is a 64×64 grid of one byte per tile; the tile index is the top 6 bits
//! (`byte >> 2`), giving 0–63 — a direct index into the 64-entry tileset. Some map files carry a
//! 128-byte NPC/monster header before the grid, so we read the **last** 4096 bytes.
//!
//! The tileset itself is embedded in `ULTIMAII.EXE` at offset `0x7C40`: 64 entries of 66 bytes
//! each (a 2-byte header followed by 64 bytes of CGA 2-bpp pixel data). See [`crate::cga`].
//!
//! Towns (identified by a companion `TLK` dialogue file) paint their shop and landmark **names**
//! directly onto the map using the tileset's built-in A–Z font (tiles 32–57, space = 58). We read
//! those labels back out of the grid and surface them as points of interest, and use a town-name
//! label (e.g. `TOWNE LINDA`, `PORT BONIFICE`) as the world's title when we can find one.
//!
//! Towers (`MAP[XG]N4`) and dungeons (`MAP[XG]N5`) don't use the top-down overworld packing; they
//! store a first-person maze as sixteen 16×16 tile-grid levels, which we reconstruct as top-down
//! "graph-paper" maps and link from each overworld's tower/dungeon entrance.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use anyhow::{ensure, Context, Result};
use image::RgbImage;

use crate::bundle::{Poi, World};
use crate::cga::{self, CGA_PALETTE1};
use crate::dungeon;
use crate::tilemap;

/// Map edge length, in tiles.
const MAP_W: usize = 64;
const MAP_H: usize = 64;
/// Bytes of tile grid in a map file (one byte per tile).
const MAP_GRID_LEN: usize = MAP_W * MAP_H;
/// A tower (`…4`) or dungeon (`…5`) map stores its maze as sixteen 16×16 tile-grid levels in the
/// same final [`MAP_GRID_LEN`] bytes (`16 × 16 × 16 = 4096`).
const DUNGEON_EDGE: usize = 16;
const DUNGEON_LEVELS: usize = 16;
const DUNGEON_LEVEL_BYTES: usize = DUNGEON_EDGE * DUNGEON_EDGE; // 256

const EXE_FILE: &str = "ULTIMAII.EXE";
/// The shipped DOS executable is exactly this size; the tileset offset below assumes it.
const EXE_LEN: usize = 37344;
/// Offset of the embedded tileset within `ULTIMAII.EXE`.
const TILESET_OFFSET: usize = 0x7C40;
const TILE_COUNT: usize = 64;
/// Each tileset entry: a 2-byte header followed by the CGA pixel data.
const TILE_HEADER: usize = 2;
const TILE_STRIDE: usize = TILE_HEADER + cga::BYTES_PER_TILE;

/// Font tile indices: 32 = `A` … 57 = `Z`, 58 = space.
const FONT_A: u8 = 32;
const FONT_Z: u8 = 57;
const FONT_SPACE: u8 = 58;

/// The `PLAYER` save file holding the party's character sheet and overworld position.
const SAVE_FILE: &str = "PLAYER";

/// This game's identifier, shared by every world it exports.
const GAME: &str = "ultima2";

/// The party's current overworld position (in tiles) from the `PLAYER` save, or `None` if there
/// is no save. Ultima II records only an overworld position, not which world/era it belongs to,
/// so the caller decides which overworld map to place it on. Uses the parser in
/// `fringe-retro-core`.
pub fn player_position(game_dir: &Path) -> Result<Option<(u32, u32)>> {
    let save = game_dir.join(SAVE_FILE);
    if !save.exists() {
        return Ok(None);
    }
    let parsed = fringe_retro_core::games::ultima2::Ultima2Save::load(&save)
        .with_context(|| format!("reading {}", save.display()))?;
    let x = parsed.get_field("x").and_then(|v| v.parse::<u32>().ok());
    let y = parsed.get_field("y").and_then(|v| v.parse::<u32>().ok());
    Ok(x.zip(y))
}

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
    let towns = collect_town_names(game_dir, &names);
    let (dungeon_level_worlds, dungeon_slugs) = dungeon_worlds(game_dir)?;
    for name in names {
        let data =
            std::fs::read(game_dir.join(&name)).with_context(|| format!("reading map {name}"))?;
        if data.len() < MAP_GRID_LEN {
            continue; // Not a full map grid; skip rather than fail the whole export.
        }
        // Some maps prepend a 128-byte NPC header; the grid is always the final 4096 bytes.
        let grid = &data[data.len() - MAP_GRID_LEN..];

        // The tile index is the top six bits of each byte; normalise before the shared render.
        let indices: Vec<u8> = grid.iter().map(|&b| b >> 2).collect();
        let image = tilemap::render(&indices, MAP_W, MAP_H, &tiles);

        // Only towns paint readable labels; on overworlds the font tiles double as terrain, so
        // reading them there would yield noise. Towns are exactly the maps with a `TLK` file.
        // We read the labels only to name the town — they're not shown as markers (the painted
        // text is already legible on the map, so markers would just obscure it).
        let is_town = has_dialogue(game_dir, &name);
        let title = if is_town {
            town_display_name(&name.to_ascii_lowercase(), grid)
                .map(|n| format!("Ultima II — {n}"))
                .unwrap_or_else(|| format!("Ultima II — Town ({})", name.to_ascii_uppercase()))
        } else {
            format!("Ultima II — {}", name.to_ascii_uppercase())
        };
        // Overworlds get typed location markers (towns/castles/dungeons/towers); towns get none.
        let pois = if is_town {
            vec![]
        } else {
            overworld_pois(grid, &name, &towns, &dungeon_slugs)
        };

        let world_id = name.to_ascii_lowercase();
        worlds.push(tilemap::world(
            GAME,
            &world_id,
            &title,
            map_kind(&name),
            &map_group(&name),
            pois,
            image,
        ));
    }

    ensure!(
        !worlds.is_empty(),
        "found Ultima II map files but none contained a full 64×64 grid"
    );
    worlds.extend(dungeon_level_worlds);
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

/// List the names of the renderable `MAP[XG]NN` tile maps in `game_dir` (case-insensitive on
/// disk). Non-tile map slots (see [`is_tile_map`]) are skipped.
fn discover_maps(game_dir: &Path) -> Result<Vec<String>> {
    let mut names = Vec::new();
    for entry in std::fs::read_dir(game_dir)? {
        let name = entry?.file_name().to_string_lossy().into_owned();
        if is_tile_map(&name) {
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

/// Whether a map is a top-down overworld/town **tile** map (final digit `0`–`3`). The `4`/`5`
/// slots hold first-person tower/dungeon mazes in a different layout, reconstructed by
/// [`dungeon_worlds`] rather than rendered here.
fn is_tile_map(name: &str) -> bool {
    is_map_name(name) && matches!(name.as_bytes()[5], b'0'..=b'3')
}

/// Whether a map holds a first-person maze: the tower (final digit `4`) or dungeon (`5`) slot.
fn is_dungeon_map(name: &str) -> bool {
    is_map_name(name) && matches!(name.as_bytes()[5], b'4' | b'5')
}

/// Whether a `…4`/`…5` maze file is a tower or a dungeon. Verified against the overworlds: every
/// region with a dungeon entrance ships a `…5` file and every region with a tower entrance a `…4`.
fn dungeon_kind(name: &str) -> &'static str {
    match name.as_bytes()[5] {
        b'4' => "tower",
        _ => "dungeon",
    }
}

/// Classify an Ultima II maze tile byte into a shared dungeon [`dungeon::Cell`]. Walls, floors,
/// doors and ladders are certain; rare region-specific bytes fall through to floor for this rough
/// pass.
fn u2_cell(byte: u8) -> dungeon::Cell {
    use dungeon::Cell;
    match byte {
        0x80 => Cell::Wall,
        0xC0 => Cell::Door,       // a doorway set into a wall
        0xE0 => Cell::SecretDoor, // a wall-like hidden passage
        0x40 => Cell::Chest,
        0x10 => Cell::Ladder {
            up: true,
            down: false,
        },
        0x20 => Cell::Ladder {
            up: false,
            down: true,
        },
        0x30 => Cell::Ladder {
            up: true,
            down: true,
        },
        _ => Cell::Floor, // 0x00 corridor, and rare unmapped bytes
    }
}

/// Render every tower (`…4`) and dungeon (`…5`) map as a stack of top-down levels, returning the
/// worlds and the set of slugs rendered (so their overworld entrances can be linked). Each file
/// holds sixteen 16×16 tile-grid levels in its final [`MAP_GRID_LEN`] bytes.
fn dungeon_worlds(game_dir: &Path) -> Result<(Vec<World>, HashSet<String>)> {
    let tiles = dungeon::tileset(u2_cell);
    let mut names: Vec<String> = std::fs::read_dir(game_dir)?
        .filter_map(|e| e.ok().map(|e| e.file_name().to_string_lossy().into_owned()))
        .filter(|n| is_dungeon_map(n))
        .collect();
    names.sort();

    let mut worlds = Vec::new();
    let mut slugs = HashSet::new();
    for name in names {
        let data = std::fs::read(game_dir.join(&name))
            .with_context(|| format!("reading maze map {name}"))?;
        if data.len() < MAP_GRID_LEN {
            continue; // not a full grid; skip rather than fail the whole export
        }
        let grid = &data[data.len() - MAP_GRID_LEN..];
        let slug = name.to_ascii_lowercase();
        let kind = dungeon_kind(&name);
        for level in 0..DUNGEON_LEVELS {
            let off = level * DUNGEON_LEVEL_BYTES;
            let cells = &grid[off..off + DUNGEON_LEVEL_BYTES];
            worlds.push(tilemap::world(
                GAME,
                &format!("{slug}-l{}", level + 1),
                &format!(
                    "Ultima II — {} ({}, level {})",
                    title_case(kind),
                    name.to_ascii_uppercase(),
                    level + 1
                ),
                kind,
                &slug,
                Vec::new(),
                tilemap::render(cells, DUNGEON_EDGE, DUNGEON_EDGE, &tiles),
            ));
        }
        slugs.insert(slug);
    }
    Ok((worlds, slugs))
}

/// The map's category from its final digit: `0` = overworld, otherwise a town/castle (`1`–`3`).
fn map_kind(name: &str) -> &'static str {
    match name.as_bytes()[5] {
        b'0' => "overworld",
        _ => "town",
    }
}

/// A grouping key that ties a world's sub-maps to it. Ultima II names encode a region in the
/// world letter + tens digit (`MAPX2N` → `x2`), so an overworld (`MAPX20`) and its towns
/// (`MAPX21`, `MAPX22`, …) all share one group.
fn map_group(name: &str) -> String {
    format!(
        "{}{}",
        (name.as_bytes()[3] as char).to_ascii_lowercase(),
        name.as_bytes()[4] as char
    )
}

/// The location type of an overworld landmark tile, if it is one. Per the Ultima II manual's
/// tile legend: `5` = village, `6` = town, `7` = tower, `8` = castle, `9` = dungeon.
fn landmark_kind(tile_index: u8) -> Option<&'static str> {
    match tile_index {
        5 => Some("village"),
        6 => Some("town"),
        7 => Some("tower"),
        8 => Some("castle"),
        9 => Some("dungeon"),
        _ => None,
    }
}

/// The town sub-map digit a linkable overworld landmark opens: villages open sub-map `1`, towns
/// `2`, castles `3`, towers `4`, dungeons `5`. Ultima II has no overworld→map table, but a region's
/// overworld and its sub-maps share a group, and each sub-map's final digit encodes the landmark
/// kind (confirmed by which files a region ships — e.g. a region with a castle but no village has a
/// `…3` town but no `…1`, and every dungeon-entrance region ships a `…5`).
fn kind_sub_map_digit(kind: &str) -> Option<char> {
    match kind {
        "village" => Some('1'),
        "town" => Some('2'),
        "castle" => Some('3'),
        "tower" => Some('4'),
        "dungeon" => Some('5'),
        _ => None,
    }
}

/// Read each town map's painted name, keyed by its world slug (the lowercased map name). The value
/// is the extracted place name, or `None` when the town paints no name label. Used to name and
/// link the overworld landmark that opens each town.
fn collect_town_names(game_dir: &Path, names: &[String]) -> HashMap<String, Option<String>> {
    let mut towns = HashMap::new();
    for name in names {
        if !has_dialogue(game_dir, name) {
            continue;
        }
        let Ok(data) = std::fs::read(game_dir.join(name)) else {
            continue;
        };
        if data.len() < MAP_GRID_LEN {
            continue;
        }
        let grid = &data[data.len() - MAP_GRID_LEN..];
        let slug = name.to_ascii_lowercase();
        let display = town_display_name(&slug, grid);
        towns.insert(slug, display);
    }
    towns
}

/// Scan an overworld grid for landmark tiles and emit a POI for each, typed by tile. Village, town
/// and castle landmarks are **named and linked** to their town sub-map; towers and dungeons link to
/// the first level of their reconstructed maze (see [`kind_sub_map_digit`]) when it was rendered.
/// The first column and the bottom two rows hold border/leftover data rather than real landmarks,
/// so they're skipped.
fn overworld_pois(
    grid: &[u8],
    name: &str,
    towns: &HashMap<String, Option<String>>,
    dungeons: &HashSet<String>,
) -> Vec<Poi> {
    let base = name[..name.len() - 1].to_ascii_lowercase(); // "MAPX20" → "mapx2"
    let mut pois = Vec::new();
    for (i, &byte) in grid.iter().enumerate() {
        let x = (i % MAP_W) as u32;
        let y = (i / MAP_W) as u32;
        if x == 0 || y as usize >= MAP_H - 2 {
            continue;
        }
        let Some(kind) = landmark_kind(byte >> 2) else {
            continue;
        };
        let slug = kind_sub_map_digit(kind).map(|d| format!("{base}{d}"));
        let (label, target) = match kind {
            // Towers and dungeons open a first-person maze: link to its first level, if rendered.
            "tower" | "dungeon" => {
                let target = slug
                    .filter(|s| dungeons.contains(s))
                    .map(|s| format!("/{GAME}/{s}-l1"));
                (title_case(kind), target)
            }
            // Villages, towns and castles open a named top-down sub-map, if that town was rendered.
            _ => {
                let slug = slug.filter(|s| towns.contains_key(s));
                let label = slug
                    .as_ref()
                    .and_then(|s| towns[s].clone())
                    .unwrap_or_else(|| title_case(kind));
                let target = slug.map(|s| format!("/{GAME}/{s}"));
                (label, target)
            }
        };
        let mut poi = tilemap::poi(x, y, kind, &label);
        poi.target = target;
        pois.push(poi);
    }
    pois
}

/// Whether a map has a companion `TLK` dialogue file, which marks it as a town. The suffix keeps
/// the map's on-disk case (`MAPX22` → `TLKX22`/`tlkX22`; `mapg61` → `tlkg61`).
fn has_dialogue(game_dir: &Path, map_name: &str) -> bool {
    let suffix = &map_name[3..];
    ["TLK", "tlk"]
        .iter()
        .any(|p| game_dir.join(format!("{p}{suffix}")).exists())
}

/// One horizontal run of font tiles read off the map grid.
struct Run {
    row: usize,
    col_start: usize,
    col_end: usize, // exclusive
    text: String,
}

/// Decode a tile index to its font character, if it is one (`A`–`Z` or space).
fn font_char(tile_index: u8) -> Option<char> {
    match tile_index {
        FONT_A..=FONT_Z => Some((b'A' + (tile_index - FONT_A)) as char),
        FONT_SPACE => Some(' '),
        _ => None,
    }
}

/// Whether a run of font tiles reads like real text rather than a run of a repeated tile. The A–Z
/// tiles double as terrain on some maps, so we reject long single-letter runs and low-variety
/// strings (e.g. the dithered `AYAYA…` terrain fill).
fn is_wordlike(text: &str) -> bool {
    let letters: Vec<char> = text.chars().filter(|c| *c != ' ').collect();
    if letters.len() < 2 || text.chars().count() > 20 {
        return false;
    }
    let mut counts: HashMap<char, usize> = HashMap::new();
    for c in &letters {
        *counts.entry(*c).or_insert(0) += 1;
    }
    if counts.len() < 2 {
        return false;
    }
    // A long run with only a couple of distinct letters is a dithered terrain fill (`AYAYA…`),
    // not a word.
    if letters.len() > 4 && counts.len() < 3 {
        return false;
    }
    let top = *counts.values().max().unwrap_or(&0);
    if letters.len() > 3 && top as f32 / letters.len() as f32 > 0.6 {
        return false;
    }
    true
}

/// Read the town's embedded labels out of a 64×64 tile grid and merge nearby words into POIs.
fn extract_labels(grid: &[u8]) -> Vec<Poi> {
    let mut runs: Vec<Run> = Vec::new();
    for row in 0..MAP_H {
        let mut text = String::new();
        let mut start: Option<usize> = None;
        for col in 0..=MAP_W {
            let ch = (col < MAP_W)
                .then(|| font_char(grid[row * MAP_W + col] >> 2))
                .flatten();
            match ch {
                Some(c) => {
                    start.get_or_insert(col);
                    text.push(c);
                }
                None => {
                    if let Some(s) = start {
                        let trimmed = text.trim();
                        if is_wordlike(trimmed) {
                            runs.push(Run {
                                row,
                                col_start: s,
                                col_end: col,
                                text: trimmed.to_string(),
                            });
                        }
                    }
                    text.clear();
                    start = None;
                }
            }
        }
    }
    merge_runs(runs)
}

/// Merge word runs that sit close together (a multi-word sign such as `ALFREDS FISH CHIPS`) into a
/// single POI, positioned at the centre of the merged label.
fn merge_runs(runs: Vec<Run>) -> Vec<Poi> {
    const ROW_GAP: usize = 2;
    const COL_GAP: usize = 6;

    struct Cluster {
        min_row: usize,
        max_row: usize,
        min_col: usize,
        max_col: usize,
        parts: Vec<(usize, usize, String)>, // (row, col_start, text)
    }

    let mut clusters: Vec<Cluster> = Vec::new();
    for run in runs {
        let hit = clusters.iter_mut().find(|c| {
            let row_ok = run.row + ROW_GAP >= c.min_row && run.row <= c.max_row + ROW_GAP;
            let col_ok = run.col_start <= c.max_col + COL_GAP && run.col_end + COL_GAP >= c.min_col;
            row_ok && col_ok
        });
        match hit {
            Some(c) => {
                c.min_row = c.min_row.min(run.row);
                c.max_row = c.max_row.max(run.row);
                c.min_col = c.min_col.min(run.col_start);
                c.max_col = c.max_col.max(run.col_end);
                c.parts.push((run.row, run.col_start, run.text));
            }
            None => clusters.push(Cluster {
                min_row: run.row,
                max_row: run.row,
                min_col: run.col_start,
                max_col: run.col_end,
                parts: vec![(run.row, run.col_start, run.text)],
            }),
        }
    }

    clusters
        .into_iter()
        .map(|mut c| {
            c.parts.sort_by_key(|(row, col, _)| (*row, *col));
            // Join words within a row into a line, then drop lines that just repeat the previous
            // one — town names are often painted on two identical adjacent rows for emphasis.
            let mut lines: Vec<String> = Vec::new();
            let mut current_row: Option<usize> = None;
            for (row, _col, text) in &c.parts {
                if current_row == Some(*row) {
                    let line = lines.last_mut().expect("row in progress");
                    line.push(' ');
                    line.push_str(text);
                } else {
                    lines.push(text.clone());
                    current_row = Some(*row);
                }
            }
            lines.dedup();
            let label = title_case(&lines.join(" "));
            let center_col = (c.min_col + c.max_col) / 2;
            let center_row = (c.min_row + c.max_row) / 2;
            tilemap::poi(center_col as u32, center_row as u32, "sign", &label)
        })
        .collect()
}

/// Names for towns whose maps paint no readable name label. Ultima II's Lord British's Castle
/// appears in more than one era and spells its name only inside merged interior signs (`Vault`,
/// `Lord British`, `Kitchen`), so those maps are named by hand.
const MANUAL_NAMES: &[(&str, &str)] = &[
    ("mapx23", "Lord British's Castle"),
    ("mapx33", "Lord British's Castle"),
];

/// A town's display name, from its painted labels or the [`MANUAL_NAMES`] override (by world slug,
/// the lowercased map name) when the map paints no readable name.
fn town_display_name(slug: &str, grid: &[u8]) -> Option<String> {
    town_name(&extract_labels(grid)).or_else(|| {
        MANUAL_NAMES
            .iter()
            .find(|&&(s, _)| s == slug)
            .map(|&(_, n)| n.to_string())
    })
}

/// Derive a town's name from its labels: prefer a label naming the settlement itself
/// (`TOWNE`/`TOWN`/`VILLAGE`), then a weaker place-type word (`PORT`, `COVE`, …), and finally the
/// label painted centred along the map's bottom edge — Ultima II's convention for a settlement's
/// own name (e.g. `NEW JESTER`, `COMPUTER CAMP`), used when no keyword identifies it.
fn town_name(pois: &[Poi]) -> Option<String> {
    const STRONG: [&str; 3] = ["TOWNE", "TOWN", "VILLAGE"];
    const WEAK: [&str; 4] = ["PORT", "COVE", "CASTLE", "KEEP"];
    let contains = |p: &Poi, words: &[&str]| {
        p.label
            .to_ascii_uppercase()
            .split_whitespace()
            .any(|w| words.contains(&w))
    };
    pois.iter()
        .find(|p| contains(p, &STRONG))
        .or_else(|| pois.iter().find(|p| contains(p, &WEAK)))
        .or_else(|| pois.iter().find(|p| is_bottom_center(p)))
        .map(|p| p.label.clone())
}

/// Whether a label sits centred along the bottom edge of the map, where Ultima II paints a town's
/// own name (roughly the middle eight columns, in the bottom rows).
fn is_bottom_center(p: &Poi) -> bool {
    let col = p.px / tilemap::TILE_SIZE;
    let row = p.py / tilemap::TILE_SIZE;
    (MAP_W as u32 / 2).abs_diff(col) <= 4 && row >= 54
}

/// Capitalise the first letter of each whitespace-separated word, lowercasing the rest.
fn title_case(text: &str) -> String {
    text.split_whitespace()
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => {
                    first.to_ascii_uppercase().to_string() + &chars.as_str().to_ascii_lowercase()
                }
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tilemap::TILE_SIZE;

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

    #[test]
    fn tile_maps_exclude_dungeon_slots() {
        // Types 0–3 render as tiles (overworlds, towns, castles).
        assert!(is_tile_map("MAPX20")); // overworld
        assert!(is_tile_map("MAPX21")); // town
        assert!(is_tile_map("MAPG93")); // castle
        assert!(is_tile_map("mapg82")); // lowercase on disk
                                        // Types 4–5 are first-person tower/dungeon mazes, handled separately.
        assert!(!is_tile_map("MAPX24"));
        assert!(!is_tile_map("MAPX35"));
        assert!(!is_tile_map("MAPG45"));
        assert!(!is_tile_map("mapg44"));
        // Those slots are the maze maps: `4` = tower, `5` = dungeon.
        assert!(is_dungeon_map("MAPX24") && dungeon_kind("MAPX24") == "tower");
        assert!(is_dungeon_map("MAPX35") && dungeon_kind("MAPX35") == "dungeon");
        assert!(is_dungeon_map("mapg44") && dungeon_kind("mapg44") == "tower");
        assert!(!is_dungeon_map("MAPX20"));
    }

    #[test]
    fn kind_and_group_from_name() {
        assert_eq!(map_kind("MAPX20"), "overworld");
        assert_eq!(map_kind("MAPX21"), "town");
        assert_eq!(map_kind("MAPG93"), "town");
        // A region shares one group across its overworld and sub-maps.
        assert_eq!(map_group("MAPX20"), "x2");
        assert_eq!(map_group("MAPX23"), "x2");
        assert_eq!(map_group("mapg82"), "g8");
    }

    #[test]
    fn overworld_pois_type_and_filter_landmarks() {
        let mut grid = vec![0u8; MAP_GRID_LEN];
        let put = |g: &mut [u8], x: usize, y: usize, tile: u8| g[y * MAP_W + x] = tile << 2;
        put(&mut grid, 20, 10, 5); // village
        put(&mut grid, 36, 27, 6); // town
        put(&mut grid, 25, 3, 7); // tower
        put(&mut grid, 38, 44, 8); // castle
        put(&mut grid, 35, 20, 9); // dungeon
        put(&mut grid, 0, 62, 6); // border artifact — must be skipped
        put(&mut grid, 10, 10, 2); // plain terrain — not a landmark

        // No town registry → every marker stays generic and unlinked.
        let pois = overworld_pois(&grid, "MAPX20", &HashMap::new(), &HashSet::new());
        let kinds: Vec<&str> = pois.iter().map(|p| p.kind.as_str()).collect();
        assert_eq!(pois.len(), 5);
        for k in ["village", "town", "tower", "castle", "dungeon"] {
            assert!(kinds.contains(&k), "missing {k}");
        }
        // The (0,62) artifact and plain terrain produced no POI.
        assert!(pois.iter().all(|p| p.px >= TILE_SIZE));
        // Labels are the title-cased kind, and nothing links without a registry.
        assert!(pois.iter().any(|p| p.label == "Town"));
        assert!(pois.iter().all(|p| p.target.is_none()));
    }

    #[test]
    fn overworld_pois_link_landmarks_to_town_sub_maps() {
        let mut grid = vec![0u8; MAP_GRID_LEN];
        let put = |g: &mut [u8], x: usize, y: usize, tile: u8| g[y * MAP_W + x] = tile << 2;
        put(&mut grid, 36, 27, 6); // town  → digit 2 → mapx22
        put(&mut grid, 38, 44, 5); // village → digit 1 → mapx21 (exists but unnamed)
        put(&mut grid, 35, 20, 8); // castle → digit 3 → mapx23 (absent → unlinked)

        let towns = HashMap::from([
            ("mapx22".to_string(), Some("Towne Linda".to_string())),
            ("mapx21".to_string(), None),
        ]);
        let pois = overworld_pois(&grid, "MAPX20", &towns, &HashSet::new());

        let town = pois.iter().find(|p| p.kind == "town").unwrap();
        assert_eq!(town.label, "Towne Linda");
        assert_eq!(town.target.as_deref(), Some("/ultima2/mapx22"));

        // A rendered but unnamed town still links, labelled by kind.
        let village = pois.iter().find(|p| p.kind == "village").unwrap();
        assert_eq!(village.label, "Village");
        assert_eq!(village.target.as_deref(), Some("/ultima2/mapx21"));

        // No town file for the castle digit → stays generic and unlinked.
        let castle = pois.iter().find(|p| p.kind == "castle").unwrap();
        assert_eq!(castle.label, "Castle");
        assert!(castle.target.is_none());
    }

    #[test]
    fn overworld_pois_link_towers_and_dungeons_to_their_first_level() {
        let mut grid = vec![0u8; MAP_GRID_LEN];
        let put = |g: &mut [u8], x: usize, y: usize, tile: u8| g[y * MAP_W + x] = tile << 2;
        put(&mut grid, 25, 3, 7); // tower   → digit 4 → mapx24 (rendered)
        put(&mut grid, 35, 20, 9); // dungeon → digit 5 → mapx25 (rendered)
        put(&mut grid, 40, 30, 7); // second tower with no rendered maze — stays unlinked

        let dungeons = HashSet::from(["mapx24".to_string(), "mapx25".to_string()]);
        let pois = overworld_pois(&grid, "MAPX20", &HashMap::new(), &dungeons);

        let tower = pois.iter().find(|p| p.kind == "tower").unwrap();
        assert_eq!(tower.label, "Tower");
        assert_eq!(tower.target.as_deref(), Some("/ultima2/mapx24-l1"));

        let dungeon = pois.iter().find(|p| p.kind == "dungeon").unwrap();
        assert_eq!(dungeon.target.as_deref(), Some("/ultima2/mapx25-l1"));
    }

    #[test]
    fn maze_bytes_classify_into_cells() {
        use dungeon::Cell;
        assert!(matches!(u2_cell(0x80), Cell::Wall));
        assert!(matches!(u2_cell(0x00), Cell::Floor));
        assert!(matches!(u2_cell(0xC0), Cell::Door));
        assert!(matches!(u2_cell(0xE0), Cell::SecretDoor));
        assert!(matches!(u2_cell(0x40), Cell::Chest));
        assert!(matches!(
            u2_cell(0x20),
            Cell::Ladder {
                up: false,
                down: true
            }
        ));
    }

    #[test]
    fn font_char_maps_letters_and_space() {
        assert_eq!(font_char(32), Some('A'));
        assert_eq!(font_char(57), Some('Z'));
        assert_eq!(font_char(58), Some(' '));
        assert_eq!(font_char(0), None); // terrain tile, not a letter
        assert_eq!(font_char(60), None); // NPC figure
    }

    #[test]
    fn wordlike_accepts_words_rejects_terrain() {
        assert!(is_wordlike("WEAPONS"));
        assert!(is_wordlike("LE JESTER"));
        assert!(!is_wordlike("A")); // single letter
        assert!(!is_wordlike("AAAAAAA")); // repeated-tile terrain
        assert!(!is_wordlike("AYAYAYAY")); // dithered terrain fill
    }

    #[test]
    fn title_case_capitalises_words() {
        assert_eq!(title_case("TOWNE LINDA"), "Towne Linda");
        assert_eq!(title_case("PORT BONIFICE"), "Port Bonifice");
    }

    /// Paint a horizontal word into a 64×64 tile-byte grid at (row, col).
    fn paint(grid: &mut [u8], row: usize, col: usize, word: &str) {
        for (i, ch) in word.chars().enumerate() {
            let idx = if ch == ' ' {
                FONT_SPACE
            } else {
                FONT_A + (ch as u8 - b'A')
            };
            grid[row * MAP_W + col + i] = idx << 2;
        }
    }

    #[test]
    fn extract_and_merge_labels() {
        let mut grid = vec![0u8; MAP_GRID_LEN];
        // A two-line sign that should merge into one POI.
        paint(&mut grid, 10, 20, "ALFREDS");
        paint(&mut grid, 12, 20, "FISH");
        paint(&mut grid, 12, 26, "CHIPS");
        // A separate sign far away.
        paint(&mut grid, 40, 5, "WEAPONS");

        let pois = extract_labels(&grid);
        let labels: Vec<&str> = pois.iter().map(|p| p.label.as_str()).collect();
        assert!(labels.contains(&"Alfreds Fish Chips"));
        assert!(labels.contains(&"Weapons"));
        assert_eq!(pois.len(), 2);
        assert!(pois.iter().all(|p| p.kind == "sign"));
    }

    #[test]
    fn town_name_prefers_place_word() {
        let pois = vec![
            Poi {
                px: 0,
                py: 0,
                kind: "sign".into(),
                label: "Weapons".into(),
                target: None,
            },
            Poi {
                px: 0,
                py: 0,
                kind: "sign".into(),
                label: "Towne Linda".into(),
                target: None,
            },
        ];
        assert_eq!(town_name(&pois).as_deref(), Some("Towne Linda"));
        let none = vec![Poi {
            px: 0,
            py: 0,
            kind: "sign".into(),
            label: "Weapons".into(),
            target: None,
        }];
        assert_eq!(town_name(&none), None);
    }

    #[test]
    fn town_name_falls_back_to_bottom_centre_label() {
        // A town whose name has no keyword: a shop sign up top and the name painted centre-bottom.
        let mut grid = vec![0u8; MAP_GRID_LEN];
        paint(&mut grid, 6, 12, "SOFTALK"); // shop, top-left
        paint(&mut grid, 56, 26, "TOMMERSVILLE"); // town name, bottom-centre (col 26..38)
        assert_eq!(
            town_name(&extract_labels(&grid)).as_deref(),
            Some("Tommersville")
        );

        // A centred label that isn't near the bottom must not be taken as the name.
        let mut middle = vec![0u8; MAP_GRID_LEN];
        paint(&mut middle, 30, 30, "GORKY");
        assert_eq!(town_name(&extract_labels(&middle)), None);
    }

    #[test]
    fn town_display_name_uses_manual_override() {
        // A castle map whose name isn't cleanly painted still resolves via the override table.
        let grid = vec![0u8; MAP_GRID_LEN];
        assert_eq!(
            town_display_name("mapx33", &grid).as_deref(),
            Some("Lord British's Castle")
        );
        assert_eq!(town_display_name("mapg50", &grid), None);
    }
}
