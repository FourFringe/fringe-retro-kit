//! Wasteland (1988, Interplay/EA — Steam re-release) world-map rendering.
//!
//! Wasteland is the project's first non-Ultima engine. Its data lives in a set of files (the
//! `ENKI` subfolder of the install): map files `MASTER1`/`MASTER2` (pristine) or `GAME1`/`GAME2`
//! (the working copies), and tile-graphics files `ALLHTDS1`/`ALLHTDS2`.
//!
//! Both map and tileset files are containers of **MSQ blocks**. Each map block holds one map:
//!
//! - The body is **partially XOR-encrypted** with a rolling key (`seed = b0 ^ b1`, then
//!   `key += 0x1F` per byte); encryption stops at the strings, which — with the tile-map — are
//!   stored plain.
//! - The decrypted body is: an action-class nibble map (`size²/2` bytes), an action map (`size²`
//!   bytes), a 44-byte central directory, a size byte, the [`Info`] block (whose byte 3 is the
//!   tileset index), then strings and — at the tail — the **Huffman-compressed tile map**.
//! - The tile map decompresses to `size²` tile indices (`size` is 32 or 64).
//!
//! Tiles come from `ALLHTDS{disk}`, a sequence of Huffman-compressed tilesets. Each 16×16 tile is
//! stored **vertically XOR-encoded** (each row XORed with the one above) as chunky 4-bit EGA
//! (two pixels per byte, high nibble first). See [`crate::huffman`]. The format matches Klaus
//! Reimer's `wlandsuite`; the rolling-XOR cipher is the same one `fringe-retro-core` uses for
//! Wasteland saves.

use std::path::{Path, PathBuf};

use anyhow::{ensure, Context, Result};
use image::RgbImage;

use crate::bundle::World;
use crate::ega::EGA_PALETTE;
use crate::huffman;
use crate::tilemap;

const GAME: &str = "wasteland";
/// One region groups every map so the browser lists them together.
const GROUP: &str = "wasteland";

/// The two disks' map files, preferred pristine (`MASTER`) then working (`GAME`), with the tileset
/// file for each. `(preferred_map, fallback_map, tileset_file, disk)`.
const DISKS: [(&str, &str, &str); 2] = [
    ("MASTER1", "GAME1", "ALLHTDS1"),
    ("MASTER2", "GAME2", "ALLHTDS2"),
];

/// Bytes per 16×16, 4-bit tile, and the per-row byte stride used by the vertical-XOR encoding.
const TILE_BYTES: usize = 128;
const ROW_BYTES: usize = 8; // 16 pixels / 2 per byte
const TILE_SIZE: u32 = 16;

/// The rolling-XOR key increment (matches the Wasteland save cipher).
const KEY_STEP: u8 = 0x1F;
/// Bytes needed to determine the map size before the full decrypt.
const PEEK_LEN: usize = 6189;
/// The `Info` block sits this many bytes past the action maps (central directory + size byte).
const INFO_SKIP: usize = 45;

/// Render every Wasteland map into its own world.
pub fn export_worlds(game_dir: &Path) -> Result<Vec<World>> {
    let dir = data_dir(game_dir);

    let mut worlds = Vec::new();
    let mut index = 0;
    for (primary, fallback, htds) in DISKS {
        let map_path = existing(&dir, &[primary, fallback]);
        let Some(map_path) = map_path else {
            continue; // this disk isn't present
        };
        let tilesets =
            read_tilesets(&dir.join(htds)).with_context(|| format!("reading tilesets {htds}"))?;
        let data =
            std::fs::read(&map_path).with_context(|| format!("reading {}", map_path.display()))?;

        for block in map_blocks(&data) {
            let Some(map) = parse_map(block) else {
                continue; // savegame or non-map block
            };
            let tileset = tilesets
                .get(map.tileset)
                .or_else(|| tilesets.first())
                .filter(|t| !t.is_empty());
            let Some(tileset) = tileset else { continue };

            // Remap tiles that fall outside this tileset (a few shared NPC/special tiles) to the
            // map's background tile so nothing renders as garbage.
            let background = if usize::from(map.background) < tileset.len() {
                map.background
            } else {
                0
            };
            let grid: Vec<u8> = map
                .tiles
                .iter()
                .map(|&t| {
                    if usize::from(t) < tileset.len() {
                        t
                    } else {
                        background
                    }
                })
                .collect();

            index += 1;
            let kind = if map.size == 64 { "overworld" } else { "town" };
            worlds.push(tilemap::world(
                GAME,
                &format!("map{index}"),
                &format!("Wasteland — Map {index}"),
                kind,
                GROUP,
                Vec::new(),
                tilemap::render(&grid, map.size, map.size, tileset),
            ));
        }
    }

    ensure!(
        !worlds.is_empty(),
        "no Wasteland maps found in {}",
        dir.display()
    );
    Ok(worlds)
}

/// The working save file that holds the live party state.
const SAVE_FILE: &str = "GAME1";
/// On-disk length of the savegame MSQ block (`msq0` + 2 seed bytes + `0x1200` body).
const SAVEGAME_BLOCK_LEN: usize = 6 + 0x1200;

/// The party's current position from the live save `GAME1`, as `(map_id, x, y)`.
///
/// The party block sits at the start of the decrypted savegame body: `0x08` = X, `0x09` = Y,
/// `0x0A` = the current map id. The map id is the map's block index on disk 1 (SoCal), which
/// equals `map{id + 1}` in our export order. Returns `None` when there's no save, or the party
/// is on a map we can't place (e.g. the Nevada disk, whose ids we don't yet disambiguate).
pub fn player_position(game_dir: &Path) -> Result<Option<(usize, u32, u32)>> {
    let path = data_dir(game_dir).join(SAVE_FILE);
    if !path.is_file() {
        return Ok(None);
    }
    let data = std::fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
    Ok(party_position(&data))
}

/// Extract `(map_id, x, y)` from a `GAME1` byte buffer, or `None` if no savegame block is found.
fn party_position(data: &[u8]) -> Option<(usize, u32, u32)> {
    for block in map_blocks(data) {
        if block.len() != SAVEGAME_BLOCK_LEN {
            continue;
        }
        // Decrypt just the head of the body, enough to cover the party position bytes.
        let Some(body) = block.get(4..) else {
            continue;
        };
        let dec = rolling_xor(body, 0x10);
        if !is_savegame_head(&dec) {
            continue;
        }
        return Some((
            usize::from(dec[0x0A]),
            u32::from(dec[0x08]),
            u32::from(dec[0x09]),
        ));
    }
    None
}

/// Whether a decrypted savegame-body head looks like the party block: bytes `1..8` are the party
/// member order — each `0..=7`, with non-zero values unique (matching `fringe-retro-core`).
fn is_savegame_head(dec: &[u8]) -> bool {
    if dec.len() < 0x0B {
        return false;
    }
    let mut seen = [false; 8];
    for &b in &dec[1..8] {
        if b > 7 {
            return false;
        }
        if b != 0 {
            if seen[usize::from(b)] {
                return false;
            }
            seen[usize::from(b)] = true;
        }
    }
    true
}

/// A decoded map: its edge length, tile-index grid, tileset selector, and background tile.
struct Map {
    size: usize,
    tiles: Vec<u8>,
    tileset: usize,
    background: u8,
}

/// Parse one MSQ map block, or `None` if it isn't a map (e.g. the savegame block).
fn parse_map(block: &[u8]) -> Option<Map> {
    let body = block.get(4..)?; // skip the "msqN" header
    if body.len() < 2 {
        return None;
    }
    let cipher = &body[2..];

    // Peek: decrypt enough to read the map size, then read the encrypted-region length and
    // decrypt exactly that much (the tail — strings and tile map — is stored plain).
    let peek = rolling_xor(body, PEEK_LEN.min(cipher.len()));
    let size = determine_map_size(&peek)?;
    let enc_off = size * size * 3 / 2;
    let enc_size = usize::from(u16::from_le_bytes([
        *peek.get(enc_off)?,
        *peek.get(enc_off + 1)?,
    ]));
    if enc_size > cipher.len() {
        return None;
    }
    let mut full = rolling_xor(body, enc_size);
    full.extend_from_slice(&cipher[enc_size..]);

    let info_off = enc_off + INFO_SKIP;
    let tileset = usize::from(*full.get(info_off + 3)?);
    let background = *full.get(info_off + 6)?;
    let tiles = tile_map(size, &full)?;
    Some(Map {
        size,
        tiles,
        tileset,
        background,
    })
}

/// Decrypt the first `n` bytes of a block body's ciphertext (which begins after the two seed
/// bytes) with the rolling-XOR cipher.
fn rolling_xor(body: &[u8], n: usize) -> Vec<u8> {
    let mut key = body[0] ^ body[1];
    body[2..2 + n]
        .iter()
        .map(|&c| {
            let plain = c ^ key;
            key = key.wrapping_add(KEY_STEP);
            plain
        })
        .collect()
}

/// Determine a map's edge length (64 or 32) from its decrypted bytes, or `None` if neither fits.
/// The size byte and two zero bytes sit at a fixed offset past the action maps.
fn determine_map_size(dec: &[u8]) -> Option<usize> {
    for size in [64usize, 32] {
        let off = size * size * 3 / 2;
        if off + 44 < dec.len()
            && dec[off + 44] == size as u8
            && dec[off + 6] == 0
            && dec[off + 7] == 0
        {
            return Some(size);
        }
    }
    None
}

/// Locate and Huffman-decompress the tile map from the tail of the (decrypted) map body.
fn tile_map(size: usize, full: &[u8]) -> Option<Vec<u8>> {
    let count = size * size;
    let high = (count >> 8) as u8;
    // The tile-map header — a 32-bit uncompressed size (`count`) and a 32-bit unknown — is found by
    // scanning back from the end for its recognisable byte pattern.
    let offset = (0..=full.len().checked_sub(9)?).rev().find(|&i| {
        full[i] == 0
            && full[i + 1] == high
            && full[i + 2] == 0
            && full[i + 3] == 0
            && full[i + 6] == 0
            && full[i + 7] == 0
    })?;
    let (tiles, _) = huffman::decompress(full, offset + 8, count).ok()?;
    Some(tiles)
}

/// Decode an `ALLHTDS` file into its tilesets (each a list of 16×16 tiles).
fn read_tilesets(path: &Path) -> Result<Vec<Vec<RgbImage>>> {
    let data = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    let mut tilesets = Vec::new();
    let mut pos = 0;
    while pos + 8 <= data.len() {
        let size = u32::from_le_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]);
        // A compressed MSQ block: [size:u32][ "msq" + raw disk ][ Huffman ].
        if &data[pos + 4..pos + 7] != b"msq" {
            break;
        }
        let (raw, end) = huffman::decompress(&data, pos + 8, size as usize)
            .with_context(|| format!("decompressing tileset at {pos:#x}"))?;
        tilesets.push(decode_tileset(&raw));
        pos = end;
    }
    Ok(tilesets)
}

/// Split a decompressed tileset into its 16×16 tiles.
fn decode_tileset(raw: &[u8]) -> Vec<RgbImage> {
    raw.chunks_exact(TILE_BYTES).map(decode_tile).collect()
}

/// Decode one 128-byte tile: undo the vertical XOR, then read chunky 4-bit EGA pixels (two per
/// byte, high nibble = left) through the standard EGA palette.
fn decode_tile(tile: &[u8]) -> RgbImage {
    let mut b = tile.to_vec();
    for i in ROW_BYTES..TILE_BYTES {
        b[i] ^= b[i - ROW_BYTES];
    }
    let mut img = RgbImage::new(TILE_SIZE, TILE_SIZE);
    for (k, &byte) in b.iter().enumerate() {
        let x = (k * 2) as u32 % TILE_SIZE;
        let y = (k * 2) as u32 / TILE_SIZE;
        img.put_pixel(x, y, EGA_PALETTE[usize::from(byte >> 4)]);
        img.put_pixel(x + 1, y, EGA_PALETTE[usize::from(byte & 0x0F)]);
    }
    img
}

/// Split a map/tileset container into its MSQ blocks (each starting at an "msqN" ASCII header).
fn map_blocks(data: &[u8]) -> Vec<&[u8]> {
    let starts: Vec<usize> = (0..data.len().saturating_sub(3))
        .filter(|&i| &data[i..i + 3] == b"msq" && matches!(data[i + 3], b'0' | b'1'))
        .collect();
    starts
        .iter()
        .enumerate()
        .map(|(k, &start)| {
            let end = starts.get(k + 1).copied().unwrap_or(data.len());
            &data[start..end]
        })
        .collect()
}

/// Resolve the directory that actually holds the data files: `game_dir`, or its `ENKI` subfolder
/// (where the Steam re-release keeps them).
fn data_dir(game_dir: &Path) -> PathBuf {
    if game_dir.join("ALLHTDS1").is_file() {
        game_dir.to_path_buf()
    } else {
        game_dir.join("ENKI")
    }
}

/// The first of `names` that exists as a file in `dir`.
fn existing(dir: &Path, names: &[&str]) -> Option<PathBuf> {
    names.iter().map(|n| dir.join(n)).find(|p| p.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rolling_xor_matches_cipher() {
        // seed = 0x10 ^ 0x20 = 0x30; then key advances by 0x1F.
        let body = [0x10, 0x20, 0x30 ^ 0xAA, (0x30 + 0x1F) ^ 0xBB];
        assert_eq!(rolling_xor(&body, 2), vec![0xAA, 0xBB]);
    }

    #[test]
    fn map_size_from_marker() {
        let mut dec = vec![0u8; 32 * 32 * 3 / 2 + 45];
        let off = 32 * 32 * 3 / 2;
        dec[off + 44] = 32;
        assert_eq!(determine_map_size(&dec), Some(32));
        dec[off + 44] = 99;
        assert_eq!(determine_map_size(&dec), None);
    }

    #[test]
    fn decode_tile_undoes_vertical_xor() {
        // Row 0 = value V; row 1 stored as (V ^ V) = 0 decodes back to V (so both rows equal V).
        let mut tile = vec![0u8; TILE_BYTES];
        for i in 0..ROW_BYTES {
            tile[i] = 0x12; // row 0
            tile[ROW_BYTES + i] = 0x12 ^ 0x12; // row 1 encoded delta = 0
        }
        let img = decode_tile(&tile);
        // Pixel (0,0) and (0,1) should be the same colour (high nibble of 0x12 = 1).
        assert_eq!(img.get_pixel(0, 0), img.get_pixel(0, 1));
        assert_eq!(*img.get_pixel(0, 0), EGA_PALETTE[1]);
    }

    #[test]
    fn party_position_reads_savegame_block() {
        // Build a savegame body: party order 1..4, then X=7, Y=1, map=10 at 0x08..0x0B.
        let mut body = vec![0u8; 0x1200];
        body[1..5].copy_from_slice(&[1, 2, 3, 4]);
        body[0x08] = 7;
        body[0x09] = 1;
        body[0x0A] = 10;

        // Encrypt it into an MSQ block (the inverse of `rolling_xor`), then a second header so
        // `map_blocks` bounds the savegame block at exactly SAVEGAME_BLOCK_LEN.
        let (s0, s1) = (0x10u8, 0x20u8);
        let mut data = vec![b'm', b's', b'q', b'0', s0, s1];
        let mut key = s0 ^ s1;
        for &p in &body {
            data.push(p ^ key);
            key = key.wrapping_add(KEY_STEP);
        }
        assert_eq!(data.len(), SAVEGAME_BLOCK_LEN);
        data.extend_from_slice(b"msq0"); // a following block bounds the first

        assert_eq!(party_position(&data), Some((10, 7, 1)));
    }
}
