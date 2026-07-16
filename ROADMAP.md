# Fringe Retro Kit — Roadmap

> What we haven't built or decided yet. Committed decisions live in
> [ARCHITECTURE.md](ARCHITECTURE.md); what the tool can do today is in
> [COMMANDS.md](COMMANDS.md).

Conservative by design: solve one problem well before expanding.

---

## Current status & near-term plan

Phases 1–6 are complete. **Seven games** — Ultima I–VI and Wasteland — are
readable and editable through both the CLI and the interactive TUI: a section-grouped
editor with an enum picker, automatic backups + on-demand snapshots, a curated Save
Library, character templates (with in-app capture), and a per-game save-file chooser.
Games are auto-detected (GOG + Steam on macOS), CI runs on macOS/Ubuntu/Windows, and
tagged releases publish macOS, Linux, and Windows binaries (Homebrew tap + `curl | sh`
installer).

Most recently: Ultima VI (party stats in the uncompressed `OBJLIST`), Wasteland character
sheets + skills (byte-faithful MSQ writes), GOG/Steam detection with `detect --all`, and
per-game save-directory resolution from a natural top-level `save_dir`.

**Next up:** the **map browser (Phase 8)** is the active track — zoomable, real-graphics world
maps baked from your own game install, **starting with Ultima I** and following the games in
play order. Behind it sit optional, non-blocking tracks: **Phase 7 (Additional Games)**,
**Phase 9 (Kit Tools)** — CLI reverse-engineering helpers — and **Phase 10 (Engine
Enhancements)**. The parsing engine, distribution (Homebrew tap, `curl | sh` installer,
cross-platform binaries), and save diff / comparison are all done (see below).

---

## Save diff / comparison ✅

`fringe-retro diff <a> [<b>]` shows what changed between two saves in **game terms** (fields,
not raw bytes), e.g. `Avatar strength 15 → 30`, `Party karma 75 → 99`. With one argument it
compares a save against its most recent automatic backup.

- Built on the editor's field model (`Session` entities + rows), so every supported game gets
  it for free; dynamic fields (Wasteland skills) diff correctly, including newly-learned ones.
- Falls back to a byte-range diff when the two files aren't the same known game.
- In the TUI: the backup browser's preview leads with a "changes since this backup" diff above
  the backup's full contents, and the restore confirmation previews "restoring will change …"
  before you commit.
- Save Library snapshot previews likewise lead with a per-file diff against the current save
  (all of a snapshot's files — e.g. Ultima III's `ROSTER.ULT` and `PARTY.ULT`) above the full
  contents, reusing the same preview pane (no extra UI).

---

## Phase 1 — Foundation + Hardcoded Ultima I MVP

Detailed plan: **[PHASE-1-ULTIMA-I.md](PHASE-1-ULTIMA-I.md)**.

- [x] Cargo workspace (`crates/core` library + `crates/cli` binary)
- [x] Error handling (`thiserror` + `anyhow`)
- [x] Parse `PLAYER*.U1` (fixed offsets; hand-rolled little-endian reads — `binrw` proved unnecessary for this simple layout)
- [x] Inspect / display fields (read-only CLI: `inspect` / `get` / `dump`)
- [x] Edit values, validated (`set`)
- [x] Save changes (atomic write; unknown bytes preserved)
- [x] Automatic timestamped backups (`backup` / `backups` / `restore`)
- [x] Default save path via `config.toml` (`save_dir`) + env override
- [x] In-game validation — an edited save loads correctly in Ultima I (no checksum)

**Status: complete** — the first major milestone is met.

---

## Phase 2 — Game Library Manifest & Identifiers

**Status: complete.** Users describe **which games they own, on which platform, and where**,
then refer to games by short identifiers instead of raw file paths.

- [x] App-level config (TOML) listing enabled games, each with: identifier (e.g. `ultima1`),
      platform (GOG / Steam / DOSBox / manual), install path, and save location
- [x] Resolve `fringe-retro inspect ultima1` (identifier → game + save path) instead of a path
- [x] Enable/disable owned games (don't surface Wasteland options if you don't own it)
- [x] Optional Save Library location (local or cloud-synced) — see Phase 5
- [x] Generalizes a single-game `config.toml` (`save_dir`) into a multi-game manifest, with
      per-game save-directory resolution (e.g. Ultima VI's `SAVEGAME`, Wasteland's slot)

This is deliberately **game-agnostic**: it manages a player's library and never touches
save-file parsing. Automatic platform *detection* (auto-filling these paths) is deferred to
Phase 6; the manifest is the manual foundation it will later populate.

---

## Phase 3 — Parsing Engines & Per-Game Schemas

**Status: complete.** Extracted only *after* several games were hardcoded, so the abstraction
was earned. The hand-mapped games showed that parsing splits into two layers, and the
architecture follows that seam (see [ARCHITECTURE.md](ARCHITECTURE.md)):

- **Codec / container layer (Rust code).** Encryption, checksums, block scanning,
  save-as-directory, exotic string encodings — small, per-*family* code (e.g. Wasteland's MSQ
  cipher). Today this is inlined in each game module; factoring it into a reusable pipeline is
  a future refinement (see Phase 10).
- **Field-schema layer (data).** Fixed-offset fields over a plaintext buffer: integers
  (LE/BE), scaled ints, BCD, enums (numeric or ASCII-letter), bitfields, booleans, names, and
  record arrays. The Ultimas are essentially 100% this layer.

Delivered:

- [x] Generic field-schema engine ([crates/core/src/schema.rs](crates/core/src/schema.rs)):
      `Field` + `FieldKind` (Name, Int LE/BE, Scaled, Bcd, Byte, Bool, Enum, Letter, Bitfield)
      over a `&[u8]` buffer at a base offset, preserving unknown bytes by construction and
      validating on write
- [x] A record model that covers a single record (Ultima I/II), an array of records (Ultima
      III's roster), a header-plus-members file (Ultima III's party), and column-array stats
      (Ultima VI's `OBJLIST`) with the same primitives
- [x] **All seven games run on the engine** — Ultima I–VI and Wasteland express their save
      layout as `Field` tables; Wasteland layers its MSQ decrypt/encrypt + block checksum in
      front of the schema
- [x] String handling as schema field kinds (null-terminated ASCII names, BCD digits)

Remaining engine work — data-driven/user-authorable schemas, a reusable codec pipeline, and
the schema-file-format decision — has moved to **Phase 10 (Engine Enhancements)**. Known byte
layouts for implemented games live in [docs/formats/](docs/formats/README.md).

---

## Phase 4 — Save Browser & Inspector (TUI)

The TUI (Ratatui + Crossterm) becomes the primary interface here. Launched by running
`fringe-retro` with no command.

- [x] Game / save browsing (games list) and per-character drill-down (Ultima III roster slots & party members)
- [x] Inspector: read-only view of known fields (scrollable, paged; shares formatting with `inspect`)
- [x] Auto-generated editors driven by the field schema (text / enum / boolean fields) — batch edits in memory, validated on commit, one backup + write on save, with an unsaved-changes guard
- [x] Backup browser — list a save's backups with a decoded preview, restore with confirmation (auto safety backup, no-op when already identical); snapshot the current save on demand
- [x] Character templates — apply saved sets of field values to a character (`templates.toml`), validated up front and applied as ordinary in-memory edits; capture new templates from a character in the editor

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

## Phase 5 — Save Management

Detailed plan: **[PHASE-5-SAVE-LIBRARY.md](PHASE-5-SAVE-LIBRARY.md)**.

### Automatic backups

- [x] Browse backups (TUI backup browser)
- [x] Restore backups (CLI + TUI)
- [x] Configurable retention (`[backups] keep` / `max_age_days`; `backups --prune`)

### Save Library

The user's permanent, curated collection — distinct from automatic backups, and intended
to become the central hub for managing a player's game history. The application handles
copying files between the library and the active save directory automatically; users
should never manipulate save files in a file manager.

- [x] Configurable location — local **or** cloud-synced (Dropbox, Google Drive, OneDrive,
      iCloud Drive); treated as ordinary storage, no assumptions about sync software
- [x] Named snapshots with notes and metadata (created date, "last updated" from file times)
- [x] Browse archived saves by game (CLI `library list`)
- [x] Restore into the active game, with overwrite protection (CLI `library restore`)
- [x] Duplicate / rename / delete (CLI `library duplicate` / `rename` / `delete`)

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

## Phase 6 — Platform Integration

- [x] GOG (macOS) + Steam (macOS) detection + `detect --write` to append found games to the
      config, or `[detect] auto` to fold them in at runtime · Windows/Linux deferred (need
      machines to test on) · manual path override still works
- [x] `detect --all` also lists recognized-but-unsupported games (Ultima VI, Bard's Tale
      Trilogy, Magic Carpet 1/2) in a separate section, pointing at the issue tracker for
      feature requests · display-only (never written to config / auto-detected)
- [x] Windows & Linux support (builds published as built-but-untested binaries; still no
      machines to exercise them against real saves — please report issues)
- [x] GitHub Actions CI: fmt + clippy + test matrix across macOS / Ubuntu / Windows
- [x] Release automation: tag-triggered builds for **macOS** (Apple Silicon + Intel),
      **Linux** (x86_64), and **Windows** (x86_64) → GitHub Releases with tarballs/zips +
      SHA-256 checksums. Only macOS is tested against real save files; Linux/Windows are
      built-but-untested (see the README caveat).
- [x] **Homebrew tap** (`FourFringe/homebrew-tap`): a binary formula installs the pre-built
      release binary (`brew install FourFringe/tap/fringe-retro`); the release workflow
      renders `packaging/homebrew/fringe-retro.rb` with the new version + checksums and pushes
      it to the tap (needs a `HOMEBREW_TAP_TOKEN` secret).
- [x] **`curl | sh` install script** ([packaging/install.sh](packaging/install.sh)): downloads
      the latest macOS/Linux release binary, verifies its SHA-256, and installs to `~/.local/bin`.
- [x] Publish Windows/Linux binaries (built-but-untested; caveat in the README)

---

## Phase 7 — Additional Games

Only after the architecture has proven itself. Byte layouts we've mapped so far live in
[docs/formats/](docs/formats/README.md).

**Selection heuristic:** prefer games that (a) the maintainers legally own and can test, and
(b) have an actively-maintained **open-source engine reimplementation** — that makes the
save format effectively a published, tested spec.

Grouped by codec complexity (which parsing engine each needs):

- **Done / in progress:** Ultima I ✅, Ultima II ✅, Ultima III ✅, Ultima IV ✅, Ultima V ✅,
  Ultima VI ✅ (party stats + names in `OBJLIST`, byte-faithful; map objects / inventory /
  spells not yet exposed), Wasteland ✅ (MSQ cipher + block scan + character sheets +
  skills, byte-faithful writes; items not yet exposed).
- **Easy extensions — same family as the Ultimas:** Ultima VI (`Nuvie`) ✅ — character stats
  are flat arrays in the uncompressed `OBJLIST` (done). The object-based, LZW-compressed map
  data (party-as-objects across ~70 files) remains unhandled, but isn't needed for editing
  character sheets.
- **Candidates (owned & installed, to investigate after Wasteland):**
  - **Magic Carpet 1 & 2** (Bullfrog, DOS via GOG/DOSBox). Saves live inside the game
    directory — `…\SAVE` for Magic Carpet, `…\GAME\NETHERW\SAVE` for Magic Carpet 2.
    **No published byte-level save-format spec found** (PCGamingWiki documents only the
    locations, and there's no known open-source reimplementation) — a from-scratch
    reverse-engineering target, and these are action games so the "save" is world/mission
    state, not a character sheet.
  - **Bard's Tale Trilogy** remaster (Krome / inXile, **Unity 2017.4**). Saves live in Unity's
    persistent-data folder (`%USERPROFILE%\AppData\LocalLow\InXile Entertainment\The Bard's
    Tale Trilogy\saves`; the macOS equivalent lives under `~/Library`), with **only a single
    autosave shared across all three games**. Closed-source Unity and **no published format
    spec** — almost certainly a Unity-serialized blob (binary or JSON) that would need RE.
    Distinct from the 1985 originals.
- **Candidates (kept in the list; may never get to them, and that's fine):** SSI Gold Box,
  Might & Magic 3–5 / World of Xeen (`OpenXeen`), Dungeon Master, Eye of the Beholder,
  Daggerfall (Daggerfall Unity; introduces RLE decompression), Wizardry, original Bard's Tale.
- **Deferred — no test machine:** **Fallout 1 & 2** (owned on Steam; big-endian ints,
  save-as-directory, well RE'd by TeamX / F12se) is Windows-only from this Mac, so it's on
  hold until a Windows system is available.

---

## Phase 8 — Map Browser

**Goal:** generate zoomable, real-graphics world maps from your **own** game install, so you
can see the big picture during a playthrough instead of hand-drawing graph-paper maps.
Delivered as standalone kit binaries (an offline **map exporter** + a small **local server**)
that share `crates/core`. **Starting with Ultima I**, then following the games in play order.

**Tile strategy — composite into a pyramid, not individual tile files.** Game tiles are tiny
(Ultima I's are 16×16 px). We do *not* place one PNG per game tile into an HTML/CSS grid (that
would be thousands of DOM nodes with no real zoom), and we don't hand-pick N×N clusters.
Instead the exporter paints the **whole world into one large composite image** at true tile
positions, then slices that into a standard **256×256 web-tile pyramid** with downsampled zoom
levels. That's exactly what browser map libraries (Leaflet / OpenSeadragon) consume, gives
smooth zoom/pan, and keeps the viewer dumb. (The "5×5 snapshot" instinct is right in spirit —
batch tiles for efficiency — but the batch size is driven by the 256 px pyramid tile, not
chosen by hand.)

**Architecture — bake-then-serve** (all proprietary-format complexity in the offline pass,
none in the viewer):

- **Offline bake (per game).** The exporter reads the game's art via `crates/core`, decodes the
  EGA/CGA tileset → RGBA, composites each world, slices a `z/x/y` PNG tile pyramid, and writes a
  `manifest.json` (world name, dimensions, tile size, zoom levels, optional legend / labels /
  points of interest). Output is inert PNG + JSON.
- **Shared export location.** A configured export root (`[map] export_dir` in `config.toml`)
  collects every game's bundles under `<export_dir>/<game>/<world>/…`. Export whichever games
  you want; they all land here.
- **Local web server (primary delivery).** A small `axum` server points at the export root,
  knows its layout, and **dynamically generates a table of contents across all exported games
  and every world within each** — no hand-maintained index. Serving over `http://localhost`
  also sidesteps the `file://` `fetch()` restriction and leaves room for later interactivity
  (live re-export, overlays, cross-map links). The front-end stays game-agnostic: it renders any
  bundle from its manifest with Leaflet / OpenSeadragon. No Electron; cross-platform for free.
- **Multi-world games** export each world (overworld, towns, dungeons) as its own bundle, listed
  under its game in the generated TOC.
- Baked art is derived from your **own installed game** (the exporter runs locally); we never
  ship game graphics.

**Checklist:**

- [x] Map exporter binary (`crates/map`, bin `fringe-retro-map`)
- [x] EGA tileset decoder → RGBA (Ultima I `EGATILES.BIN`: 16×16, 4 row-interleaved planes)
- [x] Ultima I overworld: decode the nibble-packed `MAP.BIN` grid (168×156) and composite the
      full world to one image (verified against the four-continent Sosaria map)
- [x] Tile-pyramid generator (256×256 `z/x/y` + downsampled zoom levels) + per-world `manifest.json`
- [x] `[map] export_dir` config; bundles written to `<export_dir>/<game>/<world>/…` (input dir
      also resolved from each game's `save_dir`; `just map-export` / `map-serve` wrap it)
- [x] `axum` server (`fringe-retro-map serve`): serves tiles + a dynamically generated
      cross-game / cross-world TOC
- [x] Leaflet viewer: zoom / pan, driven by the manifest (verified in-browser); Leaflet is
      vendored and served locally, so the viewer works offline
- [x] POI overlay: castles / towns / signposts detected from the overworld tiles, baked into
      the manifest, shown as toggleable Leaflet markers (curated place-names TBD)
- [x] Player "you are here" marker: the party's `Map X/Y` from the save (`PLAYER1.U1`, via
      `fringe-retro-core`) served at `/api/position` and shown on the map
- [ ] Live watch: update the player marker when the save changes (poll/notify + push)
- [ ] Curated place-names for POIs (from the game manual / lore) as a data table
- [ ] Extend to the next games in play order (Ultima II → …)

Town/castle **interiors** aren't worth exporting (no standalone layout files — they live in the
executables), and Ultima I's dungeons are first-person and procedurally generated, so there's
no static dungeon map. The overworld is the map.

The early Ultimas have **uncompressed** map data, so this phase needs no decompression codec.
When a compressed-map game arrives (Ultima VI's LZW map; later Daggerfall's RLE), that decoder
is where the **Phase 9 codec workbench** first gets pulled in.

---

## Phase 9 — Kit Tools (CLI dev tools)

The rest of the "kit": focused **researcher/dev-facing** binaries that share `crates/core`
(`fringe-retro-core`) but carry complexity we deliberately keep out of the polished
`fringe-retro` app. These are our own reverse-engineering accelerators for **save formats** (the
map browser is Phase 8) — the kind of binary spelunking done when mapping a new game.

**Design rule:** build them **CLI-first with plain-text / `--json` output** so they're
scriptable, diffable, and usable in automated / AI-assisted sessions. A TUI, if any, is a thin
shell over that text-capable core — never the only interface.

- **Schema explorer / spelunker** — packages the manual RE workflow into repeatable commands,
  and becomes the authoring front-end for the Phase 10 data-driven schema files (it emits what
  the main tool consumes). Core primitives:
  - **Value finder** — locate a known value (e.g. `gold = 12450`) across encodings
    (u16 LE/BE, u24, BCD, ASCII) → candidate offsets.
  - **Guided diff** — "save A = before, save B = after I raised STR 15→30" → highlight the
    changed bytes and infer the encoding from the delta (builds on core's byte `diff`).
  - **Hypothesis overlay** — render a tentative `Field` table over a buffer live, tuning
    offsets until values read sensibly.
  - **Stride/record detector** — autocorrelation to find repeating record sizes (rosters,
    party arrays).
  - **Schema export** — dump a confirmed layout in the eventual schema format.
- **Codec workbench / cipher lab** — decrypt/decompress a blob, dump plaintext, re-encrypt,
  and verify a byte-for-byte round-trip; plus a **checksum solver** that tries candidate
  algorithms against known-good blocks (the carry-fold-vs-negated-sum detective work from
  Wasteland, automated). Reusable for MSQ, U6 LZW, Daggerfall RLE, Fallout, etc. (First earns
  its keep decoding Ultima VI's compressed map in Phase 8.)
- **Live watch / logger** — a research-grade `watch`: monitor a save while playing and log
  timestamped byte deltas, to correlate in-game actions with byte changes. Raw and
  session-oriented, vs. the main tool's player-facing semantic `watch`/`diff`.
- **String ripper** — extract embedded text under multiple encodings (ASCIIZ, packed 5-bit,
  …) to find name/item/spell tables and dialogue — often the fastest way to anchor in an
  unknown file.
- **Archive / container extractor** — list/extract game container files (`.DAT`, GOB, Unity
  assetbundles) for formats where the save isn't a bare file (e.g. Bard's Tale remaster).

Sequencing (earn each abstraction, same discipline as Phase 3): schema explorer first (it
multiplies every future game), then the codec workbench when an encrypted/compressed format
forces it (including Ultima VI's map in Phase 8). The string ripper and archive extractor can
start as explorer subcommands and graduate to their own binaries if they grow.

---

## Phase 10 — Engine Enhancements

Infrastructure and parsing refinements that the current feature set doesn't need but that
would pay off as the game roster and supported platforms grow. **None are blocking** — they're
collected here so they don't get lost inside earlier completed phases.

### Parsing / schema

- [ ] **Data-driven, user-authorable game schemas.** Graduate the in-code `Field` tables to a
      hand-writable text format so simple fixed-layout games can be added without Rust.
      Encrypted/compressed games would still ship as official Rust codecs.
- [ ] **Reusable codec pipeline.** Factor the per-game container logic (currently inlined —
      e.g. Wasteland's MSQ decrypt/encrypt + block checksum) into a `Transform` pipeline in
      front of the schema, with a symmetric write path, so a second MSQ-family game reuses it.
- [ ] **Pluggable string codecs** beyond ASCIIZ/BCD, if a future game needs one (e.g. a packed
      5-bit table).

**Open decision — schema / config format (do NOT lock yet).** The abstraction is now proven as
a Rust type; the remaining question is whether the field schema graduates to a hand-writable
text format. Leading candidates are **YAML** (friendly for field tables) for game schemas and
**TOML** (native to Rust) for app settings (the original `serde_yaml` is unmaintained, so a
maintained fork would be used). A schema might conceptually look like:

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

### Platform / operations

- [ ] Proper per-OS path handling via the `directories` crate (config / data / cache dirs)
- [ ] File logging via `tracing` for diagnostics in bug reports (deferred since Phase 1)

---

## Future ideas (not commitments)

- Search, tags, favorites
- Screenshots, play history
- Duplicate detection
- JSON import / export
- Checksum verification
- Steam Cloud awareness
- Batch editing
- Plugin system
- Desktop GUI
- Schema validator
