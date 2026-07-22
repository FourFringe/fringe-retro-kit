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

use std::collections::HashMap;
use std::path::Path;

use anyhow::{ensure, Context, Result};
use image::{imageops::FilterType, Rgb, RgbImage, Rgba, RgbaImage};

use crate::bundle::{Poi, World};
use crate::{lzw, tilemap};

/// This game's identifier, shared by every world it exports.
const GAME: &str = "ultima6";

const MAP_FILE: &str = "MAP";
const CHUNKS_FILE: &str = "CHUNKS";
const MAPTILES_FILE: &str = "MAPTILES.VGA";
const TILEINDX_FILE: &str = "TILEINDX.VGA";
const MASKTYPE_FILE: &str = "MASKTYPE.VGA";
const PALETTE_FILE: &str = "U6PAL";
const OBJTILES_FILE: &str = "OBJTILES.VGA";
const BASETILE_FILE: &str = "BASETILE";
const OBJBLK_FILE: &str = "LZOBJBLK";
const DNGBLK_FILE: &str = "LZDNGBLK";
const TILEFLAG_FILE: &str = "TILEFLAG";

/// `TILEFLAG` is a series of per-tile arrays; the second ("flags 2") starts here, one byte per
/// tile. Bit 7 marks a double-width tile, bit 6 a double-height one.
const TILEFLAG_FLAGS2_OFFSET: usize = 0x800;
const FLAG2_DOUBLE_WIDTH: u8 = 0x80;
const FLAG2_DOUBLE_HEIGHT: u8 = 0x40;

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
/// Total tiles in the concatenated map + object tile set.
const TILE_COUNT: usize = 2048;
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
    let tiles = Tiles::load(game_dir)?;
    let terrain = decode_map_tiles(&tiles);
    let map = read(game_dir, MAP_FILE)?;
    let chunks = read(game_dir, CHUNKS_FILE)?;
    let basetile = read(game_dir, BASETILE_FILE)?;
    let tileflag = read(game_dir, TILEFLAG_FILE)?;
    ensure!(
        map.len() >= BRITANNIA_BYTES + DUNGEON_LEVELS * DUNGEON_BYTES,
        "{MAP_FILE} is {} bytes; expected at least {}",
        map.len(),
        BRITANNIA_BYTES + DUNGEON_LEVELS * DUNGEON_BYTES
    );
    ensure!(!chunks.is_empty(), "{CHUNKS_FILE} is empty");

    let mut worlds = Vec::with_capacity(1 + DUNGEON_LEVELS);

    // The seamless Britannia overworld, with its surface objects overlaid.
    let britannia = expand_grid(&chunks, WORLD_TILES, |cx, cy| {
        britannia_chunk_index(&map, cx, cy)
    });
    let mut image = tilemap::render(&britannia, WORLD_TILES, WORLD_TILES, &terrain);
    let surface = parse_objects(
        &lzw::decompress(&read(game_dir, OBJBLK_FILE)?)
            .with_context(|| format!("decompressing {OBJBLK_FILE}"))?,
    );
    composite_objects(&mut image, &surface, &basetile, &tileflag, &tiles);
    worlds.push(tilemap::world(
        GAME,
        "britannia",
        "Ultima VI — Britannia",
        "overworld",
        "britannia",
        britannia_pois(),
        image,
    ));

    // The five underground dungeon levels, each with its own objects: `LZDNGBLK` block i holds
    // level i + 1's objects, in level-local coordinates.
    let dungeon_objects = parse_object_blocks(
        &lzw::decompress(&read(game_dir, DNGBLK_FILE)?)
            .with_context(|| format!("decompressing {DNGBLK_FILE}"))?,
        DUNGEON_LEVELS,
    );
    for level in 0..DUNGEON_LEVELS {
        let base = BRITANNIA_BYTES + level * DUNGEON_BYTES;
        let grid = expand_grid(&chunks, DUNGEON_TILES, |cx, cy| {
            chunk_at(&map, base, DUNGEON_U24_PER_ROW, cx, cy)
        });
        let mut image = tilemap::render(&grid, DUNGEON_TILES, DUNGEON_TILES, &terrain);
        if let Some(objects) = dungeon_objects.get(level) {
            composite_objects(&mut image, objects, &basetile, &tileflag, &tiles);
        }
        worlds.push(tilemap::world(
            GAME,
            &format!("dungeon-{}", level + 1),
            &format!("Ultima VI — Dungeon Level {}", level + 1),
            "dungeon",
            "britannia",
            vec![],
            image,
        ));
    }
    Ok(worlds)
}

/// Named overworld locations as `(label, tile x, tile y, kind)`. Ultima VI ships no location/name
/// table (its town names appear only in prose), so these are hand-authored: positions were read
/// off the rendered overworld and cross-referenced against Ultima IV's location table, since
/// Britannia's geography carries across the series. U6's towns are baked inline into the overworld
/// — there are no separate town maps — so these are label-only markers with no click target. A few
/// rearranged southern keeps are intentionally omitted rather than guessed.
const LOCATIONS: &[(&str, u32, u32, &str)] = &[
    ("Lord British's Castle", 295, 358, "castle"),
    ("Britain", 320, 405, "town"),
    ("Yew", 232, 168, "town"),
    ("Empath Abbey", 150, 200, "castle"),
    ("Minoc", 590, 105, "town"),
    ("Vesper", 563, 348, "town"),
    ("Cove", 560, 618, "town"),
    ("Paws", 388, 585, "town"),
    ("Trinsic", 385, 725, "town"),
    ("Skara Brae", 110, 505, "town"),
    ("Jhelom", 155, 850, "town"),
    ("New Magincia", 715, 655, "town"),
    ("Moonglow", 895, 520, "town"),
    ("The Lycaeum", 880, 400, "castle"),
    ("Serpent's Hold", 565, 945, "castle"),
];

/// The named label markers for the Britannia overworld, each centred on its tile in the rendered
/// image (which uses an 8-px tile edge, so a tile centre is `x * TILE_PX + TILE_PX / 2`).
fn britannia_pois() -> Vec<Poi> {
    LOCATIONS
        .iter()
        .map(|&(label, x, y, kind)| Poi {
            px: x * TILE_PX + TILE_PX / 2,
            py: y * TILE_PX + TILE_PX / 2,
            kind: kind.to_string(),
            label: label.to_string(),
            target: None,
        })
        .collect()
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

/// The decoded tile graphics: the concatenated `MAPTILES.VGA` + `OBJTILES.VGA` pixel data, each
/// tile's byte offset (`TILEINDX.VGA`) and storage kind (`MASKTYPE.VGA`), and the palette. Any of
/// the 2048 tiles decodes on demand — terrain uses the opaque map tiles, objects the transparent
/// object tiles.
struct Tiles {
    pixels: Vec<u8>,
    index: Vec<u8>,
    mask: Vec<u8>,
    palette: [Rgb<u8>; 256],
}

impl Tiles {
    fn load(game_dir: &Path) -> Result<Self> {
        let palette = read_palette(game_dir)?;
        // Map tiles are LZW-compressed; the object tiles are stored and logically concatenated
        // after them, so a single offset table indexes one continuous buffer.
        let mut pixels = lzw::decompress(&read(game_dir, MAPTILES_FILE)?)
            .with_context(|| format!("decompressing {MAPTILES_FILE}"))?;
        pixels.extend(read(game_dir, OBJTILES_FILE)?);
        let mask = lzw::decompress(&read(game_dir, MASKTYPE_FILE)?)
            .with_context(|| format!("decompressing {MASKTYPE_FILE}"))?;
        let index = read(game_dir, TILEINDX_FILE)?;
        ensure!(
            index.len() >= TILE_COUNT * 2,
            "{TILEINDX_FILE} is too short ({} bytes)",
            index.len()
        );
        Ok(Tiles {
            pixels,
            index,
            mask,
            palette,
        })
    }

    /// Decode tile `t` into its 256 palette indices plus its storage kind (255 marks a transparent
    /// pixel in the non-opaque kinds).
    fn indices(&self, t: usize) -> ([u8; NATIVE_PX as usize * NATIVE_PX as usize], u8) {
        let offset = usize::from(u16::from_le_bytes([
            self.index[t * 2],
            self.index[t * 2 + 1],
        ])) * 16;
        let kind = self.mask.get(t).copied().unwrap_or(0);
        let mut px = [255u8; NATIVE_PX as usize * NATIVE_PX as usize];
        match kind {
            10 => decode_compressed(&self.pixels, offset, &mut px),
            _ => {
                if let Some(raw) = self.pixels.get(offset..offset + px.len()) {
                    px.copy_from_slice(raw);
                }
            }
        }
        (px, kind)
    }

    /// A terrain tile as opaque RGB (transparent pixels → black, since the base map has nothing
    /// beneath), down-scaled to [`TILE_PX`].
    fn terrain(&self, t: usize) -> RgbImage {
        let (px, kind) = self.indices(t);
        let mut img = RgbImage::new(NATIVE_PX, NATIVE_PX);
        for (i, &idx) in px.iter().enumerate() {
            let color = if idx == 255 && kind != 0 {
                Rgb([0, 0, 0])
            } else {
                self.palette[idx as usize]
            };
            img.put_pixel(i as u32 % NATIVE_PX, i as u32 / NATIVE_PX, color);
        }
        imageops_resize(&img, TILE_PX)
    }

    /// An object tile as RGBA (transparent pixels → alpha 0), down-scaled to [`TILE_PX`] by nearest
    /// sampling so its hard edges survive.
    fn object(&self, t: usize) -> RgbaImage {
        let (px, kind) = self.indices(t);
        let mut img = RgbaImage::new(NATIVE_PX, NATIVE_PX);
        for (i, &idx) in px.iter().enumerate() {
            let color = if idx == 255 && kind != 0 {
                Rgba([0, 0, 0, 0])
            } else {
                let c = self.palette[idx as usize];
                Rgba([c[0], c[1], c[2], 255])
            };
            img.put_pixel(i as u32 % NATIVE_PX, i as u32 / NATIVE_PX, color);
        }
        image::imageops::resize(&img, TILE_PX, TILE_PX, FilterType::Nearest)
    }
}

/// Decode the 256 referenced map tiles for terrain, each down-scaled to [`TILE_PX`].
fn decode_map_tiles(tiles: &Tiles) -> Vec<RgbImage> {
    (0..MAP_TILE_COUNT).map(|t| tiles.terrain(t)).collect()
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

/// One placed object from `LZOBJBLK`: its global tile position, layer, and type/frame.
struct Object {
    x: u16,
    y: u16,
    z: u8,
    obj_type: u16,
    frame: u8,
}

/// Decode the 8-byte object at `data[p..]`: `(status, uint24 position, uint16 typeAndFrame,
/// quantity, quality)`. Position packs x (10 bits), y (10 bits), z (4 bits); typeAndFrame packs
/// the object type (10 bits) and frame (6 bits).
fn object_at(data: &[u8], p: usize) -> Option<Object> {
    let o = data.get(p..p + 8)?;
    let position = usize::from(o[1]) | usize::from(o[2]) << 8 | usize::from(o[3]) << 16;
    let taf = u16::from_le_bytes([o[4], o[5]]);
    Some(Object {
        x: (position & 0x3FF) as u16,
        y: ((position >> 10) & 0x3FF) as u16,
        z: ((position >> 20) & 0xF) as u8,
        obj_type: taf & 0x3FF,
        frame: ((taf >> 10) & 0x3F) as u8,
    })
}

/// Parse the decompressed `LZOBJBLK` (surface): a sequence of super-chunk blocks, each a `uint16`
/// object count followed by that many 8-byte objects. Surface positions are global tile
/// coordinates, so the block grouping doesn't affect placement — all objects are returned flat.
fn parse_objects(data: &[u8]) -> Vec<Object> {
    let mut objects = Vec::new();
    let mut p = 0;
    while p + 2 <= data.len() {
        let count = usize::from(u16::from_le_bytes([data[p], data[p + 1]]));
        p += 2;
        for _ in 0..count {
            let Some(o) = object_at(data, p) else {
                return objects;
            };
            objects.push(o);
            p += 8;
        }
    }
    objects
}

/// Parse the first `max_blocks` object blocks separately. Used for `LZDNGBLK`, where block *i*
/// holds dungeon level *i*'s objects in level-local coordinates.
fn parse_object_blocks(data: &[u8], max_blocks: usize) -> Vec<Vec<Object>> {
    let mut blocks = Vec::new();
    let mut p = 0;
    while blocks.len() < max_blocks && p + 2 <= data.len() {
        let count = usize::from(u16::from_le_bytes([data[p], data[p + 1]]));
        p += 2;
        let mut objects = Vec::with_capacity(count);
        for _ in 0..count {
            let Some(o) = object_at(data, p) else {
                break;
            };
            objects.push(o);
            p += 8;
        }
        blocks.push(objects);
    }
    blocks
}

/// The tiles making up one object, as `(tile index, cell dx, cell dy)` offsets from the anchor.
/// Most objects are a single tile at `(0, 0)`. Double-width and double-height objects extend one
/// cell **left** and/or **up** from the anchor, using consecutive tiles *below* the base index:
/// the base tile is the bottom-right, `tile - 1` its left/lower neighbour, and a 2×2 object fills
/// out with `tile - 2` (top-right) and `tile - 3` (top-left). The flag lives on the base tile in
/// `TILEFLAG`'s "flags 2" array.
fn multi_tile_parts(tile: usize, tileflag: &[u8]) -> Vec<(usize, i32, i32)> {
    let flags2 = tileflag
        .get(TILEFLAG_FLAGS2_OFFSET + tile)
        .copied()
        .unwrap_or(0);
    let wide = flags2 & FLAG2_DOUBLE_WIDTH != 0;
    let tall = flags2 & FLAG2_DOUBLE_HEIGHT != 0;
    let mut parts = vec![(tile, 0, 0)];
    // Extra tiles come from lower indices, so guard against underflow near tile 0.
    match (wide, tall) {
        (true, true) if tile >= 3 => parts.extend([
            (tile - 1, -1, 0),  // bottom-left
            (tile - 2, 0, -1),  // top-right
            (tile - 3, -1, -1), // top-left
        ]),
        (true, false) if tile >= 1 => parts.push((tile - 1, -1, 0)), // left half
        (false, true) if tile >= 1 => parts.push((tile - 1, 0, -1)), // upper half
        _ => {}
    }
    parts
}

/// Overlay `objects` onto the rendered terrain `image`, lowest layer first. Each object's base tile
/// is `BASETILE[type] + frame`; its transparent pixels let the terrain show through. Double-width
/// and double-height objects (per `TILEFLAG`) additionally draw their left/upper tiles so trees,
/// ships, statues and large furniture render whole rather than as a single corner.
fn composite_objects(
    image: &mut RgbImage,
    objects: &[Object],
    basetile: &[u8],
    tileflag: &[u8],
    tiles: &Tiles,
) {
    let mut order: Vec<&Object> = objects.iter().collect();
    order.sort_by_key(|o| o.z);
    let mut cache: HashMap<usize, RgbaImage> = HashMap::new();
    for o in order {
        let ty = usize::from(o.obj_type);
        if ty == 0 || basetile.len() < ty * 2 + 2 {
            continue; // type 0 is "nothing"
        }
        let base = usize::from(u16::from_le_bytes([basetile[ty * 2], basetile[ty * 2 + 1]]));
        let tile = base + usize::from(o.frame);
        if tile >= TILE_COUNT {
            continue;
        }
        for (part, cell_dx, cell_dy) in multi_tile_parts(tile, tileflag) {
            let sprite = cache.entry(part).or_insert_with(|| tiles.object(part));
            let cell_x = i32::from(o.x) + cell_dx;
            let cell_y = i32::from(o.y) + cell_dy;
            if cell_x < 0 || cell_y < 0 {
                continue;
            }
            let dx = cell_x as u32 * TILE_PX;
            let dy = cell_y as u32 * TILE_PX;
            for py in 0..TILE_PX {
                for px in 0..TILE_PX {
                    let pixel = sprite.get_pixel(px, py);
                    if pixel[3] == 0 {
                        continue; // transparent → keep the terrain underneath
                    }
                    let (ix, iy) = (dx + px, dy + py);
                    if ix < image.width() && iy < image.height() {
                        image.put_pixel(ix, iy, Rgb([pixel[0], pixel[1], pixel[2]]));
                    }
                }
            }
        }
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
    fn parses_objects_from_a_block() {
        // A one-object block: object at (x=300, y=500, z=1), type 0x2A, frame 3.
        let position = 300usize | (500 << 10) | (1 << 20);
        let taf: u16 = 0x2A | (3 << 10);
        let mut data = vec![1u8, 0]; // count = 1
        data.push(0xAB); // status
        data.push((position & 0xFF) as u8);
        data.push(((position >> 8) & 0xFF) as u8);
        data.push(((position >> 16) & 0xFF) as u8);
        data.extend_from_slice(&taf.to_le_bytes());
        data.push(5); // quantity
        data.push(6); // quality
        let objects = parse_objects(&data);
        assert_eq!(objects.len(), 1);
        let o = &objects[0];
        assert_eq!((o.x, o.y, o.z), (300, 500, 1));
        assert_eq!((o.obj_type, o.frame), (0x2A, 3));
    }

    #[test]
    fn decodes_tile_indices_and_object_transparency() {
        // Tile 0 stored transparent (kind 5): pixel (0,0) is transparent (255), (1,0) is index 3.
        let mut pixels = vec![255u8; 256];
        pixels[1] = 3;
        let palette = std::array::from_fn(|i| Rgb([i as u8, 0, 0]));
        let tiles = Tiles {
            pixels,
            index: vec![0u8, 0], // tile 0 offset 0
            mask: vec![5u8],     // transparent storage
            palette,
        };
        let (px, kind) = tiles.indices(0);
        assert_eq!(kind, 5);
        assert_eq!(px[0], 255); // transparent marker survives
        assert_eq!(px[1], 3);
        // As an object tile, the transparent pixel gets alpha 0; the opaque one keeps its colour.
        let obj = tiles.object(0);
        assert_eq!(obj.get_pixel(0, 0)[3], 0);
    }

    #[test]
    fn multi_tile_parts_expand_left_and_up() {
        let mut tileflag = vec![0u8; TILEFLAG_FLAGS2_OFFSET + TILE_COUNT];
        let single = 100;
        let wide = 200;
        let tall = 201;
        let quad = 202;
        tileflag[TILEFLAG_FLAGS2_OFFSET + wide] = FLAG2_DOUBLE_WIDTH;
        tileflag[TILEFLAG_FLAGS2_OFFSET + tall] = FLAG2_DOUBLE_HEIGHT;
        tileflag[TILEFLAG_FLAGS2_OFFSET + quad] = FLAG2_DOUBLE_WIDTH | FLAG2_DOUBLE_HEIGHT;

        assert_eq!(multi_tile_parts(single, &tileflag), vec![(single, 0, 0)]);
        assert_eq!(
            multi_tile_parts(wide, &tileflag),
            vec![(wide, 0, 0), (wide - 1, -1, 0)]
        );
        assert_eq!(
            multi_tile_parts(tall, &tileflag),
            vec![(tall, 0, 0), (tall - 1, 0, -1)]
        );
        assert_eq!(
            multi_tile_parts(quad, &tileflag),
            vec![
                (quad, 0, 0),
                (quad - 1, -1, 0),
                (quad - 2, 0, -1),
                (quad - 3, -1, -1),
            ]
        );
    }

    #[test]
    fn britannia_pois_center_on_their_tiles() {
        let pois = britannia_pois();
        assert_eq!(pois.len(), LOCATIONS.len());
        let lb = pois
            .iter()
            .find(|p| p.label == "Lord British's Castle")
            .expect("Lord British's Castle POI");
        // Tile (295, 358) centred at the 8-px tile edge, and label-only (no sub-map to open).
        assert_eq!((lb.px, lb.py), (295 * TILE_PX + 4, 358 * TILE_PX + 4));
        assert_eq!(lb.kind, "castle");
        assert!(pois.iter().all(|p| p.target.is_none()));
    }
}
