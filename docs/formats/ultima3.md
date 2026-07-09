# Ultima III — Save Format (`ROSTER.ULT`, `PARTY.ULT`)

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

## Related files (not save-editing targets)

`SOSARIA.ULT` (`0x1228` = 4648 bytes) holds world/map state (64×64 map, moon phases,
whirlpool position). It changes as you walk, but the party's position lives in `PARTY.ULT`,
not here.

## References

- [Ultima III internal formats](https://wiki.ultimacodex.com/wiki/Ultima_III_internal_formats)
  — Codex of Ultima Wisdom wiki (content under CC BY-SA 3.0). Our record layout matches this
  page and was independently validated in-game.
- Original article by "nodling":
  [u3tech_cga.txt (Web Archive)](http://web.archive.org/web/20021024021405/http://www.geocities.com/nodling/text/u3tech_cga.txt).
- [Decoding of the roster file format](http://martin.brenner.de/ultima/u3roster.html) —
  Martin Brenner.
