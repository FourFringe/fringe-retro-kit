# Ultima IV Save Format (`PARTY.SAV`)

Ultima IV: Quest of the Avatar stores the whole party — the eight character slots plus the
shared party/game state (food, gold, the eight virtues, inventory, location) — in a single
fixed-size **502-byte** file, `PARTY.SAV`, in the game directory. `PARTY.NEW` is the pristine
new-game template of the same layout.

Unlike Ultima II/III (which use BCD), Ultima IV stores numbers as **plain little-endian
binary integers** (`u16`, and one `u32` for food).

This layout follows the [`xu4`](https://github.com/xu4/u4) reimplementation's `SaveGame`
structures and was verified byte-for-byte against a real save (the eight standard
companions).

## File layout

| Offset | Size | Field |
| --- | --- | --- |
| `0x000` | 4 | `unknown1` (always 0 in samples) |
| `0x004` | 4 | `moves` — move counter (`u32` LE) |
| `0x008` | 8 × 39 | eight **player records** (see below), 39 (`0x27`) bytes each |
| `0x140` | 182 | **party / game state** (see below) |

Total: `0x1F6` = 502 bytes.

## Player record (39 bytes)

Offsets are relative to the start of each record (`0x008 + index × 0x27`). All numbers are
little-endian `u16`.

| Offset | Size | Field |
| --- | --- | --- |
| `0x00` | 2 | Hit points |
| `0x02` | 2 | Max hit points |
| `0x04` | 2 | Experience |
| `0x06` | 2 | Strength |
| `0x08` | 2 | Dexterity |
| `0x0A` | 2 | Intelligence |
| `0x0C` | 2 | Magic points |
| `0x0E` | 2 | *unknown* |
| `0x10` | 2 | Weapon (enum) |
| `0x12` | 2 | Armor (enum) |
| `0x14` | 16 | Name (ASCII, null-terminated; padding **not** zeroed) |
| `0x24` | 1 | Sex (enum) |
| `0x25` | 1 | Class (enum) |
| `0x26` | 1 | Status (ASCII letter) |

A slot is empty when its name byte (`0x14`) is `0`.

### Enumerations

**Class** (`0x25`): `0` Mage · `1` Bard · `2` Fighter · `3` Druid · `4` Tinker · `5` Paladin ·
`6` Ranger · `7` Shepherd

**Sex** (`0x24`): `0x0B` Male · `0x0C` Female

**Status** (`0x26`, ASCII): `G` Good · `P` Poisoned · `S` Sleeping · `D` Dead

**Weapon** (`0x10`): `0` Hands · `1` Staff · `2` Dagger · `3` Sling · `4` Mace · `5` Axe ·
`6` Sword · `7` Bow · `8` Crossbow · `9` Flaming Oil · `10` Halberd · `11` Magic Axe ·
`12` Magic Sword · `13` Magic Bow · `14` Magic Wand · `15` Mystic Sword

**Armor** (`0x12`): `0` Skin · `1` Cloth · `2` Leather · `3` Chain Mail · `4` Plate Mail ·
`5` Magic Chain · `6` Magic Plate · `7` Mystic Robe

## Party / game state

Absolute file offsets. All numbers little-endian.

| Offset | Size | Field |
| --- | --- | --- |
| `0x140` | 4 | **Food** (`u32`) — stored **×100** (e.g. `29989` = 299 food) |
| `0x144` | 2 | Gold |
| `0x146` | 16 | **Virtues** (karma) — eight `u16`, 0–99: Honesty, Compassion, Valor, Justice, Sacrifice, Honor, Spirituality, Humility |
| `0x156` | 2 | Torches |
| `0x158` | 2 | Gems |
| `0x15A` | 2 | Keys |
| `0x15C` | 2 | Sextants |
| `0x15E` | 16 | Armor inventory counts — eight `u16` (armor types above) |
| `0x16E` | 32 | Weapon inventory counts — sixteen `u16` (weapon types above) |
| `0x18E` | 16 | **Reagents** — eight `u16`: Sulfurous Ash, Ginseng, Garlic, Spider Silk, Blood Moss, Black Pearl, Nightshade, Mandrake Root |
| `0x19E` | 52 | Spell mixtures — twenty-six `u16` |
| `0x1D2` | 2 | Items (quest-item bitmask) |
| `0x1D4` | 1 | Map X |
| `0x1D5` | 1 | Map Y |
| `0x1D6` | 1 | Stones (bitmask) |
| `0x1D7` | 1 | Runes (bitmask) |
| `0x1D8` | 2 | Party member count |
| `0x1DA` | 2 | Transport tile (`0x1F` = Avatar on foot) |
| `0x1DC` | 2 | Balloon state |
| `0x1DE` | 2 | Trammel phase |
| `0x1E0` | 2 | Felucca phase |
| `0x1E2` | 2 | Ship hull (`50` = full) |
| `0x1E4` | 2 | LB intro flag |
| `0x1E6` | 2 | Last camp |
| `0x1E8` | 2 | Last reagent |
| `0x1EA` | 2 | Last meditation |
| `0x1EC` | 2 | Last virtue |
| `0x1EE` | 1 | Dungeon X |
| `0x1EF` | 1 | Dungeon Y |
| `0x1F0` | 2 | Orientation |
| `0x1F2` | 2 | Dungeon level (`0xFFFF` = not in a dungeon) |
| `0x1F4` | 2 | Location |

## Notes

- **Food is stored ×100** (with a sub-100 remainder for the fractional food the game
  tracks). Fringe Retro Kit shows and edits it as the whole number; writing resets the
  remainder to 0.
- Name padding after the null terminator is **not** zeroed — records carry leftover bytes
  there. Editors should write a null terminator and leave the rest, or clear it; either
  way, don't interpret it.
- Fields above the character records that this project doesn't yet expose for editing
  (mixtures, items/stones/runes bitmasks, dungeon state) are still preserved on write.

## Provenance

- Structure follows the `xu4` open-source reimplementation's `savegame` definitions.
- Verified against a real 502-byte `PARTY.SAV` containing the eight canonical companions
  (Mariah, Iolo, Geoffrey, Jaana, Julia, Dupre, Shamino, Katrina): every class, sex,
  weapon, armor, HP, and stat decoded correctly, and edits load back into the game.
