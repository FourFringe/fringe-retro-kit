//! Shared helpers for turning a game's tile grid into an exportable [`World`].
//!
//! Every supported game ultimately produces the same three things — a grid of tile indices, a set
//! of points of interest in tile coordinates, and some identity metadata — so the composite render,
//! the POI pixel maths, and the [`World`] construction live here once. The format-specific parts
//! (decoding a tileset, unpacking or de-chunking a map file, reading a location table) stay in each
//! game's module.

use image::RgbImage;

use crate::bundle::{Poi, World, WorldMeta};

/// Source tile edge length, in pixels. Every supported game uses 16×16 tiles, so this is the one
/// canonical value the tileset decoders and this module share.
pub const TILE_SIZE: u32 = 16;

/// Composite a `w`×`h` grid of tile indices (row-major) into a full-resolution image, looking each
/// index up in `tiles`. Out-of-range indices fall back to tile 0, so a short tileset can't panic
/// the render. The tile edge is taken from the tileset itself (16 px for most games; Ultima VI
/// renders its huge 1024×1024 world at a smaller edge to keep the composite tractable).
///
/// Panics if `tiles` is empty; callers ensure at least one tile was decoded.
pub fn render(grid: &[u8], w: usize, h: usize, tiles: &[RgbImage]) -> RgbImage {
    let ts = tiles[0].width();
    let mut image = RgbImage::new(w as u32 * ts, h as u32 * ts);
    for ty in 0..h {
        for tx in 0..w {
            let index = grid[ty * w + tx] as usize;
            let tile = tiles.get(index).unwrap_or(&tiles[0]);
            image::imageops::replace(
                &mut image,
                tile,
                (tx as u32 * ts) as i64,
                (ty as u32 * ts) as i64,
            );
        }
    }
    image
}

/// A [`Poi`] at the centre of tile `(tx, ty)`, in image pixel coordinates.
pub fn poi(tx: u32, ty: u32, kind: &str, label: &str) -> Poi {
    Poi {
        px: tx * TILE_SIZE + TILE_SIZE / 2,
        py: ty * TILE_SIZE + TILE_SIZE / 2,
        kind: kind.to_string(),
        label: label.to_string(),
        target: None,
    }
}

/// Build a [`World`] from its identity, points of interest, and rendered image.
pub fn world(
    game: &str,
    id: &str,
    title: &str,
    kind: &str,
    group: &str,
    pois: Vec<Poi>,
    image: RgbImage,
) -> World {
    World {
        meta: WorldMeta {
            game: game.into(),
            world: id.into(),
            title: title.into(),
            kind: kind.into(),
            group: group.into(),
            map_id: None,
        },
        image,
        pois,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgb;

    #[test]
    fn render_places_tiles_by_index() {
        // Two distinct 16×16 tiles; a 2×1 grid selecting tile 1 then tile 0.
        let tiles = vec![
            RgbImage::from_pixel(TILE_SIZE, TILE_SIZE, Rgb([10, 0, 0])),
            RgbImage::from_pixel(TILE_SIZE, TILE_SIZE, Rgb([20, 0, 0])),
        ];
        let img = render(&[1, 0], 2, 1, &tiles);
        assert_eq!(img.width(), 2 * TILE_SIZE);
        assert_eq!(img.height(), TILE_SIZE);
        assert_eq!(*img.get_pixel(0, 0), Rgb([20, 0, 0])); // tile 1
        assert_eq!(*img.get_pixel(TILE_SIZE, 0), Rgb([10, 0, 0])); // tile 0
    }

    #[test]
    fn render_falls_back_to_tile_zero() {
        let tiles = vec![RgbImage::from_pixel(TILE_SIZE, TILE_SIZE, Rgb([7, 7, 7]))];
        let img = render(&[99], 1, 1, &tiles); // out-of-range index
        assert_eq!(*img.get_pixel(0, 0), Rgb([7, 7, 7]));
    }

    #[test]
    fn poi_sits_at_tile_centre() {
        let p = poi(2, 1, "town", "Britain");
        assert_eq!(
            (p.px, p.py),
            (2 * TILE_SIZE + TILE_SIZE / 2, TILE_SIZE + TILE_SIZE / 2)
        );
        assert_eq!(p.kind, "town");
        assert_eq!(p.label, "Britain");
    }
}
