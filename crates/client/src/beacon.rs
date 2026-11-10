//! Nether beacon — mineral pyramid + light beam + short nearby buff.

use rebuild_world::Cluster;

use crate::mesh::Vertex;

/// Horizontal reach of the buff around an active beacon.
pub const BUFF_REACH: f32 = 8.0;
/// Seconds the speed plate stays topped up while standing in range.
pub const BUFF_SECS: f32 = 4.0;
/// Walk pace while the beacon speed buff is active.
pub const SPEED_PACE: f32 = 1.28;
const BEAM_HEIGHT: f32 = 24.0;

/// True when `id` can form a beacon pyramid base.
pub fn is_mineral(id: &str) -> bool {
    matches!(
        id,
        "iron_block" | "gold_block" | "emerald_block" | "diamond_block"
    )
}

/// One-level 3×3 mineral pad under a beacon at `at`.
pub fn pyramid_ready(world: &Cluster, at: [i32; 3]) -> bool {
    let [x, y, z] = at;
    if world.block_at(x, y, z).id != "beacon" {
        return false;
    }
    for dx in -1..=1 {
        for dz in -1..=1 {
            if !is_mineral(world.block_at(x + dx, y - 1, z + dz).id) {
                return false;
            }
        }
    }
    true
}

/// Feet of the player inside the buff radius of an active beacon.
pub fn in_range(beacon: [i32; 3], feet: [f32; 3]) -> bool {
    let cx = beacon[0] as f32 + 0.5;
    let cy = beacon[1] as f32 + 0.5;
    let cz = beacon[2] as f32 + 0.5;
    let dx = feet[0] - cx;
    let dy = feet[1] - cy;
    let dz = feet[2] - cz;
    dx * dx + dz * dz <= BUFF_REACH * BUFF_REACH && dy.abs() <= BUFF_REACH + 4.0
}

/// Tall cyan beam rising from the beacon top.
pub fn beam_vertices(at: [i32; 3]) -> Vec<Vertex> {
    let x = at[0] as f32 + 0.5;
    let y = at[1] as f32 + 1.0;
    let z = at[2] as f32 + 0.5;
    let mut out = Vec::new();
    let core = [0.55, 0.9, 1.0];
    let glow = [0.85, 0.95, 1.0];
    push_box(
        &mut out,
        [x, y + BEAM_HEIGHT * 0.5, z],
        [0.18, BEAM_HEIGHT * 0.5, 0.18],
        core,
        glow,
    );
    push_box(
        &mut out,
        [x, y + BEAM_HEIGHT * 0.5, z],
        [0.08, BEAM_HEIGHT * 0.5, 0.08],
        glow,
        [1.0, 1.0, 1.0],
    );
    out
}

fn push_box(
    out: &mut Vec<Vertex>,
    center: [f32; 3],
    half: [f32; 3],
    color: [f32; 3],
    block: [f32; 3],
) {
    let min = [
        center[0] - half[0],
        center[1] - half[1],
        center[2] - half[2],
    ];
    let max = [
        center[0] + half[0],
        center[1] + half[1],
        center[2] + half[2],
    ];
    let corner = [
        [min[0], min[1], min[2]],
        [max[0], min[1], min[2]],
        [max[0], max[1], min[2]],
        [min[0], max[1], min[2]],
        [min[0], min[1], max[2]],
        [max[0], min[1], max[2]],
        [max[0], max[1], max[2]],
        [min[0], max[1], max[2]],
    ];
    const FACES: [[usize; 4]; 6] = [
        [0, 1, 2, 3],
        [4, 5, 6, 7],
        [0, 1, 5, 4],
        [3, 2, 6, 7],
        [0, 3, 7, 4],
        [1, 2, 6, 5],
    ];
    const NORMALS: [[f32; 3]; 6] = [
        [0.0, 0.0, -1.0],
        [0.0, 0.0, 1.0],
        [0.0, -1.0, 0.0],
        [0.0, 1.0, 0.0],
        [-1.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
    ];
    for (face, normal) in FACES.into_iter().zip(NORMALS) {
        let tri = [face[0], face[1], face[2], face[0], face[2], face[3]];
        for index in tri {
            out.push(Vertex {
                position: corner[index],
                color,
                normal,
                uv: [0.0, 0.0],
                textured: 0.0,
                block,
                bounce: [0.0, 0.0, 0.0],
                ao: 1.0,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rebuild_world::{Block, Chunk, Cluster};

    #[test]
    fn a_mineral_pad_lights_a_beacon() {
        let mut blocks = Vec::new();
        for x in 3..=5u8 {
            for z in 3..=5u8 {
                blocks.push((x, 1u16, z, Block::new("iron_block", 0)));
            }
        }
        blocks.push((4, 2, 4, Block::new("beacon", 0)));
        let world =
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, blocks).unwrap()]).unwrap();
        assert!(pyramid_ready(&world, [4, 2, 4]));
        assert!(!pyramid_ready(&world, [4, 2, 5]));
        assert!(in_range([4, 2, 4], [4.5, 2.0, 4.5]));
        assert!(!in_range([4, 2, 4], [20.0, 2.0, 4.5]));
        assert!(!beam_vertices([4, 2, 4]).is_empty());
        assert!(is_mineral("diamond_block"));
        assert!(!is_mineral("stone"));
    }
}
