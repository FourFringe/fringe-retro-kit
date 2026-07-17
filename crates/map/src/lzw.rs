//! Ultima 6-style LZW decompression, used by Ultima V and VI to pack their graphics and map data.
//!
//! A compressed buffer begins with a 4-byte little-endian **uncompressed size**, then a stream of
//! variable-width LZW codewords packed **LSB-first**. Codewords start at 9 bits and grow to 12 as
//! the dictionary fills. Two codewords are reserved: `0x100` reinitialises the dictionary (and
//! resets the width back to 9), and `0x101` marks the end of the stream. New dictionary entries
//! therefore start at `0x102`. This mirrors the algorithm in Nuvie's `U6Lzw`.

use anyhow::{ensure, Result};

/// Highest codeword value (12-bit dictionary), so scratch arrays cover every possible entry.
const MAX_CODES: usize = 0x1000;

/// Decompress a U6-style LZW buffer (including its 4-byte size header).
pub fn decompress(data: &[u8]) -> Result<Vec<u8>> {
    ensure!(data.len() >= 6, "LZW buffer too short");
    let out_size = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;
    let mut out = Vec::with_capacity(out_size);

    // Non-root dictionary entries store (root byte, previous codeword); roots 0..=0xFF are
    // implicit. Codes 0x100/0x101 are reserved, so real entries begin at 0x102.
    let mut root = [0u8; MAX_CODES];
    let mut prev = [0u16; MAX_CODES];

    let mut bit_pos = 4 * 8; // start reading just past the size header
    let mut width = 9u32;
    let mut next_free = 0x102usize;
    let mut dict_size = 0x200usize;
    let mut prev_code = 0u16;
    let mut stack: Vec<u8> = Vec::with_capacity(MAX_CODES);

    loop {
        ensure!(out.len() <= out_size, "LZW stream overran expected size");
        let code = read_code(data, &mut bit_pos, width);
        match code {
            0x100 => {
                // Reinitialise the dictionary and emit the following codeword as a root.
                width = 9;
                next_free = 0x102;
                dict_size = 0x200;
                let c = read_code(data, &mut bit_pos, width) as u8;
                out.push(c);
                prev_code = u16::from(c);
            }
            0x101 => break,
            _ => {
                stack.clear();
                // Walk the string for either `code` (already defined) or, in the KwKwK case,
                // `prev_code`. The leaf byte ends on top of the stack — that's the string's first
                // character, and the byte appended to the new dictionary entry.
                let known = (code as usize) < next_free;
                let walk_from = if known {
                    code as usize
                } else {
                    prev_code as usize
                };
                let mut cur = walk_from;
                while cur > 0xFF {
                    stack.push(root[cur]);
                    cur = prev[cur] as usize;
                }
                stack.push(cur as u8);
                let first = *stack.last().unwrap();
                while let Some(b) = stack.pop() {
                    out.push(b);
                }
                if !known {
                    // Self-referential code: its expansion is prev's string plus that first byte.
                    ensure!(code as usize == next_free, "corrupt LZW stream");
                    out.push(first);
                }
                // Add prev_code + first byte as the new entry.
                if next_free < MAX_CODES {
                    root[next_free] = first;
                    prev[next_free] = prev_code;
                    next_free += 1;
                    if next_free >= dict_size && width < 12 {
                        width += 1;
                        dict_size *= 2;
                    }
                }
                prev_code = code as u16;
            }
        }
    }

    ensure!(
        out.len() == out_size,
        "LZW produced {} bytes, header expected {out_size}",
        out.len()
    );
    Ok(out)
}

/// Read one `width`-bit codeword at `bit_pos` (LSB-first) and advance `bit_pos`.
fn read_code(data: &[u8], bit_pos: &mut usize, width: u32) -> u32 {
    let byte = *bit_pos >> 3;
    let b0 = data.get(byte).copied().unwrap_or(0) as u32;
    let b1 = data.get(byte + 1).copied().unwrap_or(0) as u32;
    let b2 = data.get(byte + 2).copied().unwrap_or(0) as u32;
    let raw = (b2 << 16) | (b1 << 8) | b0;
    let code = (raw >> (*bit_pos & 7)) & ((1u32 << width) - 1);
    *bit_pos += width as usize;
    code
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pack a sequence of `(value, bit_width)` codewords LSB-first into a byte buffer.
    fn pack(codes: &[(u32, u32)]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut acc = 0u32;
        let mut bits = 0u32;
        for &(v, w) in codes {
            acc |= v << bits;
            bits += w;
            while bits >= 8 {
                out.push((acc & 0xFF) as u8);
                acc >>= 8;
                bits -= 8;
            }
        }
        if bits > 0 {
            out.push((acc & 0xFF) as u8);
        }
        out
    }

    /// Build a full LZW buffer: 4-byte size header + a clear code + body + end code, all 9-bit.
    fn lzw_file(out_size: u32, body: &[u32]) -> Vec<u8> {
        let mut codes = vec![(0x100, 9)];
        codes.extend(body.iter().map(|&c| (c, 9)));
        codes.push((0x101, 9));
        let mut buf = out_size.to_le_bytes().to_vec();
        buf.extend(pack(&codes));
        buf
    }

    #[test]
    fn decompresses_plain_literals() {
        // "AB" as two literal roots.
        let buf = lzw_file(2, &[u32::from(b'A'), u32::from(b'B')]);
        assert_eq!(decompress(&buf).unwrap(), b"AB");
    }

    #[test]
    fn decompresses_dictionary_backreference() {
        // Encode "ABAB": A, B, then code 0x102 (the "AB" entry added after emitting B).
        let buf = lzw_file(4, &[u32::from(b'A'), u32::from(b'B'), 0x102]);
        assert_eq!(decompress(&buf).unwrap(), b"ABAB");
    }

    #[test]
    fn rejects_wrong_output_size() {
        let buf = lzw_file(99, &[u32::from(b'A')]);
        assert!(decompress(&buf).is_err());
    }
}
