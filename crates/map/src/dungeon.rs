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
const INSCRIBE: Rgb<u8> = Rgb([120, 86, 86]);
const FLOOR: Rgb<u8> = Rgb([206, 198, 176]);
const GRID: Rgb<u8> = Rgb([176, 168, 148]);
const DOOR: Rgb<u8> = Rgb([148, 96, 42]);
const DOOR_EDGE: Rgb<u8> = Rgb([90, 58, 26]);
const KNOB: Rgb<u8> = Rgb([240, 220, 120]);
const SECRET: Rgb<u8> = Rgb([170, 60, 150]);
/// A ladder up is drawn as a green up-arrow, a ladder down as an amber down-arrow.
const LADDER_UP: Rgb<u8> = Rgb([70, 180, 80]);
const LADDER_DOWN: Rgb<u8> = Rgb([230, 150, 40]);
const CHEST: Rgb<u8> = Rgb([150, 102, 40]);
const CHEST_EDGE: Rgb<u8> = Rgb([90, 58, 26]);
const FOUNTAIN: Rgb<u8> = Rgb([56, 120, 216]);
const FOUNTAIN_EDGE: Rgb<u8> = Rgb([30, 80, 170]);
const FOUNTAIN_CORE: Rgb<u8> = Rgb([150, 200, 240]);
const TRAP: Rgb<u8> = Rgb([196, 44, 44]);
const ROOM: Rgb<u8> = Rgb([150, 70, 180]);
const ORB: Rgb<u8> = Rgb([70, 208, 220]);
const ORB_HI: Rgb<u8> = Rgb([220, 255, 255]);
const ALTAR: Rgb<u8> = Rgb([120, 132, 150]);
const ALTAR_TOP: Rgb<u8> = Rgb([170, 180, 196]);
const ALTAR_EDGE: Rgb<u8> = Rgb([84, 94, 110]);

/// Build the 256 cell images a game passes to `tilemap::render`, from its byte → [`Cell`]
/// classifier (one image per possible tile byte).
pub fn tileset(classify: impl Fn(u8) -> Cell) -> Vec<RgbImage> {
    (0..=u8::MAX).map(|b| cell_image(classify(b))).collect()
}

/// The ordered legend: a human label paired with the [`Cell`] whose glyph illustrates it.
/// [`legend_labels`] and [`legend_strip`] both derive from this, so the labels and the images
/// always stay in the same order.
fn legend_entries() -> Vec<(&'static str, Cell)> {
    vec![
        ("Floor", Cell::Floor),
        ("Wall", Cell::Wall),
        ("Alt-wall", Cell::AltWall),
        ("Door", Cell::Door),
        ("Secret door", Cell::SecretDoor),
        (
            "Ladder up",
            Cell::Ladder {
                up: true,
                down: false,
            },
        ),
        (
            "Ladder down",
            Cell::Ladder {
                up: false,
                down: true,
            },
        ),
        (
            "Ladder up & down",
            Cell::Ladder {
                up: true,
                down: true,
            },
        ),
        ("Chest", Cell::Chest),
        ("Fountain", Cell::Fountain),
        ("Trap", Cell::Trap),
        ("Orb", Cell::Orb),
        ("Altar", Cell::Altar),
        ("Energy field", Cell::Field(Field::Fire)),
        ("Room", Cell::Room),
    ]
}

/// The legend labels, top to bottom, matching the rows of [`legend_strip`].
pub fn legend_labels() -> Vec<&'static str> {
    legend_entries()
        .into_iter()
        .map(|(label, _)| label)
        .collect()
}

/// The legend label a cell falls under: the three ladder directions are separate entries, while
/// every energy-field kind collapses to the single "Energy field" entry.
fn legend_key(cell: Cell) -> &'static str {
    match cell {
        Cell::Floor => "Floor",
        Cell::Wall => "Wall",
        Cell::AltWall => "Alt-wall",
        Cell::Door => "Door",
        Cell::SecretDoor => "Secret door",
        Cell::Ladder {
            up: true,
            down: false,
        } => "Ladder up",
        Cell::Ladder {
            up: false,
            down: true,
        } => "Ladder down",
        Cell::Ladder { .. } => "Ladder up & down",
        Cell::Chest => "Chest",
        Cell::Fountain => "Fountain",
        Cell::Trap => "Trap",
        Cell::Orb => "Orb",
        Cell::Altar => "Altar",
        Cell::Field(_) => "Energy field",
        Cell::Room => "Room",
    }
}

/// The legend labels a `classify` function actually produces, in canonical [`legend_labels`]
/// order — the subset of the key that appears in one game's dungeons, so its viewer legend can
/// omit symbols the game never uses.
pub fn legend_for(classify: impl Fn(u8) -> Cell) -> Vec<&'static str> {
    let used: std::collections::HashSet<&'static str> =
        (0..=u8::MAX).map(|b| legend_key(classify(b))).collect();
    legend_labels()
        .into_iter()
        .filter(|label| used.contains(label))
        .collect()
}

/// A vertical sprite strip of the legend glyphs — one [`TILE`]-sized cell per row, in
/// [`legend_labels`] order — that a viewer slices to show its key beside a dungeon map.
pub fn legend_strip() -> RgbImage {
    let entries = legend_entries();
    let mut strip = RgbImage::new(TILE, TILE * entries.len() as u32);
    for (i, (_, cell)) in entries.into_iter().enumerate() {
        let glyph = cell_image(cell);
        image::imageops::replace(&mut strip, &glyph, 0, i64::from(i as u32 * TILE));
    }
    strip
}

/// Synthesise the top-down image for one [`Cell`].
pub fn cell_image(cell: Cell) -> RgbImage {
    match cell {
        Cell::Wall => return wall_tile(WALL),
        Cell::AltWall => return alt_wall_tile(),
        Cell::SecretDoor => return secret_door_tile(),
        _ => {}
    }
    let mut img = floor_tile();
    match cell {
        Cell::Door => door(&mut img),
        Cell::Ladder { up, down } => ladder(&mut img, up, down),
        Cell::Chest => chest(&mut img),
        Cell::Fountain => fountain(&mut img),
        Cell::Trap => trap(&mut img),
        Cell::Orb => orb(&mut img),
        Cell::Altar => altar(&mut img),
        Cell::Field(f) => energy_field(&mut img, field_color(f)),
        Cell::Room => room(&mut img),
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

/// A solid wall cell.
fn wall_tile(fill: Rgb<u8>) -> RgbImage {
    let mut img = RgbImage::from_pixel(TILE, TILE, fill);
    border(&mut img, WALL_EDGE);
    img
}

/// A decorated wall: a wall block scored with faint inscription lines.
fn alt_wall_tile() -> RgbImage {
    let mut img = wall_tile(WALL_ALT);
    for y in [11u32, 17, 23] {
        hline(&mut img, 6, 26, y, INSCRIBE);
    }
    img
}

/// A secret door: a wall with a dashed seam down its centre hinting at a hidden passage.
fn secret_door_tile() -> RgbImage {
    let mut img = wall_tile(WALL);
    let mut y = 6;
    while y < 26 {
        fill_rect(
            &mut img,
            TILE / 2 - 1,
            y,
            TILE / 2 + 1,
            (y + 3).min(26),
            SECRET,
        );
        y += 5;
    }
    img
}

/// A closed door standing in the passage, with a small knob.
fn door(img: &mut RgbImage) {
    fill_rect(img, 10, 8, 22, 24, DOOR);
    rect_outline(img, 10, 8, 22, 24, DOOR_EDGE);
    disc(img, 19, 16, 1, KNOB);
}

/// A ladder, drawn as an up-arrow, a down-arrow, or both stacked.
fn ladder(img: &mut RgbImage, up: bool, down: bool) {
    match (up, down) {
        (true, true) => {
            tri(img, (16, 5), (24, 14), (8, 14), LADDER_UP);
            tri(img, (16, 27), (8, 18), (24, 18), LADDER_DOWN);
        }
        (true, false) => {
            tri(img, (16, 7), (25, 19), (7, 19), LADDER_UP);
            fill_rect(img, 14, 18, 18, 24, LADDER_UP);
        }
        (false, _) => {
            tri(img, (16, 25), (7, 13), (25, 13), LADDER_DOWN);
            fill_rect(img, 14, 8, 18, 14, LADDER_DOWN);
        }
    }
}

/// A treasure chest with a lid line and a latch.
fn chest(img: &mut RgbImage) {
    fill_rect(img, 8, 13, 24, 23, CHEST);
    rect_outline(img, 8, 13, 24, 23, CHEST_EDGE);
    hline(img, 8, 24, 16, CHEST_EDGE);
    fill_rect(img, 15, 16, 17, 19, KNOB);
}

/// A fountain: a round pool with a light centre.
fn fountain(img: &mut RgbImage) {
    disc(img, 16, 16, 8, FOUNTAIN);
    ring(img, 16, 16, 8, FOUNTAIN_EDGE);
    disc(img, 16, 16, 3, FOUNTAIN_CORE);
}

/// A trap, drawn as a bold cross.
fn trap(img: &mut RgbImage) {
    thick_line(img, 10, 10, 22, 22, TRAP, 1);
    thick_line(img, 22, 10, 10, 22, TRAP, 1);
}

/// A magic orb: a small glowing sphere.
fn orb(img: &mut RgbImage) {
    disc(img, 16, 16, 6, ORB);
    disc(img, 14, 14, 1, ORB_HI);
}

/// An altar: a stone block with a lighter top slab.
fn altar(img: &mut RgbImage) {
    fill_rect(img, 9, 13, 23, 23, ALTAR);
    fill_rect(img, 9, 11, 23, 14, ALTAR_TOP);
    rect_outline(img, 9, 11, 23, 23, ALTAR_EDGE);
}

/// An energy field: a coloured square with diagonal hazard stripes (colour = field kind).
fn energy_field(img: &mut RgbImage, c: Rgb<u8>) {
    fill_rect(img, 5, 5, 27, 27, c);
    diag_stripes(img, 5, 5, 27, 27, darken(c), 6);
}

/// A room — a separate combat map you drop into — marked with a bold frame.
fn room(img: &mut RgbImage) {
    fill_rect(img, 5, 5, 27, 7, ROOM);
    fill_rect(img, 5, 25, 27, 27, ROOM);
    fill_rect(img, 5, 5, 7, 27, ROOM);
    fill_rect(img, 25, 5, 27, 27, ROOM);
}

// --- Drawing primitives, all in the 32×32 cell's pixel space ---

/// Fill the half-open rectangle `[x0, x1) × [y0, y1)` with `c`, clipped to the image.
fn fill_rect(img: &mut RgbImage, x0: u32, y0: u32, x1: u32, y1: u32, c: Rgb<u8>) {
    for y in y0..y1.min(img.height()) {
        for x in x0..x1.min(img.width()) {
            img.put_pixel(x, y, c);
        }
    }
}

/// Draw a horizontal line at `y` across `[x0, x1)`.
fn hline(img: &mut RgbImage, x0: u32, x1: u32, y: u32, c: Rgb<u8>) {
    if y >= img.height() {
        return;
    }
    for x in x0..x1.min(img.width()) {
        img.put_pixel(x, y, c);
    }
}

/// Draw a one-pixel outline around the half-open rectangle `[x0, x1) × [y0, y1)`.
fn rect_outline(img: &mut RgbImage, x0: u32, y0: u32, x1: u32, y1: u32, c: Rgb<u8>) {
    for x in x0..x1 {
        img.put_pixel(x, y0, c);
        img.put_pixel(x, y1 - 1, c);
    }
    for y in y0..y1 {
        img.put_pixel(x0, y, c);
        img.put_pixel(x1 - 1, y, c);
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

/// Set a pixel by signed coordinates, ignoring anything outside the cell.
fn put(img: &mut RgbImage, x: i32, y: i32, c: Rgb<u8>) {
    if x >= 0 && y >= 0 && (x as u32) < img.width() && (y as u32) < img.height() {
        img.put_pixel(x as u32, y as u32, c);
    }
}

/// Fill a disc of radius `r` centred on `(cx, cy)`.
fn disc(img: &mut RgbImage, cx: i32, cy: i32, r: i32, c: Rgb<u8>) {
    for y in (cy - r)..=(cy + r) {
        for x in (cx - r)..=(cx + r) {
            let (dx, dy) = (x - cx, y - cy);
            if dx * dx + dy * dy <= r * r {
                put(img, x, y, c);
            }
        }
    }
}

/// Draw the outline of a disc of radius `r` centred on `(cx, cy)`.
fn ring(img: &mut RgbImage, cx: i32, cy: i32, r: i32, c: Rgb<u8>) {
    for y in (cy - r)..=(cy + r) {
        for x in (cx - r)..=(cx + r) {
            let d = (x - cx) * (x - cx) + (y - cy) * (y - cy);
            if d <= r * r && d > (r - 1) * (r - 1) {
                put(img, x, y, c);
            }
        }
    }
}

/// Fill the triangle `a, b, c` using edge-function tests (winding-agnostic).
fn tri(img: &mut RgbImage, a: (i32, i32), b: (i32, i32), c: (i32, i32), col: Rgb<u8>) {
    let edge = |p: (i32, i32), q: (i32, i32), r: (i32, i32)| {
        (q.0 - p.0) * (r.1 - p.1) - (q.1 - p.1) * (r.0 - p.0)
    };
    let min_x = a.0.min(b.0).min(c.0);
    let max_x = a.0.max(b.0).max(c.0);
    let min_y = a.1.min(b.1).min(c.1);
    let max_y = a.1.max(b.1).max(c.1);
    for y in min_y..=max_y {
        for x in min_x..=max_x {
            let p = (x, y);
            let (w0, w1, w2) = (edge(b, c, p), edge(c, a, p), edge(a, b, p));
            if (w0 >= 0 && w1 >= 0 && w2 >= 0) || (w0 <= 0 && w1 <= 0 && w2 <= 0) {
                put(img, x, y, col);
            }
        }
    }
}

/// Draw a line from `(x0, y0)` to `(x1, y1)`, `half` pixels thick to either side of centre.
fn thick_line(img: &mut RgbImage, x0: i32, y0: i32, x1: i32, y1: i32, c: Rgb<u8>, half: i32) {
    let steps = (x1 - x0).abs().max((y1 - y0).abs()).max(1);
    for i in 0..=steps {
        let t = i as f32 / steps as f32;
        let x = (x0 as f32 + (x1 - x0) as f32 * t).round() as i32;
        let y = (y0 as f32 + (y1 - y0) as f32 * t).round() as i32;
        for dy in -half..=half {
            for dx in -half..=half {
                put(img, x + dx, y + dy, c);
            }
        }
    }
}

/// Fill `[x0, x1) × [y0, y1)` with 45° stripes of `c`, one every `spacing` pixels.
fn diag_stripes(img: &mut RgbImage, x0: u32, y0: u32, x1: u32, y1: u32, c: Rgb<u8>, spacing: u32) {
    for y in y0..y1.min(img.height()) {
        for x in x0..x1.min(img.width()) {
            if (x + y) % spacing == 0 {
                img.put_pixel(x, y, c);
            }
        }
    }
}

/// A darker shade of `c` (60%), for hazard-stripe contrast.
fn darken(c: Rgb<u8>) -> Rgb<u8> {
    Rgb([
        (c[0] as u32 * 6 / 10) as u8,
        (c[1] as u32 * 6 / 10) as u8,
        (c[2] as u32 * 6 / 10) as u8,
    ])
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

    #[test]
    fn legend_strip_has_one_row_per_label() {
        let rows = legend_labels().len() as u32;
        let strip = legend_strip();
        assert_eq!(strip.width(), TILE);
        assert_eq!(strip.height(), TILE * rows);
        // The first row is the Floor glyph.
        assert_eq!(legend_labels()[0], "Floor");
        assert_eq!(*strip.get_pixel(TILE / 2, TILE / 2), FLOOR);
    }

    #[test]
    fn legend_for_keeps_only_used_labels_in_order() {
        // A classifier that only ever yields floors and walls.
        let labels = legend_for(|b| if b == 0 { Cell::Floor } else { Cell::Wall });
        assert_eq!(labels, vec!["Floor", "Wall"]);
        // Every field kind collapses to the single "Energy field" entry.
        let fields = legend_for(|b| Cell::Field(field(b)));
        assert_eq!(fields, vec!["Energy field"]);
    }
}
