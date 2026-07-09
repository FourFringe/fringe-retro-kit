# Fringe Retro Kit — Roadmap

> What we haven't built or decided yet. Committed decisions live in
> [ARCHITECTURE.md](ARCHITECTURE.md); the current concrete target is
> [PHASE-1-ULTIMA-I.md](PHASE-1-ULTIMA-I.md).

Conservative by design: solve one problem well before expanding.

> **Sequencing:** the first milestone **hardcodes** Ultima I in a CLI tool. The generic
> engine, the TUI, and CI are deliberately deferred; early development is CLI-only and
> local-only (macOS).

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

The TUI (Ratatui + Crossterm) becomes the primary interface here.

- [ ] Game / character / save browsing
- [ ] Inspector: read-only view of known fields
- [ ] Auto-generated editors (number / enum / boolean widgets) to minimize per-game UI code
- [ ] Backup browser

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

### Automatic backups

- [ ] Browse backups
- [ ] Restore backups
- [ ] Configurable retention

### Save Library

The user's permanent, curated collection — distinct from automatic backups, and intended
to become the central hub for managing a player's game history. The application handles
copying files between the library and the active save directory automatically; users
should never manipulate save files in a file manager.

- [ ] Configurable location — local **or** cloud-synced (Dropbox, Google Drive, OneDrive,
      iCloud Drive); treated as ordinary storage, no assumptions about sync software
- [ ] Named snapshots with notes and metadata (created date, last played)
- [ ] Browse archived saves by game and character
- [ ] Restore into the active game, with overwrite protection
- [ ] Duplicate / rename / delete

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

- **Done / in progress:** Ultima I ✅, Ultima II ✅, Ultima III ✅, Wasteland (MSQ cipher done,
  records in progress).
- **Next — plain structured binary + container (no encryption):** **Fallout 1 & 2** (owned;
  big-endian ints, save-as-directory; well RE'd by TeamX / F12se). A gentle step up from the
  Ultimas.
- **Easy extensions — same family as the Ultimas:** Ultima IV (`xu4`), Ultima V, Ultima VI
  (`Nuvie`).
- **Candidates (kept in the list; may never get to them, and that's fine):** SSI Gold Box,
  Might & Magic 3–5 / World of Xeen (`OpenXeen`), Dungeon Master, Eye of the Beholder,
  Daggerfall (Daggerfall Unity; introduces RLE decompression), Wizardry, original Bard's Tale.

Note: the Bard's Tale Trilogy remaster is a from-scratch Krome rebuild with a modern save
format, **not** the original — out of scope for preservation of the original formats.

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
