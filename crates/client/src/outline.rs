//! Wire on the block the look ray would break.
//!
//! Only edges of faces toward the camera are drawn, so the wire stays around
//! the shape instead of crossing the face. Each edge sits just off the
//! surface, then a short step toward the eye. Air, and a look that hits
//! nothing, produce no lines. Holding a compound fill item or drain tool
//! tints the wire when the looked-at host can accept or already holds a fill.

use glam::Vec3;
use rebuild_world::{
    Cluster, FILL_EMBER, FILL_NONE, FILL_WATER, Pack, compound_accepts, compound_host,
    conduit_link_mask, conduit_shell_boxes, conduit_span, for_each_collider, shape_of,
};

use crate::look;
use crate::mesh::Vertex;

/// World units the wire sits outside each solid box.
const PAD: f32 = 0.012;
/// Extra step toward the camera so the wire clears the face it traces.
const TOWARD_EYE: f32 = 0.02;

const WHITE: [f32; 3] = [1.0, 1.0, 1.0];
const CYAN: [f32; 3] = [0.25, 0.85, 1.0];
const AMBER: [f32; 3] = [1.0, 0.55, 0.15];
const LIME: [f32; 3] = [0.45, 1.0, 0.35];

/// Line-list vertices for the looked-at block. Empty when the ray hits nothing.
pub fn selection_lines(
    eye: Vec3,
    forward: Vec3,
    world: &Cluster,
    held: Option<&str>,
) -> Vec<Vertex> {
    let Some(target) = look::raycast(eye, forward, world) else {
        return Vec::new();
    };
    let block = world.block_at(target.block[0], target.block[1], target.block[2]);
    let color = outline_color(held, block.id, block.fill);
    let origin = Vec3::new(
        target.block[0] as f32,
        target.block[1] as f32,
        target.block[2] as f32,
    );
    let mut lines = Vec::new();
    if block.id == "pipe" || block.id == "wire" {
        let links = conduit_link_mask(
            world,
            block.id,
            target.block[0],
            target.block[1],
            target.block[2],
        );
        let (lo, hi) = conduit_span(block.id);
        let boxes = conduit_shell_boxes(lo, hi, links, block.id == "pipe");
        if boxes.is_empty() {
            return Vec::new();
        }
        for cell in boxes {
            let min = origin
                + Vec3::new(cell.min[0] as f32, cell.min[1] as f32, cell.min[2] as f32) / 16.0
                - Vec3::splat(PAD);
            let max = origin
                + Vec3::new(cell.max[0] as f32, cell.max[1] as f32, cell.max[2] as f32) / 16.0
                + Vec3::splat(PAD);
            push_box(&mut lines, min, max, eye, color);
        }
        return lines;
    }
    let shape = shape_of(block);
    if shape.is_empty() {
        return Vec::new();
    }
    for_each_collider(block, |min, max| {
        let min = origin + Vec3::new(min[0], min[1], min[2]) / 16.0 - Vec3::splat(PAD);
        let max = origin + Vec3::new(max[0], max[1], max[2]) / 16.0 + Vec3::splat(PAD);
        push_box(&mut lines, min, max, eye, color);
    });
    lines
}

fn outline_color(held: Option<&str>, host_id: &str, fill: u8) -> [f32; 3] {
    let Some(held) = held else {
        return WHITE;
    };
    let pack = Pack::builtin();
    if pack.is_drain(held) {
        if compound_host(host_id) && fill != FILL_NONE {
            return LIME;
        }
        return WHITE;
    }
    let Some(kind) = pack.compound_fill_of(held) else {
        return WHITE;
    };
    let want = match kind {
        "water" => FILL_WATER,
        "ember" => FILL_EMBER,
        _ => return WHITE,
    };
    if !compound_accepts(host_id, want) {
        return WHITE;
    }
    if fill == FILL_NONE || fill == want {
        match want {
            FILL_WATER => CYAN,
            FILL_EMBER => AMBER,
            _ => WHITE,
        }
    } else {
        WHITE
    }
}

fn push_box(out: &mut Vec<Vertex>, min: Vec3, max: Vec3, eye: Vec3, color: [f32; 3]) {
    let center = (min + max) * 0.5;
    let to_eye = eye - center;
    let nudge = if to_eye.length_squared() > 1.0e-8 {
        to_eye.normalize() * TOWARD_EYE
    } else {
        Vec3::ZERO
    };
    let corner = [
        Vec3::new(min.x, min.y, min.z),
        Vec3::new(max.x, min.y, min.z),
        Vec3::new(max.x, max.y, min.z),
        Vec3::new(min.x, max.y, min.z),
        Vec3::new(min.x, min.y, max.z),
        Vec3::new(max.x, min.y, max.z),
        Vec3::new(max.x, max.y, max.z),
        Vec3::new(min.x, max.y, max.z),
    ];
    // Each edge, then the two face normals that meet there.
    let edges: [([usize; 2], [Vec3; 2]); 12] = [
        (
            [0, 1],
            [Vec3::new(0.0, 0.0, -1.0), Vec3::new(0.0, -1.0, 0.0)],
        ),
        (
            [1, 2],
            [Vec3::new(0.0, 0.0, -1.0), Vec3::new(1.0, 0.0, 0.0)],
        ),
        (
            [2, 3],
            [Vec3::new(0.0, 0.0, -1.0), Vec3::new(0.0, 1.0, 0.0)],
        ),
        (
            [3, 0],
            [Vec3::new(0.0, 0.0, -1.0), Vec3::new(-1.0, 0.0, 0.0)],
        ),
        (
            [4, 5],
            [Vec3::new(0.0, 0.0, 1.0), Vec3::new(0.0, -1.0, 0.0)],
        ),
        ([5, 6], [Vec3::new(0.0, 0.0, 1.0), Vec3::new(1.0, 0.0, 0.0)]),
        ([6, 7], [Vec3::new(0.0, 0.0, 1.0), Vec3::new(0.0, 1.0, 0.0)]),
        (
            [7, 4],
            [Vec3::new(0.0, 0.0, 1.0), Vec3::new(-1.0, 0.0, 0.0)],
        ),
        (
            [0, 4],
            [Vec3::new(0.0, -1.0, 0.0), Vec3::new(-1.0, 0.0, 0.0)],
        ),
        (
            [1, 5],
            [Vec3::new(0.0, -1.0, 0.0), Vec3::new(1.0, 0.0, 0.0)],
        ),
        ([2, 6], [Vec3::new(0.0, 1.0, 0.0), Vec3::new(1.0, 0.0, 0.0)]),
        (
            [3, 7],
            [Vec3::new(0.0, 1.0, 0.0), Vec3::new(-1.0, 0.0, 0.0)],
        ),
    ];
    for ([a, b], normals) in edges {
        let facing = normals
            .into_iter()
            .any(|normal| normal.dot(to_eye) > 1.0e-4);
        if !facing {
            continue;
        }
        out.push(wire(corner[a] + nudge, color));
        out.push(wire(corner[b] + nudge, color));
    }
}

fn wire(position: Vec3, color: [f32; 3]) -> Vertex {
    Vertex {
        position: position.to_array(),
        color,
        normal: [0.0, 1.0, 0.0],
        uv: [0.0, 0.0],
        textured: 0.0,
        block: color,
        bounce: [0.0, 0.0, 0.0],
        ao: 1.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec3;
    use rebuild_world::{Block, Chunk, Cluster};

    fn world(blocks: &[(u8, u16, u8, Block)]) -> Cluster {
        let chunk = Chunk::from_blocks(0, 0, blocks.iter().copied()).unwrap();
        Cluster::try_from_chunks(0, 0, [chunk]).unwrap()
    }

    #[test]
    fn looking_at_air_draws_no_outline() {
        let world = world(&[(0, 0, 0, Block::new("grass", 0))]);
        let eye = Vec3::new(0.5, 3.0, 0.5);
        let forward = Vec3::new(0.0, 1.0, 0.0);
        assert!(look::raycast(eye, forward, &world).is_none());
        assert!(selection_lines(eye, forward, &world, None).is_empty());
    }

    #[test]
    fn grass_outline_is_the_cube_the_break_ray_hits() {
        let world = world(&[(0, 0, 0, Block::new("grass", 0))]);
        let eye = Vec3::new(0.5, 0.5, 3.0);
        let forward = Vec3::new(0.0, 0.0, -1.0);
        let hit = look::raycast(eye, forward, &world).unwrap();
        assert_eq!(hit.block, [0, 0, 0]);
        let lines = selection_lines(eye, forward, &world, None);
        assert_eq!(
            lines.len(),
            8,
            "only the four edges of the face toward the camera"
        );
        assert!(
            lines
                .iter()
                .all(|vertex| vertex.color == WHITE && vertex.textured == 0.0)
        );
        let near = 1.0 + PAD + TOWARD_EYE;
        for vertex in &lines {
            assert!(
                (vertex.position[2] - near).abs() < 1.0e-3,
                "edge crossed the face, z {}",
                vertex.position[2]
            );
            for axis in 0..2 {
                let at = vertex.position[axis];
                assert!(
                    (-PAD - 1.0e-4..=1.0 + PAD + 1.0e-4).contains(&at),
                    "grass edge left the cell, {at}"
                );
            }
        }
    }

    #[test]
    fn holding_water_bucket_tints_an_empty_grate_cyan() {
        assert_eq!(
            outline_color(Some("water_bucket"), "copper_grate", FILL_NONE),
            CYAN
        );
        assert_eq!(
            outline_color(Some("ember_charge"), "copper_grate", FILL_NONE),
            AMBER
        );
        assert_eq!(
            outline_color(Some("drain_sponge"), "copper_grate", FILL_WATER),
            LIME
        );
        assert_eq!(outline_color(Some("cobblestone"), "copper_grate", FILL_NONE), WHITE);
        assert_eq!(
            outline_color(Some("water_bucket"), "fence", FILL_NONE),
            CYAN
        );
        assert_eq!(
            outline_color(Some("ember_charge"), "leaves", FILL_NONE),
            AMBER
        );
        assert_eq!(
            outline_color(Some("drain_sponge"), "glass", FILL_EMBER),
            LIME
        );
        assert_eq!(outline_color(Some("water_bucket"), "pipe", FILL_NONE), CYAN);
        assert_eq!(outline_color(Some("ember_charge"), "pipe", FILL_NONE), WHITE);
        assert_eq!(outline_color(Some("ember_charge"), "wire", FILL_NONE), AMBER);
        assert_eq!(outline_color(Some("water_bucket"), "wire", FILL_NONE), WHITE);
    }

    #[test]
    fn stairs_outline_follows_both_boxes() {
        let world = world(&[(0, 1, 0, Block::new("oak_stairs", 0))]);
        let eye = Vec3::new(0.5, 1.25, 3.0);
        let forward = Vec3::new(0.0, 0.0, -1.0);
        assert_eq!(
            look::raycast(eye, forward, &world).unwrap().block,
            [0, 1, 0]
        );
        let lines = selection_lines(eye, forward, &world, None);
        assert_eq!(lines.len(), 22, "visible edges of both stair boxes");
        assert!(
            lines.iter().any(|vertex| vertex.position[1] < 1.1),
            "lower box"
        );
        assert!(
            lines.iter().any(|vertex| vertex.position[1] > 1.9),
            "upper step"
        );
        let upper_max_z = lines
            .iter()
            .filter(|vertex| vertex.position[1] > 1.7)
            .map(|vertex| vertex.position[2])
            .fold(0.0, f32::max);
        assert!(
            upper_max_z < 0.5 + PAD + TOWARD_EYE + 1.0e-3,
            "upper step outline filled the cell, {upper_max_z}"
        );
        let lower_max_z = lines
            .iter()
            .filter(|vertex| vertex.position[1] < 1.4)
            .map(|vertex| vertex.position[2])
            .fold(0.0, f32::max);
        assert!(
            lower_max_z > 0.9,
            "lower box outline is missing, {lower_max_z}"
        );
    }

    #[test]
    fn sign_outline_covers_the_post_and_the_board() {
        let world = world(&[(7, 1, 13, Block::new("sign", 0))]);
        let eye = Vec3::new(7.2, 1.6, 16.0);
        let forward = Vec3::new(0.0, 0.0, -1.0);
        assert_eq!(
            look::raycast(eye, forward, &world).unwrap().block,
            [7, 1, 13]
        );
        let lines = selection_lines(eye, forward, &world, None);
        assert_eq!(lines.len(), 36, "visible edges of the post and the board");
        let min_x = lines
            .iter()
            .map(|vertex| vertex.position[0])
            .fold(f32::MAX, f32::min);
        let max_x = lines
            .iter()
            .map(|vertex| vertex.position[0])
            .fold(f32::MIN, f32::max);
        let min_y = lines
            .iter()
            .map(|vertex| vertex.position[1])
            .fold(f32::MAX, f32::min);
        let max_y = lines
            .iter()
            .map(|vertex| vertex.position[1])
            .fold(f32::MIN, f32::max);
        assert!(min_x > 7.04 && max_x < 7.96, "board span {min_x}..{max_x}");
        assert!(min_y < 1.02, "post does not reach the grass, {min_y}");
        assert!(max_y > 1.9, "board top is missing, {max_y}");
    }
}
