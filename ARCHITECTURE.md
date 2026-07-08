# Architecture & Decisions

> The decisions we've committed to and the shape of the system as we've settled it.
> Anything still open or unbuilt lives in [ROADMAP.md](ROADMAP.md).
> The current concrete work item is [PHASE-1-ULTIMA-I.md](PHASE-1-ULTIMA-I.md).

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
- **Data-driven (eventually).** The long-term goal is a generic engine plus per-game
  *data* rather than per-game code — but this is deliberately deferred (see
  *Development strategy*).

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

### Interface — CLI first, TUI later

- The **headless CLI** (`inspect` / `get` / `dump` / `set` / `backup` / `restore`) is a
  **permanent** feature, not a throwaway.
- The **TUI** (Ratatui + Crossterm) will become the primary interface once the core is
  solid, built using **The Elm Architecture** (Model / Message / `update` / `view`).
- This revises the original intent of launching straight into a TUI; we start CLI-only to
  keep the core testable and the first milestone small.

### Dependencies — MIT-compatible only

Every dependency is MIT or dual **MIT OR Apache-2.0** (usable under MIT):

| Crate | Purpose |
| --- | --- |
| `binrw` | Declarative binary read + write (offsets, little-endian ints, arrays) |
| `clap` | Command-line argument parsing |
| `thiserror` | Typed error enums in `core` |
| `anyhow` | Error propagation/reporting in the `cli` binary |
| `tracing` (+ `tracing-subscriber`) | Structured logging — to a **file**, never stdout |
| `tempfile` | Safe temp files for atomic writes |
| `chrono` | Timestamps for backup filenames |

Deferred (noted so we don't reinvent them): `ratatui` + `crossterm` (TUI),
`directories` (per-OS paths), `serde` + a maintained YAML crate (user schemas).

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

### License — MIT

The project should stay welcoming to contributors and easy to integrate with other
preservation efforts.

---

## Target architecture

Once the generic engine is extracted, the intended layering is:

```
                 Rust Engine

        ┌────────────────────────────┐
        │   Binary File Layer        │
        └─────────────┬──────────────┘
                      │
        ┌─────────────▼──────────────┐
        │     Schema Engine          │
        └─────────────┬──────────────┘
                      │
        ┌─────────────▼──────────────┐
        │      Game Model            │
        └─────────────┬──────────────┘
                      │
        ┌─────────────▼──────────────┐
        │   CLI  /  Ratatui UI       │
        └────────────────────────────┘
```

The engine should understand generic concepts — integers, enums, strings, arrays,
bitfields, structures — and avoid game-specific knowledge wherever practical.

**Current state:** the "Binary File Layer" and "Schema Engine" are represented today by
hardcoded Ultima I code in `crates/core`. The diagram is the destination, not where we
are yet.

---

## Alternatives considered

Recorded so we don't re-litigate settled choices.

**Binary parsing — chose `binrw`.** It does symmetric declarative read *and* write
(offsets, endianness, arrays), which fits an editor that must round-trip files. Also
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
