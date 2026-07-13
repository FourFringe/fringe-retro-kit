# Fringe Retro Kit

> A cross-platform toolkit for exploring, editing, backing up, and preserving classic computer game save files.

Fringe Retro Kit is an open-source terminal application for working with save files from classic computer games.

The project is designed around a simple idea:

> **Users should think about games and characters—not binary file formats.**

The toolkit automatically discovers installed games, browses active save files, maintains a personal Save Library, creates automatic safety backups, and safely edits user-owned save files.

The long-term vision is to support many classic games through a small set of reusable
parsing engines plus per-game schema *data* — with bespoke Rust code only where a format
demands it (e.g. Wasteland's encryption).

---

## Features

- Native Rust application; single binary
- Command-line interface first, with a terminal UI (Ratatui) planned
- Safe editing with automatic backups (unknown bytes preserved)
- Cross-platform aim: macOS first, Windows and Linux later
- Data-driven game definitions planned (reusable parsers + per-game schema data; simple formats become user-authorable)
- Community-friendly, MIT-licensed architecture

---

## Planned Game Support

Implemented:

- Ultima I
- Ultima II
- Ultima III
- Ultima IV
- Ultima V

In progress:

- Wasteland (save decryption done; character records being mapped)

Next up:

- Ultima VI

Candidates (owned or of interest; may or may not be reached):

- Magic Carpet / Magic Carpet 2
- Bard's Tale Trilogy (remaster)
- SSI Gold Box games
- Might & Magic 3–5 (World of Xeen)
- Dungeon Master
- Eye of the Beholder
- Daggerfall
- Wizardry
- Bard's Tale (original)

Deferred (no test machine): Fallout 1 & 2 — Windows-only from the current setup.

Byte-level format notes for implemented games live in
[docs/formats/](docs/formats/README.md). Support for additional games should grow through
community contributions.

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

- [COMMANDS.md](COMMANDS.md) — the command reference (what the tool can do today)
- [docs/formats/](docs/formats/README.md) — byte-level save-format documentation (incl. original Ultima II research)
- [ARCHITECTURE.md](ARCHITECTURE.md) — decisions we've committed to
- [ROADMAP.md](ROADMAP.md) — what's planned but not yet built or decided
- [PHASE-1-ULTIMA-I.md](PHASE-1-ULTIMA-I.md) — the first milestone (complete)

---

## License

MIT
