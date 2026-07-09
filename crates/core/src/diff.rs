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
}
