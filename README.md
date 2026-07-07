# Fringe Retro Kit

> A cross-platform toolkit for exploring, editing, backing up, and preserving classic computer game save files.

Fringe Retro Kit is an open-source terminal application for working with save files from classic computer games.

The project is designed around a simple idea:

> **Users should think about games and characters—not binary file formats.**

The toolkit automatically discovers installed games, browses active save files, maintains a personal Save Library, creates automatic safety backups, and safely edits user-owned save files.

The long-term vision is to support many classic games through declarative schemas rather than game-specific code.

---

## Features

- Cross-platform (macOS, Windows, Linux)
- Native Rust application
- Terminal UI (Ratatui)
- Automatic save discovery
- Safe editing with automatic backups
- Embedded official game definitions
- User-extensible YAML schemas
- Community-friendly architecture

---

## Planned Game Support

Initial focus:

- Ultima I

Planned:

- Ultima II
- Ultima III
- Wasteland
- Bard's Tale
- Wizardry
- SSI Gold Box games

Support for additional games should grow through community contributions.

---

## Philosophy

Fringe Retro Kit is intentionally **not**:

- an emulator
- a ROM manager
- a launcher
- a collection of game assets

It exists solely to help users inspect, preserve, and edit their own save files.

---

## Documentation

- ARCHITECTURE.md
- ROADMAP.md

---

## License

MIT
