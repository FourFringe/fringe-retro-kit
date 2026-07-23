# Fringe Retro Kit — Command Reference

Fringe Retro Kit ships **three binaries**, each with its own focused command reference:

| Binary | What it does | Reference |
| --- | --- | --- |
| **`fringe-retro`** | Inspect, edit, back up, and manage save files — a command-line interface **and** an interactive terminal UI. The primary tool. | [docs/commands/fringe-retro.md](docs/commands/fringe-retro.md) |
| **`fringe-retro-map`** | Bake a game's world maps into web tiles and browse them locally, with your party's live position. | [docs/commands/fringe-retro-map.md](docs/commands/fringe-retro-map.md) |
| **`fringe-retro-kit`** | The reverse-engineering workbench — codec lab, string ripper, schema explorer, live logger, and container carver — for mapping new save formats. | [docs/commands/fringe-retro-kit.md](docs/commands/fringe-retro-kit.md) |

> **Current status:** supports **Ultima I–VI**, **Wasteland**, and **The Bard's Tale Trilogy**,
> with automatic game discovery, a Save Library, and automatic backups. The game is auto-detected
> from each save file, so the same `fringe-retro` commands work across all of them. Planned
> features are tracked in [ROADMAP.md](ROADMAP.md).

Legend used throughout the per-binary references: ✅ implemented · 🔷 planned (not yet available).

---

## Which tool do I want?

- **Editing a character, or managing saves and backups** → [`fringe-retro`](docs/commands/fringe-retro.md).
- **Seeing the world map for a playthrough** → [`fringe-retro-map`](docs/commands/fringe-retro-map.md).
- **Figuring out an unknown / undocumented save format** → [`fringe-retro-kit`](docs/commands/fringe-retro-kit.md).

## Conventions (all binaries)

```
<binary> <command> [arguments] [--flags]
```

- `--help`, `-h` — show help; works on the tool and on any subcommand.
- `--version`, `-V` — print the version.
- Numbers may be written in decimal or hexadecimal (`0x`-prefixed) where noted.
- On macOS a save path usually contains characters that need quoting, so wrap paths in quotes.

For `fringe-retro`, a `<target>` selects the save to act on — either a **game identifier** from
your library manifest (e.g. `ultima2`, optionally `ultima3:PARTY.ULT`) or a **path** to a save
file. See the [`fringe-retro` reference](docs/commands/fringe-retro.md#conventions) for details.

## Related documentation

- [ARCHITECTURE.md](ARCHITECTURE.md) — the decisions and system shape.
- [ROADMAP.md](ROADMAP.md) — what's planned but not yet built.
- [docs/formats/](docs/formats/README.md) — byte-level file-format documentation.
- [docs/templates.md](docs/templates.md) — character template files.

## Safety

Every `fringe-retro` edit backs up first, writes atomically, preserves unknown bytes, and
validates before writing; the `fringe-retro-kit` tools are read-only except where you pass an
explicit output path. Full details live in each binary's reference.
