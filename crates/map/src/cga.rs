//! CGA 2-bpp tile decoding, as used by Ultima II.
//!
//! Ultima II's tiles are stored as 16×16 images in **CGA 4-colour** format: each pixel is 2 bits,
//! packed 4 pixels per byte (the most-significant pair is the leftmost pixel), rows stored
//! top-to-bottom with no interleaving. So one tile is 16 rows × 4 bytes = 64 bytes. The 2-bit
//! value indexes a 4-colour palette; the game uses CGA palette 1 (high intensity):
//! 0 = black, 1 = cyan, 2 = magenta, 3 = white.

use image::{Rgb, RgbImage};

/// Tile edge length, in pixels.
pub const TILE_SIZE: u32 = 16;

/// Bytes of pixel data per 16×16 tile (16 rows × 4 bytes).
pub const BYTES_PER_TILE: usize = (TILE_SIZE as usize * TILE_SIZE as usize) / 4;

/// CGA palette 1, high intensity: black, cyan, magenta, white.
pub const CGA_PALETTE1: [Rgb<u8>; 4] = [
    Rgb([0, 0, 0]),
    Rgb([0x55, 0xFF, 0xFF]),
    Rgb([0xFF, 0x55, 0xFF]),
    Rgb([0xFF, 0xFF, 0xFF]),
];

/// Decode one 16×16 CGA 2-bpp tile (`BYTES_PER_TILE` bytes) into an RGB image using `palette`.
/// Bytes shorter than expected are treated as trailing zero (black) pixels.
pub fn decode_tile(bytes: &[u8], palette: &[Rgb<u8>; 4]) -> RgbImage {
    let mut img = RgbImage::new(TILE_SIZE, TILE_SIZE);
    for y in 0..TILE_SIZE {
        for byte_col in 0..(TILE_SIZE / 4) {
            let idx = (y * (TILE_SIZE / 4) + byte_col) as usize;
            let byte = bytes.get(idx).copied().unwrap_or(0);
            for pair in 0..4u32 {
                // Most-significant pair is the leftmost pixel.
                let shift = 6 - pair * 2;
                let value = (byte >> shift) & 0b11;
                let x = byte_col * 4 + pair;
                img.put_pixel(x, y, palette[value as usize]);
            }
        }
    }
    img
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_zero_is_black() {
        let tile = decode_tile(&[0u8; BYTES_PER_TILE], &CGA_PALETTE1);
        assert_eq!(*tile.get_pixel(0, 0), Rgb([0, 0, 0]));
        assert_eq!(*tile.get_pixel(15, 15), Rgb([0, 0, 0]));
    }

    #[test]
    fn all_ones_is_white() {
        // 0xFF = four pixels of colour index 3 (white).
        let tile = decode_tile(&[0xFFu8; BYTES_PER_TILE], &CGA_PALETTE1);
        assert_eq!(*tile.get_pixel(0, 0), Rgb([0xFF, 0xFF, 0xFF]));
        assert_eq!(*tile.get_pixel(15, 0), Rgb([0xFF, 0xFF, 0xFF]));
    }

    #[test]
    fn pixel_pairs_decode_left_to_right() {
        // First byte 0b00_01_10_11 → indices 0,1,2,3 across the first four pixels.
        let mut bytes = [0u8; BYTES_PER_TILE];
        bytes[0] = 0b00_01_10_11;
        let tile = decode_tile(&bytes, &CGA_PALETTE1);
        assert_eq!(*tile.get_pixel(0, 0), CGA_PALETTE1[0]);
        assert_eq!(*tile.get_pixel(1, 0), CGA_PALETTE1[1]);
        assert_eq!(*tile.get_pixel(2, 0), CGA_PALETTE1[2]);
        assert_eq!(*tile.get_pixel(3, 0), CGA_PALETTE1[3]);
    }

    #[test]
    fn short_input_pads_black() {
        let tile = decode_tile(&[], &CGA_PALETTE1);
        assert_eq!(*tile.get_pixel(8, 8), Rgb([0, 0, 0]));
    }
}
