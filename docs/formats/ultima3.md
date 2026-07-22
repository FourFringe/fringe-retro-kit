# Ultima III — File Formats

## Save Format (`ROSTER.ULT`, `PARTY.ULT`)

Ultima III stores characters as an array of fixed **64-byte (`0x40`) records**. Numeric
values are **binary-coded decimal (BCD)**; multi-byte BCD values are **little-endian**
(low digit-pair first — e.g. `50 01` = "0150" = **150**). Single-character enums are stored
as **ASCII letters**. There is **no checksum**.

> **Provenance:** corroborates the Codex of Ultima Wisdom wiki ("Ultima III internal
> formats"). Validated end-to-end against the GOG release on macOS (built a roster with our
> tool, formed a party in-game, decoded `PARTY.ULT`).

## Files

- **`ROSTER.ULT`** — `0x500` (1280) bytes = **20 character records** of `0x40` each.
  Empty slots are zero-filled.
- **`PARTY.ULT`** — `0x112` (274) bytes = an `0x12`-byte header followed by **4 character
  records** (the active party), each in the *same* 64-byte format as the roster.

The identical 64-byte record shared by both files is the reason our parser factors the
record layout into shared code — the first concrete example driving the engine's
"shared binary structure" requirement.

## `PARTY.ULT` header (`0x00–0x11`)

| Offset | Field | Notes |
| --- | --- | --- |
| `0x00` | Transport | e.g. `0x3F` on foot |
| `0x02` | Location | |
| `0x03–0x06` | Moves | BCD |
| `0x07` | Party size | |
| `0x08` | Party X | |
| `0x09` | Party Y | |
| `0x0A–0x0D` | Party order | **1-based** roster slot numbers of the 4 members |
| `0x12+` | Member records | 4 × `0x40`, in party order |

## Character record (64 bytes, offsets relative to record start)

| Offset | Field | Type | Notes |
| --- | --- | --- | --- |
| `0x00` | Name | ASCIIZ | 10 bytes (`0x00..0x09`) |
| `0x0E` | Marks/Cards | bitfield | bits 0–7: Love, Sol, Moon, Death, Force, Fire, Snake, Kings |
| `0x0F` | Torches | BCD(1) | |
| `0x10` | In party | bool | `0x00` no, `0xFF` yes |
| `0x11` | Status | ASCII letter | G Good, P Poisoned, D Dead, A Ashes |
| `0x12` | Strength | BCD(1) | |
| `0x13` | Dexterity | BCD(1) | |
| `0x14` | Intelligence | BCD(1) | |
| `0x15` | Wisdom | BCD(1) | |
| `0x16` | Race | ASCII letter | H Human, E Elf, D Dwarf, F Fuzzy, B Bobbit |
| `0x17` | Class | ASCII letter | F, C, W, T, P, B, L, I, A, D, R |
| `0x18` | Gender | ASCII letter | M, F, O |
| `0x19` | Magic Points | BCD(1) | |
| `0x1A–0x1B` | Hit Points | BCD(2) LE | |
| `0x1C–0x1D` | Max Hits | BCD(2) LE | |
| `0x1E–0x1F` | Experience | BCD(2) LE | |
| `0x20` | Food (frac) | BCD(1) | sub-morsel fraction |
| `0x21–0x22` | Food | BCD(2) LE | |
| `0x23–0x24` | Gold | BCD(2) LE | |
| `0x25` | Gems | BCD(1) | |
| `0x26` | Keys | BCD(1) | |
| `0x27` | Powders | BCD(1) | |
| `0x28` | Worn armour | byte index | |
| `0x29–0x2F` | Armour owned | BCD(1)×7 | Cloth, Leather, Chain, Plate, +2 Chain, +2 Plate, Exotic |
| `0x30` | Ready weapon | byte index | |
| `0x31–0x3F` | Weapons owned | BCD(1)×15 | Dagger, Mace, Sling, Axe, Bow, Sword, 2H Sword, +2 Axe, +2 Bow, +2 Sword, Gloves, +4 Axe, +4 Bow, +4 Sword, Exotic |

## World Map (`SOSARIA.ULT`)

Sosaria's overworld is the first **4096 bytes** of `SOSARIA.ULT` (`0x1228` = 4648 bytes total;
the rest holds live world state — moon phases, the whirlpool's position — and changes as you
walk). Those 4096 bytes are a **64×64 grid**, one byte per tile, with the tile index in the
**high 6 bits** (`tile = byte >> 2`) — the same packing as Ultima II. The party's position is
read from `PARTY.ULT` (`0x08`/`0x09`), not from here.

| Tile | Meaning |
| --- | --- |
| 0–4 | Terrain (water, grass, brush, forest, mountains) |
| 5 | Dungeon entrance |
| 6 | Town |
| 7 | Castle |
| 33 | Castle/keep wall (the multi-tile castle structures) |

## Town & Castle Maps

Each town and castle is its **own named `.ULT` file** — `BRITISH.ULT`, `YEW.ULT`, `MOON.ULT`,
`LCB.ULT` (Lord British's Castle), `EXODUS.ULT`, and so on — each a 4648-byte file in the
identical 64×64, `byte >> 2` format as the overworld, so their names come straight from the
filenames. Dungeons are their own smaller **2192-byte** files (`FIRE.ULT`, `MINE.ULT`,
`DARDIN.ULT`, `M.ULT`, `P.ULT`, `PERINIAN.ULT`, `TIME.ULT`) — **eight 16×16 tile-grid levels**
(2048 bytes) followed by a per-level name table — and are reconstructed as top-down graph-paper
maps. `CNFLCT_*.ULT` are small combat arenas and aren't exported.

## Tile Graphics (`SHAPES.ULT`)

`SHAPES.ULT` (5120 bytes) holds **80 tiles** of **16×16 CGA 2-bpp** graphics — 64 bytes each,
stored **linearly** (16 rows × 4 bytes, most-significant pixel-pair leftmost) with **no per-tile
header**, so the same decoder as Ultima II's tiles reads it directly. We render them with a
gently muted CGA palette 1 (dimmed cyan/magenta/white over a near-black blue), because the pure
CGA colours are harsh against Sosaria's largely black ocean.

## Overworld Locations (`EXODUS.BIN`)

The game executable `EXODUS.BIN` embeds two parallel resources used to name the map's points of
interest:

- A **filename table** (around `0x14EA`): null-terminated map names in order — `SOSARIA`, then
  `AMBROSIA`, `BRITISH`, `EXODUS`, `LCB`, `MOON`, `YEW`, `MONTOR_E`, `MONTOR_W`, `GREY`, `DAWN`,
  `DEVIL`, `FAWN`, `DEATH`, then the seven dungeons.
- A **coordinate table** (around `0x15E1`): **19** `(x, y)` byte pairs, one per overworld
  entrance, ordered as the **2 castles, then 10 towns, then 7 dungeons** (Ambrosia is reached by
  whirlpool and has no overworld tile). Reading each coordinate's map tile reproduces exactly
  that type breakdown, and known adjacencies pin the names — the castle beside the town of
  Britain is Lord British's, and Montor East/West sit side by side.

This is the same idea as Ultima I's `OUT.EXE` place list. We locate the table by matching the
coordinate run against the map's landmark tiles (so no offset is hard-coded) and use it to place
named town/castle/dungeon markers on the overworld.

## References

- [Ultima III internal formats](https://wiki.ultimacodex.com/wiki/Ultima_III_internal_formats)
  — Codex of Ultima Wisdom wiki (content under CC BY-SA 3.0). Our record layout matches this
  page and was independently validated in-game.
- Original article by "nodling":
  [u3tech_cga.txt (Web Archive)](http://web.archive.org/web/20021024021405/http://www.geocities.com/nodling/text/u3tech_cga.txt).
- [Decoding of the roster file format](http://martin.brenner.de/ultima/u3roster.html) —
  Martin Brenner.
