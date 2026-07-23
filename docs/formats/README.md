# Game File Formats

Byte-level documentation of the file formats Fringe Retro Kit understands — character
**saves**, world **maps**, and **tile graphics**. These are written for anyone
reverse-engineering or building tools for these games, not just for this project.

Some of this is **original research**. In particular, the [Ultima II](ultima2.md) player
save format does not appear to be documented anywhere else; we mapped it byte-by-byte by
diffing live saves (see that page's provenance notes). The **map and graphics** formats for
Ultima I and II are documented on their per-game pages, with credit to the community tools
we leaned on (see [References & credits](#references--credits)).

## Conventions

- Offsets are hexadecimal and **relative to the start of the record or file** noted in
  each section.
- **BCD** = binary-coded decimal: each nibble is a decimal digit, so the byte `0x42`
  means decimal **42**, not 66. Multi-byte BCD values note their byte order.
- "LE" / "BE" = little-endian / big-endian.
- Bytes marked *volatile* change on every save (RNG / turn state) and should be preserved,
  not interpreted.
- Bytes not listed are either always zero in our samples or not yet identified; a good
  editor **preserves unknown bytes** rather than rewriting them.

## Save formats

| Game | File(s) | Encoding | Container | Notes |
| --- | --- | --- | --- | --- |
| [Ultima I](ultima1.md) | `PLAYER*.U1` | LE `u16` | plain file | Documented elsewhere; included for completeness |
| [Ultima II](ultima2.md) | `PLAYER` | BCD (big-endian) | plain file | **Original research** |
| [Ultima III](ultima3.md) | `ROSTER.ULT`, `PARTY.ULT` | BCD (little-endian) | plain file | Corroborates the Codex of Ultima Wisdom wiki |
| [Ultima IV](ultima4.md) | `PARTY.SAV` | LE binary (`u16`/`u32`) | plain file | Matches the `xu4` reimplementation; verified against a real save |
| [Ultima V](ultima5.md) | `SAVED.GAM` | LE binary (`u16`/`u8`) | plain file | Follows the Codex of Ultima Wisdom wiki; verified against a real save |
| [Ultima VI](ultima6.md) | `OBJLIST` (in a save directory) | LE binary | plain arrays | Party stats via the Nuvie layout; verified against a real save (no LZW needed) |
| [Wasteland](wasteland.md) | `GAME1` (in a save directory) | binary | **encrypted MSQ blocks** | Character sheets editable; byte-faithful writes verified against a real save |
| [The Bard's Tale Trilogy](bardstale.md) | `Save1.dat`, `AutoSave.dat`, … | self-describing | **.NET `BinaryFormatter` (MS-NRBF)** | Stats, class/race/gender, party gold editable; in-place patch verified against a real Steam save |

## Map & graphics formats

World maps and tile graphics we've mapped so far (Ultima I–V, Wasteland):

| Game | Map(s) | Tile graphics | Notes |
| --- | --- | --- | --- |
| [Ultima I](ultima1.md) | `MAP.BIN` — 168×156, nibble-packed | `EGATILES.BIN` — 16×16 EGA, row-interleaved | Place names in `OUT.EXE` (84-entry table) |
| [Ultima II](ultima2.md) | `MAPX##` / `MAPG##` — 41 maps, 64×64, `byte>>2` | in `ULTIMAII.EXE` @ `0x7C40` — 16×16 CGA | Towns are separate maps; dungeons/towers are 16×16×16 tile-grid mazes in the `…5`/`…4` slots |
| [Ultima III](ultima3.md) | `SOSARIA.ULT` + named town/castle `.ULT` — 64×64, `byte>>2` | `SHAPES.ULT` — 80 tiles, 16×16 CGA (linear) | Place names + coordinates in `EXODUS.BIN`; dungeons are `.ULT` tile grids (8×16×16) |
| [Ultima IV](ultima4.md) | `WORLD.MAP` — 256×256, 8×8 chunks of 32×32 + named `.ULT` towns (32×32) | `SHAPES.EGA` — 256 tiles, 16×16 EGA (like Ultima I) | Byte is the tile index; dungeons are `.DNG` tile grids (8×8×8) |
| [Ultima V](ultima5.md) | `BRIT.DAT` + `UNDER.DAT` — two 256×256 worlds, 16×16 chunks; layout table in `DATA.OVL` | `TILES.16` — 512 tiles, 16×16 EGA 4-bit, LZW-compressed | Ocean chunks omitted from `BRIT.DAT`; Underworld is a second world |
| [Wasteland](wasteland.md) | `MASTER1`/`MASTER2` — 42 maps, 32×32 or 64×64, encrypted MSQ + Huffman tile map | `ALLHTDS1`/`ALLHTDS2` — Huffman tilesets, 16×16 EGA 4-bit chunky, vertical-XOR encoded | Partial rolling-XOR cipher; per-map tileset index + background tile in the Info block |

## How these were produced

Each format was validated against real saves from legally-owned copies (GOG / Steam),
using the project's `dump` and `watch` commands to observe byte changes as we performed
known in-game actions. Where a public reference existed (Ultima I, Ultima III) we cite it;
where none existed (Ultima II) we mapped it ourselves and marked confidence levels.

## References & credits

Community tools and wikis that made this work faster — and that we recommend to anyone
exploring these games:

- **[BehindTimes/UltimaTileEditor](https://github.com/BehindTimes/UltimaTileEditor)** —
  extracts and inserts tile sets for Ultima 1–5 (PC); pinned down the Ultima II tile block
  inside `ULTIMAII.EXE` for us.
- **[DocCaliban/ultima-data-parser](https://github.com/DocCaliban/ultima-data-parser)** —
  decodes Ultima III maps and tile sets; documents the family's CGA / EGA conventions.
- **[The Exodus Project](https://exodus.voyd.net/)** — modern Ultima I–III upgrades with
  alternate tile sets.
- **[DOS Game Modding Wiki](https://moddingwiki.shikadi.net/)** — Ultima I save and
  tile-graphic formats, plus general CGA / EGA references.
- **[Dino's Guide to Ultima I](https://gigi.nullneuron.net/ultima/u1/formats.php)** — file
  format notes.
- **The Codex of Ultima Wisdom wiki** — corroborated the Ultima III / V save layouts.

Everything here was validated against **legally-owned** copies (GOG / Steam); we ship no
game data — only the knowledge of how to read it.
