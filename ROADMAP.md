# Fringe Retro Kit — Roadmap

> The story of the project as a sequence of releases: what has shipped, and what's planned next.
> What the tools do **today** is in [COMMANDS.md](COMMANDS.md); committed design decisions are in
> [ARCHITECTURE.md](ARCHITECTURE.md); and the original phase-by-phase build plan (with the detailed
> per-format reverse-engineering notes) is archived in [ROADMAP-history.md](ROADMAP-history.md).

Conservative by design: solve one problem well before expanding.

**Today:** three binaries over one shared core (`fringe-retro-core`) — the **`fringe-retro`** save
editor (TUI + CLI), the **`fringe-retro-map`** map browser, and the **`fringe-retro-kit`**
reverse-engineering workbench — covering **seven games**, Ultima I–VI and Wasteland.

---

## Shipped

*Oldest first — the evolution of the app.*

### v0.1.0 — Foundation & the save editor (2026-07-14)

The starting point: a generic parsing / schema engine and an interactive terminal save editor.
Section-grouped fields with an enum picker, character templates (editable in-app), automatic
backups with on-demand snapshots and a backup browser, and a curated Save Library. First games
readable and editable — **Ultima I–V** (plus early Ultima II/III) — with a Wasteland stub, game
library config, a default game-directory path, a live save watcher, and CI-built releases.

### v0.2.0 — Detection, diffs, and two more games (2026-07-15)

Game **auto-detection** (installed GOG and Steam apps on macOS, with `--all` to also list
recognized-but-unsupported games), save **diffing** in game terms — "Avatar strength 15 → 30" — in
the CLI, the TUI's backup previews, and Save Library previews, plus **Wasteland** (file parser +
skills) and **basic Ultima VI** support. Added the `justfile`.

### v0.2.1 – v0.2.2 — Cross-platform distribution (2026-07-15)

Published **Linux and Windows** binaries alongside macOS, a `curl | sh` installer, and an
auto-updating **Homebrew tap**, with release-task polish.

### v0.3.0 — The map browser debuts (2026-07-17)

A new **`fringe-retro-map`** binary: a web server serving zoomable **tile pyramids**, an offline
mode, POI overlays (towns / castles / signposts) with real names, and live party-position
tracking from the save. First maps — **Ultima I and II** — with points of interest and current
position; the map binary joined the release bundle.

### v0.4.0 — Maps for Ultima III–V (2026-07-17)

Overworld and sub-map rendering for **Ultima III, IV, and V**, collapsible map lists in the
browser, and a map-code cleanup across all games.

### v0.5.0 — Wasteland maps & the party editor (2026-07-20)

**Wasteland's** 42 maps (tile mapping, map detection and sorting, POIs) and a **party-level
editor** (map location plus an inventory picker). Map POIs became **clickable**, opening their
sub-maps.

### v0.6.0 — The RE workbench & full map POIs (2026-07-22)

The **`fringe-retro-kit`** reverse-engineering workbench: a shared `codec` module, a string
ripper, a schema explorer, a live byte watcher, and an archive / MSQ carver. Alongside it, a wide
map-POI pass driven by the kit — clickable POIs for **Ultima III–V**, dual position markers for
**Ultima V**, the reverse-engineered **Ultima VI** world (the seamless overworld plus five dungeon
levels, object overlays, named towns and shrines, and data-driven dungeon entrances read from the
`GAME.EXE` name table), **Ultima V** shrines, and named, clickable **Ultima II** POIs. Capped with
a cross-game POI audit and this move to a release-based roadmap.

---

## Planned

### v0.7.0 — First-person dungeon maps

Ultima IV and V store their first-person dungeons as fixed tile grids (walls, doors, ladders,
fields) — the same shape as Ultima VI's dungeons, which already render top-down. Reconstruct those
grids into top-down "graph-paper" maps and slot them into the existing dungeon pipeline.

- [ ] **Ultima V dungeons** (`DUNGEON.DAT`) → top-down per-level maps.
- [ ] **Ultima IV dungeons** (fixed tile grids, eight dungeons × eight levels) → same.
- [ ] Stretch: **Ultima III** (2192-byte first-person format), then **Ultima II** (an older
      non-tile format that needs decoding first).

Out of scope: **Ultima I** dungeons are procedurally generated at runtime (no stored maps), so
there is nothing to bake — the overworld entrance markers are the honest representation. **Ultima
VI** dungeons are already rendered.

### Beyond v0.7.0 — ongoing tracks

These advance opportunistically and will feed later releases; none are pinned to a version yet.

**Live-play refinements** — ad hoc fixes that surface only by playing each game.

- [ ] **Confirm tentative fields** — capture before/after saves while playing and use
      `schema find` / `schema diff` to promote `tentative: true` fields to confirmed.
- [ ] **Map POI touch-ups** — verify and adjust names/positions against in-game reality (e.g.
      Ultima VI's curated town coordinates, the Terfin / Bonn's Shack labels, Hythloth's placement).

**Deeper save-editor coverage** — fresh reverse-engineering to widen each editor.

- [ ] **Ultima VI objects / inventory / spells** — extend past party stats and names into the
      object system, using the schema explorer + string ripper.
- [ ] Inventory / item editing for the other games as their formats are mapped.

**Engine & schema** — infrastructure the current feature set doesn't need; safe to defer until
we're bored. Full detail and the open schema-format decision are in
[ROADMAP-history.md](ROADMAP-history.md) under Phase 10.

- [ ] **Data-driven, user-authorable game schemas** — a hand-writable text format so simple
      fixed-layout games can be added without Rust.
- [ ] **Reusable codec pipeline** — compose the shared `core::codec` transforms into a declarative
      per-game pipeline with a symmetric write path.
- [ ] **Pluggable string codecs** — expose the packed 5-bit decoder as a schema `FieldKind`.
- [ ] **Kit follow-ups** — a live hypothesis-overlay renderer and schema export (the latter waits
      on the schema-format decision).
- [ ] Per-OS path handling (`directories`) and file logging (`tracing`).

**New games** — long-term expansion, once the current slate is played through (which will be a
while). Save-format research notes live in [ROADMAP-history.md](ROADMAP-history.md) under Phase 7.

- [ ] **Bard's Tale** — the original trilogy and/or the inXile / Krome remaster (Unity saves).
- [ ] **Later Ultimas** — Ultima VII–IX, and **Ultima Underworld 1–2**.
- [ ] Others in the same lineage: **Wizardry**, **Might & Magic / World of Xeen**, and
      **Daggerfall** (which would introduce RLE decompression).
