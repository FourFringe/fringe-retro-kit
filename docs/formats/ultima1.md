# Ultima I — Save Format (`PLAYER*.U1`)

A single fixed-layout file of **`0x334` (820) bytes** holding one character. All
multi-byte values are **little-endian unsigned 16-bit** integers. There is **no checksum**
— edited saves load directly.

> **Provenance:** this format is already public. Reference:
> <https://moddingwiki.shikadi.net/wiki/Ultima_I_Save_Game_Format> (reverse-engineered by
> TheAlmightyGuru and Daniel D'Agostino). Our implementation matches it and has been
> validated in-game (GOG on macOS).

## Character

| Offset | Field | Type | Notes |
| --- | --- | --- | --- |
| `0x00` | Name | ASCIIZ | up to 14 chars + null (`0x00..0x0F`) |
| `0x10` | Race | `u16` enum | 0 Human, 1 Elf, 2 Dwarf, 3 Bobbit |
| `0x12` | Class | `u16` enum | 0 Fighter, 1 Cleric, 2 Wizard, 3 Thief |
| `0x14` | Sex | `u16` enum | 0 Male, 1 Female |

## Attributes (`u16`, max 9999)

| Offset | Field |
| --- | --- |
| `0x18` | Strength |
| `0x1A` | Agility |
| `0x1C` | Stamina |
| `0x1E` | Charisma |
| `0x20` | Wisdom |
| `0x22` | Intelligence |

## Status (`u16`, max 9999)

| Offset | Field |
| --- | --- |
| `0x16` | Hits |
| `0x24` | Gold |
| `0x26` | Experience |
| `0x28` | Food |

## Equipped (`u16` enum)

| Offset | Field | Values |
| --- | --- | --- |
| `0x2A` | Ready Weapon | 0 None … 15 Blaster (see weapon list) |
| `0x2C` | Ready Spell | 0 None … 10 Kill |
| `0x2E` | Ready Armour | 0 None, 1 Leather, 2 Chain Mail, 3 Plate Mail, 4 Vacuum Suit, 5 Reflect Suit |
| `0x30` | Transport | 0 Walking, 1 Horse, 2 Cart, 3 Raft, 4 Frigate, 5 Aircar |

## Location (`u16`)

| Offset | Field |
| --- | --- |
| `0x34` | Map X |
| `0x36` | Map Y |
| `0xA8` | Last Signpost |
| `0xAC` | Steps |

## Inventory counts (`u16`, max 9999)

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

## References

- [Ultima I Save Game Format](https://moddingwiki.shikadi.net/wiki/Ultima_I_Save_Game_Format)
  — DOS Game Modding Wiki, reverse-engineered by TheAlmightyGuru and Daniel D'Agostino. Our
  implementation follows this spec.
