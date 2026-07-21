//! Low-level **scanning** primitives for schema exploration: locate a byte pattern and analyse
//! the spacing between hits. These answer the two mechanical questions when mapping an unknown
//! save format — "where is this value stored?" and "what is the record stride?" — leaving the
//! interpretation to the caller (the kit's schema explorer).

use std::collections::HashMap;

/// Every start offset where `needle` occurs in `haystack` (overlapping matches included), in
/// ascending order. An empty needle, or one longer than the haystack, yields no matches.
pub fn find_bytes(haystack: &[u8], needle: &[u8]) -> Vec<usize> {
    if needle.is_empty() || needle.len() > haystack.len() {
        return Vec::new();
    }
    (0..=haystack.len() - needle.len())
        .filter(|&i| &haystack[i..i + needle.len()] == needle)
        .collect()
}

/// Histogram of the gaps between consecutive (sorted) `offsets`, most frequent first, ties broken
/// by the smaller gap. A single dominant gap is a likely fixed record stride.
pub fn gap_histogram(offsets: &[usize]) -> Vec<(usize, usize)> {
    let mut sorted = offsets.to_vec();
    sorted.sort_unstable();
    let mut counts: HashMap<usize, usize> = HashMap::new();
    for pair in sorted.windows(2) {
        *counts.entry(pair[1] - pair[0]).or_insert(0) += 1;
    }
    let mut histogram: Vec<(usize, usize)> = counts.into_iter().collect();
    histogram.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    histogram
}

/// One carved segment of a container: where it starts, how long it is, and whether it begins with
/// the magic signature (the leading preamble before the first magic does not).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Segment {
    pub offset: usize,
    pub len: usize,
    pub has_magic: bool,
}

/// Split `data` into segments delimited by each occurrence of `magic`. Every magic starts a new
/// segment running up to the next magic (or end of data); any bytes before the first magic form a
/// leading preamble segment (`has_magic = false`). An empty `magic`, or data with no match, yields
/// a single whole-buffer segment. Empty segments are omitted.
pub fn carve(data: &[u8], magic: &[u8]) -> Vec<Segment> {
    let mut bounds: Vec<usize> = std::iter::once(0).chain(find_bytes(data, magic)).collect();
    bounds.sort_unstable();
    bounds.dedup();

    let mut segments = Vec::new();
    for (i, &start) in bounds.iter().enumerate() {
        let end = bounds.get(i + 1).copied().unwrap_or(data.len());
        if end > start {
            segments.push(Segment {
                offset: start,
                len: end - start,
                has_magic: !magic.is_empty() && data[start..].starts_with(magic),
            });
        }
    }
    segments
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_all_overlapping_offsets() {
        assert_eq!(find_bytes(b"abcabcab", b"abc"), vec![0, 3]);
        assert_eq!(find_bytes(b"aaaa", b"aa"), vec![0, 1, 2]);
        assert!(find_bytes(b"abc", b"xyz").is_empty());
        assert!(find_bytes(b"ab", b"").is_empty());
        assert!(find_bytes(b"ab", b"abcd").is_empty());
    }

    #[test]
    fn gap_histogram_surfaces_the_dominant_stride() {
        // A record of stride 9 repeated, with one odd gap of 20.
        let offsets = [0usize, 9, 18, 27, 47];
        assert_eq!(gap_histogram(&offsets), vec![(9, 3), (20, 1)]);
    }

    #[test]
    fn gap_histogram_sorts_by_count_then_gap() {
        // Gaps: 2,2,4,4 (equal counts) -> smaller gap first.
        let offsets = [0usize, 2, 4, 8, 12];
        assert_eq!(gap_histogram(&offsets), vec![(2, 2), (4, 2)]);
    }

    #[test]
    fn carve_splits_at_each_magic() {
        // Two "msq" blocks back to back.
        let data = b"msqABmsqCDE";
        assert_eq!(
            carve(data, b"msq"),
            vec![
                Segment {
                    offset: 0,
                    len: 5,
                    has_magic: true,
                },
                Segment {
                    offset: 5,
                    len: 6,
                    has_magic: true,
                },
            ]
        );
    }

    #[test]
    fn carve_keeps_a_leading_preamble() {
        // Junk before the first magic becomes a preamble segment.
        let data = b"XXmsqAB";
        assert_eq!(
            carve(data, b"msq"),
            vec![
                Segment {
                    offset: 0,
                    len: 2,
                    has_magic: false,
                },
                Segment {
                    offset: 2,
                    len: 5,
                    has_magic: true,
                },
            ]
        );
    }

    #[test]
    fn carve_without_a_match_yields_one_segment() {
        assert_eq!(
            carve(b"hello", b"zzz"),
            vec![Segment {
                offset: 0,
                len: 5,
                has_magic: false,
            }]
        );
    }
}
