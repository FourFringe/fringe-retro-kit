# Wasteland — Save Format (`GAME1`)

Unlike the Ultima games, Wasteland's save is a **directory of files** and the mutable data
is **encrypted**. `fringe-retro` can now inspect and edit the character sheets in `GAME1`.

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
| `0x80` | 60 | Skills (30 × id/level) — *not yet exposed* |
| `0xBD` | var | Item list — *not yet exposed* |

## Strings

Character **names** and **ranks** are plain null-terminated ASCII, so they read and edit
directly. Map/dialog text elsewhere in `GAME1` uses a 5-bit character-table encoding (a
glyph table appears near the start of each map block); decoding that is only needed for the
map/story content, which this tool does not edit.

## Still to map

- Per-character **skills** (`0x80`) and **item list** (`0xBD`) — layouts are known from
  `wlandsuite` but not yet surfaced as editable fields.
- The `0x038`–`0x100` party/state region (only partially labelled).
- The 5-bit string decoder (only needed for map/story text).

## References

- [kayahr/wlandsuite](https://github.com/kayahr/wlandsuite) — Klaus Reimer's Wasteland
  Suite (Java, MIT-licensed). Source of the MSQ cipher and block structure; see
  `RotatingXorInputStream` / `RotatingXorOutputStream`.
- [kayahr/wastelib](https://github.com/kayahr/wastelib) — Reimer's newer TypeScript
  Wasteland library.
