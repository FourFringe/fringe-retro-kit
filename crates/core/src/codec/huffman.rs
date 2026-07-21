//! Wasteland's Huffman decompression, used by its map tile-maps and its `ALLHTDS` tile graphics.
//!
//! The compressed stream is a serialized Huffman tree followed by the coded symbols, read
//! **MSB-first**. A tree node is one bit: `0` = internal (then the left subtree, one discarded
//! separator bit, then the right subtree), `1` = leaf (then an 8-bit payload byte). Decoding walks
//! the tree from the root — `0` left, `1` right — until a leaf. The output length is known from
//! context (the caller passes `count`); the stream carries no length. Mirrors
//! `HuffmanTree`/`HuffmanInputStream` in Klaus Reimer's `wlandsuite`.

use crate::{Error, Result};

/// An MSB-first bit reader over a byte slice.
struct BitReader<'a> {
    data: &'a [u8],
    pos: usize, // in bits
}

impl<'a> BitReader<'a> {
    fn new(data: &'a [u8], byte_offset: usize) -> Self {
        BitReader {
            data,
            pos: byte_offset * 8,
        }
    }

    fn bit(&mut self) -> Result<u8> {
        let byte = self.pos / 8;
        if byte >= self.data.len() {
            return Err(Error::Format("Huffman stream ran out of bits".into()));
        }
        let b = (self.data[byte] >> (7 - (self.pos % 8))) & 1;
        self.pos += 1;
        Ok(b)
    }

    fn byte(&mut self) -> Result<u8> {
        let mut v = 0u8;
        for _ in 0..8 {
            v = (v << 1) | self.bit()?;
        }
        Ok(v)
    }
}

/// A Huffman tree node.
enum Node {
    Leaf(u8),
    Branch(Box<Node>, Box<Node>),
}

/// Read one tree node (and its subtree) from the bit stream. Tree depth is bounded by 256, so the
/// recursion can't overflow the stack.
fn load_node(bits: &mut BitReader) -> Result<Node> {
    if bits.bit()? == 0 {
        let left = load_node(bits)?;
        bits.bit()?; // separator bit between the children, discarded
        let right = load_node(bits)?;
        Ok(Node::Branch(Box::new(left), Box::new(right)))
    } else {
        Ok(Node::Leaf(bits.byte()?))
    }
}

/// Decompress exactly `count` bytes of Huffman data whose stream begins at byte offset `start` in
/// `data`. Returns the decompressed bytes and the byte offset just past the consumed input (so a
/// caller reading a sequence of blocks can continue from there).
pub fn decompress(data: &[u8], start: usize, count: usize) -> Result<(Vec<u8>, usize)> {
    if start >= data.len() {
        return Err(Error::Format("Huffman start offset out of range".into()));
    }
    let mut bits = BitReader::new(data, start);
    let root = load_node(&mut bits)?;
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        let mut node = &root;
        loop {
            match node {
                Node::Leaf(b) => {
                    out.push(*b);
                    break;
                }
                Node::Branch(left, right) => {
                    node = if bits.bit()? == 0 { left } else { right };
                }
            }
        }
    }
    let end = bits.pos.div_ceil(8);
    Ok((out, end))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pack a list of MSB-first bits into bytes (padding the final byte with zeros).
    fn pack(bits: &[u8]) -> Vec<u8> {
        let mut out = vec![0u8; bits.len().div_ceil(8)];
        for (i, &b) in bits.iter().enumerate() {
            if b != 0 {
                out[i / 8] |= 1 << (7 - (i % 8));
            }
        }
        out
    }

    /// Build a bit list for a leaf node: `1` then the 8 payload bits (MSB-first).
    fn leaf(byte: u8) -> Vec<u8> {
        let mut v = vec![1];
        for i in (0..8).rev() {
            v.push((byte >> i) & 1);
        }
        v
    }

    #[test]
    fn decompresses_two_symbol_tree() {
        // Tree: internal(leaf 'A', leaf 'B'). Encoding: 0 <leaf A> 0(sep) <leaf B>.
        // Codes: 'A' = bit 0, 'B' = bit 1.
        let mut stream = vec![0u8]; // internal marker
        stream.extend(leaf(b'A'));
        stream.push(0); // separator
        stream.extend(leaf(b'B'));
        // Then the data: A B A B B  -> 0 1 0 1 1
        stream.extend([0, 1, 0, 1, 1]);
        let bytes = pack(&stream);
        let (out, _) = decompress(&bytes, 0, 5).unwrap();
        assert_eq!(out, b"ABABB");
    }

    #[test]
    fn single_leaf_tree_repeats() {
        // Tree: just a leaf 'Z'. Every decoded byte is 'Z' (no bits consumed per symbol).
        let stream = leaf(b'Z');
        let bytes = pack(&stream);
        let (out, _) = decompress(&bytes, 0, 4).unwrap();
        assert_eq!(out, b"ZZZZ");
    }
}
