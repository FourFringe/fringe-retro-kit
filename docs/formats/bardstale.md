# The Bard's Tale Trilogy — Save Format

The Bard's Tale Trilogy (Krome's 2018 remaster, Steam **app 843260**) is a Unity/IL2CPP
game whose saves are **.NET `BinaryFormatter` streams** — the [MS-NRBF] ".NET Remoting
Binary Format". Unlike the fixed byte layouts of the DOS-era games, an NRBF stream is
**self-describing**: every object carries its class name, its member names, and their types
inline, so a save is fully navigable *by name* with no offset hunting.

> **Provenance:** parsed and verified against a real Steam save on macOS. Class and member
> names are exactly as the game serialises them (the IL2CPP build embeds them). Every
> editable field — the character stats, the class/race/gender enums, and the pooled party
> gold — was confirmed against a live party, and edits are round-tripped byte-for-byte
> except for the one value changed.

## Where saves live

Saves are kept in **Steam Cloud**, under your per-user `userdata` directory:

```
~/Library/Application Support/Steam/userdata/<steamid>/843260/remote/saves/
    Save1.dat        # a manual save slot
    AutoSave.dat     # the autosave
    Save2.dat …      # further slots, created as you save
```

The game lists saves by scanning `saves/*.dat`, so any `.dat` there is a slot;
`fringe-retro` discovers them all (not just the first). Point a `[games.bardstale]`
`save_dir` at that `saves` folder (auto-detection fills it in for you).

### Editing and Steam Cloud

`remotecache.vdf` for the app tracks only the two `.dat` save files, each with a SHA1 and
size. A few practical consequences:

- **Close the game before editing.** This is the one hard rule (true of every game): a
  running game owns its save and will write over your edit. The Steam *client* being open
  is fine — on next launch it sees the newer local file and uploads it.
- The automatic timestamped `.bak` backup sits beside the save. It isn't a `.dat`, so it
  neither appears in-game nor syncs to the cloud — it's a purely local safety net.
- Writes are **in-place and atomic**, exactly as the game itself saves.

## Structure

A save file is **two concatenated `BinaryFormatter` streams**: a small header stream (a
`GameSaveHeader` — save name, version, current map, chapter, timestamp) followed by the main
game-state stream (the party, characters, world state, stats, automap, and so on). Each
stream is an independent object graph beginning with a `SerializationHeaderRecord` and
ending with a `MessageEnd`; object ids are scoped per stream (the reader namespaces them so
the two don't collide).

Key classes:

| Class | Role |
| --- | --- |
| `BardsTale.GameSaveHeader` | Slot metadata: `m_name`, `m_saveGameVersion`, `m_map`, `m_chapter`, `m_time` |
| `BardsTale.SaveableParty` | The party: `m_name` (party name), `m_members` (character list) |
| `BardsTale.Character` | One character — all the stats below |
| `BardsTale.Character+Class` / `+Race` / `+Gender` | Boxed enums referenced by a character (`{ value__: Int32 }`) |
| `BardsTale.GameStateData` | World/party state, including the **pooled party `m_gold`** |

## Editable fields

Editing patches a single inline integer in place — no re-serialisation — so the file stays
byte-identical apart from the changed value. Values are range-checked against their real
storage width.

**Per character** (`BardsTale.Character`):

| Field | Member | Notes |
| --- | --- | --- |
| Class / Race / Gender | `m_class` / `m_race` / `m_gender` | Boxed enums; edited by name (see tables below) |
| Strength, Intelligence, Dexterity, Constitution, Luck | `m_strength`, `m_intelligence`, `m_dexterity`, `m_constitution`, `m_luck` | |
| Hit Points / Max | `m_hitpoints` / `m_maxHitpoints` | |
| Spell Points / Max | `m_spellpoints` / `m_maxSpellpoints` | |
| Level, Experience | `m_level`, `m_experience` | |
| Condition | `m_condition` | |
| Disarm Trap / Identify / Hide in Shadows bonuses | `m_disarmTrapBonus`, `m_identifyBonus`, `m_hideInShadowsBonus` | Rogue skills |
| Bard Songs Remaining | `m_songsRemaining` | |

A character's own `m_gold` is always `0` — gold is pooled at the party level and is not
exposed per-character.

**Party-wide:**

| Field | Location | Notes |
| --- | --- | --- |
| Gold | `BardsTale.GameStateData.m_gold` | The party's pooled gold |

### Enum values

Confirmed against a real party (Warrior/Paladin/Rogue/Bard/Conjurer/Magician land exactly on
0/1/2/3/6/7); the remainder follow the game's class order.

| Class | | Race | | Gender |
| --- | --- | --- | --- | --- |
| 0 Warrior | 6 Conjurer | 0 Human | 4 Half-Elf | 0 Male |
| 1 Paladin | 7 Magician | 1 Elf | 5 Half-Orc | 1 Female |
| 2 Rogue | 8 Sorcerer | 2 Dwarf | 6 Gnome | |
| 3 Bard | 9 Wizard | 3 Hobbit | | |
| 4 Hunter | 10 Archmage | | | |
| 5 Monk | | | | |

## Why patch-in-place

`BinaryFormatter` is notoriously finicky to *re-emit* (type tables, object graph ordering,
library ids). Because an NRBF stream records the exact byte offset of every inline primitive,
we can overwrite just those bytes and leave the rest of the graph untouched — the safest
possible edit, and one that never has to reconstruct the serialiser's output. Parsing here is
also **safe**: the reader walks the structure but never deserialises into live objects (the
vector behind `BinaryFormatter`'s well-known insecurity), so untrusted saves can be inspected
without risk.

[MS-NRBF]: https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-nrbf/
