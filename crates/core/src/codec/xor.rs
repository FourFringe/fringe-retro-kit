//! Rolling-XOR stream cipher (Wasteland's "MSQ" cipher). Each output byte is `input ^ key`, and
//! the key advances by a fixed `step` (wrapping) after every byte. The transform is its own
//! inverse: applying it to ciphertext yields plaintext and vice-versa, given the same `seed` and
//! `step`. Game-specific framing (deriving the seed from header bytes) stays with the caller.

/// Apply the rolling-XOR transform to `data`, starting the key at `seed` and advancing it by
/// `step` (wrapping) after each byte. Decrypts ciphertext to plaintext and re-encrypts plaintext
/// to ciphertext with the same arguments.
pub fn rolling(data: &[u8], seed: u8, step: u8) -> Vec<u8> {
    let mut key = seed;
    data.iter()
        .map(|&byte| {
            let out = byte ^ key;
            key = key.wrapping_add(step);
            out
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        let plain = b"Wasteland MSQ block body";
        let (seed, step) = (0x2f, 0x1f);
        let cipher = rolling(plain, seed, step);
        assert_ne!(&cipher[..], &plain[..]);
        // Re-applying with the same seed/step recovers the plaintext.
        assert_eq!(rolling(&cipher, seed, step), plain);
    }

    #[test]
    fn advances_key_by_step() {
        // With input all-zero, output is just the key sequence: seed, seed+step, seed+2*step, …
        let out = rolling(&[0, 0, 0, 0], 0x10, 0x1f);
        assert_eq!(out, [0x10, 0x2f, 0x4e, 0x6d]);
    }
}
