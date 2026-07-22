//! Shared top-down rendering for the first-person Ultima dungeons.
//!
//! Ultima II, III, IV and V play their dungeons in first person, so the games ship no top-down art
//! for the dungeon cells — but each stores the maze itself as a tile grid. (Ultima I's dungeons are
//! generated at runtime and have no stored map; Ultima VI's dungeons render through the main object
//! pipeline.) The per-game byte encodings differ, so each game supplies a classifier that maps its
//! raw tile bytes into a common [`Cell`]; this module synthesises a small "graph-paper" image for
//! each cell and builds the 256-entry tile set that `tilemap::render` draws.
//!
//! The stored layouts and per-game tile-code tables are documented in each game's
//! `docs/formats/ultimaN.md`. In brief:
//!
//! - **Ultima II**: `MAP[XG]N5` (dungeon) / `MAP[XG]N4` (tower) — sixteen 16×16 levels each.
//! - **Ultima III**: `*.ULT` (2192 bytes) — eight 16×16 levels + a per-level name table.
//! - **Ultima IV**: `*.DNG` — a 512-byte level map of eight 8×8 levels (room data follows).
//! - **Ultima V**: `DUNGEON.DAT` — eight dungeons × eight 8×8 levels.
//!
//! The byte encodings are game-specific: high-nibble type codes are common, but the same nibble
//! means different things per game (e.g. `0x8` is a trap in Ultima IV but a wall in Ultima III),
//! which is why the classifier lives with each game rather than here.

use image::{Rgb, RgbImage};

/// Edge, in pixels, of a synthesised dungeon-cell image.
pub const TILE: u32 = 32;

/// What a dungeon cell means, abstracted over the games' differing byte encodings.
#[derive(Clone, Copy)]
pub enum Cell {
    /// Open hallway / corridor.
    Floor,
    /// Solid wall.
    Wall,
    /// A decorated wall (e.g. Ultima V's skeleton in manacles).
    AltWall,
    /// A normal door.
    Door,
    /// A secret door — looks like a wall, revealed here with a faint seam.
    SecretDoor,
    /// A ladder up and/or down.
    Ladder { up: bool, down: bool },
    /// A treasure chest.
    Chest,
    /// A fountain.
    Fountain,
    /// A trap, pit, or floor/ceiling hole.
    Trap,
    /// A magic orb (Ultima IV).
    Orb,
    /// An altar (Ultima IV).
    Altar,
    /// An energy field.
    Field(Field),
    /// A room — a separate combat map you drop into.
    Room,
}

/// The four energy-field kinds.
#[derive(Clone, Copy)]
pub enum Field {
    Sleep,
    Poison,
    Fire,
    Energy,
}

/// Classify the low two bits of a field code into a [`Field`] kind.
pub fn field(bits: u8) -> Field {
    match bits & 0x3 {
        0 => Field::Sleep,
        1 => Field::Poison,
        2 => Field::Fire,
        _ => Field::Energy,
    }
}

const WALL: Rgb<u8> = Rgb([54, 52, 64]);
const WALL_ALT: Rgb<u8> = Rgb([80, 54, 54]);
const WALL_EDGE: Rgb<u8> = Rgb([32, 30, 40]);
const FLOOR: Rgb<u8> = Rgb([206, 198, 176]);
const GRID: Rgb<u8> = Rgb([176, 168, 148]);
const DOOR: Rgb<u8> = Rgb([148, 96, 42]);
const SECRET: Rgb<u8> = Rgb([170, 60, 150]);
const LADDER_UP: Rgb<u8> = Rgb([232, 202, 44]);
const LADDER_DOWN: Rgb<u8> = Rgb([228, 138, 40]);
const CHEST: Rgb<u8> = Rgb([150, 102, 40]);
const FOUNTAIN: Rgb<u8> = Rgb([56, 120, 216]);
const TRAP: Rgb<u8> = Rgb([196, 44, 44]);
const ROOM: Rgb<u8> = Rgb([150, 70, 180]);
const ORB: Rgb<u8> = Rgb([70, 208, 220]);
const ALTAR: Rgb<u8> = Rgb([120, 132, 150]);

/// Build the 256 cell images a game passes to `tilemap::render`, from its byte → [`Cell`]
/// classifier (one image per possible tile byte).
pub fn tileset(classify: impl Fn(u8) -> Cell) -> Vec<RgbImage> {
    (0..=u8::MAX).map(|b| cell_image(classify(b))).collect()
}

/// Synthesise the top-down image for one [`Cell`].
pub fn cell_image(cell: Cell) -> RgbImage {
    match cell {
        Cell::Wall => return wall_tile(WALL, false),
        Cell::AltWall => return wall_tile(WALL_ALT, false),
        Cell::SecretDoor => return wall_tile(WALL, true),
        _ => {}
    }
    let mut img = floor_tile();
    match cell {
        Cell::Door => fill_rect(&mut img, 6, 13, 26, 19, DOOR),
        Cell::Ladder { up, down } => {
            if up {
                fill_rect(&mut img, 9, 9, 23, 16, LADDER_UP);
            }
            if down {
                fill_rect(&mut img, 9, 16, 23, 23, LADDER_DOWN);
            }
        }
        Cell::Chest => fill_rect(&mut img, 9, 10, 23, 22, CHEST),
        Cell::Fountain => fill_rect(&mut img, 9, 9, 23, 23, FOUNTAIN),
        Cell::Trap => fill_rect(&mut img, 10, 10, 22, 22, TRAP),
        Cell::Orb => fill_rect(&mut img, 11, 11, 21, 21, ORB),
        Cell::Altar => fill_rect(&mut img, 9, 12, 23, 22, ALTAR),
        Cell::Field(f) => fill_rect(&mut img, 5, 5, 27, 27, field_color(f)),
        Cell::Room => {
            // A bold frame on the floor cell.
            fill_rect(&mut img, 5, 5, 27, 7, ROOM);
            fill_rect(&mut img, 5, 25, 27, 27, ROOM);
            fill_rect(&mut img, 5, 5, 7, 27, ROOM);
            fill_rect(&mut img, 25, 5, 27, 27, ROOM);
        }
        Cell::Floor | Cell::Wall | Cell::AltWall | Cell::SecretDoor => {}
    }
    img
}

fn field_color(f: Field) -> Rgb<u8> {
    match f {
        Field::Sleep => Rgb([150, 80, 200]),
        Field::Poison => Rgb([70, 180, 70]),
        Field::Fire => Rgb([232, 96, 32]),
        Field::Energy => Rgb([232, 220, 44]),
    }
}

/// A parchment floor cell with a faint grid border.
fn floor_tile() -> RgbImage {
    let mut img = RgbImage::from_pixel(TILE, TILE, FLOOR);
    border(&mut img, GRID);
    img
}

/// A solid wall cell; `secret` adds a faint seam hinting at a hidden door.
fn wall_tile(fill: Rgb<u8>, secret: bool) -> RgbImage {
    let mut img = RgbImage::from_pixel(TILE, TILE, fill);
    border(&mut img, WALL_EDGE);
    if secret {
        fill_rect(&mut img, TILE / 2 - 1, 6, TILE / 2 + 1, TILE - 6, SECRET);
    }
    img
}

/// Fill the half-open rectangle `[x0, x1) × [y0, y1)` with `c`, clipped to the image.
fn fill_rect(img: &mut RgbImage, x0: u32, y0: u32, x1: u32, y1: u32, c: Rgb<u8>) {
    for y in y0..y1.min(img.height()) {
        for x in x0..x1.min(img.width()) {
            img.put_pixel(x, y, c);
        }
    }
}

/// Draw a one-pixel border around the image.
fn border(img: &mut RgbImage, c: Rgb<u8>) {
    let (w, h) = (img.width(), img.height());
    for x in 0..w {
        img.put_pixel(x, 0, c);
        img.put_pixel(x, h - 1, c);
    }
    for y in 0..h {
        img.put_pixel(0, y, c);
        img.put_pixel(w - 1, y, c);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cells_render_expected_colours() {
        let mid = TILE / 2;
        assert_eq!(*cell_image(Cell::Wall).get_pixel(mid, mid), WALL);
        assert_eq!(*cell_image(Cell::AltWall).get_pixel(mid, mid), WALL_ALT);
        assert_eq!(*cell_image(Cell::Floor).get_pixel(mid, mid), FLOOR);
        assert_eq!(*cell_image(Cell::Door).get_pixel(mid, 15), DOOR);
        // A secret door reads as a wall but carries the seam colour down its centre.
        assert_eq!(*cell_image(Cell::SecretDoor).get_pixel(mid, mid), SECRET);
        assert_eq!(tileset(|_| Cell::Floor).len(), 256);
    }
}
