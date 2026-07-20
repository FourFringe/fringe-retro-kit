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
//! (two pixels per byte, high nibble first). See [`crate::huffman`]. A map square's tile value
//! selects the graphic: values `0..10` are the shared **sprites** in `ic0_9.wlf` (planar 4-bit
//! EGA), and values `10+` index the map's tileset (tile `value - 10`). The format matches Klaus
//! Reimer's `wlandsuite`; the rolling-XOR cipher is the same one `fringe-retro-core` uses for
//! Wasteland saves.

use std::collections::HashMap;
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

/// The two disks' map files, preferred pristine (`MASTER`) then working (`GAME`).
const MAP_FILES: [(&str, &str); 2] = [("MASTER1", "GAME1"), ("MASTER2", "GAME2")];

/// The two tile-graphics files. A map's tileset id `< 4` selects a tileset from the first;
/// otherwise `id - 4` selects one from the second (matching `wlandsuite`).
const HTDS_FILES: [&str; 2] = ["ALLHTDS1", "ALLHTDS2"];

/// Manually-confirmed `(block index, game map id)` links for locations whose map block has no
/// name string, so they can't be linked to the engine's map id by parsing alone (that id → block
/// table lives in the game executable). Confirmed by playing and reading the save's current-map
/// id, e.g. the Agricultural Center. Applied on top of the name-matched links below.
const CONFIRMED_MAP_IDS: &[(usize, u32)] = &[
    // (block index, game map id) — populate as locations are confirmed in-game.
    (8, 9), // Agricultural Center: its block has no name string; confirmed in-game (curMap 9).
    (18, 43), // A mine shaft: confirmed in-game.
    (35, 35), // The Guardian's Citadel: confirmed in-game.
    (13, 29), // Ag Center's cellar ("descend down a dark tunnel"): confirmed in-game.
];

/// Bytes per 16×16, 4-bit tile, and the per-row byte stride used by the vertical-XOR encoding.
const TILE_BYTES: usize = 128;
const ROW_BYTES: usize = 8; // 16 pixels / 2 per byte
const TILE_SIZE: u32 = 16;

/// The shared sprite file (`ic0_9.wlf`): 10 planar-EGA 16×16 sprites, tile values 0-9.
const SPRITE_FILE: &str = "IC0_9.WLF";
/// Number of shared sprites (tile values `0..SPRITE_COUNT` come from [`SPRITE_FILE`]).
const SPRITE_COUNT: usize = 10;

/// The rolling-XOR key increment (matches the Wasteland save cipher).
const KEY_STEP: u8 = 0x1F;
/// Bytes needed to determine the map size before the full decrypt.
const PEEK_LEN: usize = 6189;
/// The `Info` block sits this many bytes past the action maps (central directory + size byte).
const INFO_SKIP: usize = 45;

/// Render every Wasteland map into its own world.
pub fn export_worlds(game_dir: &Path) -> Result<Vec<World>> {
    let dir = data_dir(game_dir);

    // The 10 shared sprites (`ic0_9.wlf`) are tile values 0-9; tileset tiles are values 10+.
    let sprites = read_sprites(&dir.join(SPRITE_FILE))?;
    // Tileset "banks" from both graphics files: each is the 10 sprites followed by one tileset,
    // so a raw tile value indexes it directly (0-9 = sprite, 10+ = tileset tile value-10). A map's
    // tileset id < 4 selects a bank from ALLHTDS1, otherwise id-4 from ALLHTDS2.
    let banks1 = read_banks(&dir.join(HTDS_FILES[0]), &sprites)?;
    let banks2 = read_banks(&dir.join(HTDS_FILES[1]), &sprites)?;

    // Pass 1: parse every renderable map and remap its tiles, collecting the `name -> map id`
    // table from *all* maps' transition actions (not just the overworld) so as many blocks as
    // possible can be linked to the engine's own map id, and their transitions to each other.
    let mut parsed: Vec<(Map, Vec<u8>)> = Vec::new();
    let mut id_by_name: HashMap<String, u32> = HashMap::new();
    for (primary, fallback) in MAP_FILES {
        let Some(map_path) = existing(&dir, &[primary, fallback]) else {
            continue; // this disk isn't present
        };
        let data =
            std::fs::read(&map_path).with_context(|| format!("reading {}", map_path.display()))?;

        for block in map_blocks(&data) {
            let Some(map) = parse_map(block) else {
                continue; // savegame or non-map block
            };
            let Some(bank) = select_bank(&banks1, &banks2, map.tileset, sprites.len()) else {
                continue;
            };
            // Remap tiles that fall outside this bank (a few shared NPC/special tiles) to the
            // map's background tile so nothing renders as garbage.
            let background = if usize::from(map.background) < bank.len() {
                map.background
            } else {
                0
            };
            let grid: Vec<u8> = map
                .tiles
                .iter()
                .map(|&t| {
                    if usize::from(t) < bank.len() {
                        t
                    } else {
                        background
                    }
                })
                .collect();
            for loc in &map.locations {
                if let Some(name) = &loc.name {
                    id_by_name
                        .entry(name.to_ascii_lowercase())
                        .or_insert(loc.map_id);
                }
            }
            parsed.push((map, grid));
        }
    }

    // Resolve each block's game map id: block 0 is the overworld (id 0); the rest match by their
    // recovered name, with any manually-confirmed links overriding. Then index those ids so a
    // transition's destination id resolves to a world slug (for links) and a name (for labels).
    let map_ids: Vec<Option<u32>> = parsed
        .iter()
        .enumerate()
        .map(|(i, (map, _))| {
            if i == 0 {
                return Some(0);
            }
            CONFIRMED_MAP_IDS
                .iter()
                .find(|&&(block, _)| block == i)
                .map(|&(_, id)| id)
                .or_else(|| {
                    map.name
                        .as_deref()
                        .and_then(|n| match_map_id(&id_by_name, n))
                })
        })
        .collect();
    let slug_by_id: HashMap<u32, String> = map_ids
        .iter()
        .enumerate()
        .filter_map(|(i, id)| id.map(|id| (id, format!("map{i}"))))
        .collect();
    let name_by_id: HashMap<u32, String> = parsed
        .iter()
        .enumerate()
        .filter_map(|(i, (map, _))| Some((map_ids[i]?, map.name.clone()?)))
        .collect();

    // Pass 2: render each map and attach POIs. The overworld shows every named town entrance;
    // sub-maps show only transitions that lead to another rendered map, as clickable jumps — as
    // close to in-game navigation as we can get without a POI on every unrenderable shop door.
    let mut worlds = Vec::new();
    for (i, (map, grid)) in parsed.iter().enumerate() {
        let Some(bank) = select_bank(&banks1, &banks2, map.tileset, sprites.len()) else {
            continue;
        };
        // Only the SoCal wilderness (block 0) is a true overworld; the other large maps are
        // locations, so they nest beneath it.
        let kind = if i == 0 { "overworld" } else { "town" };
        let self_id = map_ids[i];
        let mut pois = Vec::new();
        for loc in &map.locations {
            // Link to the destination map when it's a *different* rendered map (not the
            // overworld's own "Ranger Center" tile, and not a tile that loops back to itself).
            let target = (loc.map_id != 0 && Some(loc.map_id) != self_id)
                .then(|| slug_by_id.get(&loc.map_id))
                .flatten()
                .map(|slug| format!("/{GAME}/{slug}"));
            let label = loc
                .name
                .clone()
                .or_else(|| name_by_id.get(&loc.map_id).cloned());
            if i == 0 {
                // Overworld: every named location, clickable when we can open it.
                if let Some(label) = label {
                    let mut poi = tilemap::poi(loc.src_x, loc.src_y, "town", &label);
                    poi.target = target;
                    pois.push(poi);
                }
            } else if target.is_some() {
                // Sub-map: only tiles that jump to another map we render. When the destination
                // has no recovered name, label it by the map number it opens (from the slug),
                // which is what the player lands on — not the internal game id.
                let label = label.unwrap_or_else(|| {
                    slug_by_id
                        .get(&loc.map_id)
                        .map(|slug| format!("Map {}", slug.trim_start_matches("map")))
                        .unwrap_or_else(|| format!("Map {}", loc.map_id))
                });
                let mut poi = tilemap::poi(loc.src_x, loc.src_y, "passage", &label);
                poi.target = target;
                pois.push(poi);
            }
        }
        // Keep the map number (the world slug) and append the recovered name.
        let title = match &map.name {
            Some(name) => format!("Wasteland — Map {i}: {name}"),
            None => format!("Wasteland — Map {i}"),
        };
        let mut world = tilemap::world(
            GAME,
            &format!("map{i}"),
            &title,
            kind,
            GROUP,
            pois,
            tilemap::render(grid, map.size, map.size, bank),
        );
        world.meta.map_id = map_ids[i];
        worlds.push(world);
    }

    ensure!(
        !worlds.is_empty(),
        "no Wasteland maps found in {}",
        dir.display()
    );
    Ok(worlds)
}

/// Select the tileset "bank" for a map's tileset id: `< 4` picks from `banks1` (ALLHTDS1),
/// otherwise `id - 4` from `banks2` (ALLHTDS2). `None` if the bank is missing or empty (only the
/// shared sprites), in which case the map can't be rendered.
fn select_bank<'a>(
    banks1: &'a [Vec<RgbImage>],
    banks2: &'a [Vec<RgbImage>],
    tileset: usize,
    sprite_count: usize,
) -> Option<&'a Vec<RgbImage>> {
    let bank = if tileset < 4 {
        banks1.get(tileset)
    } else {
        banks2.get(tileset - 4)
    };
    bank.filter(|b| b.len() > sprite_count)
}

/// The working save file that holds the live party state.
const SAVE_FILE: &str = "GAME1";
/// On-disk length of the savegame MSQ block (`msq0` + 2 seed bytes + `0x1200` body).
const SAVEGAME_BLOCK_LEN: usize = 6 + 0x1200;

/// The party's live location from the save's party block. `map`/`x`/`y` is where the party is
/// now; `return_map`/`return_x`/`return_y` is the **parent** map and the tile the party entered
/// the current map from, so a town's position can also be drawn on the overworld it was entered
/// from. All map ids are the engine's own global ids (see [`player_position`]).
pub struct PartyLocation {
    pub map: usize,
    pub x: u32,
    pub y: u32,
    pub return_map: usize,
    pub return_x: u32,
    pub return_y: u32,
}

/// The party's current position from the live save `GAME1`.
///
/// The party block sits at the start of the decrypted savegame body: `0x08` = X, `0x09` = Y,
/// `0x0A` = the current map id, and `0x0B`/`0x0C`/`0x0D` = the return X / Y / map. The map ids
/// are the engine's own global ids, which are *not* the block index on disk — the server resolves
/// them to worlds via each manifest's `map_id` (recovered from the overworld's transition
/// actions). Returns `None` when there's no save.
pub fn player_position(game_dir: &Path) -> Result<Option<PartyLocation>> {
    let path = data_dir(game_dir).join(SAVE_FILE);
    if !path.is_file() {
        return Ok(None);
    }
    let data = std::fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
    Ok(party_position(&data))
}

/// Extract the [`PartyLocation`] from a `GAME1` byte buffer, or `None` if no savegame block found.
fn party_position(data: &[u8]) -> Option<PartyLocation> {
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
        return Some(PartyLocation {
            map: usize::from(dec[0x0A]),
            x: u32::from(dec[0x08]),
            y: u32::from(dec[0x09]),
            return_map: usize::from(dec[0x0D]),
            return_x: u32::from(dec[0x0B]),
            return_y: u32::from(dec[0x0C]),
        });
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

/// A decoded map: its edge length, tile-index grid, tileset selector, background tile, and (if
/// one could be recovered from its strings) a display name.
struct Map {
    size: usize,
    tiles: Vec<u8>,
    tileset: usize,
    background: u8,
    name: Option<String>,
    /// Outgoing "transition" actions (class `0xa`) on this map: each is a tile that moves the
    /// party to another map. On the overworld these are the town entrances, so they supply both
    /// the location markers and the `game map id -> destination name` table used to link each
    /// data block to the map id the savegame stores.
    locations: Vec<Location>,
}

/// One transition (class `0xa`) action: the source tile it sits on, the destination map id the
/// save would record on entry, and the destination's name (from the action's message), if any.
struct Location {
    src_x: u32,
    src_y: u32,
    map_id: u32,
    name: Option<String>,
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
    let tilemap_off = find_tilemap_offset(size, &full)?;
    let (tiles, _) = huffman::decompress(&full, tilemap_off + 8, size * size).ok()?;
    // The strings run from the encryption boundary (`enc_size`) up to the tile map.
    let strings = decode_strings(&full, enc_size, tilemap_off);
    let name = map_name(&strings);
    let locations = decode_locations(&full, size, &strings);
    Some(Map {
        size,
        tiles,
        tileset,
        background,
        name,
        locations,
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
/// Find the tile-map header in the (decrypted) map body: a 32-bit uncompressed size (`size²`) and
/// a 32-bit unknown, located by scanning back from the end for its recognisable byte pattern.
fn find_tilemap_offset(size: usize, full: &[u8]) -> Option<usize> {
    let high = ((size * size) >> 8) as u8;
    (0..=full.len().checked_sub(9)?).rev().find(|&i| {
        full[i] == 0
            && full[i + 1] == high
            && full[i + 2] == 0
            && full[i + 3] == 0
            && full[i + 6] == 0
            && full[i + 7] == 0
    })
}

/// A least-significant-bit-first bit reader over the map body, matching `wlandsuite`'s
/// `BitInputStream` in reverse mode (used for the 5-bit packed strings).
struct StrBits<'a> {
    data: &'a [u8],
    pos: usize,
    cur: u8,
    bit: u8,
}

impl<'a> StrBits<'a> {
    fn new(data: &'a [u8], pos: usize) -> Self {
        StrBits {
            data,
            pos,
            cur: 0,
            bit: 7,
        }
    }

    /// Read one bit, LSB-first within each byte.
    fn read_bit(&mut self) -> u8 {
        if self.bit > 6 {
            self.cur = self.data.get(self.pos).copied().unwrap_or(0);
            self.pos += 1;
            self.bit = 0;
        } else {
            self.bit += 1;
        }
        (self.cur >> self.bit) & 1
    }

    /// Read a 5-bit value, LSB-first.
    fn read5(&mut self) -> u8 {
        (0..5).fold(0u8, |v, i| v | (self.read_bit() << i))
    }
}

/// Decode a map's strings (the messages shown on that map). Layout: a 60-byte character table,
/// a word offset table (`first_word / 2` entries), then string groups of four. Each string is a
/// stream of 5-bit indices into the char table; `0x1F` selects the table's high half for the next
/// character, `0x1E` upper-cases it, and a character of `0` ends the string. See `wlandsuite`'s
/// `Strings`/`CharTable`. `start` is the strings offset, `end` bounds them (the tile map).
fn decode_strings(full: &[u8], start: usize, end: usize) -> Vec<String> {
    let mut out = Vec::new();
    let Some(char_table) = full.get(start..start + 60) else {
        return out;
    };
    let read_u16 = |p: usize| {
        full.get(p..p + 2)
            .map(|b| usize::from(u16::from_le_bytes([b[0], b[1]])))
    };
    let base = start + 60;
    let Some(first) = read_u16(base) else {
        return out;
    };
    let quantity = first / 2;
    let mut offsets = Vec::new();
    let mut prev = 0;
    for i in 0..quantity {
        let Some(off) = read_u16(base + i * 2) else {
            break;
        };
        if off < prev || base + off >= end {
            break;
        }
        offsets.push(off);
        prev = off;
    }
    for off in offsets {
        let mut bits = StrBits::new(full, base + off);
        for _ in 0..4 {
            let (mut upper, mut high) = (false, false);
            let mut s = String::new();
            loop {
                if bits.pos > end {
                    break;
                }
                match bits.read5() {
                    0x1F => high = true,
                    0x1E => upper = true,
                    index => {
                        let Some(&ch) =
                            char_table.get(usize::from(index) + usize::from(high) * 0x1E)
                        else {
                            break;
                        };
                        if ch == 0 {
                            break;
                        }
                        let c = ch as char;
                        if upper {
                            s.extend(c.to_uppercase());
                        } else {
                            s.push(c);
                        }
                        upper = false;
                        high = false;
                    }
                }
            }
            out.push(s);
        }
    }
    out
}

/// Best-effort map name: the place named by the first "Welcome to X" / "Leaving X" message, if it
/// looks like a proper location (rejecting generic fragments like "the room" or "a hole …").
fn map_name(strings: &[String]) -> Option<String> {
    for s in strings {
        let lower = s.to_ascii_lowercase();
        for prefix in ["welcome to ", "leaving "] {
            let Some(pos) = lower.find(prefix) else {
                continue;
            };
            let rest = &s[pos + prefix.len()..];
            let name: String = rest
                .chars()
                .take_while(|c| !matches!(c, '.' | '?' | '!' | '"' | ',' | ';'))
                .collect();
            let name = name.trim();
            let low = name.to_ascii_lowercase();
            let generic = ["hole", "room", "through", "area", "wall"]
                .iter()
                .any(|w| low.contains(w));
            if (2..=24).contains(&name.len()) && !generic {
                return Some(name.to_owned());
            }
        }
    }
    None
}

/// The action class of transition (map-change) actions in the central directory's per-class
/// action tables.
const TRANSITION_CLASS: usize = 0xa;

/// Decode a map's transition actions (class [`TRANSITION_CLASS`]): the tiles that move the party
/// to another map. The decrypted body begins with an **action-class nibble map** (`size²/2`
/// bytes, two tiles per byte, high nibble first) then an **action-selector map** (`size²` bytes).
/// The 44-byte central directory at `size²*3/2` holds, after three words, a 16-word master table
/// of per-class action-offset tables; index [`TRANSITION_CLASS`] gives the transition table's
/// offset. A tile's selector indexes that table (a word; `0` = none) to reach its action, whose
/// bytes are: message (6 low bits of byte 0), signed dx/dy, then the destination map id (byte 3).
/// Matches `wlandsuite`'s `TransitionAction`/`CentralDirectory`.
fn decode_locations(full: &[u8], size: usize, strings: &[String]) -> Vec<Location> {
    let dir_off = size * size * 3 / 2;
    let read_u16 = |p: usize| {
        full.get(p..p + 2)
            .map(|b| usize::from(u16::from_le_bytes([b[0], b[1]])))
    };
    // The master table starts after three words (strings/monster-name/monster-data offsets).
    let Some(table) = read_u16(dir_off + 6 + TRANSITION_CLASS * 2) else {
        return Vec::new();
    };
    if table == 0 {
        return Vec::new(); // this map has no transition actions
    }
    let selector_map = size * size / 2; // action-selector map follows the class nibble map
    let mut seen: Vec<u32> = Vec::new();
    let mut out = Vec::new();
    for y in 0..size {
        for x in 0..size {
            let cell = y * size + x;
            let Some(&packed) = full.get(cell / 2) else {
                continue;
            };
            let class = if cell.is_multiple_of(2) {
                packed >> 4
            } else {
                packed & 0x0f
            };
            if usize::from(class) != TRANSITION_CLASS {
                continue;
            }
            let Some(&selector) = full.get(selector_map + cell) else {
                continue;
            };
            let Some(action) = read_u16(table + usize::from(selector) * 2).filter(|&o| o != 0)
            else {
                continue;
            };
            let (Some(&flags), Some(&target)) = (full.get(action), full.get(action + 3)) else {
                continue;
            };
            let map_id = u32::from(target);
            if seen.contains(&map_id) {
                continue; // one marker per destination (towns span several tiles)
            }
            seen.push(map_id);
            let name = location_name(strings.get(usize::from(flags & 0x3f)));
            out.push(Location {
                src_x: x as u32,
                src_y: y as u32,
                map_id,
                name,
            });
        }
    }
    out
}

/// The place a transition **leads to**, from its message ("Entering X" / "X"), cleaned to a bare
/// name — or `None` if it doesn't look like a destination. A "Leaving X" message names where you
/// *are* (the exit), not where you're going, so it's rejected: used as a destination name it would
/// mislink a block (e.g. "Leaving Nomads" → the overworld would tag Nomads with the overworld's
/// id). Generic fragments are rejected as [`map_name`] does.
fn location_name(message: Option<&String>) -> Option<String> {
    let raw = message?.trim();
    let lower = raw.to_ascii_lowercase();
    if lower.starts_with("leaving ") {
        return None;
    }
    let mut rest = raw;
    for prefix in ["entering ", "welcome to "] {
        if lower.starts_with(prefix) {
            rest = &raw[prefix.len()..];
            break;
        }
    }
    let name: String = rest
        .chars()
        .take_while(|c| !matches!(c, '.' | '?' | '!' | '"' | ',' | ';'))
        .collect();
    let name = name.trim();
    let low = name.to_ascii_lowercase();
    let generic = ["hole", "room", "through", "area", "wall"]
        .iter()
        .any(|w| low.contains(w));
    ((2..=24).contains(&name.len()) && !generic).then(|| name.to_owned())
}

/// Match a data block's recovered [`map_name`] against the `name -> map id` table (built from
/// every map's transition messages), returning the engine map id for that block. To stay
/// deterministic and avoid false links, matching only succeeds when it's **unambiguous**: an exact
/// (case-insensitive) match, else a space-insensitive one ("Stagecoach Inn" = "Stage Coach Inn"),
/// else a trailing-word match ("Nomads" ⊂ "Desert Nomads") — the last two only when exactly one
/// map id qualifies. The shared part must be at least [`MIN_FUZZY`] chars so short words like
/// "inn" or "bar" don't link unrelated places.
const MIN_FUZZY: usize = 5;

fn match_map_id(table: &HashMap<String, u32>, block_name: &str) -> Option<u32> {
    let name = block_name.trim().to_ascii_lowercase();
    if let Some(&id) = table.get(&name) {
        return Some(id);
    }
    // A single id whose (normalized) name matches, or `None` if zero or several do.
    let unique = |ids: &mut Vec<u32>| -> Option<u32> {
        ids.sort_unstable();
        ids.dedup();
        (ids.len() == 1).then(|| ids[0])
    };
    let squashed = name.replace(' ', "");
    let mut spaced: Vec<u32> = table
        .iter()
        .filter(|(k, _)| k.replace(' ', "") == squashed)
        .map(|(_, &id)| id)
        .collect();
    if let Some(id) = unique(&mut spaced) {
        return Some(id);
    }
    let mut tail: Vec<u32> = table
        .iter()
        .filter(|(k, _)| {
            (k.len() >= MIN_FUZZY && name.ends_with(&format!(" {k}")))
                || (name.len() >= MIN_FUZZY && k.ends_with(&format!(" {name}")))
        })
        .map(|(_, &id)| id)
        .collect();
    unique(&mut tail)
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

/// Read the tilesets from an `ALLHTDS` file (empty if absent) and prefix each with the shared
/// sprites, so a raw tile value indexes the result directly (`0..10` = sprite, `10+` = tile).
fn read_banks(path: &Path, sprites: &[RgbImage]) -> Result<Vec<Vec<RgbImage>>> {
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let tilesets = read_tilesets(path)?;
    Ok(tilesets
        .iter()
        .map(|t| sprites.iter().chain(t).cloned().collect())
        .collect())
}

/// Decode the shared sprites (`ic0_9.wlf`): [`SPRITE_COUNT`] planar-EGA 16×16 tiles. Padded to
/// [`SPRITE_COUNT`] if the file is short so tile values keep their `+10` offset.
fn read_sprites(path: &Path) -> Result<Vec<RgbImage>> {
    let data = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    let mut sprites: Vec<RgbImage> = data
        .chunks_exact(TILE_BYTES)
        .take(SPRITE_COUNT)
        .map(decode_sprite)
        .collect();
    sprites.resize_with(SPRITE_COUNT, || RgbImage::new(TILE_SIZE, TILE_SIZE));
    Ok(sprites)
}

/// Decode one 128-byte **planar** EGA sprite: four bit-planes (`bit` 0..3), each `height` rows of
/// `width / 8` bytes, MSB = leftmost pixel. (Unlike tileset tiles, sprites are planar and are not
/// vertical-XOR encoded.)
fn decode_sprite(bytes: &[u8]) -> RgbImage {
    let mut idx = [[0u8; TILE_SIZE as usize]; TILE_SIZE as usize];
    let mut pos = 0;
    for bit in 0..4 {
        for row in idx.iter_mut() {
            for byte in 0..(TILE_SIZE as usize / 8) {
                let b = bytes[pos];
                pos += 1;
                for p in 0..8 {
                    if (b >> (7 - p)) & 1 != 0 {
                        row[byte * 8 + p] |= 1 << bit;
                    }
                }
            }
        }
    }
    let mut img = RgbImage::new(TILE_SIZE, TILE_SIZE);
    for (y, row) in idx.iter().enumerate() {
        for (x, &c) in row.iter().enumerate() {
            img.put_pixel(x as u32, y as u32, EGA_PALETTE[usize::from(c)]);
        }
    }
    img
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
    fn decode_sprite_reads_planar_planes() {
        // Planar layout: plane 0 first (32 bytes), then planes 1..3. Byte 0 = row 0, cols 0-7,
        // MSB = leftmost pixel. Setting only plane 0's bit for cols 0 and 2 => colour index 1.
        let mut bytes = vec![0u8; TILE_BYTES];
        bytes[0] = 0b1010_0000;
        let img = decode_sprite(&bytes);
        assert_eq!(*img.get_pixel(0, 0), EGA_PALETTE[1]);
        assert_eq!(*img.get_pixel(1, 0), EGA_PALETTE[0]);
        assert_eq!(*img.get_pixel(2, 0), EGA_PALETTE[1]);
    }

    #[test]
    fn strbits_reads_five_bits_lsb_first() {
        // 0x15 = 0b10101; read5 accumulates bits LSB-first, reproducing the low five bits.
        let data = [0x15];
        let mut bits = StrBits::new(&data, 0);
        assert_eq!(bits.read5(), 0x15 & 0x1F);
    }

    #[test]
    fn map_name_extracts_place_names() {
        let s = |t: &str| t.to_owned();
        assert_eq!(
            map_name(&[s("\rLeaving Highpool.\r")]).as_deref(),
            Some("Highpool")
        );
        assert_eq!(
            map_name(&[s("\rWelcome to Hobo Dogs.\r")]).as_deref(),
            Some("Hobo Dogs")
        );
        // Generic fragments are rejected; scanning continues to the next candidate.
        assert_eq!(
            map_name(&[s("\rLeaving the room.\r"), s("\rLeaving Needles.\r")]).as_deref(),
            Some("Needles")
        );
        assert_eq!(map_name(&[s("nothing to see here")]), None);
    }

    #[test]
    fn location_name_cleans_transition_messages() {
        let s = |t: &str| t.to_owned();
        assert_eq!(
            location_name(Some(&s("Entering Highpool."))).as_deref(),
            Some("Highpool")
        );
        assert_eq!(
            location_name(Some(&s("Agricultural Center"))).as_deref(),
            Some("Agricultural Center")
        );
        // "Leaving X" names the exit, not the destination, so it's rejected as a link target.
        assert_eq!(location_name(Some(&s("Leaving Highpool."))), None);
        // Generic fragments (a hole through the wall, …) aren't locations.
        assert_eq!(location_name(Some(&s("a hole in the wall"))), None);
        assert_eq!(location_name(None), None);
    }

    #[test]
    fn match_map_id_links_block_names_to_ids() {
        let table = HashMap::from([
            ("highpool".to_owned(), 10u32),
            ("desert nomads".to_owned(), 8),
            ("stage coach inn".to_owned(), 3),
            ("las vegas".to_owned(), 12),
        ]);
        // Exact (case-insensitive) match.
        assert_eq!(match_map_id(&table, "Highpool"), Some(10));
        // Trailing-word match: a block named "Nomads" links to "Desert Nomads".
        assert_eq!(match_map_id(&table, "Nomads"), Some(8));
        // Space-insensitive: "Stagecoach Inn" links to "Stage Coach Inn".
        assert_eq!(match_map_id(&table, "Stagecoach Inn"), Some(3));
        // No spurious match on a shared word.
        assert_eq!(match_map_id(&table, "the Savage Village"), None);
        // Ambiguous trailing-word match (two ids qualify) resolves to nothing, not a guess.
        let ambiguous = HashMap::from([
            ("going downtown".to_owned(), 32u32),
            ("old downtown".to_owned(), 33u32),
        ]);
        assert_eq!(match_map_id(&ambiguous, "Downtown"), None);
    }

    #[test]
    fn party_position_reads_savegame_block() {
        // Build a savegame body: party order 1..4, then X=7, Y=1, map=10 at 0x08..0x0B, and a
        // return position (overworld map 0, tile 47,59) at 0x0B..0x0E.
        let mut body = vec![0u8; 0x1200];
        body[1..5].copy_from_slice(&[1, 2, 3, 4]);
        body[0x08] = 7;
        body[0x09] = 1;
        body[0x0A] = 10;
        body[0x0B] = 47;
        body[0x0C] = 59;
        body[0x0D] = 0;

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

        let pos = party_position(&data).expect("savegame block");
        assert_eq!((pos.map, pos.x, pos.y), (10, 7, 1));
        assert_eq!((pos.return_map, pos.return_x, pos.return_y), (0, 47, 59));
    }
}
