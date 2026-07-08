# Phase 1 — Hardcoded Ultima I (Proof of Concept)

> This document defines the **first, narrow milestone**: a working, well-tested
> command-line tool that can read, display, and safely edit a real Ultima I save
> file — with Ultima I knowledge **hardcoded** in Rust. No generic schema engine,
> no TUI, no multi-OS support yet. Those come later.

Status: **in progress** — workspace scaffolded and building; Ultima I engine next.
This is our working guideline; we'll refine it as we go.

---

## 1. Goal

By the end of Phase 1 we can, from a terminal on macOS:

1. Point the tool at a real `player*.u1` file (path given explicitly for now).
2. **Inspect** it: print the character's name, stats, gold, food, inventory, etc.
3. **Edit** a known field safely (e.g. set Strength, Gold, Food).
4. **Save** the change back, having **automatically created a timestamped backup**
   first, and **preserving every byte we don't understand.**

That's it. If we build *the best little Ultima I save inspector/editor* and it
never once corrupts a file, Phase 1 is a success.

**Explicitly deferred:** TUI, generic YAML/schema engine, save discovery, the Save
Library, Windows/Linux support, CI, multi-character UX. We keep the code honest so
these are easy to add, but we don't build them yet.

---

## 2. Rust terminology primer (as we go)

You know C/C++, so here's the Rust vocabulary mapped to concepts you already have.
I'll keep correcting terms as they come up.

| Term | What it is | Rough C/C++ analogy |
| --- | --- | --- |
| **crate** | A single Rust compilation unit / library or binary package | A library or an executable target |
| **`Cargo.toml`** | Manifest declaring the crate, its deps, metadata | `CMakeLists.txt` + a package manifest |
| **`cargo`** | Build tool + package manager + test runner | CMake + your package manager + ctest, combined |
| **crates.io** | The public package registry | Conan / vcpkg registry |
| **workspace** | A set of related crates built together, sharing one lock file | A multi-target CMake project / monorepo |
| **module (`mod`)** | Namespacing *within* a crate | A namespace / header grouping |
| **`rustc`** | The compiler (you rarely call it directly; `cargo` does) | `clang` / `gcc` |
| **`rustup`** | Installs and manages Rust toolchain versions | `nvm`-style version manager, but for Rust |
| **`clippy`** | The official linter | clang-tidy |
| **`rustfmt`** | The official autoformatter | clang-format |
| **trait** | A shared behavior interface types can implement | An abstract base class / interface / concept |
| **derive macro** | Auto-generates trait impls from an annotation | Codegen, but built into the language |

We're building **one workspace** containing **two crates**: a **library crate**
(`core`) and a **binary crate** (`cli`).

---

## 3. Toolchain setup (do this first)

### Install Rust — use `rustup`, not `brew install rust`

**Recommended:**

```bash
brew install rustup      # installs the rustup toolchain manager
rustup default stable    # downloads + activates the stable toolchain (rustc, cargo, clippy, rustfmt)
```

Note: Homebrew's `rustup` has **no separate `rustup-init` binary** — the `rustup`
command does the setup itself. It installs the `cargo` / `rustc` / etc. *shims* into
`$(brew --prefix rustup)/bin` (e.g. `/opt/homebrew/opt/rustup/bin`), **not** `~/.cargo/bin`.
Add that directory to your `PATH` in `~/.zshrc`, open a new terminal, and verify:

```bash
rustc --version
cargo --version
```

**Why `rustup` instead of Homebrew's `rust` formula?**

- Homebrew's `rust` is a single pinned version that you upgrade like any other brew
  package. Fine, but limiting.
- `rustup` is the *official* toolchain manager. It lets you switch stable/beta/nightly,
  pin a toolchain per-project, and — crucially for us later — **add cross-compilation
  targets** (e.g. Linux/Windows) with one command when we get to CI. It also manages
  `clippy` and `rustfmt` as components.
- Basically everyone in the Rust ecosystem uses `rustup`; tutorials assume it.

`cargo` (build/test/run) comes bundled — no separate install.

### Other Homebrew packages?

**Nothing else is required right now.** Optional, only if you want them:

- `just` — a simple command runner (like a friendlier `make`) for saving recipes like
  `just test`. Nice-to-have, not needed. We can add it later.
- A hex viewer for reverse-engineering / eyeballing saves: `brew install hexyl` (pretty
  colored hex dumps) or just use `xxd`, which ships with macOS.

That's it. Rust is refreshingly self-contained: no system libraries to wrangle for
what we're building.

---

## 4. Project structure

A Cargo **workspace** with two crates. The library holds all the real logic (so we
can unit-test it precisely); the binary is a thin CLI wrapper over it.

```
fringe-retro-kit/
  Cargo.toml                # [workspace] — lists the member crates
  rust-toolchain.toml       # pins the toolchain version for reproducibility
  crates/
    core/                   # library crate: "fringe-retro-core"
      Cargo.toml
      src/
        lib.rs
        save.rs             # generic "save file as byte buffer" helpers
        backup.rs           # timestamped backups + atomic writes
        games/
          mod.rs
          ultima1.rs        # HARDCODED Ultima I: offsets, enums, read/write
    cli/                    # binary crate: "fringe-retro" (the executable)
      Cargo.toml
      src/
        main.rs             # arg parsing + dispatch into core
  tests/                    # (workspace-level) integration tests, added later
  docs/                     # design docs may migrate here over time
```

**Naming note:** the *binary* is `fringe-retro` (what users type). The *library crate*
is `fringe-retro-core`. Users never see the library; it's an implementation detail we
split out purely for testability and future reuse (a TUI or GUI can depend on the same
core).

---

## 5. Dependencies (crates) — all MIT-compatible

Every crate below is either MIT or dual **MIT OR Apache-2.0** (dual-licensing lets us
use them under MIT), so we're clear on licensing. We prefer well-maintained crates over
hand-rolling anything.

| Crate | Purpose | License |
| --- | --- | --- |
| **`binrw`** | Declarative binary **read + write** via derive macros. Handles little-endian INT16, fixed offsets, arrays. Our workhorse for parsing `player*.u1`. | MIT OR Apache-2.0 |
| **`clap`** | Command-line argument parsing (subcommands, flags, help text). | MIT OR Apache-2.0 |
| **`thiserror`** | Ergonomic typed error enums for the `core` library. | MIT OR Apache-2.0 |
| **`anyhow`** | Easy error propagation/reporting in the `cli` binary. | MIT OR Apache-2.0 |
| **`tracing`** + **`tracing-subscriber`** | Structured logging. Logs go to a **file**, never stdout (matters once there's a TUI). | MIT |
| **`tempfile`** | Safe temp files for the atomic write-then-rename pattern. | MIT OR Apache-2.0 |
| **`chrono`** *(or `time`)* | Timestamps for backup filenames. | MIT OR Apache-2.0 |

Deferred until later phases (noting them so we don't accidentally reinvent them):

- **`ratatui`** + **`crossterm`** — the TUI (Phase: TUI).
- **`directories`** — correct per-OS config/data paths (Phase: multi-OS / discovery).
- **`serde`** + a maintained YAML crate (e.g. `serde_yaml_ng`) — only if/when we add
  user-editable schema files. (`serde_yaml` itself is archived/unmaintained.)

> On config/schema format: we've **deferred** this decision. When we get there, the
> guiding requirement is "users can easily find, read, and add their own game configs."
> YAML is the friendliest for hand-writing field tables; TOML is the most native to Rust
> for app settings. We'll likely use TOML for app config and a YAML-ish format for game
> schemas — but we'll decide when it's real, not now.

---

## 6. The Ultima I save format (hardcoded reference)

Source: **[Ultima I Save Game Format — ModdingWiki](https://moddingwiki.shikadi.net/wiki/Ultima_I_Save_Game_Format)**,
reverse-engineered by *TheAlmightyGuru* and *Daniel D'Agostino*. (We'll credit them in
the source and README.)

**File facts**

- Save files are named **`PLAYER*.U1`** — DOS 8.3, uppercase on our GOG/macOS install
  (e.g. `PLAYER1.U1`); up to four character slots. The ModdingWiki writes them lowercase
  (`player*.u1`); we should match **case-insensitively**.
- Total size: **0x334 = 820 bytes**.
- **Confirmed against a real save (2026-07):** a real `PLAYER1.U1` (a Wizard named
  "Enki") decodes exactly per the table below — Str 12, Agi 13, Stamina 14, Cha 15,
  Wis 16, Int 35, Gold 100, Food 200, Ready Weapon Dagger, Ready Armour Leather — and
  the `0xFFFF` list markers fall exactly at `0x54` and `0x60`. The spec is accurate.
- All multi-byte numbers are **little-endian 16-bit integers (`INT16LE`)** — natural
  for DOS/x86.
- `inuse.u1` is a transient file the game uses to hand a character between its EXEs;
  we ignore it for editing.
- Saving is only possible on the overworld, so a valid save is always overworld state.

**Header + stats (the fields we'll expose first)**

| Offset | Type | Field | Notes |
| --- | --- | --- | --- |
| 0x00 | ASCIIZ[15] | Name | ≤14 chars, byte 15 is the null terminator |
| 0x0F | BYTE | Null | padding |
| 0x10 | INT16LE | Race | 0 Human · 1 Elf · 2 Dwarf · 3 Bobbit |
| 0x12 | INT16LE | Class | 0 Fighter · 1 Cleric · 2 Wizard · 3 Thief |
| 0x14 | INT16LE | Sex | 0 Male · 1 Female |
| 0x16 | INT16LE | Hits | 0–9999 |
| 0x18 | INT16LE | Strength | |
| 0x1A | INT16LE | Agility | |
| 0x1C | INT16LE | Stamina | |
| 0x1E | INT16LE | Charisma | |
| 0x20 | INT16LE | Wisdom | |
| 0x22 | INT16LE | Intelligence | |
| 0x24 | INT16LE | Coin (Gold) | 0–9999 |
| 0x26 | INT16LE | Experience | 0–9999 |
| 0x28 | INT16LE | Food | 0–9999 |
| 0x2A | INT16LE | Ready Weapon | 1 Dagger … F Blaster (enum) |
| 0x2C | INT16LE | Ready Spell | 1 Open … A Kill (enum) |
| 0x2E | INT16LE | Ready Armour | 1 Leather … 5 Reflect suit (enum) |
| 0x30 | INT16LE | Boarded Transport | 0 Walking · 1 Horse · 2 Cart · 3 Raft · 4 Frigate · 5 Aircar (icon won't change if set alone) |
| 0x34 | INT16LE | Player X | overworld position |
| 0x36 | INT16LE | Player Y | overworld position |

**Rest of the file (parsed/preserved, exposed later)**

- Inventory quantity lists — gems, armour, weapons, spells, transports — each list
  terminated by an `0xFFFF` marker (e.g. gems 0x4C–0x52, armour 0x56–0x5E, weapons
  0x62–0x7E, spells 0x82–0x94, transports 0x98–0xA4).
- Misc: `Last Signpost` (0xA8), `Steps` (0xAC), plus several **Unknown** INT16 fields.
- **Map Objects**: 0xB4–0x333, an array of **40 objects × 16 bytes** (monsters and the
  player's vehicles). Each object is eight INT16LE values (type, unknown, X, Y, hits, …).

**Design consequence:** several fields are documented as *Unknown*, and there's a whole
map-object array we won't edit at first. This is exactly why our editing model is
**"load all 820 bytes, mutate only the offsets we understand, write the buffer back."**
Unknown bytes ride along untouched, guaranteed.

> ✅ **Checksum check:** the real `PLAYER1.U1` we inspected stores every stat as a plain
> `INT16LE` exactly where the spec says, with **no checksum byte evident in the header** —
> encouraging. We can't yet be *certain* nothing validates the file, so the plan stands:
> a **round-trip** test (load → save unchanged → compare byte-identical) plus an **in-game
> load** of an edited copy. If the game ever rejects an edit, a checksum elsewhere is the
> prime suspect.

---

## 7. CLI design (Phase 1 surface)

The binary is `fringe-retro`. Subcommands operate on an explicit save path for now
(discovery comes later). This headless CLI is a **permanent feature**, not a throwaway.

```bash
# Show everything we understand about a save (human-readable)
fringe-retro inspect <path/to/player0.u1>

# Print one field
fringe-retro get <path> strength

# Raw hex dump (handy while reverse-engineering / verifying)
fringe-retro dump <path> [--range 0x18:0x24]

# Edit a field. Automatically backs up first, writes atomically.
fringe-retro set <path> strength 50
fringe-retro set <path> gold 9999
fringe-retro set <path> transport aircar     # enums accept names or numbers

# Backup management
fringe-retro backup <path>            # make a manual timestamped backup
fringe-retro backups <path>           # list backups for this save
fringe-retro restore <path> <backup>  # restore a chosen backup
```

Field names are a fixed, hardcoded set for Ultima I (Strength, Gold, Food, Transport,
etc.), validated against their documented ranges/enums before writing.

---

## 8. Core library API (rough shape)

The `core` crate exposes something like:

- A parsed representation of an Ultima I save (name, stats, inventory, and a retained
  copy of the raw 820 bytes).
- `load(path) -> Ultima1Save` — read + parse, keep raw bytes.
- Typed getters for known fields.
- `set_field(name, value) -> Result<()>` — validates range/enum, mutates the in-memory
  byte buffer at the correct offset.
- `save(path) -> Result<()>` — **auto-backup**, then **atomic** write of the buffer.

**Safety rules baked into `core`:**

1. **Never write in place.** Write to a temp file in the same directory, then `rename`
   over the original (atomic on one filesystem).
2. **Always back up first.** Copy the current file to a timestamped backup before the
   first write.
3. **Preserve unknown bytes.** Editing mutates offsets in the retained buffer; we never
   reconstruct the file from only-known fields.
4. **Validate before write.** Range-check numeric fields (0–9999 etc.); reject unknown
   enum values.

---

## 9. Testing strategy

Because all logic lives in `core`, we can test it hard without any terminal:

- **Golden round-trip test:** load a fixture save, save it back with no edits, assert the
  output is **byte-identical**. This single test proves unknown-byte preservation.
- **Field read tests:** a fixture with known stats → assert getters return the documented
  values (name, Strength, Gold, Transport enum, …).
- **Field write tests:** set a field, re-read, assert only the intended bytes changed
  (diff the before/after buffers).
- **Validation tests:** out-of-range and bad-enum inputs are rejected.
- **Backup tests:** saving creates a backup; restore round-trips.

**Fixtures:** we'll keep a small set of real/anonymized `player*.u1` files under
`tests/fixtures/`. We can't create these until we locate a real save — see §11.
Until then we can start with a hand-built synthetic 820-byte buffer matching the spec.

Integration tests that drive the actual `fringe-retro` binary can come later; unit
tests on `core` are the priority.

---

## 10. The Elm Architecture (preview — for the later TUI)

Not built in Phase 1, but since you asked and it'll shape the TUI: **The Elm Architecture
(TEA)** is a simple, predictable pattern for interactive apps, borrowed from the Elm
language and very popular with Ratatui. It has three parts:

- **Model** — all your application state in one place (which save is open, current field,
  edit-in-progress, etc.).
- **Message** — an enum of every event that can happen (`KeyPressed`, `FieldEdited`,
  `SaveRequested`, …).
- **`update(model, message) -> model`** — the *only* place state changes, in response to
  messages. Pure and easy to test.
- **`view(model)`** — draws the UI purely from the current model. In Ratatui you redraw
  the whole screen each frame from the model, so the view is a pure function of state.

The appeal: state changes are centralized and testable, and the UI can never
"drift" out of sync with the data. It maps naturally onto our flow (open save → edit
state → render). We'll adopt it when we build the TUI; Phase 1's clean `core`/`cli`
split already sets us up for it, because the `Model` will mostly wrap `core` types.

---

## 11. Immediate next steps / open items

1. **Save file located & format confirmed.** A real `PLAYER1.U1` from the GOG/macOS
   install has been captured and decodes exactly per §6 (character "Enki", a Wizard).
   Remaining: **confirm the exact on-disk directory** — the file lives *outside* the
   `.app` bundle, most likely under `~/Library/Application Support/GOG.com/Galaxy/...`
   (to be confirmed) — so we can hardcode a sensible default path and drop a copy into
   `tests/fixtures/`.
2. ✅ **Workspace scaffolded.** A Cargo workspace with `crates/core` (library) and
   `crates/cli` (the `fringe-retro` binary) builds cleanly; `fringe-retro --version` and
   `--help` work, and `cargo clippy` / `cargo fmt --check` / `cargo test` are all green.
   Subcommands (`inspect` / `get` / `dump` / `set` / `backup` / `backups` / `restore`)
   are defined but stubbed.
3. ✅ **Ultima I parsing implemented** in `core::games::ultima1` against the spec in §6:
   a data-driven field table (name, stats, gold/food, ready weapon/spell/armour, transport,
   map X/Y) over the raw 820-byte buffer, with unit tests (synthetic-fixture field reads,
   round-trip byte preservation, length + unknown-value handling).
4. ✅ **Read-only commands wired up** — `inspect` / `get` / `dump` work against the real
   `PLAYER1.U1` (verified: "Enki", a Human Wizard). Zero risk to real files.
5. ✅ **Editing + backups + atomic save implemented.** `set` (validated against each
   field's range/enum), plus `backup` / `backups` / `restore`. Every write first makes a
   timestamped backup and then writes atomically (temp file + rename); `restore` also
   backs up the current save before overwriting. Verified end-to-end on a *copy* of the
   real `PLAYER1.U1`. **Remaining: your in-game validation** — load an edited copy in
   Ultima I to confirm the character is intact (our checksum sanity check).

**Definition of done for Phase 1:** we can inspect a real save, change a stat, load the
edited character successfully in Ultima I, and a backup of the original exists — with a
passing test suite proving we never disturb unknown bytes.

---

## 12. Deferred (tracked so we don't forget, not doing now)

- Generic schema/format engine (extract *after* 2–3 games are hardcoded).
- Config + user-editable game schema files (format TBD: TOML and/or YAML).
- TUI (Ratatui + Crossterm, using TEA).
- Save discovery across GOG/Steam and OSes; proper per-OS path handling (`directories`).
- Save Library (named snapshots, cloud-friendly storage).
- Windows/Linux support + GitHub Actions CI (build/test matrix, `dist` releases, Homebrew tap).
