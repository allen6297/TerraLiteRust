//! Thrown ender pearls.
//!
//! A right-click throw arcs with gravity. On the first solid hit the pearl
//! pops and the thrower teleports to a safe standing spot nearby.

use rebuild_world::Cluster;

use crate::mesh::Vertex;

/// Flight speed at throw (blocks / second).
pub const THROW_SPEED: f32 = 16.0;
const GRAVITY: f32 = 16.0;
const FLY_LIFE: f32 = 6.0;
/// Hearts dealt on a survival teleport.
pub const LAND_HURT: i32 = 1;
const HALF: f32 = 0.08;
const COLOR: [f32; 3] = [0.22, 0.72, 0.58];
const GLOW: [f32; 3] = [0.10, 0.38, 0.32];

struct Flight {
    center: [f32; 3],
    velocity: [f32; 3],
    life: f32,
}

#[derive(Default)]
pub struct Pearls {
    flights: Vec<Flight>,
}

impl Pearls {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn count(&self) -> usize {
        self.flights.len()
    }

    /// Throw from the eye along `forward`.
    pub fn throw(&mut self, eye: [f32; 3], forward: [f32; 3]) {
        let len = (forward[0] * forward[0] + forward[1] * forward[1] + forward[2] * forward[2])
            .sqrt()
            .max(1.0e-4);
        let dir = [forward[0] / len, forward[1] / len, forward[2] / len];
        self.flights.push(Flight {
            center: [
                eye[0] + dir[0] * 0.4,
                eye[1] + dir[1] * 0.4,
                eye[2] + dir[2] * 0.4,
            ],
            velocity: [
                dir[0] * THROW_SPEED,
                dir[1] * THROW_SPEED + 2.0,
                dir[2] * THROW_SPEED,
            ],
            life: FLY_LIFE,
        });
    }

    /// Pin a mid-flight pearl for still shots.
    pub fn pin(&mut self, center: [f32; 3], velocity: [f32; 3]) {
        self.flights.push(Flight {
            center,
            velocity,
            life: FLY_LIFE,
        });
    }

    /// Advance flights. Returns mesh dirty + impact centers that just landed.
    pub fn tick(&mut self, dt: f32, world: &Cluster) -> (bool, Vec<[f32; 3]>) {
        if self.flights.is_empty() {
            return (false, Vec::new());
        }
        let step = dt.max(0.0);
        let mut dirty = false;
        let mut landings = Vec::new();
        self.flights.retain_mut(|flight| {
            dirty = true;
            flight.life -= step;
            if flight.life <= 0.0 {
                landings.push(flight.center);
                return false;
            }
            flight.velocity[1] -= GRAVITY * step;
            let next = [
                flight.center[0] + flight.velocity[0] * step,
                flight.center[1] + flight.velocity[1] * step,
                flight.center[2] + flight.velocity[2] * step,
            ];
            if blocked(world, next) {
                landings.push(flight.center);
                return false;
            }
            flight.center = next;
            true
        });
        (dirty, landings)
    }

    pub fn vertices(&self) -> Vec<Vertex> {
        let mut out = Vec::with_capacity(self.flights.len() * 36);
        for flight in &self.flights {
            push_box(&mut out, flight.center, HALF, COLOR, GLOW);
        }
        out
    }
}

fn blocked(world: &Cluster, at: [f32; 3]) -> bool {
    let x = at[0].floor() as i32;
    let y = at[1].floor() as i32;
    let z = at[2].floor() as i32;
    if y < 0 {
        return true;
    }
    let block = world.block_at(x, y, z);
    block.id != "air" && !block.id.is_empty()
}

fn push_box(
    out: &mut Vec<Vertex>,
    center: [f32; 3],
    half: f32,
    color: [f32; 3],
    block: [f32; 3],
) {
    let min = [center[0] - half, center[1] - half, center[2] - half];
    let max = [center[0] + half, center[1] + half, center[2] + half];
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

    #[test]
    fn throw_spawns_a_pearl_flight() {
        let mut pearls = Pearls::new();
        pearls.throw([0.0, 2.0, 0.0], [0.0, 0.0, 1.0]);
        assert_eq!(pearls.count(), 1);
        assert_eq!(pearls.vertices().len() % 36, 0);
    }
}
