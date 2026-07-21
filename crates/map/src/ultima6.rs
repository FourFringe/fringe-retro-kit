//! Ultima VI (The False Prophet, DOS) world-map rendering — the Britannia overworld.
//!
//! Britannia is a single seamless **1024×1024-tile** world (far larger than the earlier games'
//! 256×256). It is built from **chunks** — 8×8-tile templates — assembled through two files:
//!
//! - **`CHUNKS`** is `Chunk[1024]`, each chunk `uint8[8*8]` in `x + y*8` order; every byte is a
//!   map-tile index (0–255).
//! - **`MAP`** places the chunks. Britannia is `SuperChunk16[8*8]` (super-chunks in `x + y*8`
//!   order), each super-chunk `uint24[8*16]`: every `uint24` packs **two** 12-bit chunk indices
//!   (the first in the low 12 bits, the second in the high 12). A super-chunk is 16×16 chunks, so
//!   8×8 of them make the 128×128-chunk world. (The five dungeon levels that follow in `MAP` are
//!   not rendered here.)
//!
//! Tiles are 16×16 indexed images. `MAPTILES.VGA` (LZW-compressed) holds the 512 map-tile bitmaps;
//! `TILEINDX.VGA` gives each tile's byte offset (`value * 16`); `MASKTYPE.VGA` (LZW) gives each
//! tile's storage kind (opaque / transparent / span-compressed). Colours come from `U6PAL`
//! (256 RGB triples, 6-bit). The huge world is rendered at a reduced 8-px tile edge (see
//! [`TILE_PX`]) to keep the composite and its tile pyramid tractable.
//!
//! Format reference: the Ultima Codex "Ultima VI internal formats" page, cross-checked against the
//! shipped files (`MAPTILES.VGA` decompresses to 117 408 bytes; `U6PAL` colours are 0–63).

use std::path::Path;

use anyhow::{ensure, Context, Result};
use image::{imageops::FilterType, Rgb, RgbImage};

use crate::bundle::World;
use crate::{lzw, tilemap};

/// This game's identifier, shared by every world it exports.
const GAME: &str = "ultima6";

const MAP_FILE: &str = "MAP";
const CHUNKS_FILE: &str = "CHUNKS";
const MAPTILES_FILE: &str = "MAPTILES.VGA";
const TILEINDX_FILE: &str = "TILEINDX.VGA";
const MASKTYPE_FILE: &str = "MASKTYPE.VGA";
const PALETTE_FILE: &str = "U6PAL";

/// Britannia is 1024×1024 tiles.
const WORLD_TILES: usize = 1024;
/// A chunk is 8×8 tiles.
const CHUNK: usize = 8;
const CHUNK_BYTES: usize = CHUNK * CHUNK; // 64
/// A super-chunk is 16×16 chunks.
const SC_CHUNKS: usize = 16;
/// The world is 8×8 super-chunks.
const SC_PER_EDGE: usize = 8;
/// A super-chunk is `uint24[8*16]` = 128 packed pairs = 384 bytes.
const SC_BYTES: usize = 8 * 16 * 3;
/// Chunk map-tile indices are bytes, so only the first 256 map tiles are ever referenced.
const MAP_TILE_COUNT: usize = 256;
/// Native tile edge, in pixels.
const NATIVE_PX: u32 = 16;
/// Rendered tile edge. Britannia at native 16 px would be a 16 384² image (~0.8 GB); halving it to
/// 8 px keeps the composite and pyramid manageable while staying legible.
const TILE_PX: u32 = 8;

/// Bytes of `MAP` occupied by Britannia (8×8 super-chunks), before the dungeon levels.
const BRITANNIA_BYTES: usize = SC_PER_EDGE * SC_PER_EDGE * SC_BYTES;
/// Each dungeon level is one `SuperChunk32` = `uint24[16*32]` = 1536 bytes.
const DUNGEON_BYTES: usize = 16 * 32 * 3;
/// A dungeon level is 32×32 chunks = 256×256 tiles.
const DUNGEON_TILES: usize = 256;
/// `uint24`s per chunk row: 8 within a Britannia super-chunk, 16 across a dungeon level.
const BRIT_U24_PER_ROW: usize = 8;
const DUNGEON_U24_PER_ROW: usize = 16;
/// The five top-down dungeon levels stored after Britannia in `MAP`.
const DUNGEON_LEVELS: usize = 5;

/// Render Ultima VI into its worlds: the Britannia overworld plus the five dungeon levels.
pub fn export_worlds(game_dir: &Path) -> Result<Vec<World>> {
    let tiles = decode_map_tiles(game_dir)?;
    let map = read(game_dir, MAP_FILE)?;
    let chunks = read(game_dir, CHUNKS_FILE)?;
    ensure!(
        map.len() >= BRITANNIA_BYTES + DUNGEON_LEVELS * DUNGEON_BYTES,
        "{MAP_FILE} is {} bytes; expected at least {}",
        map.len(),
        BRITANNIA_BYTES + DUNGEON_LEVELS * DUNGEON_BYTES
    );
    ensure!(!chunks.is_empty(), "{CHUNKS_FILE} is empty");

    let mut worlds = Vec::with_capacity(1 + DUNGEON_LEVELS);

    // The seamless Britannia overworld.
    let britannia = expand_grid(&chunks, WORLD_TILES, |cx, cy| {
        britannia_chunk_index(&map, cx, cy)
    });
    worlds.push(tilemap::world(
        GAME,
        "britannia",
        "Ultima VI — Britannia",
        "overworld",
        "britannia",
        vec![],
        tilemap::render(&britannia, WORLD_TILES, WORLD_TILES, &tiles),
    ));

    // The five underground dungeon levels.
    for level in 0..DUNGEON_LEVELS {
        let base = BRITANNIA_BYTES + level * DUNGEON_BYTES;
        let grid = expand_grid(&chunks, DUNGEON_TILES, |cx, cy| {
            chunk_at(&map, base, DUNGEON_U24_PER_ROW, cx, cy)
        });
        worlds.push(tilemap::world(
            GAME,
            &format!("dungeon-{}", level + 1),
            &format!("Ultima VI — Dungeon Level {}", level + 1),
            "dungeon",
            "britannia",
            vec![],
            tilemap::render(&grid, DUNGEON_TILES, DUNGEON_TILES, &tiles),
        ));
    }
    Ok(worlds)
}

/// Read `U6PAL` into a 256-entry RGB palette (its 6-bit `0..=63` channels scaled to 8-bit).
fn read_palette(game_dir: &Path) -> Result<[Rgb<u8>; 256]> {
    let data = std::fs::read(game_dir.join(PALETTE_FILE))
        .with_context(|| format!("reading {PALETTE_FILE} from {}", game_dir.display()))?;
    ensure!(
        data.len() >= 256 * 3,
        "{PALETTE_FILE} is {} bytes; expected at least 768",
        data.len()
    );
    let mut palette = [Rgb([0u8, 0, 0]); 256];
    for (i, entry) in palette.iter_mut().enumerate() {
        let (r, g, b) = (data[i * 3], data[i * 3 + 1], data[i * 3 + 2]);
        *entry = Rgb([scale6(r), scale6(g), scale6(b)]);
    }
    Ok(palette)
}

/// Scale a 6-bit VGA channel (`0..=63`) to a full 8-bit value.
fn scale6(v: u8) -> u8 {
    (v << 2) | (v >> 4)
}

/// Decode the 256 referenced map tiles, each down-scaled to [`TILE_PX`].
fn decode_map_tiles(game_dir: &Path) -> Result<Vec<RgbImage>> {
    let palette = read_palette(game_dir)?;
    let pixels = lzw::decompress(&read(game_dir, MAPTILES_FILE)?)
        .with_context(|| format!("decompressing {MAPTILES_FILE}"))?;
    let mask = lzw::decompress(&read(game_dir, MASKTYPE_FILE)?)
        .with_context(|| format!("decompressing {MASKTYPE_FILE}"))?;
    let index = read(game_dir, TILEINDX_FILE)?;
    ensure!(
        index.len() >= MAP_TILE_COUNT * 2,
        "{TILEINDX_FILE} is too short ({} bytes)",
        index.len()
    );

    let mut tiles = Vec::with_capacity(MAP_TILE_COUNT);
    for t in 0..MAP_TILE_COUNT {
        let offset = usize::from(u16::from_le_bytes([index[t * 2], index[t * 2 + 1]])) * 16;
        let storage = mask.get(t).copied().unwrap_or(0);
        let native = decode_tile(&pixels, offset, storage, &palette);
        tiles.push(imageops_resize(&native, TILE_PX));
    }
    Ok(tiles)
}

/// Decode one 16×16 map tile at `offset` in the decompressed `MAPTILES.VGA` data, honouring its
/// storage `kind` (0 = opaque, 5 = transparent, 10 = span-compressed). Transparent pixels (value
/// 255 in the transparent/compressed kinds) are painted black, since a base-terrain tile has no
/// layer showing through beneath it.
fn decode_tile(data: &[u8], offset: usize, kind: u8, palette: &[Rgb<u8>; 256]) -> RgbImage {
    // 256 palette indices; 255 marks a transparent pixel in the non-opaque kinds.
    let mut px = [255u8; NATIVE_PX as usize * NATIVE_PX as usize];
    match kind {
        10 => decode_compressed(data, offset, &mut px),
        // Opaque and transparent are both a raw 16×16 block of palette indices.
        _ => {
            if let Some(raw) = data.get(offset..offset + px.len()) {
                px.copy_from_slice(raw);
            }
        }
    }

    let mut img = RgbImage::new(NATIVE_PX, NATIVE_PX);
    for (i, &idx) in px.iter().enumerate() {
        let color = if idx == 255 && kind != 0 {
            Rgb([0, 0, 0]) // transparent → black under the base map
        } else {
            palette[idx as usize]
        };
        img.put_pixel(i as u32 % NATIVE_PX, i as u32 / NATIVE_PX, color);
    }
    img
}

/// Span-decode a compressed (`kind` 10) tile into `px` (initialised to transparent). Each span is
/// `(uint16 displacement, uint8 length, uint8[length] data)`; a zero length ends the tile. The
/// displacement maps to an output advance of `displacement % 160` (plus 160 when `>= 1760`).
fn decode_compressed(data: &[u8], offset: usize, px: &mut [u8]) {
    let mut p = offset + 1; // skip the tile-length (page count) byte
    let mut out = 0usize;
    while p + 3 <= data.len() {
        let displacement = usize::from(data[p]) | usize::from(data[p + 1]) << 8;
        let length = usize::from(data[p + 2]);
        p += 3;
        if length == 0 {
            break;
        }
        out += displacement % 160;
        if displacement >= 1760 {
            out += 160;
        }
        let Some(src) = data.get(p..p + length) else {
            break;
        };
        for (i, &b) in src.iter().enumerate() {
            if let Some(slot) = px.get_mut(out + i) {
                *slot = b;
            }
        }
        p += length;
        out += length;
    }
}

/// Down-scale a native 16×16 tile to a `size`×`size` tile.
fn imageops_resize(tile: &RgbImage, size: u32) -> RgbImage {
    if size == NATIVE_PX {
        tile.clone()
    } else {
        image::imageops::resize(tile, size, size, FilterType::Triangle)
    }
}

/// Expand a map into a `edge`×`edge` grid of map-tile indices, resolving each chunk cell through
/// `chunk_index` and copying its 8×8 tiles from `CHUNKS`.
fn expand_grid(chunks: &[u8], edge: usize, chunk_index: impl Fn(usize, usize) -> usize) -> Vec<u8> {
    let mut grid = vec![0u8; edge * edge];
    for ty in 0..edge {
        for tx in 0..edge {
            let chunk = chunk_index(tx / CHUNK, ty / CHUNK);
            let base = chunk * CHUNK_BYTES + (ty % CHUNK) * CHUNK + (tx % CHUNK);
            grid[ty * edge + tx] = chunks.get(base).copied().unwrap_or(0);
        }
    }
    grid
}

/// The chunk index at chunk coordinate `(cx, cy)` (each `0..128`) within Britannia, unpacked from
/// the super-chunk-major `MAP` layout.
fn britannia_chunk_index(map: &[u8], cx: usize, cy: usize) -> usize {
    let sc = (cy / SC_CHUNKS) * SC_PER_EDGE + (cx / SC_CHUNKS);
    chunk_at(
        map,
        sc * SC_BYTES,
        BRIT_U24_PER_ROW,
        cx % SC_CHUNKS,
        cy % SC_CHUNKS,
    )
}

/// Unpack a 12-bit chunk index from a `uint24[..]` block: chunk `(cx, cy)` lives in the uint24 at
/// `cy * u24_per_row + cx/2`, in the low 12 bits when `cx` is even and the high 12 when odd.
fn chunk_at(map: &[u8], block: usize, u24_per_row: usize, cx: usize, cy: usize) -> usize {
    let at = block + (cy * u24_per_row + cx / 2) * 3;
    let u24 = usize::from(map[at]) | usize::from(map[at + 1]) << 8 | usize::from(map[at + 2]) << 16;
    if cx.is_multiple_of(2) {
        u24 & 0xFFF
    } else {
        (u24 >> 12) & 0xFFF
    }
}

fn read(game_dir: &Path, name: &str) -> Result<Vec<u8>> {
    std::fs::read(game_dir.join(name))
        .with_context(|| format!("reading {name} from {}", game_dir.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scales_six_bit_channels() {
        assert_eq!(scale6(0), 0);
        assert_eq!(scale6(63), 255);
        assert_eq!(scale6(0x2a), 170); // 42/63 ≈ 0.667 → 170
    }

    #[test]
    fn unpacks_paired_chunk_indices() {
        // One super-chunk (index 0): the first uint24 packs chunk 0x123 (low 12) and 0x456 (high).
        let mut map = vec![0u8; SC_PER_EDGE * SC_PER_EDGE * SC_BYTES];
        // uint24 = 0x456123: bytes 23 61 45.
        map[0] = 0x23;
        map[1] = 0x61;
        map[2] = 0x45;
        // Local chunk (0,0) is the low 12 bits; (1,0) is the high 12 bits.
        assert_eq!(britannia_chunk_index(&map, 0, 0), 0x123);
        assert_eq!(britannia_chunk_index(&map, 1, 0), 0x456);
    }

    #[test]
    fn super_chunk_selection_uses_x_plus_y8_order() {
        let mut map = vec![0u8; SC_PER_EDGE * SC_PER_EDGE * SC_BYTES];
        // Super-chunk (scx=1, scy=2) → index 2*8 + 1 = 17; put chunk 0x0AB in its first pair.
        let sc = 17;
        map[sc * SC_BYTES] = 0xAB;
        map[sc * SC_BYTES + 1] = 0x00;
        // Chunk coord in that super-chunk: cx = 1*16 = 16, cy = 2*16 = 32.
        assert_eq!(britannia_chunk_index(&map, 16, 32), 0x0AB);
    }

    #[test]
    fn dungeon_level_unpacks_at_16_uint24_per_row() {
        // A dungeon level (16 uint24 per row). Chunk (cx=3, cy=1) → uint24 at 1*16 + 3/2 = 17,
        // odd cx → the high 12 bits.
        let mut map = vec![0u8; DUNGEON_BYTES];
        let at = 17 * 3;
        // uint24 = 0x0CD000: high 12 = 0x0CD.
        map[at + 1] = 0xD0;
        map[at + 2] = 0x0C;
        assert_eq!(chunk_at(&map, 0, DUNGEON_U24_PER_ROW, 3, 1), 0x0CD);
    }

    #[test]
    fn decodes_an_opaque_tile() {
        let palette = std::array::from_fn(|i| Rgb([i as u8, 0, 0]));
        // A 256-byte opaque tile: pixel (x,y) = x (so column index becomes the red channel).
        let mut data = vec![0u8; 256];
        for y in 0..16 {
            for x in 0..16 {
                data[y * 16 + x] = x as u8;
            }
        }
        let tile = decode_tile(&data, 0, 0, &palette);
        assert_eq!(*tile.get_pixel(5, 3), Rgb([5, 0, 0]));
        assert_eq!(*tile.get_pixel(15, 0), Rgb([15, 0, 0]));
    }
}
