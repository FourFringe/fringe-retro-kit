# Ultima V Save Format (`SAVED.GAM`)

Ultima V: Warriors of Destiny stores the whole game in a single **4192-byte** file,
`SAVED.GAM`, in the game directory. `INIT.GAM` is the pristine new-game template of the same
layout (the game copies it to `SAVED.GAM` when you start a new game). A companion file,
`SAVED.OOL` (512 bytes), holds the dynamic object list and is **not** needed to read or edit
characters or party state.

`SAVED.GAM` is a snapshot of the game's working RAM. Only the first `0x1060` (4192) bytes are
written to disk; everything past that is RAM-only scratch. Numbers are **plain little-endian
binary integers** (`u16` where noted, otherwise single bytes).

This layout follows the [Ultima Codex](https://wiki.ultimacodex.com/wiki/Ultima_V_internal_formats)
"Ultima V internal formats" page (the *SAVED.GAM and RAM* section) and was verified
byte-for-byte against a real save (the Avatar plus two companions).

## File layout

| Offset | Size | Field |
| --- | --- | --- |
| `0x000` | 2 | *unknown* header |
| `0x002` | 16 × 32 | sixteen **character records** (see below), 32 (`0x20`) bytes each |
| `0x202` | — | **party / game state** (see below) |

A slot is empty when its class byte (`0x0A` within the record) is `0`; a real record always
carries a class letter. The name alone is not reliable, because the Avatar's name can be
blank early in a game.

## Character record (32 bytes)

Offsets are relative to the start of each record (`0x002 + index × 0x20`).

| Offset | Size | Field |
| --- | --- | --- |
| `0x00` | 9 | Name (ASCII, null-terminated; 8 chars + terminator) |
| `0x09` | 1 | Sex (enum) |
| `0x0A` | 1 | Class (ASCII letter) |
| `0x0B` | 1 | Status (ASCII letter) |
| `0x0C` | 1 | Strength (1–30) |
| `0x0D` | 1 | Dexterity (1–30) |
| `0x0E` | 1 | Intelligence (1–30) |
| `0x0F` | 1 | Magic points (0–30) |
| `0x10` | 2 | Hit points (`u16`, 1–240) |
| `0x12` | 2 | Max hit points (`u16`) |
| `0x14` | 2 | Experience (`u16`, 0–9999) |
| `0x16` | 1 | Level (1–8) |
| `0x17` | 1 | Months at inn (0–25) |
| `0x18` | 1 | *unknown* (always `7`, even in `INIT.GAM`) |
| `0x19` | 1 | Helmet (item index, `0`–`0x2F`, `0xFF` = none) |
| `0x1A` | 1 | Armor (item index) |
| `0x1B` | 1 | Weapon / shield, left hand (item index) |
| `0x1C` | 1 | Weapon / shield, right hand (item index) |
| `0x1D` | 1 | Ring (item index) |
| `0x1E` | 1 | Amulet (item index) |
| `0x1F` | 1 | Inn / party: `0` = in party, `0xFF` = not joined, `0x7F` = permanently killed, else = inn settlement number |

### Enumerations

**Sex** (`0x09`): `0x0B` Male · `0x0C` Female

**Class** (`0x0A`, ASCII): `A` Avatar · `B` Bard · `F` Fighter · `M` Mage

**Status** (`0x0B`, ASCII): `G` Good · `P` Poisoned · `C` Charmed · `S` Asleep · `D` Dead

Equipment bytes (`0x19`–`0x1E`) are indices into the game's shared item table (the same
numbering used by the inventory counts at `0x21A`+). Fringe Retro Kit currently exposes them
as raw indices.

## Party / game state

Absolute file offsets. Counts marked 0–99 are single bytes.

| Offset | Size | Field |
| --- | --- | --- |
| `0x202` | 2 | Food (`u16`, 0–9999) |
| `0x204` | 2 | Gold (`u16`, 0–9999) |
| `0x206` | 1 | Keys |
| `0x207` | 1 | Gems |
| `0x208` | 1 | Torches |
| `0x209` | 1 | Grapple (`0` / `0xFF`) |
| `0x20A` | 1 | Magic carpets |
| `0x20B` | 1 | Skull keys |
| `0x216` | 1 | Sextants |
| `0x21A`–`0x243` | 1 each | Armor & weapon inventory counts (shared item table) |
| `0x244`–`0x289` | 1 each | Rings, amulets, mixed spells, scrolls, potions |
| `0x2AA`–`0x2B1` | 8 | Reagents: Sulfurous Ash, Ginseng, Garlic, Spider Silk, Blood Moss, Black Pearl, Nightshade, Mandrake Root |
| `0x2B5` | 1 | Number of party members (1–6) |
| `0x2CE` | 2 | Current year (`u16`) |
| `0x2D5` | 1 | Active character (`0`–`5`, `0xFF` = none) |
| `0x2D6` | 1 | Mode of transport |
| `0x2D7` | 1 | Current month (1–13) |
| `0x2D8` | 1 | Current day (1–28) |
| `0x2D9` | 1 | Current hour (0–23) |
| `0x2DB` | 1 | Current minute (0–59) |
| `0x2E2` | 1 | Karma (0–255) |
| `0x2ED` | 1 | Current party location (see the Codex "Party Location" table) |
| `0x2EF` | 1 | Party Z-coordinate (`0xFF` = Underworld, `0`–`7` = dungeon level / floor) |
| `0x2F0` | 1 | Party X-coordinate |
| `0x2F1` | 1 | Party Y-coordinate |

Fringe Retro Kit exposes the food, gold, party-member count, the loose inventory items
(keys, gems, torches, magic carpets, skull keys, sextants), the eight reagents, the date and
time, karma, and the party location/coordinates. The many item-count bytes in
`0x21A`–`0x289` are documented above but not individually surfaced yet.

## Provenance

Verified against a real GOG `SAVED.GAM`: the Avatar (class `A`, Good, str/dex/int 15,
HP 60/60), the "always 7" byte at record offset `0x18`, and party state (food 63, gold 150,
3 party members) all decoded exactly as documented. The full RAM/save table (dungeon maps,
NPC schedules, monster tables, and other RAM-only regions beyond `0x1060`) is on the Codex
page but is out of scope for character/party editing.
