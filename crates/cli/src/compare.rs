//! Field-level (semantic) comparison between two saves.
//!
//! Built on the same entity/field model the editor uses, so it works uniformly across every
//! supported game: two saves of the same game are diffed field-by-field (`Strength 15 → 30`)
//! rather than byte-by-byte. Files that aren't both parseable as the same game fall back to a
//! byte-range diff.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::Result;

use crate::edit::Session;

/// One changed field within an entity (character/party).
pub struct FieldChange {
    pub entity: String,
    pub label: String,
    pub old: String,
    pub new: String,
}

/// The result of comparing two saves.
pub enum Comparison {
    /// The two files are byte-for-byte identical.
    Identical,
    /// Field-level changes for a known game (grouped by entity, in entity order).
    Fields {
        game: &'static str,
        changes: Vec<FieldChange>,
        /// Entities present in only one save (e.g. a companion who joined/left).
        notes: Vec<String>,
    },
    /// A byte-range diff, for files not both parseable as the same game.
    Bytes {
        ranges: Vec<(usize, usize)>,
        len_old: usize,
        len_new: usize,
    },
}

/// Compare `old` against `new`, preferring a field-level diff when both parse as the same
/// game and falling back to a byte-range diff otherwise.
pub fn compare(old: &Path, new: &Path) -> Result<Comparison> {
    match (Session::load(old)?, Session::load(new)?) {
        (Some(so), Some(sn)) if so.kind() == sn.kind() => Ok(compare_sessions(&so, &sn)),
        _ => compare_bytes(old, new),
    }
}

/// Reduce an entity label (e.g. `"1. Avatar — L8 STR 15 …"`) to a stable header (`"1. Avatar"`)
/// so the summary's own values don't clutter the diff.
fn entity_name(label: &str) -> String {
    label.split(" — ").next().unwrap_or(label).to_string()
}

fn compare_sessions(old: &Session, new: &Session) -> Comparison {
    let old_ents: BTreeMap<usize, String> = old
        .entities()
        .into_iter()
        .map(|e| (e.index, e.label))
        .collect();
    let new_ents: BTreeMap<usize, String> = new
        .entities()
        .into_iter()
        .map(|e| (e.index, e.label))
        .collect();

    let mut indices: Vec<usize> = old_ents.keys().chain(new_ents.keys()).copied().collect();
    indices.sort_unstable();
    indices.dedup();

    let mut changes = Vec::new();
    let mut notes = Vec::new();

    for idx in indices {
        match (old_ents.get(&idx), new_ents.get(&idx)) {
            (Some(label), Some(_)) => {
                let entity = entity_name(label);
                let old_rows = old.rows(idx);
                let new_rows = new.rows(idx);
                let old_keys: BTreeMap<&str, &str> =
                    old_rows.iter().map(|r| (r.key, r.value.as_str())).collect();
                let new_keys: BTreeMap<&str, &str> =
                    new_rows.iter().map(|r| (r.key, r.value.as_str())).collect();

                // Changed or removed fields (present in `old`).
                for r in &old_rows {
                    match new_keys.get(r.key) {
                        Some(nv) if *nv != r.value => changes.push(FieldChange {
                            entity: entity.clone(),
                            label: r.label.to_string(),
                            old: r.value.clone(),
                            new: nv.to_string(),
                        }),
                        None => changes.push(FieldChange {
                            entity: entity.clone(),
                            label: r.label.to_string(),
                            old: r.value.clone(),
                            new: "(none)".to_string(),
                        }),
                        _ => {}
                    }
                }
                // Fields present only in `new` (e.g. a Wasteland skill learned).
                for r in &new_rows {
                    if !old_keys.contains_key(r.key) {
                        changes.push(FieldChange {
                            entity: entity.clone(),
                            label: r.label.to_string(),
                            old: "(none)".to_string(),
                            new: r.value.clone(),
                        });
                    }
                }
            }
            (Some(label), None) => {
                notes.push(format!("- {} (only in the old save)", entity_name(label)))
            }
            (None, Some(label)) => {
                notes.push(format!("+ {} (only in the new save)", entity_name(label)))
            }
            _ => {}
        }
    }

    if changes.is_empty() && notes.is_empty() {
        Comparison::Identical
    } else {
        Comparison::Fields {
            game: old.kind().title(),
            changes,
            notes,
        }
    }
}

fn compare_bytes(old: &Path, new: &Path) -> Result<Comparison> {
    let bo = std::fs::read(old)?;
    let bn = std::fs::read(new)?;
    if bo == bn {
        return Ok(Comparison::Identical);
    }
    let max = bo.len().max(bn.len());
    let mut ranges = Vec::new();
    let mut i = 0;
    while i < max {
        if bo.get(i) != bn.get(i) {
            let start = i;
            while i < max && bo.get(i) != bn.get(i) {
                i += 1;
            }
            ranges.push((start, i));
        } else {
            i += 1;
        }
    }
    Ok(Comparison::Bytes {
        ranges,
        len_old: bo.len(),
        len_new: bn.len(),
    })
}

/// The maximum number of byte ranges to list before summarising the rest.
const MAX_BYTE_RANGES: usize = 40;

/// Render a comparison into display lines.
pub fn report(cmp: &Comparison) -> Vec<String> {
    let mut out = Vec::new();
    match cmp {
        Comparison::Identical => out.push("No differences.".to_string()),
        Comparison::Fields {
            game,
            changes,
            notes,
        } => {
            let total = changes.len() + notes.len();
            out.push(format!(
                "{game}: {total} change{}",
                if total == 1 { "" } else { "s" }
            ));
            let mut current = "";
            for c in changes {
                if c.entity != current {
                    out.push(String::new());
                    out.push(c.entity.clone());
                    current = &c.entity;
                }
                out.push(format!("  {:<16} {} -> {}", c.label, c.old, c.new));
            }
            if !notes.is_empty() {
                out.push(String::new());
                out.extend(notes.iter().cloned());
            }
        }
        Comparison::Bytes {
            ranges,
            len_old,
            len_new,
        } => {
            out.push("(no known save format — comparing raw bytes)".to_string());
            if len_old != len_new {
                out.push(format!("size: {len_old} -> {len_new} bytes"));
            }
            out.push(format!(
                "{} byte range{} differ:",
                ranges.len(),
                if ranges.len() == 1 { "" } else { "s" }
            ));
            for (start, end) in ranges.iter().take(MAX_BYTE_RANGES) {
                let n = end - start;
                if n == 1 {
                    out.push(format!("  0x{start:04X} (1 byte)"));
                } else {
                    out.push(format!("  0x{start:04X}-0x{:04X} ({n} bytes)", end - 1));
                }
            }
            if ranges.len() > MAX_BYTE_RANGES {
                out.push(format!("  … and {} more", ranges.len() - MAX_BYTE_RANGES));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use fringe_retro_core::games::ultima1;

    fn ultima1_save(name: &[u8], gold: u16) -> Vec<u8> {
        let mut buf = vec![0u8; ultima1::SAVE_LEN];
        buf[0..name.len()].copy_from_slice(name);
        buf[0x24..0x26].copy_from_slice(&gold.to_le_bytes());
        buf
    }

    fn write(dir: &std::path::Path, name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let p = dir.join(name);
        std::fs::write(&p, bytes).unwrap();
        p
    }

    #[test]
    fn identical_saves_report_no_differences() {
        let dir = tempfile::tempdir().unwrap();
        let a = write(dir.path(), "A.U1", &ultima1_save(b"Enki", 100));
        let b = write(dir.path(), "B.U1", &ultima1_save(b"Enki", 100));
        assert!(matches!(compare(&a, &b).unwrap(), Comparison::Identical));
    }

    #[test]
    fn field_change_is_detected() {
        let dir = tempfile::tempdir().unwrap();
        let a = write(dir.path(), "A.U1", &ultima1_save(b"Enki", 100));
        let b = write(dir.path(), "B.U1", &ultima1_save(b"Enki", 500));
        let cmp = compare(&a, &b).unwrap();
        let Comparison::Fields { changes, game, .. } = cmp else {
            panic!("expected field-level changes");
        };
        assert_eq!(game, "Ultima I");
        let gold = changes
            .iter()
            .find(|c| c.label.eq_ignore_ascii_case("gold"))
            .expect("gold change");
        assert_eq!(gold.old, "100");
        assert_eq!(gold.new, "500");
    }

    #[test]
    fn unknown_format_falls_back_to_byte_diff() {
        let dir = tempfile::tempdir().unwrap();
        let a = write(dir.path(), "a.bin", &[1, 2, 3, 4, 5]);
        let b = write(dir.path(), "b.bin", &[1, 9, 3, 4, 8]);
        let cmp = compare(&a, &b).unwrap();
        let Comparison::Bytes { ranges, .. } = cmp else {
            panic!("expected a byte diff");
        };
        assert_eq!(ranges, vec![(1, 2), (4, 5)]);
    }
}
