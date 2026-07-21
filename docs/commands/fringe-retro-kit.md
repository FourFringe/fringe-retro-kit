# `fringe-retro-kit` — Command Reference

> Part of the [Fringe Retro Kit command reference](../../COMMANDS.md). See also the save editor
> ([`fringe-retro`](fringe-retro.md)) and the map browser ([`fringe-retro-map`](fringe-retro-map.md)).

**`fringe-retro-kit`** is the project's **reverse-engineering workbench** — the low-level tools
used to *understand* a save format in the first place. It's kept separate from the polished
`fringe-retro` app on purpose: it exposes bytes, offsets, ciphers, and checksums, not
games and characters. Everything shares `crates/core`, so a codec proven here is the same one the
player-facing tools use.

**Design rule:** every command is **CLI-first with plain-text output and a `--json` mode**, so the
tools are scriptable, diffable, and usable in automated or AI-assisted sessions. Numbers accept
decimal or `0x` hex throughout.

There are five tool groups:

| Command | Purpose |
| --- | --- |
| [`codec`](#codec--the-codec-workbench) | Decode/round-trip encrypted or compressed blobs and identify checksums. |
| [`strings`](#strings--the-string-ripper) | Extract embedded text (printable ASCII or Wasteland 5-bit packed). |
| [`schema`](#schema--the-schema-explorer) | Find a known value, diff two saves, detect record strides. |
| [`watch`](#watch--the-live-logger) | Poll a save while you play and log which bytes change. |
| [`carve`](#carve--the-archive-extractor) | Split a container into its member blocks (with optional Wasteland MSQ decrypt). |

### The workflow

The tools compose into the reverse-engineering loop we used to map Wasteland — each step is a
command, so the whole process is repeatable and scriptable:

1. **Unpack** a packed executable or container to get at its data:
   `fringe-retro-kit codec decode WL.EXE --codec exepack --out wl.bin`
2. **Rip strings** to anchor yourself — filenames, item/spell names, dialogue:
   `fringe-retro-kit strings ascii wl.bin --min 5`
3. **Carve** a container into blocks (and decrypt them, for Wasteland MSQ):
   `fringe-retro-kit carve GAME1 --savegame-only --out ./blocks`
4. **Find** a value you know from in-game (e.g. 500 gold) to pin it to an offset:
   `fringe-retro-kit schema find blocks/GAME1_0000.bin --value 500 --width u24`
5. **Diff** a before/after pair to see exactly which bytes an action changed:
   `fringe-retro-kit schema diff before.bin after.bin`
6. **Solve the checksum** guarding a block by trying candidate algorithms:
   `fringe-retro-kit codec checksum block.bin --expect 0xc7a8`
7. **Watch live** while you play to correlate in-game actions with byte deltas:
   `fringe-retro-kit watch GAME1 --json`

---

### `codec` — the codec workbench

Decrypt/decompress a blob, dump the plaintext, and (for symmetric codecs) verify a byte-for-byte
round-trip; plus a checksum solver.

#### ✅ `codec list`

List the available codecs, their kind, whether they're invertible, and the arguments each takes.

```bash
fringe-retro-kit codec list
```

The codecs today: **`xor`** (Wasteland's rolling-XOR stream cipher — symmetric),
**`huffman`** (Wasteland Huffman decompression), and **`exepack`** (EXEPACK executable
decompression, whole-file).

#### ✅ `codec decode <file> --codec <name>`

Apply a codec to a region of a file and print a hex dump (or save/emit the bytes).

```bash
fringe-retro-kit codec decode WL.EXE --codec exepack --out wl.bin
fringe-retro-kit codec decode block.bin --codec xor --seed 0x64 --offset 6
```

| Flag | Applies to | Purpose |
| --- | --- | --- |
| `--codec <name>` | all | `xor`, `huffman`, or `exepack`. |
| `--offset <n>` | xor, huffman | Where the encoded region begins (default `0`). |
| `--len <n>` | xor | Length of the region (default: to end of file). |
| `--seed <byte>` | xor | Initial key byte (**required** for `xor`). |
| `--step <byte>` | xor | Key advance per byte (default `0x1f`). |
| `--count <n>` | huffman | Number of decompressed output bytes (**required** for `huffman`). |
| `--out <file>` | all | Write the decoded bytes to a file instead of a hex dump. |
| `--json` | all | Emit `{codec, output_len, hex}` instead of a hex dump. |

`exepack` operates on the whole file (offset/len ignored).

#### ✅ `codec roundtrip <file> --codec <name>`

Decode a region then re-encode it and confirm the result is byte-identical — a self-consistency
check for invertible codecs. Decode-only codecs (`huffman`, `exepack`) are rejected with a clear
message. Takes the same arguments as `decode`.

```bash
fringe-retro-kit codec roundtrip GAME1 --codec xor --seed 0x64 --offset 6 --len 4608
```

#### ✅ `codec checksum <file> --expect <value>`

Run a region through every candidate checksum algorithm and report which one(s) produce the
expected value — the "which variant is it?" detective work, automated.

```bash
fringe-retro-kit codec checksum block.bin --len 0x1200 --expect 0xc7a8
```

| Flag | Purpose |
| --- | --- |
| `--expect <value>` | The stored/expected checksum to match against. |
| `--offset <n>` / `--len <n>` | Restrict the checksummed region (defaults: whole file). |
| `--json` | Emit every algorithm's value plus the list of matches. |

Algorithms tried: `sum8`, `xor8`, `sum16`, `negated_sum16`, `carry_fold_sum16`, and
`wasteland_msq` (the carry-folded, negated sum the shipped Wasteland uses).

---

### `strings` — the string ripper

Pull human-readable text out of an opaque file — often the fastest way to anchor in an unknown
format.

#### ✅ `strings ascii <file>`

Extract runs of printable ASCII (the classic `strings`), reported with their absolute file offset.

```bash
fringe-retro-kit strings ascii wl.bin --min 5
```

| Flag | Default | Purpose |
| --- | --- | --- |
| `--min <n>` | `4` | Minimum run length to report. |
| `--offset <n>` / `--len <n>` | whole file | Restrict the scanned region. |
| `--json` | — | Emit `[{offset, text}]`. |

#### ✅ `strings five-bit <file> --char-table <off> --start <off>`

Decode Wasteland's 5-bit packed strings from an explicit character table and stream offset.

```bash
fringe-retro-kit strings five-bit wl.bin --char-table 0x1a2b --start 0x1a67 --count 20
```

| Flag | Default | Purpose |
| --- | --- | --- |
| `--char-table <off>` | — | Offset of the 60-byte character table (**required**). |
| `--start <off>` | — | Offset where the packed stream begins (**required**). |
| `--count <n>` | `1` | Number of consecutive strings to decode. |
| `--json` | — | Emit an array of strings. |

---

### `schema` — the schema explorer

The mechanical half of mapping a layout: locate a value, diff two saves, and spot repeating
records.

#### ✅ `schema find <file> --value <v>`

Find every offset where a value is stored, trying whatever width/endianness you specify — the way
to pin a field like "500 gold" to a byte.

```bash
fringe-retro-kit schema find GAME1_0000.bin --value 500 --width u24 --endian both
```

| Flag | Default | Purpose |
| --- | --- | --- |
| `--value <v>` | — | The value to search for (**required**). |
| `--width <w>` | `u16` | Encoding width: `byte`, `u16`, `u24`, or `u32`. |
| `--endian <e>` | `le` | Byte order: `le`, `be`, or `both`. |
| `--offset <n>` / `--len <n>` | whole file | Restrict the scan; reported offsets stay absolute. |
| `--json` | — | Emit matches per endianness. |

#### ✅ `schema diff <a> <b>`

Byte-level guided diff of two saves — "`a` = before, `b` = after I raised STR 15→30" — grouping
consecutive changed bytes into runs.

```bash
fringe-retro-kit schema diff before.bin after.bin
# 000000bd  0f -> 1e        (one changed byte: 15 -> 30)
```

Adds a `--json` mode (`{a_len, b_len, changed_bytes, runs:[{offset, old, new}]}`) and notes any
length difference between the files.

#### ✅ `schema stride <file> --value <v>`

Find all occurrences of a value and report the gaps between them — a dominant gap is a likely
fixed record size (rosters, party arrays). Takes `--value`/`--width`/`--endian` like `find`, plus
`--json`.

```bash
fringe-retro-kit schema stride roster.bin --value 0 --width u16
```

---

### `watch` — the live logger

Poll a save file while you play and log which bytes change — the core feedback loop for
correlating an in-game action with its bytes.

```bash
fringe-retro-kit watch GAME1 --json
```

| Flag | Default | Purpose |
| --- | --- | --- |
| `--interval <ms>` | `500` | Poll interval. |
| `--offset <n>` / `--len <n>` | whole file | Restrict the watched region. |
| `--exit-after <n>` | `0` (forever) | Stop after this many change events. |
| `--timeout-ms <ms>` | `0` (none) | Stop after this much total run time — a hard bound for scripted captures. |
| `--json` | — | Emit one compact JSON object per change (JSONL), streamable. |

Text output logs each change as `[#1 t+2.31s] 2 run(s), 6 byte(s) changed` followed by the
`offset  old -> new` runs. A transient read (mid-save) is skipped and retried.

---

### `carve` — the archive extractor

Split a container file into its member blocks at a magic signature — the way into formats that
are a run of self-delimiting records with no index (e.g. Wasteland's back-to-back `msq` blocks).
Lists blocks by default; `--out` extracts them.

```bash
# List the blocks in a Wasteland GAME1 (all start with the msq signature):
fringe-retro-kit carve GAME1 --magic msq

# Extract just the decrypted savegame block (party + character records):
fringe-retro-kit carve GAME1 --savegame-only --out ./blocks
```

| Flag | Default | Purpose |
| --- | --- | --- |
| `--magic <spec>` | (required, or `msq` under `--decrypt`) | Signature that starts each block: literal ASCII (`msq`) or `0x` hex (`0x6d7371`). |
| `--min-size <n>` | `0` | Skip segments shorter than this (filters spurious matches). |
| `--decrypt` | off | Treat each block as a Wasteland MSQ block: strip the `msqN`+seed header and undo the rolling-XOR cipher. Implies `--magic msq`. |
| `--savegame-only` | off | Keep only the Wasteland *savegame* block (the party/character record). Implies `--decrypt`. |
| `--out <dir>` | — | Write each block to `<stem>_NNNN.bin` in this directory. Without it, blocks are only listed. |
| `--json` | — | Emit the block table (offset, length, `has_magic`, `decrypted`, `savegame`, path). |

Under `--decrypt`, the listing marks the savegame block with `<= savegame` and shows each block's
decrypted length.

