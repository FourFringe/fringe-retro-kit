//! Block **checksums** and a small solver that identifies which algorithm produced a stored check
//! value. Retro formats guard blocks with an ad-hoc checksum (a sum, a negated sum, an XOR); the
//! solver automates the "which variant is it?" detective work (e.g. Wasteland's carry-folded,
//! negated sum, distinct from the plain negated sum other tools assume).

/// A plain 16-bit sum of every byte (wrapping).
pub fn sum16(data: &[u8]) -> u16 {
    data.iter().fold(0u16, |acc, &b| acc.wrapping_add(b as u16))
}

/// Two's-complement negation of [`sum16`] — a reader that subtracts each byte from zero lands on
/// the stored value. (This is `wlandsuite`'s Wasteland variant; it is *not* byte-faithful to the
/// shipped game, which folds carries — see [`wasteland_msq`].)
pub fn negated_sum16(data: &[u8]) -> u16 {
    0u16.wrapping_sub(sum16(data))
}

/// A 16-bit sum that folds each 16-bit overflow back in as `+0x100` (an artifact of the original
/// game's byte-wise add-with-carry), without negating.
pub fn carry_fold_sum16(data: &[u8]) -> u16 {
    let mut acc: u32 = 0;
    for &b in data {
        acc += b as u32;
        if acc > 0xFFFF {
            acc = (acc & 0xFFFF) + 0x100;
        }
    }
    (acc & 0xFFFF) as u16
}

/// Wasteland's MSQ block checksum: [`carry_fold_sum16`] then two's-complement negated, stored
/// little-endian as the two seed bytes. Reproduces the shipped game's saves exactly.
pub fn wasteland_msq(data: &[u8]) -> u16 {
    0u16.wrapping_sub(carry_fold_sum16(data))
}

/// XOR of every byte.
pub fn xor8(data: &[u8]) -> u8 {
    data.iter().fold(0u8, |acc, &b| acc ^ b)
}

/// An 8-bit sum of every byte (wrapping).
pub fn sum8(data: &[u8]) -> u8 {
    data.iter().fold(0u8, |acc, &b| acc.wrapping_add(b))
}

/// A named checksum algorithm, widened to `u32` so 8- and 16-bit variants share one signature.
type Algorithm = fn(&[u8]) -> u32;

/// The candidate algorithms the solver tries, in order.
pub const ALGORITHMS: &[(&str, Algorithm)] = &[
    ("sum8", |d| sum8(d) as u32),
    ("xor8", |d| xor8(d) as u32),
    ("sum16", |d| sum16(d) as u32),
    ("negated_sum16", |d| negated_sum16(d) as u32),
    ("carry_fold_sum16", |d| carry_fold_sum16(d) as u32),
    ("wasteland_msq", |d| wasteland_msq(d) as u32),
];

/// Every candidate algorithm whose output over `data` equals `expected` (compared at the width of
/// each algorithm's result). Useful for recovering an unknown format's checksum from a known block.
pub fn solve(data: &[u8], expected: u32) -> Vec<&'static str> {
    ALGORITHMS
        .iter()
        .filter(|(_, f)| f(data) == expected)
        .map(|(name, _)| *name)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wasteland_checksum_matches_the_game() {
        // No 16-bit overflow: a plain negated sum. sum(1,2,3)=6 -> -6.
        assert_eq!(wasteland_msq(&[1, 2, 3]), 0xFFFA);
        // Overflows 16 bits, exercising the carry fold (golden value from a real GAME1).
        assert_eq!(wasteland_msq(&[0xFF; 300]), 0xD42C);
    }

    #[test]
    fn solver_identifies_the_wasteland_algorithm() {
        let data = [0xFFu8; 300];
        let expected = wasteland_msq(&data) as u32;
        let matches = solve(&data, expected);
        assert!(matches.contains(&"wasteland_msq"));
        // The plain negated sum differs here (carry fold), so it is not reported.
        assert!(!matches.contains(&"negated_sum16"));
    }
}
