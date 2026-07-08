# Fringe Retro Kit — Roadmap

> What we haven't built or decided yet. Committed decisions live in
> [ARCHITECTURE.md](ARCHITECTURE.md); the current concrete target is
> [PHASE-1-ULTIMA-I.md](PHASE-1-ULTIMA-I.md).

Conservative by design: solve one problem well before expanding.

> **Sequencing:** the first milestone **hardcodes** Ultima I in a CLI tool. The generic
> engine, the TUI, and CI are deliberately deferred; early development is CLI-only and
> local-only (macOS).

---

## Phase 1 — Foundation + Hardcoded Ultima I MVP

Detailed plan: **[PHASE-1-ULTIMA-I.md](PHASE-1-ULTIMA-I.md)**.

- [x] Cargo workspace (`crates/core` library + `crates/cli` binary)
- [x] Error handling (`thiserror` + `anyhow`)
- [ ] Logging to file (via `tracing`) — deferred, not yet wired up
- [x] Parse `PLAYER*.U1` (fixed offsets; hand-rolled little-endian reads — `binrw` proved unnecessary for this simple layout)
- [x] Inspect / display fields (read-only CLI: `inspect` / `get` / `dump`)
- [x] Edit values, validated (`set`)
- [x] Save changes (atomic write; unknown bytes preserved)
- [x] Automatic timestamped backups (`backup` / `backups` / `restore`)
- [x] Default save path via `config.toml` (`save_dir`) + env override
- [x] In-game validation — an edited save loads correctly in Ultima I (no checksum)

**Status: complete** — the first major milestone is met.

---

## Phase 2 — Generic Binary / Schema Engine

Extracted only **after** 2–3 games have been hardcoded, so the abstraction is earned.

- [ ] Generic binary reader / writer
- [ ] Generic field model (integers, enums, strings, arrays, bitfields, structures)
- [ ] Generic "preserve unknown bytes"
- [ ] Validation
- [ ] Schema loader
- [ ] Embedded official schemas (compiled into the binary) **plus** user schemas loaded
      from a per-OS config directory (no recompile required)

**Open decision — schema / config format.** The requirement: users can easily find,
read, and add their own game configs. Leading candidates are **YAML** (friendliest for
hand-writing field tables) for game schemas and **TOML** (native to Rust) for app
settings. Note the original `serde_yaml` crate is unmaintained, so we'd use a maintained
fork. This is direction only — the format is not locked. A schema might conceptually look
like:

```yaml
game:
  name: Ultima I

saveFiles:
  - player*.u1

fields:
  strength:
    type: i16le
    offset: 0x18
    label: Strength

  transport:
    type: enum
    offset: 0x30
```

---

## Phase 3 — Save Browser & Inspector (TUI)

The TUI (Ratatui + Crossterm) becomes the primary interface here.

- [ ] Game / character / save browsing
- [ ] Inspector: read-only view of known fields
- [ ] Auto-generated editors (number / enum / boolean widgets) to minimize per-game UI code
- [ ] Backup browser

Illustrative mockups (not final):

```
Browser                         Inspector

Games                           Strength         42
▶ Ultima I                      Agility          35
                                Gold         12,450
Character                       Food          9,999
▶ Lord British                  Transport     Aircar

Current Save
player0.u1                      Editors
                                  Strength     [ 42 ]
Backups                           Transport    < Aircar >
2026-07-06 14:22                  Time Machine [✓]
2026-07-06 14:05
```

---

## Phase 4 — Save Management

### Automatic backups

- [ ] Browse backups
- [ ] Restore backups
- [ ] Configurable retention

### Save Library

The user's permanent, curated collection — distinct from automatic backups, and intended
to become the central hub for managing a player's game history. The application handles
copying files between the library and the active save directory automatically; users
should never manipulate save files in a file manager.

- [ ] Configurable location — local **or** cloud-synced (Dropbox, Google Drive, OneDrive,
      iCloud Drive); treated as ordinary storage, no assumptions about sync software
- [ ] Named snapshots with notes and metadata (created date, last played)
- [ ] Browse archived saves by game and character
- [ ] Restore into the active game, with overwrite protection
- [ ] Duplicate / rename / delete

Example workflow and configurable locations:

```
Ultima I  ›  Lord British  ›  Library

    New Character
    Before Time Machine
    Entering Dungeon
    Endgame

Actions: View · Edit · Restore · Duplicate · Rename · Delete

Library location examples:
    ~/Documents/Fringe Retro Kit/
    ~/Dropbox/Retro Saves/
    D:\Games\Retro Saves\
```

---

## Phase 5 — Platform Integration

- [ ] Proper per-OS path handling (`directories` crate)
- [ ] GOG detection · DOSBox detection · Steam detection (if feasible) · manual path override
- [ ] Windows & Linux support
- [ ] GitHub Actions CI: build/test matrix across macOS / Windows / Linux
- [ ] Release automation (`dist`) with GitHub Releases + a Homebrew tap

---

## Phase 6 — Additional Games

Only after the architecture has proven itself. Candidate games are listed in the
[README](README.md).

---

## Future ideas (not commitments)

- Search, tags, favorites
- Screenshots, play history
- Save diff / comparison, duplicate detection
- JSON import / export
- Checksum verification
- Steam Cloud awareness
- Batch editing
- Plugin system
- Desktop GUI
- Schema validator
