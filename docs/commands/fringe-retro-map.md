# `fringe-retro-map` — Command Reference

> Part of the [Fringe Retro Kit command reference](../../COMMANDS.md). See also the save editor
> ([`fringe-retro`](fringe-retro.md)) and the reverse-engineering workbench
> ([`fringe-retro-kit`](fringe-retro-kit.md)).

**`fringe-retro-map`** renders a game's world maps — using the real in-game graphics — into a
zoomable web tile bundle, then **serves** it in your browser. It **bakes** offline (all the
proprietary-format work happens once) and serves inert PNG + JSON, so the viewer stays dumb and
game-agnostic. It reads the same `config.toml` as `fringe-retro`
([`config.example.toml`](../../config.example.toml)): each game's `save_dir` for the input data,
plus a `[map]` `export_dir` where baked tiles are written.

Currently supported: **Ultima I**, **Ultima II**, **Ultima III**, **Ultima IV**, **Ultima V**,
and **Wasteland**. More worlds and games are planned; see [ROADMAP.md](../../ROADMAP.md), Phase 8.

Legend: ✅ implemented · 🔷 planned (not yet available)

---

## Quick start (via `just`)

With [`just`](https://github.com/casey/just) installed, one step bakes every supported game and
serves them (`just map ultima2` for a single game):

```bash
just map-export    # bake the configured game map(s) into the export dir
just map-serve     # serve the maps and open your browser
just map           # do both: export, then serve
```

## Configuration

Add a `[map]` table to `config.toml` pointing at a persistent folder for the baked maps
(`~` expands to your home directory):

```toml
[map]
export_dir = "~/Documents/Fringe Retro Kit/maps"
```

With that in place, the input game directory is taken from each game's `save_dir`, and the output
from `export_dir`, so the commands below need no paths.

---

## ✅ `export [--game <id>]`

Bake a game's world map(s) into a tile bundle under the export directory.

```
fringe-retro-map export [--game <id>] [--input <dir>] [--out <root>] [--png <file>]
```

| Flag | Default | Purpose |
| --- | --- | --- |
| `--game` | `ultima1` | Which game to export (`ultima1`–`ultima5`, `wasteland`). |
| `--input` | the game's `save_dir` from `config.toml` | Directory holding the game's data (e.g. `MAP.BIN`). |
| `--out`, `-o` | `[map] export_dir` from `config.toml` | Export root; the bundle lands in `<out>/<game>/<world>/`. |
| `--png <file>` | — | Also write the first world's flat composite image (handy for debugging). |

What each game bakes:

- **Ultima I** — one overworld (Sosaria).
- **Ultima II** — every overworld and town (dungeons and other non-tile maps are skipped).
- **Ultima III** — Sosaria plus its named towns and castles.
- **Ultima IV** — the 256×256 Britannia overworld plus its towns, villages and castles.
- **Ultima V** — the 256×256 Britannia surface and the Underworld, plus every town, dwelling,
  castle and keep (one map per floor).
- **Wasteland** — all 42 maps (the desert overworld plus every town and building) from the
  pristine `MASTER1`/`MASTER2` disks, with clickable overworld↔sub-map navigation.

```bash
# Fully config-driven (input + output resolved from config.toml):
fringe-retro-map export --game ultima1

# Explicit paths (no config needed), plus a debug composite PNG:
fringe-retro-map export \
  --game ultima1 \
  --input "/Applications/Ultima I™.app/Contents/Resources/game" \
  --out ~/maps \
  --png /tmp/ultima1-overworld.png
```

Each bundle is `<out>/<game>/<world>/` containing a `manifest.json` and a `z/x/y` PNG tile
pyramid. Re-running `export` overwrites the bundle in place; the export dir is persistent and
never auto-cleaned.

## ✅ `serve [--open]`

Serve the exported map bundles in a local web browser. A single server spans every game you've
baked into the export directory.

```
fringe-retro-map serve [--root <dir>] [--port <n>] [--open]
```

| Flag | Default | Purpose |
| --- | --- | --- |
| `--root`, `-r` | `[map] export_dir` from `config.toml` | Directory of baked bundles to serve. |
| `--port <n>` | `8737` | Port to listen on. |
| `--open` | off | Open the map browser in your default browser once the server is up. |

```bash
# Serve the configured export dir and open a browser:
fringe-retro-map serve --open

# Serve a specific folder on a custom port:
fringe-retro-map serve --root ~/maps --port 9000
```

The server prints its address (default `http://127.0.0.1:8737`). The landing page is a table of
contents generated from whatever bundles it finds — every game grouped with its worlds, and each
overworld's towns nested beneath it. The viewer is offline-friendly: Leaflet is served locally,
so no internet connection is needed.

The overworld view also shows **named landmark markers** — towns, castles, monuments, villages,
towers, and dungeons (toggleable in the top-right control), read from the game's own location
table so each marker shows its real name (e.g. *The Castle of Lord British*, *The Dungeon of
Perinia*). A **"you are here"** marker tracks the party's current position, read live from the
save file: the server watches the save and pushes updates over Server-Sent Events, so the marker
moves the moment you save in-game. Everything is served from `http://127.0.0.1` — no internet is
required, and no game assets are copied or redistributed.
