//! Ocean conduit — prismarine frame in water + nearby underwater power.

use rebuild_world::Cluster;

use crate::mesh::Vertex;

/// Horizontal reach of conduit power.
pub const BUFF_REACH: f32 = 10.0;
/// Seconds the conduit plate stays topped up while in range underwater.
pub const BUFF_SECS: f32 = 5.0;
/// How much underwater fog/veil to clear while powered (`0` none … `1` full clear).
pub const VISION_CLEAR: f32 = 0.72;

/// Prismarine-family blocks that can form the frame.
pub fn is_frame(id: &str) -> bool {
    matches!(
        id,
        "prismarine" | "prismarine_bricks" | "dark_prismarine" | "sea_lantern"
    )
}

/// Conduit sits in (or next to) water with at least four prismarine in its ring.
pub fn frame_ready(world: &Cluster, at: [i32; 3]) -> bool {
    let [x, y, z] = at;
    if world.block_at(x, y, z).id != "conduit" {
        return false;
    }
    let mut prism = 0;
    for dx in -1..=1 {
        for dz in -1..=1 {
            if dx == 0 && dz == 0 {
                continue;
            }
            if is_frame(world.block_at(x + dx, y, z + dz).id) {
                prism += 1;
            }
        }
    }
    if prism < 4 {
        return false;
    }
    let wet = [
        [x + 1, y, z],
        [x - 1, y, z],
        [x, y + 1, z],
        [x, y - 1, z],
        [x, y, z + 1],
        [x, y, z - 1],
    ]
    .into_iter()
    .any(|c| world.block_at(c[0], c[1], c[2]).id == "water");
    wet
}

/// Player body inside the buff radius.
pub fn in_range(conduit: [i32; 3], feet: [f32; 3]) -> bool {
    let cx = conduit[0] as f32 + 0.5;
    let cy = conduit[1] as f32 + 0.5;
    let cz = conduit[2] as f32 + 0.5;
    let dx = feet[0] - cx;
    let dy = feet[1] - cy;
    let dz = feet[2] - cz;
    dx * dx + dz * dz <= BUFF_REACH * BUFF_REACH && dy.abs() <= BUFF_REACH + 3.0
}

/// Soft cyan aura around an active conduit.
pub fn aura_vertices(at: [i32; 3]) -> Vec<Vertex> {
    let x = at[0] as f32 + 0.5;
    let y = at[1] as f32 + 0.5;
    let z = at[2] as f32 + 0.5;
    let mut out = Vec::new();
    let core = [0.25, 0.75, 0.9];
    let glow = [0.55, 0.95, 1.0];
    push_box(&mut out, [x, y, z], [0.55, 0.55, 0.55], core, glow);
    push_box(&mut out, [x, y + 0.85, z], [0.18, 0.35, 0.18], glow, [1.0, 1.0, 1.0]);
    push_box(&mut out, [x, y - 0.85, z], [0.18, 0.35, 0.18], glow, [1.0, 1.0, 1.0]);
    push_box(&mut out, [x + 0.85, y, z], [0.35, 0.18, 0.18], glow, [1.0, 1.0, 1.0]);
    push_box(&mut out, [x - 0.85, y, z], [0.35, 0.18, 0.18], glow, [1.0, 1.0, 1.0]);
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
    fn a_prismarine_ring_in_water_powers_a_conduit() {
        let mut blocks = Vec::new();
        // Floor + water around the conduit cell.
        for x in 3..=5u8 {
            for z in 3..=5u8 {
                blocks.push((x, 0u16, z, Block::new("sand", 0)));
                if !(x == 4 && z == 4) {
                    blocks.push((x, 1, z, Block::new("water", 0)));
                }
            }
        }
        // Four prismarine on the cardinals, conduit in the middle.
        blocks.push((4, 1, 3, Block::new("prismarine", 0)));
        blocks.push((4, 1, 5, Block::new("prismarine", 0)));
        blocks.push((3, 1, 4, Block::new("prismarine", 0)));
        blocks.push((5, 1, 4, Block::new("prismarine", 0)));
        blocks.push((4, 1, 4, Block::new("conduit", 0)));
        // Water above so the wet check still passes.
        blocks.push((4, 2, 4, Block::new("water", 0)));
        let world =
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, blocks).unwrap()]).unwrap();
        assert!(frame_ready(&world, [4, 1, 4]));
        assert!(!frame_ready(&world, [4, 1, 3]));
        assert!(in_range([4, 1, 4], [4.5, 1.0, 4.5]));
        assert!(!aura_vertices([4, 1, 4]).is_empty());
        assert!(is_frame("sea_lantern"));
        assert!(!is_frame("stone"));
    }
}
