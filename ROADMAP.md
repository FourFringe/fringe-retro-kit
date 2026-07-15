# Fringe Retro Kit — Roadmap

> What we haven't built or decided yet. Committed decisions live in
> [ARCHITECTURE.md](ARCHITECTURE.md); what the tool can do today is in
> [COMMANDS.md](COMMANDS.md).

Conservative by design: solve one problem well before expanding.

---

## Current status & near-term plan

Phases 1–6 are effectively complete. **Seven games** — Ultima I–VI and Wasteland — are
readable and editable through both the CLI and the interactive TUI: a section-grouped
editor with an enum picker, automatic backups + on-demand snapshots, a curated Save
Library, character templates (with in-app capture), and a per-game save-file chooser.
Games are auto-detected (GOG + Steam on macOS), CI runs on macOS/Ubuntu/Windows, and
tagged releases publish macOS binaries.

Most recently: Ultima VI (party stats in the uncompressed `OBJLIST`), Wasteland character
sheets + skills (byte-faithful MSQ writes), GOG/Steam detection with `detect --all`, and
per-game save-directory resolution from a natural top-level `save_dir`.

**Next up:** deepening existing games (Wasteland items, Ultima VI inventory/spells) or
distribution polish (`cargo-dist` installers, a Homebrew tap). Save diff / comparison is
done (see below).

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
- [ ] **Save Library snapshot diffs** (later) — compare a Library snapshot against the current
  save (or another snapshot). More involved than a backup diff because a snapshot is a whole
  save-*set* (a directory, possibly several files for Wasteland / Ultima VI), so it needs to
  pick each game's primary file to diff, and probably its own TUI view.

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

Extracted only **after** several games are hardcoded, so the abstraction is earned. The four
hand-mapped games (Ultima I/II/III, Wasteland) showed that parsing splits into two layers,
and the architecture follows that seam (see
[ARCHITECTURE.md](ARCHITECTURE.md)):

- **Codec / container layer (Rust code).** Encryption, compression, checksums, block
  scanning, save-as-directory, exotic string encodings. Small and reusable per *family*
  (e.g. the Wasteland MSQ cipher would be reused by other MSQ-based games).
- **Field-schema layer (data).** Fixed-offset fields over a plaintext buffer: integers
  (LE/BE), BCD, enums (numeric or letter), bitfields, arrays, sub-records. The Ultimas are
  essentially 100% this layer.

Planned work:

- [ ] Generic binary reader/writer with a field model (int LE/BE, BCD, enum, bitfield,
      string, array, struct) + "preserve unknown bytes" + validation
- [ ] A `Transform`/codec pipeline in front of the schema (identity for the Ultimas;
      MSQ-decrypt for Wasteland) with a symmetric write path
- [ ] Pluggable string codecs (ASCIIZ, BCD, Wasteland 5-bit table)
- [ ] Migrate the hardcoded Ultimas onto the schema engine to prove it
- [ ] **Tiered extensibility:** simple fixed-layout games become user-authorable schema
      files; encrypted/compressed games ship as official Rust codecs

**Open decision — schema / config format (do NOT lock yet).** Prove the abstraction as a
Rust type first; only then decide whether the field schema graduates to a hand-writable
text format. Leading candidates are **YAML** (friendly for field tables) for game schemas
and **TOML** (native to Rust) for app settings (the original `serde_yaml` is unmaintained,
so we'd use a maintained fork). A schema might conceptually look like:

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

Known byte layouts for implemented games live in [docs/formats/](docs/formats/README.md).

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

- [ ] Proper per-OS path handling (`directories` crate)
- [x] GOG (macOS) + Steam (macOS) detection + `detect --write` to append found games to the
      config, or `[detect] auto` to fold them in at runtime · Windows/Linux deferred (need
      machines to test on) · manual path override still works
- [x] `detect --all` also lists recognized-but-unsupported games (Ultima VI, Bard's Tale
      Trilogy, Magic Carpet 1/2) in a separate section, pointing at the issue tracker for
      feature requests · display-only (never written to config / auto-detected)
- [ ] Windows & Linux support (deferred — no machines to test on; CI builds/tests them, but
      only macOS is published for now)
- [x] GitHub Actions CI: fmt + clippy + test matrix across macOS / Ubuntu / Windows
- [x] Release automation: tag-triggered **macOS** builds (Apple Silicon + Intel) → GitHub
      Releases with tarballs + SHA-256 checksums. Follow-ups: `cargo-dist` installers and a
      Homebrew tap (needs a tap repo + token); publishing Windows/Linux binaries.

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
