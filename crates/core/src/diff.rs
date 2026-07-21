//! Byte-level diffing between two versions of a file.
//!
//! Used by the `watch` command to surface which offsets change as a game is played,
//! which is the core feedback loop for reverse-engineering undocumented save formats.

/// A single changed byte between two versions of a buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ByteChange {
    /// Offset of the changed byte.
    pub offset: usize,
    /// Value before the change.
    pub old: u8,
    /// Value after the change.
    pub new: u8,
}

/// Per-byte differences between `old` and `new`, over their common prefix.
///
/// Length differences are not reported here; callers compare `old.len()` and `new.len()`
/// separately.
pub fn diff_bytes(old: &[u8], new: &[u8]) -> Vec<ByteChange> {
    old.iter()
        .zip(new.iter())
        .enumerate()
        .filter_map(|(offset, (&old, &new))| {
            (old != new).then_some(ByteChange { offset, old, new })
        })
        .collect()
}

/// A run of consecutive changed bytes — adjacent [`ByteChange`]s merged into one span, which is
/// how a multi-byte field (e.g. a 16-bit counter) appears when a save is edited.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeRun {
    /// Offset of the first changed byte in the run.
    pub offset: usize,
    /// The old bytes, in order.
    pub old: Vec<u8>,
    /// The new bytes, in order.
    pub new: Vec<u8>,
}

/// Merge per-byte [`ByteChange`]s into runs of consecutive offsets. Input is assumed sorted by
/// offset (as produced by [`diff_bytes`]).
pub fn group_runs(changes: &[ByteChange]) -> Vec<ChangeRun> {
    let mut runs: Vec<ChangeRun> = Vec::new();
    for change in changes {
        match runs.last_mut() {
            Some(run) if run.offset + run.old.len() == change.offset => {
                run.old.push(change.old);
                run.new.push(change.new);
            }
            _ => runs.push(ChangeRun {
                offset: change.offset,
                old: vec![change.old],
                new: vec![change.new],
            }),
        }
    }
    runs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_changed_bytes() {
        let a = [1u8, 2, 3, 4];
        let b = [1u8, 9, 3, 8];
        assert_eq!(
            diff_bytes(&a, &b),
            vec![
                ByteChange {
                    offset: 1,
                    old: 2,
                    new: 9
                },
                ByteChange {
                    offset: 3,
                    old: 4,
                    new: 8
                },
            ]
        );
    }

    #[test]
    fn identical_is_empty() {
        assert!(diff_bytes(&[1, 2, 3], &[1, 2, 3]).is_empty());
    }

    #[test]
    fn compares_common_prefix_only() {
        // Extra trailing bytes in `new` are not reported here.
        assert!(diff_bytes(&[1, 2], &[1, 2, 3, 4]).is_empty());
    }

    #[test]
    fn groups_adjacent_changes_into_runs() {
        // Changes at offsets 1,2 (adjacent) and 5 (separate) -> two runs.
        let a = [0u8, 1, 2, 3, 4, 5];
        let b = [0u8, 9, 8, 3, 4, 7];
        let runs = group_runs(&diff_bytes(&a, &b));
        assert_eq!(
            runs,
            vec![
                ChangeRun {
                    offset: 1,
                    old: vec![1, 2],
                    new: vec![9, 8],
                },
                ChangeRun {
                    offset: 5,
                    old: vec![5],
                    new: vec![7],
                },
            ]
        );
    }
}
