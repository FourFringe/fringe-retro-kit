# Fringe Retro Kit — Command Reference

The command-line tool is `fringe-retro`. This document lists every command it supports
today, plus commands that are planned but not yet built.

> **Current status:** Phase 1 — a command-line tool with **Ultima I** and **Ultima III**
> support hardcoded. The game is auto-detected from the save file, so the same commands
> work across both. A terminal UI (TUI), automatic game discovery, and the Save Library
> are planned; see [ROADMAP.md](ROADMAP.md) and [PHASE-1-ULTIMA-I.md](PHASE-1-ULTIMA-I.md).

Legend: ✅ implemented · 🔷 planned (not yet available)

---

## Conventions

```
fringe-retro <command> [arguments] [--flags]
```

- `<target>` selects the save to act on. It can be either:
  - a **game identifier** from your library manifest (e.g. `ultima2`), optionally with a
    `:file` selector to pick a specific save file (e.g. `ultima3:PARTY.ULT`), or
  - a full or relative **path** to a save file (used exactly as given).

  List your configured games with `fringe-retro games` (see [Library](#library)).
- Global flags:
  - `--help`, `-h` — show help. Works on the tool and on any subcommand
    (`fringe-retro set --help`).
  - `--version`, `-V` — print the version.
- Numbers may be written in decimal or hexadecimal (`0x`-prefixed) where noted.
- The **game is auto-detected** from the save file, so the same commands work across
  games. Multi-character games (Ultima III) use a `--slot` flag — see
  [Ultima III](#ultima-iii-rosters--parties).

On macOS the path usually contains characters that need quoting, so wrap it in quotes:

```bash
fringe-retro inspect "/Applications/Ultima I™.app/Contents/Resources/game/PLAYER1.U1"
```

---

## Interactive UI

Run `fringe-retro` **with no command** to launch the interactive terminal UI:

```bash
fringe-retro
```

The interactive UI is a **batch editor**. Each game appears once in the list. Selecting a
game that has more than one save file present shows a **file chooser** first; games with a
single save file open straight away. Examples of multi-file games: **Ultima I**'s character
slots (`PLAYER1.U1`…`PLAYER4.U1`) and **Ultima III**'s character **roster** (`ROSTER.ULT`)
plus active **party** (`PARTY.ULT`). Single-character saves (Ultima I/II) go to a field
editor, while multi-character saves (an Ultima III roster or party) show a list of characters
to drill into. The editor lists every field the tool understands as `label: value`.

The party file is the one that includes the **"Party settings"** entry (the party header).
`Esc` from a character list steps back to the file chooser, then to the games list.

To change a field, select it and press `Enter` (or `e`). **Enum fields** (race, class,
weapon, and the like) and boolean fields open a **picker**: use `←`/`→` to cycle through the
valid values and `Enter` to set the chosen one. **Text and number fields** open an inline
editor on the bottom line — type a new value and press `Enter` to commit (invalid values are
rejected and the field stays in edit mode so you can fix them). `Esc` cancels either. Edits
accumulate in memory — a `●` in the title marks unsaved changes — and are only written to
disk when you press `s`, which takes a single timestamped backup and one write. Leaving a
game or quitting with unsaved edits prompts you to save, discard, or cancel.

Related fields are grouped under **section headers** in the editor (e.g. Ultima I's
Character / Attributes / Status / Inventory groups). For **Ultima III party files**
(`PARTY.ULT`), the party's own settings appear as a **"Party settings"** entry alongside the
characters — open it to edit the transport, move count, party size, map position, and marching
order.

### Backup browser

Every save writes a timestamped `.bak` file beside the original. Press `b` in the editor to
open the **backup browser**: a list of that save's backups (newest first, with the one
matching the current file marked `← current`) alongside a decoded preview of the selected
backup. Press `Enter` or `r` to restore the selected backup — you're asked to confirm, a
fresh safety backup of the current file is made first, and the editor reloads to show the
restored values. Restoring a backup that already matches the current file is a no-op (no
write, no extra backup).

Press `n` to take a **snapshot**: a manual backup of the current save file on disk (a
"bookmark" of the state you just saved in-game), even when you haven't edited anything. If
an identical backup already exists the snapshot is skipped, so repeatedly snapshotting an
unchanged save won't pile up duplicates.

### Templates

Press `t` in the editor to open the **template picker**: a list of the [character
templates](docs/templates.md) defined for that game, with a preview of the fields each one
sets. Invalid templates are marked `✗` and can't be applied. Press `Enter` or `a` to apply
the selected template — its fields are set on the current character (marking the session
unsaved), just like editing them by hand. Apply as many as you like, then press `s` to save.

Press `T` (capital) to **capture** the current character into a new template: the field list
gains checkboxes (any fields you've changed this session are pre-checked). Use `Space` to
toggle a field, `a` to toggle all, then `Enter` to name it and save. The template is
**appended** to `templates.toml` (existing content is preserved) and becomes available in
the picker immediately.

### Resources

Press `r` on the **games list** to open a game's **web resources** — curated links to
wikis, walkthroughs, maps, and save-format references. Select one and press `Enter` (or `o`)
to open it in your operating system's default browser; the TUI keeps running underneath.
`Esc` returns to the games list. See [`resources`](#-resources-game) below for how to add or
override links.

### Library

Press `L` on the **games list** to open that game's **Save Library** — your curated
snapshots (see [`library`](#-library-add--list--view--restore--rename--duplicate--delete)
below). A list of snapshots sits beside a decoded preview of the selected one. From here you
can manage the whole collection:

- `a` — **add** a snapshot of the game's current save (type a name, `Enter`).
- `Enter` / `r` — **restore** the selected snapshot into the active game (confirm with `y`; a
  safety backup is made first).
- `R` — **rename**, `D` — **duplicate** (type a name, `Enter`).
- `d` — **delete** the selected snapshot (confirm with `y`).
- `PgUp` / `PgDn` scroll the preview; `Esc` returns to the games list.

(If no `[library] path` is configured, `L` shows a short message explaining how to set one.)

| Key | Action |
| --- | --- |
| `↑` / `↓` (or `k` / `j`) | Move selection · scroll one line |
| `Enter` (or `→`) | Open the selected game / character |
| `r` | Open the selected game's web resources (games list) |
| `L` | Open the selected game's Save Library (games list) |
| `Enter` / `e` | Edit the selected field |
| `s` | Save the session (backup + write) |
| `b` | Open the backup browser (from the editor) |
| `t` | Open the template picker (from the editor) |
| `T` | Capture the current character as a new template (from the editor) |
| `Enter` / `r` | Restore the selected backup (backup browser) |
| `n` | Snapshot the current save (backup browser) |
| `Enter` / `a` | Apply the selected template (template picker) |
| `Enter` / `o` | Open the selected link in your browser (resources) |
| `a` / `Enter`·`r` / `R` / `D` / `d` | Library: add / restore / rename / duplicate / delete |
| `PgUp` / `PgDn` (or `Space`) | Scroll a page (messages / previews) |
| `Home` / `End` | Jump to top / bottom (messages / backup preview) |
| `Esc` (or `←` / `Backspace`) | Cancel edit · back one screen |
| `q` | Quit |

When a save/discard prompt is open: `s` saves and continues, `d` discards, `Esc` cancels.
When a restore or delete prompt is open: `y` confirms, `Esc` cancels.

---

## Read-only commands

### ✅ `inspect <path>`

Show every field the tool understands, grouped into sections.

```bash
fringe-retro inspect "/…/PLAYER1.U1"
```
```
Character:
  Name             Enki
  Race             Human
  Class            Wizard
  Sex              Male

Attributes:
  Strength         12
  …

Inventory: Weapons:
  Dagger           2
  …
```

Reads only; never modifies the file.

### ✅ `get <path> <field>`

Print a single field's value (handy for scripting). See [Fields](#fields) for names.

```bash
fringe-retro get "/…/PLAYER1.U1" gold      # -> 100
```

If the field name is unknown, the tool lists the valid field names.

### ✅ `dump <path> [--range START:END]`

Print an `xxd`-style hex dump (offset, hex bytes, ASCII). Useful for verifying edits or
exploring bytes we don't yet interpret.

```bash
fringe-retro dump "/…/PLAYER1.U1"                 # whole 820-byte file
fringe-retro dump "/…/PLAYER1.U1" --range 0x18:0x24   # just the six core stats
```

`START` and `END` accept decimal or `0x` hex. The range is **start-inclusive,
end-exclusive**; `END` is clamped to the file length.

### ✅ `watch <path> [--interval MS]`

Poll a save file and print **byte-level changes** as they happen — offset, hex and
decimal old→new, and ASCII. Runs until you press Ctrl-C. This is the primary tool for
**reverse-engineering** an undocumented save format: run it, do one thing in the game,
and watch which bytes move.

```bash
fringe-retro watch "/…/PLAYER"                 # default 500 ms poll
fringe-retro watch "/…/PLAYER" --interval 200
```
```
[10:27:58] 2 byte(s) changed:
  0x0005: 0C -> 19   ( 12 ->  25)   '.' -> '.'
  0x0007: 64 -> FF   (100 -> 255)   'd' -> '.'
```

Reads only; never modifies the file.

---

## Editing commands

### ✅ `set <path> <field> <value>`

Change one field, safely. Before writing, the tool **automatically creates a timestamped
backup** of the current file, then writes the change **atomically** (see
[Safety model](#safety-model)). Only the target field's bytes change; everything else —
including bytes we don't understand — is preserved exactly.

```bash
fringe-retro set "/…/PLAYER1.U1" gold 9999
fringe-retro set "/…/PLAYER1.U1" strength 25
fringe-retro set "/…/PLAYER1.U1" transport aircar   # enums accept a name…
fringe-retro set "/…/PLAYER1.U1" transport 5        # …or its number
```
```
gold: 100 -> 9999
backup: /…/PLAYER1.U1.2026-07-08T12-13-19.620.bak
```

Values are validated before anything is written:

- **Number fields** must parse as an integer and fall within the field's range
  (rejected otherwise — the file is left untouched).
- **Enum fields** accept a variant name (case-insensitive) or its numeric value; unknown
  inputs are rejected and the valid options are listed.
- **Name** must be ASCII and at most 14 characters.

---

## Backup commands

### ✅ `backup <path>`

Make a manual timestamped backup right now and print its path. (The same backup is made
automatically by `set` and `restore`.)

```bash
fringe-retro backup "/…/PLAYER1.U1"
# -> /…/PLAYER1.U1.2026-07-08T12-14-03.911.bak
```

### ✅ `backups <path>`

List existing backups for a save file, oldest first.

```bash
fringe-retro backups "/…/PLAYER1.U1"
```

Pass `--prune` to first delete old backups per your retention policy:

```bash
fringe-retro backups "/…/PLAYER1.U1" --prune
```

**Retention.** Backups pile up over time, so you can cap them with a `[backups]` table in
your config:

```toml
[backups]
keep = 20          # keep at most this many recent backups per save (0 = no limit)
max_age_days = 90  # also delete backups older than this (0 = no limit)
```

With a policy set, old backups are pruned automatically after each `set`, `backup`, and
`restore` (and after saving or snapshotting in the interactive UI). A backup is removed if it
falls outside the newest `keep`, or is older than `max_age_days`. **Save Library snapshots
are never pruned** — they're your curated collection. Use `backups --prune` to apply a new
policy to an existing pile immediately.

### ✅ `restore <path> <backup>`

Restore a chosen backup over the active save. As a safety net, the **current** save is
itself backed up first (its path is printed), so a restore is never destructive.

```bash
fringe-retro restore "/…/PLAYER1.U1" "/…/PLAYER1.U1.2026-07-08T12-13-19.620.bak"
```
```
restored /…/PLAYER1.U1.2026-07-08T12-13-19.620.bak -> /…/PLAYER1.U1
previous save backed up to /…/PLAYER1.U1.2026-07-08T12-14-59.004.bak
```

---

## Fields

Field names used by `get` and `set` (Ultima I). All numeric values are stored as
little-endian 16-bit integers.

| Field | Label | Type | Range / options |
| --- | --- | --- | --- |
| `name` | Name | text | ASCII, ≤ 14 characters |
| `race` | Race | enum | Human, Elf, Dwarf, Bobbit |
| `class` | Class | enum | Fighter, Cleric, Wizard, Thief |
| `sex` | Sex | enum | Male, Female |
| `hits` | Hits | number | 0–9999 |
| `strength` | Strength | number | 0–9999 |
| `agility` | Agility | number | 0–9999 |
| `stamina` | Stamina | number | 0–9999 |
| `charisma` | Charisma | number | 0–9999 |
| `wisdom` | Wisdom | number | 0–9999 |
| `intelligence` | Intelligence | number | 0–9999 |
| `gold` | Gold | number | 0–9999 |
| `experience` | Experience | number | 0–9999 |
| `food` | Food | number | 0–9999 |
| `weapon` | Ready Weapon | enum | None, Dagger, Mace, Axe, Rope & Spikes, Sword, Great Sword, Bow & Arrows, Amulet, Wand, Staff, Triangle, Pistol, Light Sword, Phazor, Blaster |
| `spell` | Ready Spell | enum | None, Open, Unlock, Magic Missile, Steal, Ladder Down, Ladder Up, Blink, Create, Destroy, Kill |
| `armour` | Ready Armour | enum | None, Leather, Chain Mail, Plate Mail, Vacuum Suit, Reflect Suit |
| `transport` | Transport | enum | Walking, Horse, Cart, Raft, Frigate, Aircar |
| `x` | Map X | number | 0–65535 (overworld position) |
| `y` | Map Y | number | 0–65535 (overworld position) |
| `last_signpost` | Last Signpost | number | 0–65535 (index; default 65535) |
| `steps` | Steps | number | 0–65535 |

> Editing `x`/`y` moves the character on the overworld and is best left alone unless you
> know the coordinates. Setting `transport` alone changes the value but not the on-screen
> vehicle icon (a quirk of the game).

### Inventory quantities

Each item you can carry has its own count field (`number`, `0–9999`). The keys follow a
`<category>_<item>` pattern:

- **Gems:** `gem_red`, `gem_green`, `gem_blue`, `gem_white`
- **Armour:** `armour_leather`, `armour_chain_mail`, `armour_plate_mail`, `armour_vacuum_suit`, `armour_reflect_suit`
- **Weapons:** `weapon_dagger`, `weapon_mace`, `weapon_axe`, `weapon_rope_spikes`, `weapon_sword`, `weapon_great_sword`, `weapon_bow`, `weapon_amulet`, `weapon_wand`, `weapon_staff`, `weapon_triangle`, `weapon_pistol`, `weapon_light_sword`, `weapon_phazor`, `weapon_blaster`
- **Spells:** `spell_open`, `spell_unlock`, `spell_magic_missile`, `spell_steal`, `spell_ladder_down`, `spell_ladder_up`, `spell_blink`, `spell_create`, `spell_destroy`, `spell_kill`
- **Transports:** `transport_horse`, `transport_cart`, `transport_raft`, `transport_frigate`, `transport_aircar`, `transport_shuttle`, `transport_time_machine`

Note the difference between an *equipped* item and its *inventory count*: `weapon` (above)
is what's readied, while `weapon_blaster` is how many Blasters you own. Run
`fringe-retro inspect <save>` for the full grouped list.

```bash
fringe-retro set "/…/PLAYER1.U1" weapon_blaster 1
fringe-retro set "/…/PLAYER1.U1" transport_time_machine 1
```

---

## Ultima III (rosters & parties)

Ultima III stores **multiple characters per file**, and the game is auto-detected from
the save, so the same `inspect` / `get` / `set` commands work — with a `--slot` flag to
pick the character. Two file types are supported:

- **`ROSTER.ULT`** — your pool of up to **20 characters** (`--slot 1`…`20`).
- **`PARTY.ULT`** — the **4 active party members** (`--slot 1`…`4`), plus a party header
  (transport, move count, position, and the roster slots that form the party).

```bash
fringe-retro inspect "/…/ROSTER.ULT"                  # every occupied slot
fringe-retro get  "/…/ROSTER.ULT" strength --slot 2  # slot 2's strength
fringe-retro set  "/…/ROSTER.ULT" gold 9999 --slot 2 # edit slot 2
fringe-retro inspect "/…/PARTY.ULT"                  # party header + 4 members
fringe-retro set  "/…/PARTY.ULT" hits 999 --slot 1   # edit active party member 1
```

`--slot` is 1-based and defaults to 1. (Ultima I has a single character, so it ignores
`--slot`.)

> ⚠️ **Active party members live in two files.** A character who is in the active party
> exists both in `ROSTER.ULT` and as a copy in `PARTY.ULT`. To reliably change such a
> character, edit **both** files (or edit while no party is formed).

### Ultima III character fields

Numbers are stored as **BCD** (binary-coded decimal); race/class/sex/status are stored as
**letters**, and `set` accepts either the full name or the letter.

| Field | Type | Range / options |
| --- | --- | --- |
| `name` | text | ASCII, ≤ 9 characters |
| `race` | letter | Human, Elf, Dwarf, Fuzzy, Bobbit |
| `class` | letter | Fighter, Cleric, Wizard, Thief, Paladin, Barbarian, Lark, Illusionist, Alchemist, Druid, Ranger |
| `gender` | letter | Male, Female, Other |
| `status` | letter | Good, Poisoned, Dead, Ashes |
| `strength`, `dexterity`, `intelligence`, `wisdom` | number | 0–99 |
| `hits`, `max_hits`, `experience` | number | 0–9999 |
| `magic`, `torches`, `gems`, `keys`, `powders` | number | 0–99 |
| `food` | number | 0–9999 |
| `food_frac` | number | 0–99 (fractional food) |
| `gold` | number | 0–9999 |
| `in_party` | yes/no | whether the character is in the active party |
| `marks_cards` | bitfield | Love, Sol, Moon, Death, Force, Fire, Snake, Kings (set as a raw 0–255 value for now) |
| `worn_armor`, `weapon` | number | currently worn armor / readied weapon index |
| `armor_*` (7) | number | owned armor counts: `armor_cloth`, `_leather`, `_chain`, `_plate`, `_chain_plus2`, `_plate_plus2`, `_exotic` |
| `weapon_*` (15) | number | owned weapon counts: `weapon_dagger`, `_mace`, `_sling`, `_axe`, `_bow`, `_sword`, `_2h_sword`, `_axe_plus2`, `_bow_plus2`, `_sword_plus2`, `_gloves`, `_axe_plus4`, `_bow_plus4`, `_sword_plus4`, `_exotic` |

Run `fringe-retro inspect <file>` for the full decoded list of every character.

---

## Ultima IV (party save)

Ultima IV keeps the whole party in one file, **`PARTY.SAV`**, holding up to **8 character
slots** plus shared party/game state. In the interactive editor these appear as a **"Party
& Virtues"** entry (food, gold, the eight virtues, inventory, map position) followed by each
character. For the CLI, party-wide fields are addressed by name, while a character's own
fields use `--slot 1`…`8`:

```bash
fringe-retro inspect ultima4               # party state + every occupied slot
fringe-retro get  ultima4 gold             # party gold
fringe-retro get  ultima4 hp --slot 1      # slot 1's hit points
fringe-retro set  ultima4 food 500         # party food
fringe-retro set  ultima4 class Paladin --slot 6
```

### Ultima IV fields

Numbers are plain little-endian binaries. Sex, class, status, weapon, and armor are enums;
`set` accepts the name or the number.

**Party & Virtues:** `food` (0–9999; stored ×100 on disk but shown/edited as the whole
number), `gold` (0–9999); the eight virtues `honesty`, `compassion`, `valor`, `justice`,
`sacrifice`, `honor`, `spirituality`, `humility` (0–99); `torches`, `gems`, `keys`,
`sextants` (0–99); the eight reagents `reagent_ash`, `_ginseng`, `_garlic`, `_silk`, `_moss`,
`_pearl`, `_nightshade`, `_mandrake` (0–99); and map `x` / `y`.

**Per character (`--slot`):** `name` (≤ 15 chars), `sex` (Male, Female), `class` (Mage, Bard,
Fighter, Druid, Tinker, Paladin, Ranger, Shepherd), `status` (Good, Poisoned, Sleeping,
Dead), `strength` / `dexterity` / `intelligence` (0–99), `hp` / `hp_max` / `experience`
(0–9999), `magic` (0–99), `weapon` and `armor` (the enums above).

See [docs/formats/ultima4.md](docs/formats/ultima4.md) for the byte-level layout.

---

## Ultima V (game save)

Ultima V keeps the whole game in one file, **`SAVED.GAM`** (a 4192-byte RAM snapshot),
holding up to **16 character slots** plus shared party/game state. In the interactive editor
these appear as a **"Party & Provisions"** entry (food, gold, party size, inventory,
reagents, date/time, karma, map position) followed by each character. For the CLI,
party-wide fields are addressed by name, while a character's own fields use `--slot 1`…`16`:

```bash
fringe-retro inspect ultima5               # party state + every occupied character
fringe-retro get  ultima5 gold             # party gold
fringe-retro get  ultima5 hp --slot 1      # slot 1's hit points
fringe-retro set  ultima5 food 500         # party food
fringe-retro set  ultima5 class Mage --slot 1
```

### Ultima V fields

Numbers are plain little-endian binaries. Sex is a numeric enum; class and status are ASCII
letters; `set` accepts the name, letter, or number.

**Party & Provisions:** `food`, `gold` (0–9999); `members` (party size, 1–6); the inventory
counts `keys`, `gems`, `torches`, `magic_carpets`, `skull_keys`, `sextants` (0–99); the eight
reagents `reagent_ash`, `_ginseng`, `_garlic`, `_silk`, `_moss`, `_pearl`, `_nightshade`,
`_mandrake` (0–99); `year`, `month`, `day`, `hour`, `minute`; `karma`; and `location`, `z`,
`x`, `y` (raw map coordinates).

**Per character (`--slot`):** `name` (≤ 8 chars), `sex` (Male, Female), `class` (Avatar,
Bard, Fighter, Mage), `status` (Good, Poisoned, Charmed, Asleep, Dead), `strength` /
`dexterity` / `intelligence` (1–30), `magic` (0–99), `hp` / `hp_max` / `experience`
(0–9999), `level` (1–8), `months_inn`, and equipment `helmet`, `armor`, `weapon_left`,
`weapon_right`, `ring`, `amulet` (raw item indices, `0xFF` = none).

See [docs/formats/ultima5.md](docs/formats/ultima5.md) for the byte-level layout.

---

## Library

### ✅ `games`

List the games configured in your library manifest, each with its default save file and
whether that file is present:

```bash
fringe-retro games
```

```
ultima1        Ultima I  [found]
    save:     /Applications/Ultima I™.app/Contents/Resources/game/PLAYER1.U1
    platform: gog
ultima2        Ultima II  [found]
    save:     /Applications/Ultima II™.app/Contents/Resources/game/PLAYER
    platform: gog
```

Games with `enabled = false` are hidden. See [Configuration](#configuration) to set up the
manifest and [Game identifiers](#game-identifiers) to use these ids in other commands.

### ✅ `detect [--write]`

Scan for installed games and report where their saves live — so you don't have to find the
paths by hand. Currently detects **GOG** and **Steam** games on **macOS**. GOG games install
to `/Applications` with saves inside the bundle; Steam games are found via their app
manifests, and their saves may live elsewhere (e.g. Wasteland saves to
`~/Library/Application Support/Wasteland/<slot>`).

```bash
fringe-retro detect          # report what's installed
fringe-retro detect --write  # also add new games to your config
```

```
ultima4    Ultima IV    [gog]
    app:   /Applications/Ultima IV™.app
    saves: /Applications/Ultima IV™.app/Contents/Resources/game  (PARTY.SAV found)
```

Plain `detect` only reports. With `--write`, any detected game **not already in your config**
is appended to it (matched by game, so nothing is duplicated); your existing `config.toml` is
**backed up first** (a timestamped `.bak` beside it), and its current entries are left
untouched. Re-running is safe — already-configured games are skipped.

Prefer not to write anything? Set `[detect] auto = true` in your config and every run will
scan and make detected games usable **in-memory** (they show as `(auto-detected)` in `games`,
and your explicit entries always win). It's **off by default** so behavior stays predictable.

Other platforms (Windows/Linux) are deferred until they're officially supported.

### ✅ `resources [<game>]`

List curated **web resources** for a game — links to wikis, walkthroughs, maps, and
save-format references — or open one in your browser.

```bash
fringe-retro resources                 # every game's links
fringe-retro resources ultima5         # one game's links, numbered
fringe-retro resources ultima5 --open 2 # open link #2 in your default browser
```

```
Ultima V (ultima5):
   1. [wiki] Ultima V — Ultima Codex wiki
      https://wiki.ultimacodex.com/wiki/Ultima_V:_Warriors_of_Destiny
   2. [walkthrough] Ultima V walkthrough
      https://wiki.ultimacodex.com/wiki/Ultima_V_walkthrough
   3. [map] Ultima V map of Britannia
      https://wiki.ultimacodex.com/wiki/Ultima_V_map_of_Britannia
   4. [format] Ultima V internal formats (save layout)
      https://wiki.ultimacodex.com/wiki/Ultima_V_internal_formats
```

A curated default set ships with the tool (`resources.toml`). To add or override links, set
`FRINGE_RETRO_RESOURCES` to your own file, or place a `resources.toml` in the working
directory — your entries are merged in (links whose URL is already present are skipped). Each
entry has a `title`, `url`, and free-form `category` (e.g. `wiki`, `walkthrough`, `map`,
`format`, `play`). In the interactive UI, press `r` on a game to browse and open these links.

### ✅ `library` (`add` · `list` · `view` · `restore` · `rename` · `duplicate` · `delete`)

The **Save Library** is your curated, permanent collection of named **snapshots** — kept
separate from the automatic `.bak` backups. Each snapshot captures a game's **whole** save
set (e.g. Ultima III's `ROSTER.ULT` **and** `PARTY.ULT` together) as one atomic unit, stored
in a self-describing folder under your library path. Set the location with `[library] path`
in your config (see [Configuration](#configuration)); `~` expands to your home directory. Use
`library` or its alias `lib`.

```bash
fringe-retro library add ultima3 "Before the Dungeon" --notes "roster + party"
fringe-retro lib list                    # every game's snapshots
fringe-retro lib list ultima3            # one game's snapshots
fringe-retro lib view ultima3 before-the-dungeon      # inspect without restoring
fringe-retro lib restore ultima3 before-the-dungeon
fringe-retro lib rename ultima3 before-the-dungeon "Deep Dungeon"
fringe-retro lib duplicate ultima3 deep-dungeon --name "Backup"
fringe-retro lib delete ultima3 deep-dungeon          # prompts; add -y to skip
```

```
Ultima III (ultima3):
  Before the Dungeon       [before-the-dungeon]
      created: 2026-07-14 09:39:11
      updated: 2026-07-14 09:39
      roster + party
```

- **`add <game> <name> [--notes <text>]`** copies the game's current save set into a new
  snapshot. The folder is named with a slug of the name; if that slug already exists, a
  numeric suffix is added (you're told the final slug).
- **`list [<game>]`** shows snapshots grouped by game, each with its slug, when it was last
  updated (from the save files' timestamps), and any notes.
- **`view <game> <slug>`** decodes a snapshot's saved fields (like `inspect`) **without**
  restoring it.
- **`restore <game> <slug>`** copies a snapshot's files back into the game's active save
  directory. Any pre-existing active file is safety-backed-up first; files already identical
  are skipped (a full match is a no-op).
- **`rename <game> <slug> <new-name>`** updates the snapshot's name and renames its folder to
  the new slug.
- **`duplicate <game> <slug> [--name <name>]`** copies a snapshot to a new one (defaulting to
  `"<name> copy"`); notes are carried over.
- **`delete <game> <slug> [-y]`** removes a snapshot. You're prompted to confirm unless you
  pass `-y`/`--yes`.

Snapshots are portable: each folder carries an `entry.toml` describing it, so you can move
one to another machine or cloud folder and it stays valid. The same operations are available
interactively — press `L` on a game in the [interactive UI](#library).

### ✅ `templates`

List your character templates and check that each one is valid for its game:

```bash
fringe-retro templates
```

```
ultima1    Starter boost            3 field(s)  [ok]
ultima2    Fighter                  5 field(s)  [ok]
ultima2    Top up resources         2 field(s)  [ok]
```

Templates are read from `templates.toml` (or `$FRINGE_RETRO_TEMPLATES`). A template with an
unknown field or an invalid value is shown with an `ERROR:` note and can't be applied. Apply
templates interactively with `t` in the editor. See [docs/templates.md](docs/templates.md)
for the file format and the allowed field names/values per game.

---

## Where files live

### Active save files

Fringe Retro Kit reads and writes the game's own save files in place. For **Ultima I**
via **GOG on macOS**, the save lives *inside* the application bundle:

```
/Applications/Ultima I™.app/Contents/Resources/game/PLAYER1.U1
```

- Save files are named `PLAYER1.U1` … `PLAYER4.U1` (up to four character slots), using
  DOS 8.3, uppercase names. The tool matches case-insensitively.
- On a normal GOG install this file is owned by you and writable **without `sudo`**.
- Each save is exactly **820 bytes**.

Windows/Linux/Steam locations and **automatic discovery** are planned (today you pass the
path explicitly).

### Backups

Backups are written **next to the save file**, in the same directory, named:

```
<save-file>.<timestamp>.bak
# e.g. PLAYER1.U1.2026-07-08T12-13-19.620.bak
```

- The timestamp is local time, `YYYY-MM-DDThh-mm-ss.mmm` (millisecond precision), chosen
  so that a plain alphabetical sort is also chronological.
- Backups are created automatically before every `set` and `restore`, and manually via
  `backup`. They are meant purely for recovery.
- **Retention:** with a `[backups]` policy set (see [`backups`](#-backups-path)), old
  backups are pruned automatically after each save; otherwise they accumulate beside the
  save until you delete them. A configurable backup directory is not yet implemented.

### ✅ Save Library

Separate from automatic backups, the **Save Library** is your *curated, named* collection of
game moments ("Before Time Machine", "Endgame", …), intended for long-term preservation. See
the [`library`](#-library-add--list--view--restore--rename--duplicate--delete) command and
the interactive [Library](#library) screen. In brief:

- A **configurable location** (`[library] path`) — point it at a synced folder (Dropbox,
  Google Drive, OneDrive, iCloud Drive) and the tool treats it as ordinary storage.
- Named snapshots with notes/metadata, browsable per game, and restorable into the active
  save directory. Each snapshot captures a game's whole save set as one portable folder.

See [ROADMAP.md](ROADMAP.md) for the full plan.

### Game identifiers

Instead of a path, most commands accept a **game identifier** from your
[library manifest](#configuration). The identifier resolves to that game's save directory
plus its default save file:

```bash
fringe-retro inspect ultima2             # -> <ultima2 save_dir>/PLAYER
fringe-retro get ultima1 gold            # -> <ultima1 save_dir>/PLAYER1.U1
```

For games with more than one save file, add a `:file` selector:

```bash
fringe-retro inspect ultima3             # -> ROSTER.ULT (default)
fringe-retro inspect ultima3:PARTY.ULT   # -> the active party
fringe-retro set ultima1:PLAYER2.U1 gold 500
```

Anything that isn't a configured identifier is treated as a plain filesystem path, so
absolute and relative paths always work:

```bash
fringe-retro inspect "/full/path/PLAYER"   # used as-is
```

### Configuration

Your **library manifest** is a TOML file listing the games you own. The tool reads
`config.toml` from the current working directory (or the path in the `FRINGE_RETRO_CONFIG`
environment variable); a template lives at [config.example.toml](config.example.toml).
Copy it to a local `config.toml` (gitignored) and add one `[games.<id>]` table per game:

```toml
[games.ultima2]
platform = "gog"                         # informational for now
save_dir = "/Applications/Ultima II™.app/Contents/Resources/game"

[games.u1-steam]
game = "ultima1"                         # `game` is needed when the id isn't a known name
save_dir = "/path/to/steam/ultima1"
enabled = false                          # hide a game you don't currently play
```

The `<id>` is what you type on the command line. `game` defaults to `<id>`, so
`[games.ultima1]` needs no `game` key. Run `fringe-retro games` to list what's configured.
A proper per-OS config directory (via the `directories` crate) is planned.

---

## 🔷 Planned commands

These are **not implemented yet**; they reflect the direction in [ROADMAP.md](ROADMAP.md).

| Command (tentative) | Purpose |
| --- | --- |
| `list` (auto-discovery) | Auto-detect installed games and fill in save paths, so you don't configure them by hand. (`games` already lists your manually-configured library.) |
| `library …` | Manage the Save Library: save a named snapshot, list, restore, duplicate, rename, delete. |
| `config …` | View and edit configuration (save-library location, discovered game paths, etc.). |

Command names above are provisional and may change as those features are designed.

---

## Safety model

Every edit follows the same rules so that working with real saves never feels risky:

1. **Back up first.** `set` and `restore` create a timestamped backup before changing the
   target file.
2. **Write atomically.** Changes are written to a temporary file in the same directory,
   then renamed over the original — so a crash or interruption never leaves a
   half-written save.
3. **Preserve unknown bytes.** Edits change only the specific bytes of the field you set;
   everything else in the file is copied through untouched.
4. **Validate before writing.** Out-of-range numbers, unknown enum values, and bad names
   are rejected without modifying the file.

While the tool is young, the safest habit is to **edit a copy** of a save and confirm it
loads in the game before touching your primary file.
