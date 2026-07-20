# Wasteland — Save & Map Formats

Unlike the Ultima games, Wasteland's save is a **directory of files** and the mutable data
is **encrypted**. `fringe-retro` can inspect and edit the character sheets in `GAME1`, and
`fringe-retro-map` renders every map (see [Map rendering](#map-rendering-master1master2--allhtds)).

> **Provenance:** the MSQ cipher and block/record structure are taken from Klaus Reimer's
> `wlandsuite` (the definitive open-source Wasteland file library) and verified against a
> real Steam ("The Original Classic") save on macOS — including a byte-for-byte round trip.
> One correction to `wlandsuite` was needed for that fidelity (see the checksum note below).

## A save is a directory

A save slot lives in `<save root>/<SLOTNAME>/` (the active slot name is stored in the
`LASTSAVE` file). Key files:

| File | Role |
| --- | --- |
| `GAME1` | **Mutable** saved game — party, characters, world state (encrypted MSQ blocks) |
| `MASTER1` | Pristine original of `GAME1` (same size; useful as a diff baseline) |
| `GAME2` / `MASTER2` | Static game data (identical to each other in a fresh save) |
| `LASTSAVE` | Active slot name (ASCII + CRLF) |
| `INFO` | 2 bytes (e.g. `43 00`) — difficulty/version (unconfirmed) |
| `ALLPICS*`, `ALLHTDS*`, `*.FNT`, `TITLE.PIC`, … | Assets |

This is the first format that forces the engine to model a save as **"a directory with
per-file roles"** rather than a single file.

## MSQ blocks

`GAME1` (and `GAME2`) are a concatenation of **MSQ blocks**. Each block begins with a
4-byte header `msq` + a disk digit (`msq0` for `GAME1`, `msq1` for `GAME2`), followed by
two **seed** bytes, then the encrypted body:

```
+0  "msq" + disk digit   (4 bytes)
+4  seed0, seed1         (2 bytes)
+6  ciphertext ...       (rest of block)
```

Blocks are located by scanning for the `msq`+digit boundary. A `GAME1` holds ~24 blocks;
most are visited maps. The **savegame** block is the one whose size is exactly **4614
bytes** and whose first decrypted bytes are a valid party order (bytes 1..7 each in `0..=7`,
non-zero values unique). It is *not* the first block — you must scan for it.

## The cipher — rotating XOR

Decryption of a block's body (from offset `+6`):

```
key = seed0 XOR seed1
for each ciphertext byte c:
    plain = c XOR key
    key   = (key + 0x1F) & 0xFF
```

The two seed bytes also encode a **checksum**. Bytes of the plaintext body are summed into
a 16-bit accumulator; on each 16-bit overflow the carry is folded back as `+0x100` (an
artifact of the original game's byte-wise add-with-carry). The seed is the two's-complement
negation of that sum, stored little-endian, and the initial key is `seed0 XOR seed1`.
Editing therefore requires **recomputing and rewriting the seed/checksum**.

> **Note — `wlandsuite` divergence:** `wlandsuite` stores a plain negated sum (no carry
> fold), so its rewritten seeds differ from the game's. Its *reads* still work (the seed→key
> relation is unchanged) and the game tolerates it, but it is not byte-faithful. The carry
> fold above reproduces the original game's saves exactly (validated against every
> uncompressed block — the savegame and all shop-list blocks — in a real `GAME1`).

**Verified:** in a real `GAME1`, seed `bf f0` gives `key = 0x4F` and decrypts the leading
body bytes to a run of `0xBB`. See `crates/core/src/games/wasteland.rs` for the
implementation and test vectors.

## Savegame block layout

The decrypted savegame body is `0x1200` (4608) bytes:

| Offset | Size | Field |
| --- | --- | --- |
| `0x000` | `0x38` | Parties (member order + positions) |
| `0x038` | 200 | Assorted state (viewport, current party/map, time, serial, …) |
| `0x100` | `0x100`×7 | **Seven character records** (256 bytes each) |
| `0x800` | 2560 | Padding |

### Character record (256 bytes)

All little-endian; `int3` = 3-byte integer. Names are null-terminated ASCII (not the 5-bit
map-string encoding).

| Offset | Size | Field |
| --- | --- | --- |
| `0x00` | 14 | Name (null-terminated) |
| `0x0E`–`0x14` | 1 each | Strength, IQ, Luck, Speed, Agility, Dexterity, Charisma |
| `0x15` | int3 | Money |
| `0x18` | 1 | Gender (0 male, 1 female) |
| `0x19` | 1 | Nationality (0 US … 4 Chinese) |
| `0x1A` | 1 | Armor class |
| `0x1B` | 2 | Max CON |
| `0x1D` | 2 | CON (current HP) |
| `0x20` | 1 | Skill points |
| `0x21` | int3 | Experience |
| `0x24` | 1 | Level |
| `0x26` | 2 | Last CON |
| `0x28` | 1 | Afflictions (bitmap) |
| `0x29` | 1 | NPC flag |
| `0x32` | 25 | Rank (null-terminated ASCII) |
| `0x80` | 60 | Skills (30 × id/level) |
| `0xBD` | var | Item list — *not yet exposed* |

### Skills (`0x80`)

Thirty `(id, level)` slots, packed contiguously from the start; an id of `0` marks an empty
slot. The id indexes the game's skill list:

| id | Skill | id | Skill | id | Skill |
| --- | --- | --- | --- | --- | --- |
| 1 | Brawling | 13 | Acrobat | 25 | Medic |
| 2 | Climb | 14 | Gamble | 26 | Safecrack |
| 3 | Clip Pistol | 15 | Picklock | 27 | Cryptology |
| 4 | Knife Fight | 16 | Silent Move | 28 | Metallurgy |
| 5 | Pugilism | 17 | Combat Shooting | 29 | Helicopter Piloting |
| 6 | Rifle | 18 | Confidence | 30 | Electronics |
| 7 | Swim | 19 | Sleight of Hand | 31 | Toaster Repair |
| 8 | Knife Throw | 20 | Demolitions | 32 | Doctor |
| 9 | Perception | 21 | Forgery | 33 | Clone Tech |
| 10 | Assault Rifle | 22 | Alarm Disarm | 34 | Energy Weapon |
| 11 | AT Weapon | 23 | Bureaucracy | 35 | Cyborg Tech |
| 12 | SMG | 24 | Bomb Disarm | | |

Validated against a real save: all four starting Rangers share ids 3/7/9 (Clip Pistol / Swim
/ Perception), and Angela Deth carries Demolitions, Alarm Disarm, Picklock, Safecrack, and
Medic — matching her documented loadout.


## Strings

Character **names** and **ranks** are plain null-terminated ASCII, so they read and edit
directly. Map/dialog text elsewhere in `GAME1` uses a 5-bit character-table encoding (a
glyph table appears near the start of each map block); decoding that is only needed for the
map/story content, which this tool does not edit.

## Still to map

- The per-character **item list** (`0xBD`) — layout is known from `wlandsuite` but not yet
  surfaced (variable-length records + an item-type table).
- The `0x038`–`0x100` party/state region (only partially labelled).
- The 5-bit string decoder (only needed for map/story text).

## Map rendering (`MASTER1`/`MASTER2` + `ALLHTDS*`)

`fringe-retro-map` renders every Wasteland map. Maps are read from the pristine `MASTER1`
(disk 1, incl. the 64×64 desert overworld) and `MASTER2` files — `GAME1`/`GAME2` are used
only as a fallback, because their block 0 holds the savegame rather than the overworld.
Tiles come from `ALLHTDS1`/`ALLHTDS2`. See `crates/map/src/wasteland.rs` and
`crates/map/src/huffman.rs`.

### Map block

Each MSQ map block uses the same rolling-XOR cipher as the save, but encryption **stops at
the strings** — the tail (strings + tile map) is stored plain. The decrypted body is:

```
size²/2   action-class nibble map
size²     action map
44        central directory
1         size byte (32 or 64)
...       Info block (see below), then strings
tail      Huffman-compressed tile map (plain)
```

1. **Map size** (`size` = 32 or 64) is found where a size byte and two zero bytes sit at a
   fixed offset (`size²·3/2 + 44`) past the action maps.
2. **Encrypted length** — a `u16` LE at `size²·3/2` marks where the XOR cipher stops; decrypt
   exactly that many bytes and take the rest of the block verbatim.
3. **Info block** (`size²·3/2 + 45`): byte 3 = **tileset index** (0–3), byte 6 = **background
   tile** (used to backfill the ~2–3 % of tiles that reference shared graphics outside the
   local tileset).
4. **Tile map** is a Huffman stream at the block tail (a `u32` LE uncompressed size = `size²`,
   a `u32` LE unknown, then the bitstream), decoding to `size²` tile **values**.

### Tile values → graphics

A map square's decoded value is **not** a direct tileset index. Values `0–9` are the ten
shared **sprites** in `IC0_9.WLF`; values `10+` index the map's tileset as `value − 10`.
(Getting this wrong shifts every tile by ten and can land on an entirely different-looking
tileset region — e.g. Highpool rendering as water instead of grass.)

### Sprites (`IC0_9.WLF`)

Ten 16×16 sprites, 128 bytes each (1280 total), in **planar** 4-bit EGA: four bit-planes,
each `height` rows of `width/8` bytes, MSB = leftmost pixel. (Planar — unlike the *chunky*
tileset tiles below — and not vertical-XOR encoded.)

### Huffman bitstream

MSB-first. The tree is serialized inline: a `0` bit is an internal node (read left subtree,
skip one separator bit, read right subtree); a `1` bit is a leaf followed by an 8-bit byte.
Decoding walks from the root (`0` = left, `1` = right) until a leaf.

### Tilesets (`ALLHTDS*`)

A sequence of compressed-MSQ blocks: `[size:u32 LE]["msq" + raw-disk byte][Huffman]`. Each
decompresses to `size / 128` tiles. `ALLHTDS1` holds four tilesets of 66, 141, 163 and 107
tiles. The `Info` tileset byte selects one: `< 4` → `ALLHTDS1[id]`, else `ALLHTDS2[id − 4]`.

### Tile pixels

Each 16×16 tile is 128 bytes. First undo a **vertical XOR** (each 8-byte row XORed with the
row above: `b[i] ^= b[i-8]` for `i` in `8..128`), then read **chunky 4-bit EGA** — two pixels
per byte, high nibble = left pixel — through the standard 16-colour EGA palette. (Unlike the
Ultima tiles and the sprites above, this is *chunky*, not planar.)

## References

- [kayahr/wlandsuite](https://github.com/kayahr/wlandsuite) — Klaus Reimer's Wasteland
  Suite (Java, MIT-licensed). Source of the MSQ cipher and block structure; see
  `RotatingXorInputStream` / `RotatingXorOutputStream`. The map/tileset codecs (`GameMap`,
  `TileMap`, `Htds`, `Sprite`, `HuffmanInputStream`) and the `MapSquare` tile-value rule
  drove the renderer above.
- [kayahr/wastelib](https://github.com/kayahr/wastelib) — Reimer's newer TypeScript
  Wasteland library.
