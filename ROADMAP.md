# Fringe Retro Kit — Roadmap

> What we haven't built or decided yet. Committed decisions live in
> [ARCHITECTURE.md](ARCHITECTURE.md); the current concrete target is
> [PHASE-1-ULTIMA-I.md](PHASE-1-ULTIMA-I.md).

Conservative by design: solve one problem well before expanding.

> **Sequencing:** the first milestone **hardcodes** Ultima I in a CLI tool. The generic
> engine, the TUI, and CI are deliberately deferred; early development is CLI-only and
> local-only (macOS).

---

## Current status & near-term plan

Phases 1–4 are effectively complete: all four Ultimas (I–IV) plus Ultima V are fully
readable and editable via both the CLI and the interactive TUI — a section-grouped editor
with an enum picker, automatic backups + on-demand snapshots, character templates (with
in-app capture), and a per-game save-file chooser.

Agreed next steps, in order:

1. **Ultima I multi-slot** (quick win) ✅ — surface `PLAYER1.U1`…`PLAYER4.U1` through the
   existing file chooser by extending `GameKind::save_files`.
2. **Ultima IV** (Phase 7) ✅ — same family as I–III and documented by the `xu4`
   reimplementation; `PARTY.SAV` (players + party/virtue state) is readable and editable,
   validated against a real 8-companion save.
3. **Ultima V** (Phase 7) ✅ — `SAVED.GAM` (4192-byte RAM snapshot): sixteen 32-byte
   character records plus party/game state (provisions, inventory, reagents, date, karma,
   location). Plain little-endian binary; readable and editable, verified against a real
   save.
4. Then pick among **Phase 5** (Save Library), **Wasteland** records + the codec/Transform
   pipeline, or **Phase 6** (platform detection + CI). **Ultima VI** is a much larger effort
   (object-based, LZW-compressed, party-as-objects across ~70 files) and is deferred.

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

The next concrete work item, and independent of parsing. Lets users describe **which games
they own, on which platform, and where** — then refer to games by short identifiers instead
of raw file paths. Also simplifies our own testing.

- [ ] App-level config (TOML) listing enabled games, each with: identifier (e.g. `ultima1`),
      platform (GOG / Steam / DOSBox / manual), install path, and save location
- [ ] Resolve `fringe-retro inspect ultima1` (identifier → game + save path) instead of a path
- [ ] Enable/disable owned games (don't surface Wasteland options if you don't own it)
- [ ] Optional Save Library location (local or cloud-synced) — see Phase 5
- [ ] Generalizes today's single-game `config.toml` (`save_dir`) into a multi-game manifest

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
- [ ] GOG detection · DOSBox detection · Steam detection (if feasible) · manual path override
- [ ] Windows & Linux support
- [ ] GitHub Actions CI: build/test matrix across macOS / Windows / Linux
- [ ] Release automation (`dist`) with GitHub Releases + a Homebrew tap

---

## Phase 7 — Additional Games

Only after the architecture has proven itself. Byte layouts we've mapped so far live in
[docs/formats/](docs/formats/README.md).

**Selection heuristic:** prefer games that (a) the maintainers legally own and can test, and
(b) have an actively-maintained **open-source engine reimplementation** — that makes the
save format effectively a published, tested spec.

Grouped by codec complexity (which parsing engine each needs):

- **Done / in progress:** Ultima I ✅, Ultima II ✅, Ultima III ✅, Ultima IV ✅, Ultima V ✅,
  Wasteland (MSQ cipher done, records in progress).
- **Easy extensions — same family as the Ultimas:** Ultima VI (`Nuvie`) — larger effort:
  object-based, LZW-compressed, party stored as world objects across ~70 files.
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
- Save diff / comparison, duplicate detection
- JSON import / export
- Checksum verification
- Steam Cloud awareness
- Batch editing
- Plugin system
- Desktop GUI
- Schema validator
