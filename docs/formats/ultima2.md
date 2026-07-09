# Ultima II — Save Format (`PLAYER`)

A single fixed-layout file of **`0x180` (384) bytes** holding one character. Numeric values
are **binary-coded decimal (BCD)** stored **big-endian** (most-significant digit-pair at
the lower offset) — e.g. the two bytes `02 89` decode to **289**. There is **no enforced
load-time checksum**: edited saves load directly (verified by editing HP, Food, and Gold
and reloading in-game).

> **Provenance — original research.** We could not find this format documented anywhere.
> It was mapped byte-by-byte by diffing live saves from the GOG release on macOS, using
> known in-game values and the project's `watch`/`dump` commands. Confidence is noted per
> field. Corrections welcome.

## The "player disk" and blank state

Ultima II descends from the floppy era and treats `PLAYER` as an emulated **player disk**.
A **blank** disk is 384 bytes of `0x00` **except byte `0x100` = `0x1A`**; the game requires
this blank state (or a dead character) before it will create a new character. A living
character on the disk yields the message *"NOT A BLANK PLAYER DISK"*; a missing/renamed
file yields *"WRONG DISK"*.

## Field map

Confidence: **✓** confirmed against known values · **~** strongly inferred.

| Offset | Field | Type | Conf. | Notes |
| --- | --- | --- | :-: | --- |
| `0x00` | Name | ASCIIZ | ✓ | up to 15 chars + null (`0x00..0x0F`) |
| `0x10` | Sex | ASCII letter | ✓ | `M` = Male, `F` = Female |
| `0x11` | Class | byte index | ✓ | 0 Fighter, 1 Cleric, 2 Wizard, 3 Thief |
| `0x12` | Race | byte index | ✓ | 0 Human, 1 Elf, 2 Dwarf, 3 Hobbit |
| `0x13` | *unknown* | byte | | varies by character (e.g. 3 vs 2) |
| `0x15` | Strength | BCD(1) | ✓ | stored **adjusted** (base + race/class/sex bonus) |
| `0x16` | Agility | BCD(1) | ✓ | |
| `0x17` | Stamina | BCD(1) | ✓ | |
| `0x18` | Charisma | BCD(1) | ✓ | |
| `0x19` | Wisdom | BCD(1) | ✓ | |
| `0x1A` | Intelligence | BCD(1) | ✓ | |
| `0x1B–0x1C` | Hits (HP) | BCD(2) BE | ✓ | |
| `0x1D–0x1E` | Food | BCD(2) BE | ✓ | |
| `0x1F` | *volatile* | byte | | RNG / turn state — changes every save |
| `0x20–0x21` | Experience | BCD(2) BE | ✓ | |
| `0x22–0x23` | Gold | BCD(2) BE | ✓ | |
| `0x24` | Map X | byte | ✓ | fresh character starts at (20, 20) |
| `0x25` | Map Y | byte | ✓ | |
| `0x27` | *dynamic* | byte | | turn / tile state; churns constantly |
| `0x2B` | Readied weapon | byte index | ✓ | 0 None, 1 Dagger … 9 Quicksword (see counts) |
| `0x2C` | Worn armour | byte index | ✓ | 0 None, 1 Cloth … 6 Power; gated by Strength |
| `0x39` | *volatile* | byte | | RNG / turn state |
| `0x41–0x49` | Weapons owned | BCD(1)×9 | ✓ | Dagger, Mace, Axe, Bow, Sword, Great sword, Light sword, Phaser, Quicksword |
| `0x61–0x66` | Armour owned | BCD(1)×6 | ✓ | Cloth, Leather, Chain, Plate, Reflect, Power |

Weapon and armour **arrays** are indexed by item number in the game's order; index 0
(`0x40` Hands, `0x60` Skin) is the "nothing" slot. The count encoding is assumed BCD to
match the rest of the file (all observed counts were ≤ 9, where BCD and binary coincide).

## Attribute bonuses (character creation)

Attributes are stored *after* applying creation bonuses:

- **Sex:** Male +5 Strength · Female +10 Charisma
- **Race:** Human +5 Int · Elf +5 Agility · Dwarf +5 Strength · Hobbit +10 Wisdom
- **Class:** Fighter +15 Strength · Cleric +10 Wisdom · Wizard +10 Int · Thief +10 Agility

## Still unmapped

`0x13`, `0x26`, most of `0x28–0x40` and `0x4A–0x60`, and the item categories the game UI
calls **Torches, Keys, Tools,** and **Spells** (all zero in our sample; need a save where
the character owns some). The volatile bytes `0x1F` and `0x39` are RNG/turn state and are
**not** a data checksum (they change on every save even when nothing else does).

## References

The `PLAYER` byte layout on this page is our own reverse engineering — we found no prior
published spec (corrections and pointers to prior art are welcome). The character-creation
attribute bonuses were taken from:

- [Ultima II walkthrough](https://www.wiki.ultimacodex.com/wiki/Ultima_II_walkthrough) —
  Codex of Ultima Wisdom wiki (gender/race/class attribute bonuses).
