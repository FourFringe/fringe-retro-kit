# Fringe Retro Kit

> A cross-platform toolkit for exploring, editing, backing up, and preserving classic computer game save files.

---

# Vision

Fringe Retro Kit is an open-source, terminal-based application for interacting with save files from classic computer games.

The project is built around one central idea:

> Users should think about games and characters—not binary file formats.

Fringe Retro Kit should make it easy to browse installed games, inspect save files, safely edit known values, and automatically preserve previous versions.

Although the first supported game will likely be **Ultima I**, the project is intentionally designed to support many classic games through a generic architecture.

This project is both a practical utility and a software preservation effort.

---

# Naming

## Product

Fringe Retro Kit

## Repository

fringe-retro-kit

## Binary

fringe-retro

The executable launches directly into an interactive terminal interface.

Unlike tools such as Git, the primary workflow is expected to happen inside the application rather than through numerous command-line subcommands.

Example:

```bash
fringe-retro
```

---

# Core Philosophy

## User-Centered

Users should work with concepts like:

- Games
- Characters
- Save files
- Backups

The application should hide operating system details whenever possible.

---

## Data-Driven

Knowledge about individual games should live primarily in declarative configuration rather than application code.

Rust should provide the generic engine.

Game support should be data.

---

## Extensible

The application should ship with officially supported game definitions.

Users should be able to extend support by adding local configuration files without recompiling the application.

---

## Safe

Save editing should never feel risky.

Whenever practical:

- automatically create backups
- preserve unknown bytes
- make reverting easy

Data loss should be extremely unlikely.

---

## Preserve Player History

Fringe Retro Kit should help users preserve their gaming history, not merely edit save files.

The application distinguishes between:

- Active game saves
- Automatic backups
- The user's curated Save Library

These concepts serve different purposes and should remain separate throughout the application.

Users should be able to revisit games years later without manually managing save files.

---

## Cross Platform

Support:

- macOS
- Windows
- Linux

The user experience should remain as consistent as practical across all platforms.

---

# Technology

## Language

Rust

Reasons:

- native executables
- no runtime dependency
- excellent binary parsing support
- strong ecosystem
- easy packaging
- long-term maintainability

---

## User Interface

Terminal User Interface (TUI)

Likely libraries:

- Ratatui
- Crossterm

The TUI is considered the primary interface.

A desktop GUI may be added in the future, but is not an initial goal.

---

# Distribution

Native binaries should be available for:

- macOS
- Windows
- Linux

Distribution targets include:

- GitHub Releases
- Homebrew

Continuous Integration should automatically build all supported platforms.

---

# High-Level Architecture

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
        │        Ratatui UI          │
        └────────────────────────────┘
```

The parser should understand concepts like:

- integers
- enums
- strings
- arrays
- bitfields
- structures

It should **not** contain game-specific knowledge whenever practical.

---

# Proposed YAML System

**This section intentionally describes direction rather than specification.**

The exact schema format should evolve naturally during development.

The goal is to describe game knowledge declaratively.

Possible responsibilities include:

- supported save files
- binary field definitions
- enums
- display labels
- validation ranges
- install detection hints
- editor hints
- browser behavior

A future schema might look conceptually like:

```yaml
game:
  name: Ultima I

saveFiles:
  - PLAYER

fields:

  strength:
    type: u16
    offset: 0x20
    label: Strength

  transport:
    type: enum
    offset: 0x52

platforms:
  gog:
    ...
```

This is **not** intended to lock down the schema design.

Instead, it illustrates the philosophy:

> The Rust engine should be generic.
> The YAML should describe games.

---

# Embedded and Local Schemas

Official schemas should be embedded into the executable during compilation.

Benefits:

- single executable
- offline operation
- version compatibility

The application should also load user schemas from a local configuration directory.

Example:

```
~/.fringe-retro/schemas/
```

User schemas may:

- add new games
- override existing definitions
- experiment with unsupported formats

No recompilation should be required.

---

# Save Discovery

The user should configure games—not save paths.

Example:

```
Ultima I

Installation:
    GOG
```

Whenever practical the application should automatically locate:

- installation directories (different for each OS)
- save files

Supporting platforms such as:

- GOG
- Steam

should remain an implementation detail rather than something users manage manually.

Manual path overrides should always be available.

Users configure game installations.

Fringe Retro Kit manages:

- active save locations
- backup storage
- the Save Library

The application should move files between these locations automatically.

---

# Save Browser

The application should discover save files and present them naturally.

Example:

```
Games

▶ Ultima I

Character

▶ Lord British

Current Save

PLAYER

Library

Before Time Machine
Endgame
First Dungeon

Backups

2026-07-06 14:22
2026-07-06 14:05
2026-07-05 19:41
```

Browsing should be the primary workflow.

---

# Save Inspector

Users should be able to inspect known values before editing.

Example:

```
Strength         42
Agility          35
Gold         12,450
Food          9,999
Transport     Aircar
```

Unknown binary regions should remain untouched.

---

# Editing

The editor should generate itself whenever practical.

Examples:

```
Strength

[ 42 ]
```

Enum

```
Transport

< Aircar >
```

Boolean

```
Has Time Machine

[✓]
```

The goal is to minimize game-specific UI code.

---

# Save Management

## Automatic Backups

Before modifying any save file, Fringe Retro Kit should automatically create a timestamped backup.

Backups exist solely for recovery.

Characteristics:

- automatic
- timestamped
- temporary
- never renamed
- rarely edited

Backups should require no user intervention.

## Save Library

The Save Library is the user's curated collection of game progress.

Unlike backups, library entries are intentionally managed by the user.

Characteristics:

- named
- browsable
- restorable
- optionally annotated
- intended for long-term preservation

Library entries may live outside the game installation and should support cloud-synchronized folders such as Dropbox, Google Drive, OneDrive, or iCloud Drive.

The Save Library should become the primary way users revisit previous adventures.

---

# Repository Layout

```
fringe-retro-kit/

    src/

    schemas/

    docs/

    tests/

    examples/
```

Additional structure should emerge as the project grows.

Avoid premature abstraction.

---

# Development Strategy

The first milestone is intentionally narrow:

> Build the best Ultima I save editor available.

Only after solving one real problem should the architecture expand.

Likely progression:

1. Ultima I
2. Ultima II
3. Ultima III
4. Wasteland
5. Bard's Tale
6. Wizardry
7. SSI Gold Box games
8. Community-contributed formats

---

# Non-Goals

Fringe Retro Kit does **not** aim to:

- distribute game assets
- include game executables
- emulate games
- manage ROM collections
- modify game code

The project exists solely to understand, preserve, and safely manipulate user-owned save files.

---

# Open Source

License:

MIT

The project should remain welcoming to contributors and easy to integrate with other preservation efforts.

---

# Guiding Principle

Whenever possible, supporting a new game should involve writing a schema—not modifying Rust source code.

If adding another game consistently requires changing the engine, reconsider the architecture before adding more features.
