//! Wasteland (1988) save support. **Work in progress.**
//!
//! Wasteland stores its saved game in `GAME1` as a series of **MSQ blocks**. Each block
//! begins with a 4-byte `msqN` header (`N` is the disk digit), followed by two seed bytes
//! and a ciphertext body encrypted with a simple "rotating XOR" stream cipher.
//!
//! The cipher (from Klaus Reimer's `wlandsuite`, <https://github.com/kayahr/wlandsuite>):
//! the initial key is `seed0 ^ seed1`, each decrypted byte is `cipher ^ key`, and after
//! every byte the key advances by `0x1F` (wrapping at 256). Verified against a real
//! `GAME1`: seed `bf f0` decrypts the first bytes to a run of `0xBB`.

use crate::{Error, Result};

/// The value added to the rolling XOR key after each decrypted byte.
const KEY_STEP: u8 = 0x1F;

/// Decrypt a single MSQ block.
///
/// `block` must start with the 4-byte `msqN` header, then the two seed bytes, then the
/// ciphertext. Returns the decrypted body (everything after the seed bytes).
pub fn decrypt(block: &[u8]) -> Result<Vec<u8>> {
    if block.len() < 6 {
        return Err(Error::Format(format!(
            "MSQ block too short: {} bytes",
            block.len()
        )));
    }
    if &block[0..3] != b"msq" {
        return Err(Error::Format(
            "not an MSQ block (missing 'msq' header)".into(),
        ));
    }
    let mut key = block[4] ^ block[5];
    let mut out = Vec::with_capacity(block.len() - 6);
    for &cipher in &block[6..] {
        out.push(cipher ^ key);
        key = key.wrapping_add(KEY_STEP);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decrypts_rotating_xor() {
        // Verified against a real GAME1 save: seed bytes bf f0 decrypt the leading
        // ciphertext to a run of 0xBB (the save game's initial fill).
        let block = [
            b'm', b's', b'q', b'0', 0xbf, 0xf0, 0xf4, 0xd5, 0x36, 0x17, 0x70, 0x51,
        ];
        let out = decrypt(&block).unwrap();
        assert_eq!(out, vec![0xbb; 6]);
    }

    #[test]
    fn rejects_non_msq_block() {
        let block = [b'x', b'y', b'z', b'0', 0x00, 0x00, 0x11];
        assert!(decrypt(&block).is_err());
    }

    #[test]
    fn rejects_short_block() {
        assert!(decrypt(b"msq0").is_err());
    }
}
