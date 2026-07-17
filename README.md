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
- A command-line interface **and** an interactive terminal UI (Ratatui)
- Automatic game discovery (GOG + Steam on macOS) plus a simple game library manifest
- Inspect and edit character sheets field-by-field; the TUI adds a section-grouped editor
- Safe editing with automatic backups (unknown bytes preserved; writes are byte-faithful)
- A curated Save Library alongside automatic backup retention
- A local **world-map browser** (`fringe-retro-map`): bake a game's world maps into web tiles and explore them in your browser, with your party's live position (Ultima I–IV)
- Cross-platform: fully tested on macOS; Linux and Windows binaries published as built-but-untested
- Data-driven game definitions planned (reusable parsers + per-game schema data; simple formats become user-authorable)
- Community-friendly, MIT-licensed architecture

---

## Installation

Every release publishes binaries for **macOS** (Apple Silicon + Intel), **Linux** (x86_64), and
**Windows** (x86_64).

> ⚠️ **Platform support caveat:** macOS is the only platform actively tested against real save
> files. Linux and Windows binaries are built from the same source and pass CI, but are otherwise
> **untested** — use them at your own risk, and please report any problems on the
> [issue tracker](https://github.com/FourFringe/fringe-retro-kit/issues). No guarantees.

**Homebrew (macOS):**

```sh
brew install FourFringe/tap/fringe-retro
```

**Install script (macOS + Linux)** — downloads the latest release binary, verifies its
checksum, and installs to `~/.local/bin`:

```sh
curl -fsSL https://raw.githubusercontent.com/FourFringe/fringe-retro-kit/main/packaging/install.sh | sh
```

Options can be passed after `-s --`, e.g. a specific version or install directory:

```sh
curl -fsSL https://raw.githubusercontent.com/FourFringe/fringe-retro-kit/main/packaging/install.sh | sh -s -- --version v0.2.0 --bin-dir ~/bin
```

**Windows:** download the `.zip` for `x86_64-pc-windows-msvc` from the
[latest release](https://github.com/FourFringe/fringe-retro-kit/releases/latest), extract it,
and put `fringe-retro.exe` and `fringe-retro-map.exe` somewhere on your `PATH`.

**From source** (any platform with a Rust toolchain):

```sh
cargo install --path crates/cli   # fringe-retro (save-file tools)
cargo install --path crates/map   # fringe-retro-map (world-map browser)
```

---

## Map browser

Alongside the save-file tools, **`fringe-retro-map`** bakes a game's world maps into web tiles
and serves them in your browser — pan and zoom a full overworld, browse its towns, and see your
party's current position. It reads the same `config.toml` as the main tool (see
[`config.example.toml`](config.example.toml)): each game's `save_dir`, plus a `[map] export_dir`
for the baked tiles.

```sh
# Bake a game's maps into the export directory (Ultima I–IV today):
fringe-retro-map export --game ultima2

# Serve every baked map and open it in your browser:
fringe-retro-map serve --open
```

The listing groups each overworld with its towns and marks villages, towns, towers, castles, and
dungeons. When a game's save is present, your party's position is shown and updates live as you
play. Everything runs locally — no internet required, and no game assets are copied or
redistributed.

---

## Planned Game Support

Implemented:

- Ultima I
- Ultima II
- Ultima III
- Ultima IV
- Ultima V
- Ultima VI
- Wasteland

In progress:


Next up:

- Magic Carpet / Magic Carpet 2
- Bard's Tale Trilogy (remaster)

Candidates (owned or of interest; may or may not be reached):

- SSI Gold Box games
- Might & Magic 3–5 (World of Xeen)
- Dungeon Master
- Eye of the Beholder
- Daggerfall
- Wizardry
- Bard's Tale (original)

Deferred (no test machine): Fallout 1 & 2 — Windows-only from the current setup.

Byte-level format notes for implemented games — character **saves**, world **maps**, and
**tile graphics** — live in [docs/formats/](docs/formats/README.md). Support for additional
games should grow through community contributions.

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
- [docs/formats/](docs/formats/README.md) — byte-level file-format documentation: character
  saves, world maps, and tile graphics (incl. original Ultima II research)
- [ARCHITECTURE.md](ARCHITECTURE.md) — decisions we've committed to
- [ROADMAP.md](ROADMAP.md) — what's planned but not yet built or decided
- [PHASE-1-ULTIMA-I.md](PHASE-1-ULTIMA-I.md) — the first milestone (complete)

---

## License

MIT
