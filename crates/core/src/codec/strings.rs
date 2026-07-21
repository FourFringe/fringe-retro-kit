//! Extraction of **printable ASCII strings** from a byte buffer — the classic `strings(1)`
//! primitive, used to spot names, messages, and file signatures inside opaque game data. A
//! terminating `NUL` simply ends a run, so this finds ASCIIZ strings as well as any other
//! embedded text.

/// A printable run found in a buffer, tagged with the byte offset where it starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    /// Byte offset of the first character.
    pub offset: usize,
    /// The decoded text.
    pub text: String,
}

/// A byte is "printable" if it is a normal ASCII graphic character or a space.
fn is_printable(b: u8) -> bool {
    (0x20..=0x7e).contains(&b)
}

/// Extract every run of printable ASCII at least `min_len` bytes long, in order of appearance.
///
/// `min_len` is clamped to at least 1. Runs are split by any non-printable byte (including `NUL`,
/// tabs, and high-bit bytes), matching `strings(1)`'s default behaviour.
pub fn ascii(data: &[u8], min_len: usize) -> Vec<Found> {
    let min_len = min_len.max(1);
    let mut out = Vec::new();
    let mut start = 0;
    let mut run = String::new();

    let flush = |start: usize, run: &mut String, out: &mut Vec<Found>| {
        if run.len() >= min_len {
            out.push(Found {
                offset: start,
                text: std::mem::take(run),
            });
        } else {
            run.clear();
        }
    };

    for (i, &b) in data.iter().enumerate() {
        if is_printable(b) {
            if run.is_empty() {
                start = i;
            }
            run.push(b as char);
        } else {
            flush(start, &mut run, &mut out);
        }
    }
    flush(start, &mut run, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_asciiz_runs_with_offsets() {
        let data = b"\x00\x01HELLO\x00hi\x00WORLD!";
        let found = ascii(data, 3);
        assert_eq!(
            found,
            vec![
                Found {
                    offset: 2,
                    text: "HELLO".into()
                },
                Found {
                    offset: 11,
                    text: "WORLD!".into()
                },
            ]
        );
    }

    #[test]
    fn min_len_filters_short_runs() {
        // "hi" (len 2) is dropped when min_len is 3; the longer run survives.
        let data = b"hi\x00there";
        assert_eq!(
            ascii(data, 3),
            vec![Found {
                offset: 3,
                text: "there".into()
            }]
        );
        // With min_len 2 (and the 1-clamp irrelevant here), both appear.
        assert_eq!(ascii(data, 2).len(), 2);
    }
}
