//! A boat that floats on marsh water.
//!
//! One boat can be out at a time. It is not a block: water is not air, so a
//! place would be refused. Right-click puts the hull on a water cell. F boards
//! and leaves. WASD slides it across water and stops at the edge. It is not saved.

use rebuild_world::Cluster;

use crate::body::EYE_HEIGHT;
use crate::mesh::Vertex;

/// Horizontal speed while aboard, in blocks per second. A walk, not a swim.
pub const SPEED: f32 = 6.0;
/// How close the eye must be to board.
const BOARD: f32 = 2.5;

pub struct Boat {
    /// Cell center x, water-block y, cell center z. `None` when it is in the bag.
    pos: Option<[f32; 3]>,
    aboard: bool,
}

impl Boat {
    pub fn new() -> Self {
        Self {
            pos: None,
            aboard: false,
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn is_out(&self) -> bool {
        self.pos.is_some()
    }

    pub fn aboard(&self) -> bool {
        self.aboard
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn origin(&self) -> Option<[f32; 3]> {
        self.pos
    }

    /// Sit the hull on the water block `(x, y, z)`. False when one is already out.
    pub fn place(&mut self, x: i32, y: i32, z: i32) -> bool {
        if self.pos.is_some() {
            return false;
        }
        self.pos = Some([x as f32 + 0.5, y as f32, z as f32 + 0.5]);
        self.aboard = false;
        true
    }

    /// Put it back in the inventory. False while someone is aboard.
    pub fn take(&mut self) -> bool {
        if self.pos.is_none() || self.aboard {
            return false;
        }
        self.pos = None;
        true
    }

    pub fn mount(&mut self) -> bool {
        if self.pos.is_none() {
            return false;
        }
        self.aboard = true;
        true
    }

    pub fn dismount(&mut self) {
        self.aboard = false;
    }

    /// True when `eye` is close enough to board.
    pub fn near(&self, eye: [f32; 3]) -> bool {
        let Some([x, y, z]) = self.pos else {
            return false;
        };
        let dx = eye[0] - x;
        let dy = eye[1] - (y + 1.0);
        let dz = eye[2] - z;
        dx * dx + dy * dy + dz * dz < BOARD * BOARD
    }

    /// True when the look ray passes close to the hull.
    pub fn aimed(&self, eye: [f32; 3], forward: [f32; 3]) -> bool {
        let Some([x, y, z]) = self.pos else {
            return false;
        };
        let center = [x, y + 0.75, z];
        let to = [center[0] - eye[0], center[1] - eye[1], center[2] - eye[2]];
        let along = to[0] * forward[0] + to[1] * forward[1] + to[2] * forward[2];
        if !(0.0..=6.0).contains(&along) {
            return false;
        }
        let closest = [
            eye[0] + forward[0] * along,
            eye[1] + forward[1] * along,
            eye[2] + forward[2] * along,
        ];
        let dx = closest[0] - center[0];
        let dy = closest[1] - center[1];
        let dz = closest[2] - center[2];
        dx * dx + dy * dy + dz * dz < 0.81
    }

    /// Eye position while aboard.
    pub fn seat(&self) -> Option<[f32; 3]> {
        if !self.aboard {
            return None;
        }
        let [x, y, z] = self.pos?;
        Some([x, y + 0.85 + EYE_HEIGHT, z])
    }

    /// Slide by `vx` and `vz` (blocks per second). A step that leaves water is dropped.
    ///
    /// Returns true when the hull moved.
    pub fn steer(&mut self, dt: f32, vx: f32, vz: f32, world: &Cluster) -> bool {
        let Some(pos) = self.pos else {
            return false;
        };
        if !self.aboard || dt <= 0.0 {
            return false;
        }
        let layer = pos[1] as i32;
        let mut next = pos;
        let nx = pos[0] + vx * dt;
        if water_layer(world, nx, pos[2], layer).is_some() {
            next[0] = nx;
        }
        let nz = pos[2] + vz * dt;
        if water_layer(world, next[0], nz, layer).is_some() {
            next[2] = nz;
        }
        if let Some(y) = water_layer(world, next[0], next[2], layer) {
            next[1] = y as f32;
        }
        let moved = next != pos;
        self.pos = Some(next);
        moved
    }

    pub fn vertices(&self) -> Vec<Vertex> {
        let Some([x, y, z]) = self.pos else {
            return Vec::new();
        };
        let mut out = Vec::new();
        let deck = y + 0.72;
        push_box(
            &mut out,
            [x, deck, z],
            [0.45, 0.12, 0.7],
            [0.55, 0.36, 0.16],
        );
        push_box(
            &mut out,
            [x, deck + 0.16, z + 0.55],
            [0.38, 0.1, 0.1],
            [0.42, 0.26, 0.12],
        );
        push_box(
            &mut out,
            [x, deck + 0.16, z - 0.55],
            [0.38, 0.1, 0.1],
            [0.42, 0.26, 0.12],
        );
        out
    }
}

fn water_layer(world: &Cluster, x: f32, z: f32, layer: i32) -> Option<i32> {
    let ix = x.floor() as i32;
    let iz = z.floor() as i32;
    for y in (layer - 1)..=(layer + 1) {
        if world.block_at(ix, y, iz).id == "water" {
            return Some(y);
        }
    }
    None
}

fn push_box(out: &mut Vec<Vertex>, center: [f32; 3], half: [f32; 3], color: [f32; 3]) {
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
                block: color,
                bounce: [0.0, 0.0, 0.0],
                ao: 1.0,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rebuild_world::{Block, Chunk};

    fn sheet() -> Cluster {
        let mut blocks = Vec::new();
        for z in 0..4u8 {
            for x in 0..4u8 {
                blocks.push((x, 0, z, Block::new("stone", 0)));
                blocks.push((x, 2, z, Block::new("water", 0)));
            }
        }
        Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, blocks).unwrap()]).unwrap()
    }

    #[test]
    fn a_boat_rides_marsh_water_and_stops_at_the_edge() {
        let world = sheet();
        let mut boat = Boat::new();
        assert!(boat.place(1, 2, 1));
        assert!(!boat.place(2, 2, 1));
        assert!(boat.near([1.5, 2.0 + EYE_HEIGHT, 1.5]));
        assert!(boat.mount());
        assert!(boat.steer(0.2, SPEED, 0.0, &world));
        let [x, y, z] = boat.origin().unwrap();
        assert!((x - 2.7).abs() < 0.02, "{x}");
        assert_eq!(y, 2.0);
        assert!((z - 1.5).abs() < 0.02);
        for _ in 0..80 {
            boat.steer(1.0 / 60.0, SPEED, 0.0, &world);
        }
        let [x, _, _] = boat.origin().unwrap();
        assert!(x > 3.0 && x < 4.0, "stayed on the sheet at {x}");
        let stuck = x;
        assert!(!boat.steer(0.5, SPEED, 0.0, &world));
        assert_eq!(boat.origin().unwrap()[0], stuck);
        assert_eq!(boat.vertices().len(), 108);
        boat.dismount();
        assert!(!boat.aboard());
        assert!(boat.seat().is_none());
        assert!(boat.take());
        assert!(!boat.is_out());
        assert!(boat.vertices().is_empty());
    }
}
