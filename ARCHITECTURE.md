# Architecture & Decisions

> The decisions we've committed to and the shape of the system as we've settled it.
> Anything still open or unbuilt lives in [ROADMAP.md](ROADMAP.md).
> What the tool can do today is in [COMMANDS.md](COMMANDS.md).

---

## Vision

Fringe Retro Kit is an open-source, terminal-based tool for inspecting, editing,
backing up, and preserving save files from classic computer games. One idea drives it:

> Users should think about games and characters — not binary file formats.

It is both a practical utility and a software-preservation effort.

---

## Guiding principles

- **User-centered.** Users work in terms of games, characters, saves, and backups; the
  application hides operating-system details wherever practical.
- **Safe.** Editing must never feel risky — automatic backups, preservation of bytes we
  don't understand, and easy reverts. Data loss should be extremely unlikely.
- **Preserve player history.** Active saves, automatic backups, and the curated Save
  Library are treated as three distinct things.
- **Cross-platform (eventually).** macOS first; Windows and Linux later.
- **Data-driven where it works (eventually).** The long-term goal is a small set of reusable
  parsing engines plus per-game *schema data*, with bespoke Rust code for the parts data
  can't express (encryption, compression, checksums). Deliberately deferred (see
  *Development strategy* and *Parsing architecture*).

---

## Decisions

### Language — Rust

Native single binary, no runtime dependency, strong binary-parsing ecosystem,
easy packaging, and long-term maintainability.

### Project structure — Cargo workspace

Two crates (a *crate* is Rust's unit of compilation — a library or binary package):

- **`crates/core`** — library crate (`fringe-retro-core`): all parsing, editing, and
  backup logic. Unit-testable without any UI.
- **`crates/cli`** — binary crate (`fringe-retro`): a thin command-line wrapper over the
  core.

Rationale: precise unit testing of the risky logic, a permanent headless CLI, and future
reuse of the same core by a TUI (or GUI).

### Interface — CLI first, then TUI

- The **headless CLI** (`inspect` / `get` / `dump` / `set` / `backup` / `restore`, and more)
  is a **permanent** feature, not a throwaway.
- The **TUI** (Ratatui + Crossterm) is now built — a section-grouped field editor with an
  enum picker, a backup browser, and a Save Library screen — and shares the exact same
  `core` as the CLI.
- We started CLI-only to keep the core testable and the first milestone small; the TUI
  followed once the core was solid.

### Dependencies — MIT-compatible only

Every dependency is MIT or dual **MIT OR Apache-2.0** (usable under MIT):

| Crate | Purpose |
| --- | --- |
| `ratatui` (bundles `crossterm`) | Terminal UI |
| `clap` | Command-line argument parsing |
| `serde` + `toml` | Reading the `config.toml` game manifest |
| `thiserror` | Typed error enums in `core` |
| `anyhow` | Error propagation/reporting in the `cli` binary |
| `tempfile` | Safe temp files for atomic writes |
| `chrono` | Timestamps for backup filenames |
| `open` | Opening web resources in the OS default browser |

Binary parsing was originally slated for `binrw`, but the fixed-offset formats turned out to
need so little that we hand-wrote a small field-schema engine (`crates/core/src/schema.rs`)
instead — no extra dependency.

Deferred (noted so we don't reinvent them): `directories` (per-OS paths) and a maintained
TOML/YAML crate for user-authored schema files.

### Save-editing safety model

1. Read the entire file into a byte buffer.
2. Edit only **known** offsets in place — unknown bytes are preserved by construction.
3. Validate values (ranges / enums) before writing.
4. Create a timestamped **backup** before the first write.
5. Write **atomically**: to a temp file in the same directory, then `rename` over the
   original.

### Save concepts — three distinct things

- **Active game saves** — the game's live save files.
- **Automatic backups** — recovery only; automatic, timestamped, temporary.
- **Save Library** — user-curated, named, annotated, long-term. (Detailed feature set
  lives in [ROADMAP.md](ROADMAP.md).)

### Development strategy — hardcode first, generalize later

- Build the best **Ultima I** editor first, with Ultima I knowledge hardcoded in Rust.
- Extract the generic, data-driven schema engine only **after** 2–3 games are hardcoded,
  so the abstraction is earned rather than speculated.
- Supported and planned games are listed in the [README](README.md).

### Parsing architecture — codec pipeline + field schema

Reverse-engineering four games (Ultima I/II/III plus Wasteland) revealed that save parsing
splits cleanly into two layers, and this is the seam the engine should follow. Neither
"pure data-driven" nor "hardcode every game" is right; it's a **hybrid**.

1. **Codec / container layer (Rust code).** Getting from bytes-on-disk to a flat plaintext
   buffer: encryption, compression, checksums, block scanning, "a save is a directory of
   files," and exotic string encodings. These are procedural and reusable *per family* (the
   Wasteland MSQ cipher would serve any MSQ-based game). Modeled as a `Transform` pipeline
   with a symmetric write path (re-encrypt / re-checksum).
2. **Field-schema layer (data).** Once plaintext exists: fixed-offset fields — integers
   (LE/BE), BCD, enums (numeric or letter), bitfields, arrays, sub-records. The three
   Ultimas are essentially 100% this layer; Wasteland is a heavy codec layer with a normal
   schema on top.

**Tiered extensibility.** Simple fixed-layout games (the Ultimas) can eventually be
user-authorable **schema files**; encrypted/compressed games (Wasteland) require an
official Rust codec and ship in releases. We do not promise "any game via YAML" — we
promise data-authoring for the tier where it actually works, with a code escape hatch for
the rest. The text schema format is intentionally **not** designed yet: prove the Rust
abstraction across the Ultimas first, then decide.

### Game library manifest — a separate concern

Managing *which* games a user owns (identifiers, platform, install path, save location,
enable/disable) is game-agnostic and independent of parsing. It is implemented as the
multi-game `config.toml` manifest, letting users run `fringe-retro inspect ultima1` instead
of passing raw paths.

### License — MIT

The project should stay welcoming to contributors and easy to integrate with other
preservation efforts.

---

## Target architecture

Once the generic engine is extracted, the intended layering is:

```
                 Rust Engine

        ┌────────────────────────────┐
        │   Save Container Layer      │  files / directories on disk
        └─────────────┬──────────────┘
                      │
        ┌─────────────▼──────────────┐
        │   Codec Pipeline            │  decrypt · decompress · block · checksum
        └─────────────┬──────────────┘
                      │
        ┌─────────────▼──────────────┐
        │   Field-Schema Engine       │  ints/BCD/enums/bitfields/arrays over plaintext
        └─────────────┬──────────────┘
                      │
        ┌─────────────▼──────────────┐
        │      Game Model             │
        └─────────────┬──────────────┘
                      │
        ┌─────────────▼──────────────┐
        │   CLI  /  Ratatui UI        │
        └────────────────────────────┘

   The game **Library Manifest** (identifiers, platforms, paths) is game-agnostic and
   sits beside this stack — it selects what to load, not how to parse it.
```

The field-schema engine understands generic concepts — integers, enums, strings, arrays,
bitfields, structures — and avoids game-specific knowledge; the codec pipeline handles the
container/crypto concerns that data can't express.

**Current state:** seven games are supported in `crates/core` — Ultima I–VI and Wasteland —
all editable via both the CLI and the TUI. The field-schema engine
(`crates/core/src/schema.rs`) has been extracted and the Ultimas run on it; Wasteland adds
an MSQ codec (decrypt + re-encrypt with the game's own checksum), and Ultima VI reads party
stats from the uncompressed `OBJLIST`. The full `Transform`-pipeline codec layer and
user-authored schema files remain the destination, not yet where we are.

---

## Alternatives considered

Recorded so we don't re-litigate settled choices.

**Binary parsing — originally chose `binrw`, ultimately hand-rolled.** `binrw` does symmetric
declarative read *and* write (offsets, endianness, arrays), which fits an editor that must
round-trip files — but the fixed-offset formats needed so little that we wrote a small
field-schema engine (`crates/core/src/schema.rs`) instead, with no extra dependency. Also
considered: `deku` (bit-level — keep in mind if a future game packs flags into individual
bits), `nom` (parser combinators — more manual than we need), and **Kaitai Struct** (a
declarative `.ksy` binary-description language with a Rust runtime and a community format
library — great inspiration for our eventual schema format, but read-focused, so weak at
write-back and preserving unknown bytes, which is exactly what a save editor needs). The
ImHex pattern language and 010 Editor templates are further inspiration for a
"describe-a-layout" DSL when we design our own schema.

**Terminal UI — chose Ratatui + Crossterm with The Elm Architecture.** Ratatui is the
actively maintained successor to `tui-rs` with the largest ecosystem; Crossterm is the
cross-platform backend (works on Windows consoles, unlike termion). Also considered:
`cursive` (retained-mode widget tree — more batteries-included but less flexible) and
`tui-realm` (a component framework atop Ratatui — possibly useful later, overkill now).

---

## Non-goals

Fringe Retro Kit does **not** aim to:

- distribute game assets or executables
- emulate games
- manage ROM collections
- modify game code

It exists solely to understand, preserve, and safely edit user-owned save files.
