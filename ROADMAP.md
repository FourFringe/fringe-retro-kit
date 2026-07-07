# Fringe Retro Kit Roadmap

This roadmap is intentionally conservative.

The goal is to solve one problem well before expanding.

---

# Phase 1 — Foundation

Project setup

- [ ] Repository
- [ ] CI
- [ ] Rust workspace
- [ ] Ratatui skeleton
- [ ] Configuration system
- [ ] Logging
- [ ] Error handling

---

# Phase 2 — Binary Engine

- [ ] Binary reader
- [ ] Binary writer
- [ ] Generic field model
- [ ] Schema loader
- [ ] Validation
- [ ] Preserve unknown bytes

---

# Phase 3 — Ultima I MVP

- [ ] Parse PLAYER save
- [ ] Display fields
- [ ] Edit values
- [ ] Save changes
- [ ] Automatic backups

This is the first major milestone.

---

# Phase 4 — Save Browser

- [ ] Game list
- [ ] Character list
- [ ] Save discovery
- [ ] Backup browser

---

# Phase 5 — Save Management

## Automatic Backups

- [ ] Timestamped backups
- [ ] Browse backups
- [ ] Restore backups
- [ ] Configurable backup retention

## Save Library

- [ ] Configurable library location
- [ ] Named snapshots
- [ ] Notes
- [ ] Restore into active game
- [ ] Duplicate
- [ ] Rename
- [ ] Delete
- [ ] Cloud-friendly storage

The Save Library should become the canonical place for users to preserve memorable moments from long-running games. Beyond editing individual save files, Fringe Retro Kit should help users organize and preserve their game history.

The toolkit should manage a user-configurable Save Library that stores named snapshots independently of the game's active save directory.

The Save Library should be treated as the user's permanent collection rather than a temporary backup location.

---

# Phase 6 — Platform Integration

- [ ] GOG detection
- [ ] DOSBox detection
- [ ] Steam detection (if possible)
- [ ] Manual path configuration

---

# Phase 7 — Additional Games

Potential candidates:

- Ultima II
- Ultima III
- Wasteland
- Bard's Tale
- Wizardry

Only after the architecture has proven itself.

---

## Goals

- [ ] User-configurable Save Library location
- [ ] Support local folders and cloud-synchronized directories (Dropbox, Google Drive, OneDrive, iCloud Drive, etc.)
- [ ] Name and annotate save snapshots
- [ ] Browse archived saves by game and character
- [ ] Restore archived saves directly into the game's save directory
- [ ] Preserve metadata such as creation date, last played date, and optional notes
- [ ] Prevent accidental overwrites during restore

Example workflow:

```
Ultima I

Character
    Lord British

Library

    New Character
    Before Time Machine
    Entering Dungeon
    Endgame
```

Selecting an entry should allow actions such as:

- View
- Edit
- Restore
- Duplicate
- Rename
- Delete

The application should handle copying files between the Save Library and the active game installation automatically.

Users should not need to manually manipulate save files within operating system file managers.

## Configuration

The Save Library location should be configurable.

Examples:

```
~/Documents/Fringe Retro Kit/

~/Dropbox/Retro Saves/

~/Google Drive/Retro Saves/

D:\Games\Retro Saves\
```

This allows users to synchronize their save collections across multiple computers using their preferred cloud storage provider.

The application should avoid making assumptions about synchronization software and simply treat the configured location as ordinary storage.

## Long-Term Vision

Over time, the Save Library should become the central hub for managing a player's game history.

Possible future capabilities include:

- search
- tags
- favorites
- screenshots
- play history
- notes
- save comparison
- duplicate detection
- import/export

---

# Future Ideas

These are intentionally **not** commitments.

Possible additions:

- Save diff viewer
- Binary inspector
- JSON export/import
- Save history
- Plugin system
- Desktop GUI
- Checksum verification
- Steam Cloud awareness
- Batch editing
- Schema validator
