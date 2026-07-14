# Phase 5 — Save Management & the Save Library

> This document defines **Phase 5**: durable save management on top of the Phase 1–4
> foundation. Two independent pieces — **auto-backup retention** (small) and the
> **Save Library** (the main feature) — plus the design decisions we've agreed on so we
> don't paint ourselves into a corner later (multiple libraries, cloud sync).

Status: **design agreed, not yet implemented.** This is the working guideline for the
Phase 5 build. See [ROADMAP.md](ROADMAP.md) for where this sits in the overall plan.

---

## 1. Goal

Give the player a **permanent, curated collection** of game saves — named snapshots they
deliberately keep — that is completely separate from the ephemeral automatic backups. The
tool copies files between the library and the active game directory automatically; the user
should never have to touch save files in a file manager.

By the end of Phase 5 we can, from the CLI and the TUI:

1. **Archive** the current save of a game into the library under a name (with optional notes).
2. **Browse** the library by game, seeing each snapshot's name, notes, and when it was last
   updated.
3. **Restore** a snapshot back into the active game directory, safely (with overwrite
   protection).
4. **Manage** snapshots: view, rename, duplicate, delete.
5. **Prune** automatic backups by a configurable retention policy.

**Explicitly deferred (but designed-for):** multiple libraries and moving/copying snapshots
between them; per-OS default library location (that rides along with Phase 6's `directories`
work). We choose a data model that makes these cheap to add later.

---

## 2. Vocabulary (official)

| Term | Definition |
| --- | --- |
| **Active save** | The game's live save file(s) in its real `save_dir` (for GOG on macOS, inside the app bundle). |
| **Auto-backup** | The timestamped `.bak` files we write *before every edit* (`backup.rs`). An ephemeral safety net that lives next to the active save and is subject to **retention** pruning. |
| **Library** | A curated, permanent collection of **named snapshots** the user deliberately keeps. Distinct from auto-backups. |
| **Snapshot** (aka **entry**) | One archived copy of a game's **complete** save set, plus metadata. The unit the user names, browses, and restores. |

---

## 3. The atomic unit

A snapshot captures **all of a game's save files as one indivisible set.** The Ultima games
cross-reference their files (e.g. Ultima III's `ROSTER.ULT` and `PARTY.ULT`); if they drift
apart they're useless. We already know each game's set from `GameKind::save_files()`:

| Game | Save set |
| --- | --- |
| Ultima I | `PLAYER1.U1` … `PLAYER4.U1` (whichever exist) |
| Ultima II | `PLAYER` |
| Ultima III | `ROSTER.ULT` + `PARTY.ULT` |
| Ultima IV | `PARTY.SAV` |
| Ultima V | `SAVED.GAM` |
| Wasteland | a **save directory** (e.g. `GAME1` and friends) |

A snapshot copies every file in the set that exists. For directory-based saves (Wasteland),
the unit generalizes to "the directory the game needs," captured and restored as a whole.
Restore always puts back the **entire** set, so the files can never get out of sync.

---

## 4. On-disk layout — self-describing snapshot folders

**Each snapshot is a self-contained folder**: the save files plus a small `entry.toml`
sidecar that describes it. The library "listing" is simply *scan the subfolders and read each
`entry.toml`* — there is **no central index** to corrupt or keep in sync.

```
<library>/
  ultima3/
    my-thief-party/
      entry.toml          # name, notes, created, game id
      ROSTER.ULT
      PARTY.ULT
  ultima4/
    before-the-abyss/
      entry.toml
      PARTY.SAV
  ultima5/
    fresh-avatar/
      entry.toml
      SAVED.GAM
```

- Top level groups by **game id**, giving "browse by game" for free.
- Each snapshot folder is **portable**: because it carries its own `entry.toml`, you can drag
  it to another library, another machine, or a different cloud folder and it stays valid.
- The design is **robust to partial cloud syncs** (Dropbox / iCloud) and manual moves —
  everything is relative, nothing points at absolute paths.

**Analogy:** a snapshot folder is like a macOS `.app` bundle — a directory that looks like a
single self-describing item, carries its own metadata, and keeps working when you move or
rename it.

### No root index

We will **not** create a central index file. The folders *are* the database. (If browsing a
huge library ever becomes slow, we can add an *optional* root cache that is regenerated from
the folders and is never the source of truth — but we don't build it now.)

---

## 5. `entry.toml`

Minimal and self-describing. The `entry.toml` is the source of truth for a snapshot's
**display name** and notes; the folder name is cosmetic (see §6).

```toml
game    = "ultima3"                     # which built-in game/parser this snapshot is for
name    = "My Thief Party"              # display name (source of truth)
notes   = "Right before the Dungeon of Doom"
created = "2026-07-13T15:20:00"         # when the snapshot was archived (ISO 8601, local)
```

The captured save files are simply **everything in the folder except `entry.toml`**, so we
never have to keep a file list in sync. `game` is stored (even though it's usually the parent
folder's name) so a snapshot remains self-describing if moved.

**Deliberately not stored:**
- **Character summary** — derived on demand via `inspect` when browsing, so it's always
  correct (no stale cache).
- **`last_played` / "Last Updated"** — see §7; derived from the save files' modification time,
  not tracked separately.

---

## 6. Naming & collisions

- The **display name** (in `entry.toml`) is free text: `"Before the Abyss"`.
- The **folder name** is a **slug** derived from the display name (lowercased, spaces →
  hyphens, filesystem-safe): `before-the-abyss`. **No date prefix** (and no date in the slug
  at all). We keep the slug purely for easy human identification in Finder / a file manager.
- **Rename** updates `entry.toml`'s `name` **and renames the folder** to the new slug — just
  like renaming a `.app` bundle. Since the snapshot is self-describing, the folder move is
  safe.
- **Collisions within one library:** because the folders *are* the listing, existing slugs
  are just the existing folder names. On `add`/`rename`, if the target slug already exists in
  that game's directory, we **warn** and append a numeric suffix (`before-the-abyss-2`),
  reporting the final slug. The display name may legitimately repeat; the slug is what must be
  unique on disk.
- **Cross-library collisions** (the same name created independently in Dropbox *and* iCloud)
  are a **multiple-library** concern and are **deferred** (see §9). We note it now so the
  data model doesn't preclude reconciling them later.

---

## 7. Timestamps

- **`created`** — stored in `entry.toml`; when the snapshot was archived.
- **"Last Updated"** — **not** stored; **derived from the modification time of the snapshot's
  save file(s).** This approximates when the user last saved in-game. It isn't perfectly
  accurate, but it's close enough and requires no extra bookkeeping.
- To make "Last Updated" meaningful, **archiving preserves the source files' modification
  times** when copying them into the library (and restore likewise preserves the snapshot's
  times onto the active save). The game reads saves regardless of mtime, so this is safe.

---

## 8. Operations (CLI + TUI)

CLI namespace: **`library`** (alias **`lib`**). An entry is referenced as `<game>/<slug>`
(matching its on-disk path), e.g. `ultima3/my-thief-party`.

| Command | Behavior |
| --- | --- |
| `library add <game> [--name <name>] [--notes <notes>]` | Copy the active save set of `<game>` into a new snapshot folder. Preserves mtimes. Warns + suffixes on slug collision. |
| `library list [<game>]` | Scan the library; list snapshots grouped by game with name, "Last Updated", and notes. |
| `library view <game>/<slug>` | `inspect` the snapshot's files **without** restoring. |
| `library restore <game>/<slug>` | Copy the snapshot's files back into the game's `save_dir`, with overwrite protection (see §8.1). Confirm in the TUI. |
| `library rename <game>/<slug> <new-name>` | Update `entry.toml` name and rename the folder to the new slug. |
| `library duplicate <game>/<slug> [--name <name>]` | Copy a snapshot to a new one. |
| `library delete <game>/<slug>` | Delete the snapshot folder (confirm first). |

**TUI:** a Library screen (browse by game → snapshots), plus a "Save to Library" action from
the editor / backup screens. Restore and delete go through the existing confirmation-modal
pattern.

### 8.1 Restore & overwrite protection

Restoring overwrites the active save, so first we **auto safety-backup the current active
save** (reuse `backup::create` per file, exactly like the existing restore path), then copy
the snapshot's files in. If the active files are already byte-identical, restore is a no-op
(mirrors current backup-restore behavior).

---

## 9. Multiple libraries (deferred, but designed-for)

Real use case: keep most saves in Dropbox, but shuffle some into iCloud Drive for a RetroArch
experiment. We are **not building this now**, but the self-describing-folder model makes it
cheap to add later:

- **Now:** a single active library at `[library] path = "..."`.
- **Later:** named roots (`[library.<name>] path = "..."`) plus `library move` / `library copy
  <entry> --to <lib>`. Because snapshots are portable folders, moving one between libraries is
  just a folder move — **no index to reconcile**.
- **The trap we avoid:** a single central index containing absolute paths that assumes one
  location. Choosing per-snapshot self-describing folders sidesteps this entirely.

---

## 10. Automatic backup retention (separate, smaller piece)

Independent of the Library. Auto-backups stay where they are (next to the active save) and
gain a **configurable retention policy**:

```toml
[backups]
keep         = 20     # keep at most N most-recent .bak files per save (0 = unlimited)
max_age_days = 90     # also delete .bak files older than this (0 = no age limit)
```

Pruning runs after each `backup::create`. **Library snapshots are never auto-pruned** — they
are curated by hand. This item is small and self-contained and is a good warm-up before (or
alongside) the Library.

---

## 11. Configuration summary

```toml
# The Save Library (Phase 5). Explicit path for now; a per-OS default arrives with Phase 6.
[library]
path = "~/Dropbox/Retro Saves"

# Auto-backup retention (Phase 5).
[backups]
keep         = 20
max_age_days = 90
```

---

## 12. Relationship to existing code

- `GameKind::save_files()` already enumerates each game's atomic set — the archiving unit.
- `backup.rs` (`create` / `list` / `restore`) provides the safety-backup + copy primitives we
  reuse for restore and can extend for retention pruning.
- `save.rs::atomic_write` is the safe write primitive.
- `config.rs` (the `Config` manifest) gains `[library]` and `[backups]` tables.
- `inspect.rs::inspect_lines` powers `library view` without a restore.

---

## 13. Implementation order (proposed)

1. **`[library] path` config + library scan** (list snapshots by reading folders/`entry.toml`).
2. **`library add`** (archive the active set; slug + collision handling; preserve mtimes).
3. **`library list` / `view`**.
4. **`library restore`** (with safety backup + overwrite protection).
5. **`library rename` / `duplicate` / `delete`**.
6. **TUI Library screen** + "Save to Library" action.
7. **Auto-backup retention** (can be done at any point; independent).

Multiple libraries and a per-OS default path are **out of scope** for this phase.
