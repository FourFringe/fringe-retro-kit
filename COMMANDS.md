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

The interactive UI is a **batch editor**. Select a game to open it: single-character
games (Ultima I/II) go straight to a field editor, while multi-character games (Ultima III
rosters and parties) first show a list of characters to drill into. The editor lists every
field the tool understands as `label: value`.

To change a field, select it and press `Enter` (or `e`): the current value appears on the
bottom line for editing, and for enum/letter fields the valid options are shown. Type a new
value and press `Enter` to commit it (invalid values are rejected and the field is left in
edit mode so you can fix them), or `Esc` to cancel. Edits accumulate in memory — a `●` in
the title marks unsaved changes — and are only written to disk when you press `s`, which
takes a single timestamped backup and one write. Leaving a game or quitting with unsaved
edits prompts you to save, discard, or cancel.

Section grouping (Ultima I) and the party header (Ultima III) are not shown in the editor's
flat field list; use the `inspect` command to see those.

### Backup browser

Every save writes a timestamped `.bak` file beside the original. Press `b` in the editor to
open the **backup browser**: a list of that save's backups (newest first, with the one
matching the current file marked `← current`) alongside a decoded preview of the selected
backup. Press `Enter` or `r` to restore the selected backup — you're asked to confirm, a
fresh safety backup of the current file is made first, and the editor reloads to show the
restored values. Restoring a backup that already matches the current file is a no-op (no
write, no extra backup).

| Key | Action |
| --- | --- |
| `↑` / `↓` (or `k` / `j`) | Move selection · scroll one line |
| `Enter` (or `→`) | Open the selected game / character |
| `Enter` / `e` | Edit the selected field |
| `s` | Save the session (backup + write) |
| `b` | Open the backup browser (from the editor) |
| `Enter` / `r` | Restore the selected backup (backup browser) |
| `PgUp` / `PgDn` (or `Space`) | Scroll a page (messages / backup preview) |
| `Home` / `End` | Jump to top / bottom (messages / backup preview) |
| `Esc` (or `←` / `Backspace`) | Cancel edit · back one screen |
| `q` | Quit |

When a save/discard prompt is open: `s` saves and continues, `d` discards, `Esc` cancels.
When a restore prompt is open: `y` restores, `Esc` cancels.

The Save Library is planned (see [ROADMAP.md](ROADMAP.md)).

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
- **Not yet implemented:** automatic pruning / retention limits, or a configurable backup
  directory — backups currently accumulate beside the save until you delete them.

### 🔷 Save Library (planned)

Separate from automatic backups, the **Save Library** will be your *curated, named*
collection of game moments ("Before Time Machine", "Endgame", …), intended for long-term
preservation. Planned behavior:

- A **configurable location**, defaulting to an OS-appropriate folder (e.g. a
  `Fringe Retro Kit` folder under your Documents).
- Explicitly **cloud-friendly** — you can point it at a synced folder (Dropbox, Google
  Drive, OneDrive, iCloud Drive) and the tool treats it as ordinary storage.
- Named snapshots with notes/metadata, browsable per game and character, and restorable
  into the active save directory.

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
