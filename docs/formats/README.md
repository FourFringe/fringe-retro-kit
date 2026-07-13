# Save File Formats

Byte-level documentation of the save formats Fringe Retro Kit understands. These are
written for anyone reverse-engineering or building tools for these games — not just for
this project.

Some of this is **original research**. In particular, the [Ultima II](ultima2.md) player
save format does not appear to be documented anywhere else; we mapped it byte-by-byte by
diffing live saves (see that page's provenance notes).

## Conventions

- Offsets are hexadecimal and **relative to the start of the record or file** noted in
  each section.
- **BCD** = binary-coded decimal: each nibble is a decimal digit, so the byte `0x42`
  means decimal **42**, not 66. Multi-byte BCD values note their byte order.
- "LE" / "BE" = little-endian / big-endian.
- Bytes marked *volatile* change on every save (RNG / turn state) and should be preserved,
  not interpreted.
- Bytes not listed are either always zero in our samples or not yet identified; a good
  editor **preserves unknown bytes** rather than rewriting them.

## Formats

| Game | File(s) | Encoding | Container | Notes |
| --- | --- | --- | --- | --- |
| [Ultima I](ultima1.md) | `PLAYER*.U1` | LE `u16` | plain file | Documented elsewhere; included for completeness |
| [Ultima II](ultima2.md) | `PLAYER` | BCD (big-endian) | plain file | **Original research** |
| [Ultima III](ultima3.md) | `ROSTER.ULT`, `PARTY.ULT` | BCD (little-endian) | plain file | Corroborates the Codex of Ultima Wisdom wiki |
| [Ultima IV](ultima4.md) | `PARTY.SAV` | LE binary (`u16`/`u32`) | plain file | Matches the `xu4` reimplementation; verified against a real save |
| [Ultima V](ultima5.md) | `SAVED.GAM` | LE binary (`u16`/`u8`) | plain file | Follows the Codex of Ultima Wisdom wiki; verified against a real save |
| [Wasteland](wasteland.md) | `GAME1` (in a save directory) | binary | **encrypted MSQ blocks** | Cipher documented; record layout in progress |

## How these were produced

Each format was validated against real saves from legally-owned copies (GOG / Steam),
using the project's `dump` and `watch` commands to observe byte changes as we performed
known in-game actions. Where a public reference existed (Ultima I, Ultima III) we cite it;
where none existed (Ultima II) we mapped it ourselves and marked confidence levels.
