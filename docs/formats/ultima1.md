# Ultima I — File Formats

Byte-level notes for **Ultima I** (the DOS remake): the character **save**, the overworld
**map**, the **tile graphics**, and the in-executable **location/name table**. Validated
against a legally-owned GOG copy on macOS.

## Save File (`PLAYER*.U1`)

A single fixed-layout file of **`0x334` (820) bytes** holding one character. All
multi-byte values are **little-endian unsigned 16-bit** integers. There is **no checksum**
— edited saves load directly.

> **Provenance:** this format is already public. Reference:
> <https://moddingwiki.shikadi.net/wiki/Ultima_I_Save_Game_Format> (reverse-engineered by
> TheAlmightyGuru and Daniel D'Agostino). Our implementation matches it and has been
> validated in-game (GOG on macOS).

### Character

| Offset | Field | Type | Notes |
| --- | --- | --- | --- |
| `0x00` | Name | ASCIIZ | up to 14 chars + null (`0x00..0x0F`) |
| `0x10` | Race | `u16` enum | 0 Human, 1 Elf, 2 Dwarf, 3 Bobbit |
| `0x12` | Class | `u16` enum | 0 Fighter, 1 Cleric, 2 Wizard, 3 Thief |
| `0x14` | Sex | `u16` enum | 0 Male, 1 Female |

### Attributes (`u16`, max 9999)

| Offset | Field |
| --- | --- |
| `0x18` | Strength |
| `0x1A` | Agility |
| `0x1C` | Stamina |
| `0x1E` | Charisma |
| `0x20` | Wisdom |
| `0x22` | Intelligence |

### Status (`u16`, max 9999)

| Offset | Field |
| --- | --- |
| `0x16` | Hits |
| `0x24` | Gold |
| `0x26` | Experience |
| `0x28` | Food |

### Equipped (`u16` enum)

| Offset | Field | Values |
| --- | --- | --- |
| `0x2A` | Ready Weapon | 0 None … 15 Blaster (see weapon list) |
| `0x2C` | Ready Spell | 0 None … 10 Kill |
| `0x2E` | Ready Armour | 0 None, 1 Leather, 2 Chain Mail, 3 Plate Mail, 4 Vacuum Suit, 5 Reflect Suit |
| `0x30` | Transport | 0 Walking, 1 Horse, 2 Cart, 3 Raft, 4 Frigate, 5 Aircar |

### Location (`u16`)

| Offset | Field |
| --- | --- |
| `0x34` | Map X |
| `0x36` | Map Y |
| `0xA8` | Last Signpost |
| `0xAC` | Steps |

### Inventory counts (`u16`, max 9999)

Each item type has its own 16-bit count.

**Gems** — `0x4C` Red, `0x4E` Green, `0x50` Blue, `0x52` White.

**Armour** — `0x56` Leather, `0x58` Chain Mail, `0x5A` Plate Mail, `0x5C` Vacuum Suit,
`0x5E` Reflect Suit.

**Weapons** — `0x62` Dagger, `0x64` Mace, `0x66` Axe, `0x68` Rope & Spikes, `0x6A` Sword,
`0x6C` Great Sword, `0x6E` Bow & Arrows, `0x70` Amulet, `0x72` Wand, `0x74` Staff,
`0x76` Triangle, `0x78` Pistol, `0x7A` Light Sword, `0x7C` Phazor, `0x7E` Blaster.

**Spells** — `0x82` Open, `0x84` Unlock, `0x86` Magic Missile, `0x88` Steal,
`0x8A` Ladder Down, `0x8C` Ladder Up, `0x8E` Blink, `0x90` Create, `0x92` Destroy,
`0x94` Kill.

**Transports** — `0x98` Horse, `0x9A` Cart, `0x9C` Raft, `0x9E` Frigate, `0xA0` Aircar,
`0xA2` Shuttle, `0xA4` Time Machine.

## World Map (`MAP.BIN`)

The overworld is a single **168 × 156** tile grid stored in `MAP.BIN` (13,104 bytes),
row-major. It is **nibble-packed — two tiles per byte, 84 bytes per row**: the **high**
nibble is the left tile, the **low** nibble is the right tile.

Each nibble is a **direct index** into the tile set (`EGATILES.BIN`). Only values `0–7`
appear on the overworld:

| Index | Tile | Index | Tile |
| :-: | --- | :-: | --- |
| 0 | Water | 4 | Castle |
| 1 | Grasslands | 5 | Monument / signpost |
| 2 | Woods | 6 | Town |
| 3 | Mountains | 7 | Dungeon |

There is no separate object layer — towns, castles, monuments, and dungeons are simply
their own tiles placed on the grid. The map is the four lands of Sosaria in a 2×2
arrangement.

## Tile Graphics (`EGATILES.BIN`)

`EGATILES.BIN` (6,656 bytes) holds **52 tiles of 16×16 pixels** in the standard 16-colour
**EGA** palette, **128 bytes per tile**. Each tile is stored as **four bit-planes, row
interleaved**: every pixel row is `plane0 plane1 plane2 plane3`, two bytes (16 pixels,
MSB = leftmost) per plane. A pixel's colour index is `p0 | p1<<1 | p2<<2 | p3<<3`.

Parallel tile sets exist for other display modes: `CGATILES.BIN` (3,328 = 52×64 bytes,
4-colour CGA, 2 bits/pixel) and `T1KTILES.BIN` (Tandy 1000). Town tiles live in
`EGATOWN.BIN` / `CGATOWN.BIN`; `CASTLE.16` / `CASTLE.4` are full-screen EGA / CGA pictures.

## Named Locations (`OUT.EXE`)

The names of the overworld's towns, castles, monuments, and dungeons live in the game
executable `OUT.EXE`, as a table of **84 entries**:

- **Names** — null-terminated ASCII strings, contiguous, beginning
  `Moon` `Fawn` `Paws` `Montor` … (anchor the block on that sequence).
- **Coordinates** — two parallel **84-byte arrays**: all X bytes, then all Y bytes (tile
  coordinates in the 168×156 grid).
- **Order** — 31 towns, then 8 castles, then 8 monuments, then 37 dungeons.

The coordinate arrays can be located automatically by scanning for a pair of 84-byte runs
whose `(x, y)` pairs land on landmark tiles (indices 4–7) in `MAP.BIN`. Example entries:
*The Castle of Lord British* `(40, 38)`, *Moon* `(66, 41)`, *The Dungeon of Perinia*
`(18, 13)`.

## References

- [Ultima I Save Game Format](https://moddingwiki.shikadi.net/wiki/Ultima_I_Save_Game_Format)
  — DOS Game Modding Wiki, reverse-engineered by TheAlmightyGuru and Daniel D'Agostino. Our
  save implementation follows this spec.
- [Ultima I Tile Graphic Format](https://moddingwiki.shikadi.net/wiki/Ultima_I_Tile_Graphic_Format)
  — DOS Game Modding Wiki; the `*TILES.BIN` / `*TOWN.BIN` graphics formats.
- [Dino's Guide to Ultima I — File Formats](https://gigi.nullneuron.net/ultima/u1/formats.php)
  — map and graphics file notes.
- [BehindTimes/UltimaTileEditor](https://github.com/BehindTimes/UltimaTileEditor)
  — extracts / inserts the tile sets for Ultima 1–5 (PC).
