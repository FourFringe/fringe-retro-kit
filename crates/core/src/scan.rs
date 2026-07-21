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
}
