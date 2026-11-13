//! Shulker levitation bullets.
//!
//! Slow purple bolts. Burst on a solid hit or when they clip the player,
//! applying a short levitation lift.

use rebuild_world::Cluster;

use crate::mesh::Vertex;

/// Flight speed (blocks / second).
pub const SPEED: f32 = 7.0;
const GRAVITY: f32 = 0.4;
const FLY_LIFE: f32 = 5.5;
/// Hearts dealt when a bullet clips the player.
pub const HIT_HEARTS: i32 = 1;
/// Seconds of upward float after a hit.
pub const LEVITATION_SECS: f32 = 2.4;
const HALF: f32 = 0.14;
const COLOR: [f32; 3] = [0.72, 0.42, 0.86];
const GLOW: [f32; 3] = [0.95, 0.7, 1.0];
/// Player hit radius around the torso under the eye.
pub const PLAYER_REACH: f32 = 0.9;

struct Flight {
    center: [f32; 3],
    velocity: [f32; 3],
    life: f32,
}

#[derive(Default)]
pub struct ShulkerShots {
    flights: Vec<Flight>,
}

impl ShulkerShots {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn count(&self) -> usize {
        self.flights.len()
    }

    /// Launch from the open shulker toward a world point (usually the player eye).
    pub fn shoot(&mut self, from: [f32; 3], toward: [f32; 3]) {
        let dx = toward[0] - from[0];
        let dy = toward[1] - from[1];
        let dz = toward[2] - from[2];
        let len = (dx * dx + dy * dy + dz * dz).sqrt().max(1.0e-4);
        let dir = [dx / len, dy / len, dz / len];
        self.flights.push(Flight {
            center: [
                from[0] + dir[0] * 0.4,
                from[1] + dir[1] * 0.4,
                from[2] + dir[2] * 0.4,
            ],
            velocity: [dir[0] * SPEED, dir[1] * SPEED + 0.3, dir[2] * SPEED],
            life: FLY_LIFE,
        });
    }

    /// Pin a mid-flight bullet for still shots.
    pub fn pin(&mut self, center: [f32; 3], velocity: [f32; 3]) {
        self.flights.push(Flight {
            center,
            velocity,
            life: FLY_LIFE,
        });
    }

    /// Advance flights. Returns mesh dirty, burst centers, and whether any
    /// bolt clipped the player body around `eye`.
    pub fn tick(
        &mut self,
        dt: f32,
        world: &Cluster,
        eye: [f32; 3],
    ) -> (bool, Vec<[f32; 3]>, bool) {
        if self.flights.is_empty() {
            return (false, Vec::new(), false);
        }
        let step = dt.max(0.0);
        let mut dirty = false;
        let mut bursts = Vec::new();
        let mut hit_player = false;
        let reach2 = PLAYER_REACH * PLAYER_REACH;
        self.flights.retain_mut(|flight| {
            dirty = true;
            flight.life -= step;
            if flight.life <= 0.0 {
                return false;
            }
            flight.velocity[1] -= GRAVITY * step;
            let next = [
                flight.center[0] + flight.velocity[0] * step,
                flight.center[1] + flight.velocity[1] * step,
                flight.center[2] + flight.velocity[2] * step,
            ];
            let dx = next[0] - eye[0];
            let dy = next[1] - (eye[1] - 0.7);
            let dz = next[2] - eye[2];
            if dx * dx + dy * dy + dz * dz <= reach2 {
                bursts.push(next);
                hit_player = true;
                return false;
            }
            if blocked(world, next) {
                bursts.push(flight.center);
                return false;
            }
            flight.center = next;
            true
        });
        (dirty, bursts, hit_player)
    }

    pub fn vertices(&self) -> Vec<Vertex> {
        let mut out = Vec::with_capacity(self.flights.len() * 72);
        for flight in &self.flights {
            push_box(&mut out, flight.center, HALF, COLOR, GLOW);
            push_box(
                &mut out,
                [
                    flight.center[0],
                    flight.center[1] + 0.04,
                    flight.center[2],
                ],
                HALF * 0.55,
                [0.95, 0.85, 1.0],
                GLOW,
            );
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
    block.id != "air"
        && block.id != "nether_portal"
        && block.id != "end_portal"
        && block.id != "end_gateway"
        && !block.id.is_empty()
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
    use rebuild_world::{Chunk, Cluster};

    #[test]
    fn a_shulker_bullet_flies_and_can_hit() {
        let mut shots = ShulkerShots::new();
        shots.shoot([0.0, 2.0, 0.0], [3.5, 1.6, 0.0]);
        assert_eq!(shots.count(), 1);
        let world = Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, []).unwrap()]).unwrap();
        let mut hit = false;
        for _ in 0..50 {
            let (_, _, clipped) = shots.tick(0.05, &world, [3.5, 1.6, 0.0]);
            if clipped {
                hit = true;
                break;
            }
        }
        assert!(hit, "bullet should reach the player");
        assert_eq!(shots.count(), 0);
    }
}
