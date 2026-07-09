# Wasteland — Save Format (`GAME1`)

**Work in progress.** Unlike the Ultima games, Wasteland's save is a **directory of
files** and the mutable data is **encrypted**. This page documents the parts we've
solved (the container and the MSQ cipher) and marks the rest as in-progress.

> **Provenance:** the MSQ cipher and block structure are taken from Klaus Reimer's
> `wlandsuite` (the definitive open-source Wasteland file library) and verified against a
> real Steam ("The Original Classic") save on macOS. The savegame record layout has not
> yet been mapped in this project.

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

Blocks are located by scanning for the `msq`+digit boundary. The **first** block in
`GAME1` is the save game (party + characters); the remaining blocks are visited maps.

## The cipher — rotating XOR

Decryption of a block's body (from offset `+6`):

```
key = seed0 XOR seed1
for each ciphertext byte c:
    plain = c XOR key
    key   = (key + 0x1F) & 0xFF
```

The two seed bytes also encode a **checksum** used to verify writes: on save the game
computes `checksum = (checksum - plain) & 0xFFFF` accumulated over the plaintext, stores
it little-endian as `seed0,seed1`, and derives the initial key as `seed0 XOR seed1`.
Editing therefore requires **recomputing and rewriting the seed/checksum** — unlike the
Ultima games, you cannot just poke a byte in place.

**Verified:** in a real `GAME1`, seed `bf f0` gives `key = 0x4F` and decrypts the leading
body bytes to a run of `0xBB` (the save's initial fill). See
`crates/core/src/games/wasteland.rs` for the implementation and test vector.

## Strings

Wasteland does **not** store map/character text as plain ASCII; it uses a 5-bit
character-table encoding (a glyph table appears near the start of the decrypted save
block). Decoding this is required before names become readable — another reason the
"field schema over plaintext" layer needs pluggable **string codecs**.

## Still to map

- The savegame record layout (party lists + per-character sheets: name, STR/IQ/LCK/SPD/AGL/
  DEX/CHA, skills, CON/max-CON, inventory). Reference: `wlandsuite`'s `Char`/`Savegame`
  classes, and/or diffing the decrypted `GAME1` before/after known in-game changes.
- The 5-bit string decoder.
- The write path (re-encrypt + checksum) needed for editing.

## References

- [kayahr/wlandsuite](https://github.com/kayahr/wlandsuite) — Klaus Reimer's Wasteland
  Suite (Java, MIT-licensed). Source of the MSQ cipher and block structure; see
  `RotatingXorInputStream` / `RotatingXorOutputStream`.
- [kayahr/wastelib](https://github.com/kayahr/wastelib) — Reimer's newer TypeScript
  Wasteland library.
