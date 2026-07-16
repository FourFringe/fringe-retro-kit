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

use std::collections::HashMap;
use std::path::Path;

use anyhow::{ensure, Context, Result};
use image::RgbImage;

use crate::bundle::{Poi, World, WorldMeta};
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

/// Font tile indices: 32 = `A` … 57 = `Z`, 58 = space.
const FONT_A: u8 = 32;
const FONT_Z: u8 = 57;
const FONT_SPACE: u8 = 58;

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

        // Only towns paint readable labels; on overworlds the font tiles double as terrain, so
        // reading them there would yield noise. Towns are exactly the maps with a `TLK` file.
        let (pois, title) = if has_dialogue(game_dir, &name) {
            let pois = extract_labels(grid);
            let title = town_name(&pois)
                .map(|n| format!("Ultima II — {n}"))
                .unwrap_or_else(|| format!("Ultima II — Town ({})", name.to_ascii_uppercase()));
            (pois, title)
        } else {
            (vec![], format!("Ultima II — {}", name.to_ascii_uppercase()))
        };

        let world_id = name.to_ascii_lowercase();
        worlds.push(World {
            meta: WorldMeta {
                game: "ultima2".into(),
                title,
                world: world_id,
            },
            image,
            pois,
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
            Poi {
                px: center_col as u32 * TILE_SIZE + TILE_SIZE / 2,
                py: center_row as u32 * TILE_SIZE + TILE_SIZE / 2,
                kind: "sign".to_string(),
                label,
            }
        })
        .collect()
}

/// Derive a town's name from its labels: prefer a label naming the settlement itself
/// (`TOWNE`/`TOWN`/`VILLAGE`), then fall back to a weaker place-type word (`PORT`, `COVE`, …).
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
        .map(|p| p.label.clone())
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
            },
            Poi {
                px: 0,
                py: 0,
                kind: "sign".into(),
                label: "Towne Linda".into(),
            },
        ];
        assert_eq!(town_name(&pois).as_deref(), Some("Towne Linda"));
        let none = vec![Poi {
            px: 0,
            py: 0,
            kind: "sign".into(),
            label: "Weapons".into(),
        }];
        assert_eq!(town_name(&none), None);
    }
}
