# Fringe Retro Kit — Command Reference

The command-line tool is `fringe-retro`. This document lists every command it supports
today, plus commands that are planned but not yet built.

> **Current status:** Phase 1 — a command-line tool with **Ultima I** support hardcoded.
> A terminal UI (TUI), automatic game discovery, and the Save Library are planned; see
> [ROADMAP.md](ROADMAP.md) and [PHASE-1-ULTIMA-I.md](PHASE-1-ULTIMA-I.md).

Legend: ✅ implemented · 🔷 planned (not yet available)

---

## Conventions

```
fringe-retro <command> [arguments] [--flags]
```

- `<path>` is the path to a save file. It can be either:
  - a full or relative path (used exactly as given), or
  - a **bare file name** like `PLAYER1.U1`, which is resolved against a configured save
    directory (see [Default save directory](#default-save-directory)).

  Automatic game discovery is planned.
- Global flags:
  - `--help`, `-h` — show help. Works on the tool and on any subcommand
    (`fringe-retro set --help`).
  - `--version`, `-V` — print the version.
- Numbers may be written in decimal or hexadecimal (`0x`-prefixed) where noted.

On macOS the path usually contains characters that need quoting, so wrap it in quotes:

```bash
fringe-retro inspect "/Applications/Ultima I™.app/Contents/Resources/game/PLAYER1.U1"
```

---

## Read-only commands

### ✅ `inspect <path>`

Show every field the tool understands, as a labeled table.

```bash
fringe-retro inspect "/…/PLAYER1.U1"
```
```
Name           Enki
Race           Human
Class          Wizard
Strength       12
Gold           100
Food           200
Transport      Walking
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

> Editing `x`/`y` moves the character on the overworld and is best left alone unless you
> know the coordinates. Setting `transport` alone changes the value but not the on-screen
> vehicle icon (a quirk of the game).

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

### Default save directory

So you can type `fringe-retro inspect PLAYER1.U1` instead of the full in-bundle path, the
tool resolves a **bare file name** against a configured directory. Resolution order:

1. `FRINGE_RETRO_SAVE_DIR` environment variable, if set (a quick, per-shell override).
2. `save_dir` under `[games.ultima1]` in `config.toml`.

Paths that are absolute or contain a directory component are always used exactly as given,
so nothing changes if you prefer to pass full paths. If you pass a bare name and no
directory is configured, the tool tells you how to set one.

```bash
fringe-retro inspect PLAYER1.U1          # resolved against save_dir
fringe-retro inspect "/full/path/PLAYER1.U1"   # used as-is
```

### Configuration

Application settings use **TOML**. In Phase 1 the tool reads `config.toml` from the
current working directory (or the path in the `FRINGE_RETRO_CONFIG` environment variable);
a template lives at [config.example.toml](config.example.toml). Copy it to a local
`config.toml` (which is gitignored) and set `save_dir`:

```toml
[games.ultima1]
save_dir = "/Applications/Ultima I™.app/Contents/Resources/game"
```

A fuller configuration system — including a proper per-OS config directory (via the
`directories` crate) — is planned.

---

## 🔷 Planned commands

These are **not implemented yet**; they reflect the direction in [ROADMAP.md](ROADMAP.md).

| Command (tentative) | Purpose |
| --- | --- |
| `fringe-retro` (no arguments) | Launch the interactive **terminal UI** (browse games, characters, saves, and edit visually). The TUI is intended to become the primary interface. |
| `list` / `games` | List discovered games and their save files, so you don't pass paths by hand. |
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
