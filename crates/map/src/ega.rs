//! EGA graphics decoding for classic DOS tiles.
//!
//! The Ultima I DOS tiles are 16×16 pixels in the standard 16-colour EGA palette, stored as
//! four bit-planes **row-interleaved**: each pixel row is `plane0 plane1 plane2 plane3`, with
//! two bytes (16 pixels, MSB = leftmost) per plane. A pixel's colour index is
//! `p0 | p1<<1 | p2<<2 | p3<<3`. See `docs`/memory for the reverse-engineering notes.

use image::{Rgb, RgbImage};

use crate::tilemap::TILE_SIZE;

/// Bytes per tile: 16 rows × 4 planes × 2 bytes.
pub const BYTES_PER_TILE: usize = 128;

/// The standard EGA 16-colour palette (RGB).
pub const EGA_PALETTE: [Rgb<u8>; 16] = [
    Rgb([0, 0, 0]),
    Rgb([0, 0, 170]),
    Rgb([0, 170, 0]),
    Rgb([0, 170, 170]),
    Rgb([170, 0, 0]),
    Rgb([170, 0, 170]),
    Rgb([170, 85, 0]),
    Rgb([170, 170, 170]),
    Rgb([85, 85, 85]),
    Rgb([85, 85, 255]),
    Rgb([85, 255, 85]),
    Rgb([85, 255, 255]),
    Rgb([255, 85, 85]),
    Rgb([255, 85, 255]),
    Rgb([255, 255, 85]),
    Rgb([255, 255, 255]),
];

/// Decode one row-interleaved 4-plane EGA tile (`BYTES_PER_TILE` bytes) into a 16×16 image,
/// looking each pixel's colour index up in `palette` (pass [`EGA_PALETTE`] for the standard
/// colours, or a remapped one to brighten or tint the output).
///
/// Panics if `bytes` is shorter than [`BYTES_PER_TILE`]; callers slice per tile.
pub fn decode_tile(bytes: &[u8], palette: &[Rgb<u8>; 16]) -> RgbImage {
    let mut img = RgbImage::new(TILE_SIZE, TILE_SIZE);
    for y in 0..TILE_SIZE {
        let row = (y as usize) * 8;
        for x in 0..TILE_SIZE {
            let byte_in_plane = (x / 8) as usize; // 0 for pixels 0..8, 1 for 8..16
            let bit = 7 - (x % 8); // MSB is the leftmost pixel
            let mut color = 0u8;
            for plane in 0..4 {
                let b = bytes[row + plane * 2 + byte_in_plane];
                color |= ((b >> bit) & 1) << plane;
            }
            img.put_pixel(x, y, palette[color as usize]);
        }
    }
    img
}

/// Decode a tileset blob into its constituent 16×16 tiles, colouring them with `palette`.
pub fn decode_tileset(data: &[u8], palette: &[Rgb<u8>; 16]) -> Vec<RgbImage> {
    data.chunks_exact(BYTES_PER_TILE)
        .map(|tile| decode_tile(tile, palette))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_zero_tile_is_black() {
        let tile = decode_tile(&[0u8; BYTES_PER_TILE], &EGA_PALETTE);
        assert_eq!(*tile.get_pixel(0, 0), EGA_PALETTE[0]);
        assert_eq!(*tile.get_pixel(15, 15), EGA_PALETTE[0]);
    }

    #[test]
    fn all_ones_tile_is_white() {
        let tile = decode_tile(&[0xFFu8; BYTES_PER_TILE], &EGA_PALETTE);
        assert_eq!(*tile.get_pixel(0, 0), EGA_PALETTE[15]);
        assert_eq!(*tile.get_pixel(15, 15), EGA_PALETTE[15]);
    }

    #[test]
    fn only_plane0_set_is_color_one() {
        // Set just plane 0 (the two low bytes of every row) → colour index 1 everywhere.
        let mut bytes = [0u8; BYTES_PER_TILE];
        for y in 0..16 {
            bytes[y * 8] = 0xFF;
            bytes[y * 8 + 1] = 0xFF;
        }
        let tile = decode_tile(&bytes, &EGA_PALETTE);
        assert_eq!(*tile.get_pixel(0, 0), EGA_PALETTE[1]);
        assert_eq!(*tile.get_pixel(15, 0), EGA_PALETTE[1]);
    }

    #[test]
    fn decode_tileset_splits_by_tile() {
        let data = vec![0u8; BYTES_PER_TILE * 3];
        assert_eq!(decode_tileset(&data, &EGA_PALETTE).len(), 3);
    }
}
