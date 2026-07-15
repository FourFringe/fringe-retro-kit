# Ultima VI — Save Format (`OBJLIST`)

Ultima VI's save is a **directory** (`SAVEGAME/`) of object files. Most are map-object
blocks (`OBJBLK*`, some LZW-compressed via `LZOBJBLK`), but the party's **character sheets**
live in **`OBJLIST`**, which is **uncompressed** and laid out as flat, fixed arrays — so
editing character stats needs no decompression or object-graph parsing.

`fringe-retro` edits the party members' stats and names in `OBJLIST` (configure `ultima6`
with `save_dir` set to the game folder — the tool finds `SAVEGAME/OBJLIST` — or pass the
`OBJLIST` path directly).

> **Provenance:** the layout is taken from the [Nuvie](https://github.com/nuvie/nuvie)
> reimplementation (`ActorManager`, `Party`, `save/Objlist.h`, `docs/martian/objlist.txt`)
> and verified byte-for-byte against a real `OBJLIST` — the Avatar/Dupre/Shamino/Iolo
> starting party, including a byte-faithful round trip.

## Column arrays indexed by actor number

`OBJLIST` is a fixed 7539-byte (`0x1D73`) file. Per-actor data is stored as **column
arrays** indexed by actor number `n` (0–255), *not* as contiguous per-actor records:

| Offset | Per entry | Field |
| --- | --- | --- |
| `0x000` | 1 | Object flags |
| `0x100` | 3 | Position (x, y, z) |
| `0x400` | 2 | `obj_n` + frame |
| `0x800` | 1 | Status flags (align, asleep, dead, in-party, …) |
| `0x900` | 1 | **Strength** |
| `0xA00` | 1 | **Dexterity** |
| `0xB00` | 1 | **Intelligence** |
| `0xC00` | 2 | **Experience** (LE) |
| `0xE00` | 1 | **Hit points** |
| `0xFF1` | 1 | **Level** |
| `0x13F1` | 1 | **Magic points** |

So actor `n`'s strength is at `0x900 + n`, experience at `0xC00 + n*2`, and so on.

## Party and player-wide data

| Offset | Size | Field |
| --- | --- | --- |
| `0xF00` | 16 × 14 | **Party names** (null-terminated, indexed by *party position*) |
| `0xFE0` | up to 16 | **Party roster** — the actor number of each member |
| `0xFF0` | 1 | **Number in party** |
| `0x1BF9` | 1 | **Karma** |
| `0x1C71` | 1 | **Gender** (0 = male, 1 = female) |

A party member at position `i` therefore has its **name** at `0xF00 + i*14` and its **stats**
via the actor number `roster[i]` (`0xFE0 + i`). The Avatar is actor number 1.

## Editing

Stats/names are plain values written in place (atomic write, preserving unknown bytes) —
there's no compression or checksum on `OBJLIST`, so a re-save with no changes reproduces the
file exactly. Only the party members' `name`, `strength`, `dexterity`, `intelligence`,
`experience`, `hp`, `level`, and `magic`, plus party-wide `karma`/`gender`, are exposed; the
map object blocks, inventory, and spellbook are not yet handled.

## References

- [Nuvie](https://github.com/nuvie/nuvie) — `actors/ActorManager.cpp`, `Party.cpp`,
  `save/Objlist.h`, `docs/martian/objlist.txt`.
