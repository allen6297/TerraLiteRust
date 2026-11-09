//! A held map: a small north-up patch, and a full sheet you open to read.
//!
//! The corner chart stays a glance. Right-click opens the sheet: a larger
//! north-up page, with you in the middle, north labeled, and the camp marked
//! when it falls on the page.

use rebuild_world::{Block, Cluster};

use crate::body::EYE_HEIGHT;
use crate::hotbar::Quad;
use crate::palette;

/// Cells on a side of the corner chart. The player stands in the middle.
pub const SPAN: usize = 9;
/// Cells on a side of the sheet you open to read.
pub const SHEET: usize = 21;
/// Widest chart. Each step doubles the blocks one cell covers.
pub const ZOOM_MAX: u8 = 2;

/// Blocks between samples. Zoom 0 is one block per cell.
pub fn step_of(zoom: u8) -> i32 {
    1 << i32::from(zoom.min(ZOOM_MAX))
}
const CELL: f32 = 6.0;
const SHEET_CELL: f32 = 14.0;
const VOID: [f32; 3] = [0.08, 0.10, 0.12];
const MARK: [f32; 3] = [0.95, 0.95, 0.95];
const FACING: [f32; 3] = [0.90, 0.20, 0.15];
const CAMP: [f32; 3] = [0.95, 0.45, 0.12];
const MINE: [f32; 3] = [0.25, 0.55, 0.78];
const STAIRS: [f32; 3] = [0.95, 0.82, 0.28];
const PAPER: [f32; 3] = [0.16, 0.13, 0.09];
const INK: [f32; 3] = [0.93, 0.90, 0.82];
const BACK: [f32; 3] = [0.04, 0.05, 0.06];

/// The highest solid block in the column, or air when the column is empty.
fn ground(world: &Cluster, x: i32, z: i32, feet_y: f32) -> Block {
    let top = (feet_y + 6.0).floor() as i32;
    let bottom = (feet_y - 16.0).floor().max(0.0) as i32;
    for y in (bottom..=top).rev() {
        let block = world.block_at(x, y, z);
        if !block.is_air() {
            return block;
        }
    }
    Block::AIR
}

/// Row-major colors. Row 0 is north (−Z) and column 0 is west (−X).
///
/// `zoom` skips blocks so the same grid covers more land. `0` is one block per cell.
pub fn sample_at(world: &Cluster, eye: [f32; 3], zoom: u8) -> Vec<[f32; 3]> {
    sample_span(world, eye, zoom, SPAN)
}

/// Like [`sample_at`], with `span` cells on a side. `span` should be odd.
pub fn sample_span(world: &Cluster, eye: [f32; 3], zoom: u8, span: usize) -> Vec<[f32; 3]> {
    let feet_y = eye[1] - EYE_HEIGHT;
    let origin_x = eye[0].floor() as i32;
    let origin_z = eye[2].floor() as i32;
    let half = (span as i32) / 2;
    let step = step_of(zoom);
    let mut colors = Vec::with_capacity(span * span);
    for row in 0..span {
        let dz = (row as i32 - half) * step;
        for col in 0..span {
            let dx = (col as i32 - half) * step;
            let block = ground(world, origin_x + dx, origin_z + dz, feet_y);
            let color = if block.is_air() {
                VOID
            } else {
                palette::color(block.id, block.metadata)
            };
            colors.push(color);
        }
    }
    colors
}

/// Row and column of `target` on a north-up grid, when it falls on the page.
///
/// Row 0 is north. The sample nearest the block is the cell.
pub fn cell_of(
    eye_x: f32,
    eye_z: f32,
    zoom: u8,
    span: usize,
    target_x: i32,
    target_z: i32,
) -> Option<(usize, usize)> {
    let origin_x = eye_x.floor() as i32;
    let origin_z = eye_z.floor() as i32;
    let half = (span as i32) / 2;
    let step = step_of(zoom).max(1);
    let col = half + nearest_step(target_x - origin_x, step);
    let row = half + nearest_step(target_z - origin_z, step);
    if col < 0 || row < 0 || col >= span as i32 || row >= span as i32 {
        None
    } else {
        Some((row as usize, col as usize))
    }
}

fn nearest_step(delta: i32, step: i32) -> i32 {
    if delta >= 0 {
        (delta + step / 2) / step
    } else {
        -(((-delta) + step / 2) / step)
    }
}

/// The patch in the top-left, plus a mark and a tick for the way you face.
pub fn quads(width: f32, height: f32, colors: &[[f32; 3]], yaw: f32) -> Vec<Quad> {
    if width <= 0.0 || height <= 0.0 || colors.len() != SPAN * SPAN {
        return Vec::new();
    }
    let span = SPAN as f32 * CELL;
    let x0 = 16.0;
    let top = height - 16.0;
    let mut quads = vec![pixel(
        width,
        height,
        x0 - 2.0,
        top - span - 2.0,
        x0 + span + 2.0,
        top + 2.0,
        [0.10, 0.08, 0.06],
    )];
    for row in 0..SPAN {
        for col in 0..SPAN {
            let x = x0 + col as f32 * CELL;
            let y = top - (row as f32 + 1.0) * CELL;
            quads.push(pixel(
                width,
                height,
                x,
                y,
                x + CELL,
                y + CELL,
                colors[row * SPAN + col],
            ));
        }
    }
    let cx = x0 + SPAN as f32 * CELL * 0.5;
    let cy = top - SPAN as f32 * CELL * 0.5;
    quads.push(pixel(
        width,
        height,
        cx - 2.0,
        cy - 2.0,
        cx + 2.0,
        cy + 2.0,
        MARK,
    ));
    let (sin, cos) = yaw.sin_cos();
    let tx = cx + cos * 6.0;
    let ty = cy - sin * 6.0;
    quads.push(pixel(
        width,
        height,
        tx - 1.5,
        ty - 1.5,
        tx + 1.5,
        ty + 1.5,
        FACING,
    ));
    quads
}

/// A full page. North is the top. `camp`, `mine`, and `stairs` are cells to mark.
pub fn sheet_quads(
    width: f32,
    height: f32,
    colors: &[[f32; 3]],
    yaw: f32,
    camp: Option<(usize, usize)>,
    mine: Option<(usize, usize)>,
    stairs: Option<(usize, usize)>,
) -> Vec<Quad> {
    if width <= 0.0 || height <= 0.0 || colors.len() != SHEET * SHEET {
        return Vec::new();
    }
    let grid = SHEET as f32 * SHEET_CELL;
    let panel_w = grid + 36.0;
    let panel_h = grid + 78.0;
    let x0 = (width - panel_w) * 0.5;
    let y0 = (height - panel_h) * 0.5;
    let mut quads = vec![
        pixel(width, height, 0.0, 0.0, width, height, BACK),
        pixel(width, height, x0, y0, x0 + panel_w, y0 + panel_h, PAPER),
    ];
    let grid_left = x0 + 18.0;
    let grid_top = y0 + panel_h - 40.0;
    for row in 0..SHEET {
        for col in 0..SHEET {
            let x = grid_left + col as f32 * SHEET_CELL;
            let y = grid_top - (row as f32 + 1.0) * SHEET_CELL;
            quads.push(pixel(
                width,
                height,
                x,
                y,
                x + SHEET_CELL,
                y + SHEET_CELL,
                colors[row * SHEET + col],
            ));
        }
    }
    let mark_cell = |quads: &mut Vec<Quad>, cell: (usize, usize), color: [f32; 3]| {
        let (row, col) = cell;
        let x = grid_left + col as f32 * SHEET_CELL;
        let y = grid_top - (row as f32 + 1.0) * SHEET_CELL;
        let inset = SHEET_CELL * 0.28;
        quads.push(pixel(
            width,
            height,
            x + inset,
            y + inset,
            x + SHEET_CELL - inset,
            y + SHEET_CELL - inset,
            color,
        ));
    };
    if let Some(cell) = stairs {
        if cell != (SHEET / 2, SHEET / 2) {
            mark_cell(&mut quads, cell, STAIRS);
        }
    }
    if let Some(cell) = mine {
        mark_cell(&mut quads, cell, MINE);
    }
    if let Some(cell) = camp {
        mark_cell(&mut quads, cell, CAMP);
    }
    let cx = grid_left + SHEET as f32 * SHEET_CELL * 0.5;
    let cy = grid_top - SHEET as f32 * SHEET_CELL * 0.5;
    quads.push(pixel(
        width,
        height,
        cx - 3.0,
        cy - 3.0,
        cx + 3.0,
        cy + 3.0,
        MARK,
    ));
    let (sin, cos) = yaw.sin_cos();
    let reach = SHEET_CELL * 0.85;
    let tx = cx + cos * reach;
    let ty = cy - sin * reach;
    quads.push(pixel(
        width,
        height,
        tx - 2.0,
        ty - 2.0,
        tx + 2.0,
        ty + 2.0,
        FACING,
    ));
    crate::gui::draw_text(
        &mut quads,
        width,
        height,
        x0 + 18.0,
        y0 + panel_h - 28.0,
        "Map",
        INK,
        2.0,
    );
    crate::gui::draw_text(
        &mut quads,
        width,
        height,
        cx - 4.0,
        grid_top + 4.0,
        "N",
        INK,
        1.5,
    );
    let stairs_shown = stairs.is_some_and(|cell| cell != (SHEET / 2, SHEET / 2));
    let note = map_note(camp.is_some(), mine.is_some(), stairs_shown);
    crate::gui::draw_text(
        &mut quads,
        width,
        height,
        x0 + 18.0,
        y0 + 8.0,
        note,
        INK,
        1.0,
    );
    quads
}

fn map_note(camp: bool, mine: bool, stairs: bool) -> &'static str {
    match (camp, mine, stairs) {
        (true, true, true) => "you white   camp orange   mine blue   stairs gold",
        (true, true, false) => "you white   camp orange   mine blue",
        (true, false, true) => "you white   camp orange   stairs gold",
        (true, false, false) => "you white   camp orange",
        (false, true, true) => "you white   mine blue   stairs gold   camp off page",
        (false, true, false) => "you white   mine blue   camp off page",
        (false, false, true) => "you white   stairs gold   camp off page",
        (false, false, false) => "you white   camp off page",
    }
}

fn pixel(width: f32, height: f32, x0: f32, y0: f32, x1: f32, y1: f32, color: [f32; 3]) -> Quad {
    Quad {
        min: [x0 / width * 2.0 - 1.0, y0 / height * 2.0 - 1.0],
        max: [x1 / width * 2.0 - 1.0, y1 / height * 2.0 - 1.0],
        color,
        tex: 0.0,
        span: [0.0, 0.0],
        hit: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rebuild_world::{Block, Chunk, Cluster};

    #[test]
    fn the_map_colors_the_ground_and_marks_the_player() {
        let mut blocks = Vec::new();
        for z in 0..16u8 {
            for x in 0..16u8 {
                if (x, z) != (8, 9) {
                    blocks.push((x, 0, z, Block::new("stone", 0)));
                }
            }
        }
        blocks.push((8, 1, 7, Block::new("grass", 0)));
        blocks.push((9, 1, 8, Block::new("water", 0)));
        let world =
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, blocks).unwrap()]).unwrap();
        let eye = [8.5, 1.0 + EYE_HEIGHT, 8.5];
        let colors = sample_at(&world, eye, 0);
        assert_eq!(colors.len(), SPAN * SPAN);
        let half = SPAN / 2;
        assert_eq!(colors[half * SPAN + half], palette::color("stone", 0));
        assert_eq!(colors[(half - 1) * SPAN + half], palette::color("grass", 0));
        assert_eq!(colors[half * SPAN + half + 1], palette::color("water", 0));
        assert_eq!(colors[(half + 1) * SPAN + half], VOID);

        let east = quads(400.0, 200.0, &colors, 0.0);
        assert_eq!(east.len(), SPAN * SPAN + 3);
        let mark = east.iter().find(|quad| quad.color == MARK).unwrap();
        let tick = east.iter().find(|quad| quad.color == FACING).unwrap();
        assert!(tick.max[0] > mark.max[0], "yaw 0 looks east, to the right");
        let north = quads(400.0, 200.0, &colors, -std::f32::consts::FRAC_PI_2);
        let mark = north.iter().find(|quad| quad.color == MARK).unwrap();
        let tick = north.iter().find(|quad| quad.color == FACING).unwrap();
        assert!(
            tick.max[1] > mark.max[1],
            "looking north puts the tick above the mark"
        );
        assert!(quads(0.0, 200.0, &colors, 0.0).is_empty());
    }

    #[test]
    fn a_wider_chart_skips_the_near_block() {
        let mut blocks = Vec::new();
        for z in 0..16u8 {
            for x in 0..16u8 {
                blocks.push((x, 0, z, Block::new("stone", 0)));
            }
        }
        blocks.push((8, 1, 7, Block::new("dirt", 0)));
        blocks.push((8, 1, 6, Block::new("grass", 0)));
        let world =
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, blocks).unwrap()]).unwrap();
        let eye = [8.5, 1.0 + EYE_HEIGHT, 8.5];
        let half = SPAN / 2;
        let near = sample_at(&world, eye, 0);
        let wide = sample_at(&world, eye, 1);
        assert_eq!(near[(half - 1) * SPAN + half], palette::color("dirt", 0));
        assert_eq!(wide[(half - 1) * SPAN + half], palette::color("grass", 0));
        assert_eq!(step_of(0), 1);
        assert_eq!(step_of(1), 2);
        assert_eq!(step_of(2), 4);
        assert_eq!(step_of(9), 4);
    }

    #[test]
    fn the_sheet_is_a_page_and_the_camp_sits_on_the_wide_one() {
        let colors = vec![[0.2, 0.5, 0.2]; SHEET * SHEET];
        let eye_x = 8.5;
        let eye_z = 14.0;
        let camp = cell_of(eye_x, eye_z, 2, SHEET, 39, -25).expect("camp on the wide page");
        let mine = cell_of(eye_x, eye_z, 2, SHEET, -24, 40).expect("mine on the wide page");
        assert_ne!(camp, mine);
        assert!(cell_of(eye_x, eye_z, 0, SHEET, 39, -25).is_none());
        assert!(cell_of(eye_x, eye_z, 0, SHEET, -24, 40).is_none());
        let sheet = sheet_quads(800.0, 600.0, &colors, 0.0, Some(camp), Some(mine), None);
        assert!(sheet.len() > SHEET * SHEET);
        assert_eq!(sheet[0].color, BACK);
        assert_eq!(sheet[1].color, PAPER);
        assert!(sheet[1].min[0] > -0.8 && sheet[1].max[0] < 0.8);
        assert!(sheet.iter().any(|quad| quad.color == CAMP));
        assert!(sheet.iter().any(|quad| quad.color == MINE));
        let mark = sheet.iter().find(|quad| quad.color == MARK).unwrap();
        let tick = sheet.iter().find(|quad| quad.color == FACING).unwrap();
        assert!(tick.max[0] > mark.max[0], "yaw 0 looks east");
        assert!(sheet_quads(0.0, 600.0, &colors, 0.0, None, None, None).is_empty());
        assert_eq!(sample_span_len(), SHEET * SHEET);
    }

    fn sample_span_len() -> usize {
        let world = Cluster::try_from_chunks(
            0,
            0,
            [Chunk::from_blocks(0, 0, [(8u8, 0u16, 8u8, Block::new("stone", 0))]).unwrap()],
        )
        .unwrap();
        sample_span(&world, [8.5, 1.0 + EYE_HEIGHT, 8.5], 0, SHEET).len()
    }
}
