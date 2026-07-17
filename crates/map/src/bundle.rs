//! Bake a rendered world image into a web-servable bundle: a `z/x/y` PNG tile pyramid plus a
//! `manifest.json`. The bundle is self-describing and game-agnostic — the viewer reads only the
//! manifest and the tiles, with no knowledge of any game format.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use image::{
    imageops::{self, FilterType},
    DynamicImage, RgbImage, Rgba, RgbaImage,
};
use serde::Serialize;

/// Web tile edge length, in pixels (standard slippy-map tile size).
pub const TILE_SIZE: u32 = 256;

/// Identity + labelling for one exported world.
pub struct WorldMeta {
    pub game: String,
    pub world: String,
    pub title: String,
    /// Map category, used for badges and ordering, e.g. `overworld` or `town`.
    pub kind: String,
    /// A key that clusters related maps within a game so the browser can list a world's
    /// sub-maps (towns, castles) beside it — e.g. every Ultima II map in the same region shares
    /// one group. Worlds with the same `(game, group)` belong together.
    pub group: String,
}

/// A fully rendered world ready to be baked into a bundle: its identity, composite image, and
/// points of interest. Each game's exporter produces one or more of these (Ultima I has a single
/// overworld; Ultima II has many map files), and the bake pipeline is otherwise game-agnostic.
pub struct World {
    pub meta: WorldMeta,
    pub image: RgbImage,
    pub pois: Vec<Poi>,
}

/// A point of interest on a map, in image **pixel** coordinates (so the viewer needs no
/// game-tile knowledge). `kind` groups markers (e.g. `castle`, `town`, `signpost`).
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Poi {
    pub px: u32,
    pub py: u32,
    pub kind: String,
    pub label: String,
}

/// The `manifest.json` a viewer reads to render a world. Serialized as camelCase for JS.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Manifest {
    game: String,
    world: String,
    title: String,
    kind: String,
    group: String,
    tile_size: u32,
    min_zoom: u32,
    max_zoom: u32,
    width: u32,
    height: u32,
    tile_pattern: String,
    pois: Vec<Poi>,
}

/// Write the bundle for `world` under `<export_root>/<game>/<world>/`, returning that directory.
pub fn write_bundle(export_root: &Path, world: &World) -> Result<PathBuf> {
    let meta = &world.meta;
    let image = &world.image;
    let dir = export_root.join(&meta.game).join(&meta.world);
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;

    let width = image.width();
    let height = image.height();
    let max_zoom = zoom_levels(width.max(height));

    // z == max_zoom is native resolution; each lower level halves the image (a mipmap chain).
    let mut level: RgbaImage = DynamicImage::ImageRgb8(image.clone()).into_rgba8();
    for z in (0..=max_zoom).rev() {
        write_level(&level, &dir, z)
            .with_context(|| format!("writing zoom level {z} of {}", dir.display()))?;
        if z > 0 {
            let w = (level.width() / 2).max(1);
            let h = (level.height() / 2).max(1);
            level = imageops::resize(&level, w, h, FilterType::Triangle);
        }
    }

    let manifest = Manifest {
        game: meta.game.clone(),
        world: meta.world.clone(),
        title: meta.title.clone(),
        kind: meta.kind.clone(),
        group: meta.group.clone(),
        tile_size: TILE_SIZE,
        min_zoom: 0,
        max_zoom,
        width,
        height,
        tile_pattern: "{z}/{x}/{y}.png".to_string(),
        pois: world.pois.clone(),
    };
    let json = serde_json::to_string_pretty(&manifest)?;
    std::fs::write(dir.join("manifest.json"), json)
        .with_context(|| format!("writing manifest to {}", dir.display()))?;
    Ok(dir)
}

/// Number of halvings needed for the largest dimension to fit within a single tile.
fn zoom_levels(max_dim: u32) -> u32 {
    let mut z = 0;
    while (max_dim >> z) > TILE_SIZE {
        z += 1;
    }
    z
}

/// Slice one pyramid level into `<dir>/<z>/<x>/<y>.png` tiles, padding edge tiles transparently.
fn write_level(level: &RgbaImage, dir: &Path, z: u32) -> Result<()> {
    let cols = level.width().div_ceil(TILE_SIZE);
    let rows = level.height().div_ceil(TILE_SIZE);
    for tx in 0..cols {
        let x_dir = dir.join(z.to_string()).join(tx.to_string());
        std::fs::create_dir_all(&x_dir)?;
        for ty in 0..rows {
            let sx = tx * TILE_SIZE;
            let sy = ty * TILE_SIZE;
            let w = TILE_SIZE.min(level.width() - sx);
            let h = TILE_SIZE.min(level.height() - sy);
            let mut tile = RgbaImage::from_pixel(TILE_SIZE, TILE_SIZE, Rgba([0, 0, 0, 0]));
            let sub = imageops::crop_imm(level, sx, sy, w, h).to_image();
            imageops::replace(&mut tile, &sub, 0, 0);
            tile.save(x_dir.join(format!("{ty}.png")))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zoom_levels_fit_one_tile() {
        assert_eq!(zoom_levels(256), 0);
        assert_eq!(zoom_levels(257), 1);
        assert_eq!(zoom_levels(512), 1);
        assert_eq!(zoom_levels(2688), 4); // Ultima I overworld width
    }

    #[test]
    fn writes_pyramid_and_manifest() {
        let dir = tempfile::tempdir().unwrap();
        let world = World {
            meta: WorldMeta {
                game: "ultima1".into(),
                world: "overworld".into(),
                title: "Test".into(),
                kind: "overworld".into(),
                group: "overworld".into(),
            },
            image: RgbImage::from_pixel(300, 300, image::Rgb([1, 2, 3])),
            pois: vec![],
        };
        let bundle = write_bundle(dir.path(), &world).unwrap();
        assert!(bundle.join("manifest.json").exists());
        // 300px → max_zoom 1: z1 is 2×2 tiles, z0 is a single tile.
        assert!(bundle.join("1/0/0.png").exists());
        assert!(bundle.join("1/1/1.png").exists());
        assert!(bundle.join("0/0/0.png").exists());
        assert!(!bundle.join("0/1/0.png").exists());

        let manifest: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(bundle.join("manifest.json")).unwrap())
                .unwrap();
        assert_eq!(manifest["maxZoom"], 1);
        assert_eq!(manifest["tileSize"], 256);
        assert_eq!(manifest["width"], 300);
    }
}
