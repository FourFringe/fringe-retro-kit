# Fringe Retro Kit

> A cross-platform toolkit for exploring, editing, backing up, and preserving classic computer game save files.

Fringe Retro Kit is an open-source terminal application for working with save files from classic computer games.

The project is designed around a simple idea:

> **Users should think about games and characters—not binary file formats.**

The toolkit automatically discovers installed games, browses active save files, maintains a personal Save Library, creates automatic safety backups, and safely edits user-owned save files.

The long-term vision is to support many classic games through declarative schemas rather than game-specific code.

---

## Features

- Native Rust application; single binary
- Command-line interface first, with a terminal UI (Ratatui) planned
- Safe editing with automatic backups (unknown bytes preserved)
- Cross-platform aim: macOS first, Windows and Linux later
- Data-driven game definitions planned, so new games can be added as data
- Community-friendly, MIT-licensed architecture

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

- [ARCHITECTURE.md](ARCHITECTURE.md) — decisions we've committed to
- [ROADMAP.md](ROADMAP.md) — what's planned but not yet built or decided
- [PHASE-1-ULTIMA-I.md](PHASE-1-ULTIMA-I.md) — the current, concrete development target

---

## License

MIT
