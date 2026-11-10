//! In-flight bow arrows.
//!
//! A charged bow release spawns one flight that arcs with gravity, damages the
//! first mob it clips, then sticks in a block or fades out.

use rebuild_world::Cluster;

use crate::mesh::Vertex;

/// Seconds to reach a full draw.
pub const DRAW_TIME: f32 = 0.85;
/// Minimum charge before a release fires.
pub const MIN_CHARGE: f32 = 0.22;
/// Top flight speed at full charge (blocks / second).
pub const MAX_SPEED: f32 = 28.0;
const GRAVITY: f32 = 14.0;
const FLY_LIFE: f32 = 6.0;
const STUCK_LIFE: f32 = 2.8;
/// Hearts dealt on a mob hit.
pub const HIT_HEARTS: i32 = 2;
const HALF: f32 = 0.06;
const COLOR: [f32; 3] = [0.72, 0.58, 0.32];
const GLOW: [f32; 3] = [0.35, 0.28, 0.18];

struct Flight {
    center: [f32; 3],
    velocity: [f32; 3],
    life: f32,
    stuck: bool,
}

#[derive(Default)]
pub struct Arrows {
    flights: Vec<Flight>,
}

impl Arrows {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn count(&self) -> usize {
        self.flights.len()
    }

    /// Spawn a flight from the eye along `forward` scaled by `charge` (`0..1`).
    pub fn fire(&mut self, eye: [f32; 3], forward: [f32; 3], charge: f32) {
        let charge = charge.clamp(0.0, 1.0);
        if charge < MIN_CHARGE {
            return;
        }
        let speed = MAX_SPEED * (0.35 + 0.65 * charge);
        let len = (forward[0] * forward[0] + forward[1] * forward[1] + forward[2] * forward[2])
            .sqrt()
            .max(1.0e-4);
        let dir = [forward[0] / len, forward[1] / len, forward[2] / len];
        self.flights.push(Flight {
            center: [
                eye[0] + dir[0] * 0.45,
                eye[1] + dir[1] * 0.45,
                eye[2] + dir[2] * 0.45,
            ],
            velocity: [dir[0] * speed, dir[1] * speed, dir[2] * speed],
            life: FLY_LIFE,
            stuck: false,
        });
    }

    /// Pin a mid-flight arrow for still shots.
    pub fn pin(&mut self, center: [f32; 3], velocity: [f32; 3]) {
        self.flights.push(Flight {
            center,
            velocity,
            life: FLY_LIFE,
            stuck: false,
        });
    }

    /// Advance flights. Returns tip positions of arrows that just stuck or are
    /// still flying (for mob hit tests), plus whether the mesh should rebuild.
    pub fn tick(&mut self, dt: f32, world: &Cluster) -> (bool, Vec<[f32; 3]>) {
        if self.flights.is_empty() {
            return (false, Vec::new());
        }
        let step = dt.max(0.0);
        let mut dirty = false;
        let mut tips = Vec::new();
        self.flights.retain_mut(|flight| {
            dirty = true;
            flight.life -= step;
            if flight.life <= 0.0 {
                return false;
            }
            if flight.stuck {
                return true;
            }
            flight.velocity[1] -= GRAVITY * step;
            let next = [
                flight.center[0] + flight.velocity[0] * step,
                flight.center[1] + flight.velocity[1] * step,
                flight.center[2] + flight.velocity[2] * step,
            ];
            if blocked(world, next) {
                flight.stuck = true;
                flight.life = flight.life.min(STUCK_LIFE);
                flight.velocity = [0.0, 0.0, 0.0];
                tips.push(flight.center);
                return true;
            }
            flight.center = next;
            tips.push(flight.center);
            true
        });
        (dirty, tips)
    }

    /// Centers of arrows still in the air.
    pub fn flying_tips(&self) -> Vec<[f32; 3]> {
        self.flights
            .iter()
            .filter(|flight| !flight.stuck)
            .map(|flight| flight.center)
            .collect()
    }

    /// Remove the nearest flying arrow within `reach` of `at` (mob hit).
    pub fn take_near(&mut self, at: [f32; 3], reach: f32) -> bool {
        let reach2 = reach * reach;
        let Some((index, _)) = self
            .flights
            .iter()
            .enumerate()
            .filter(|(_, flight)| !flight.stuck)
            .map(|(index, flight)| {
                let dx = flight.center[0] - at[0];
                let dy = flight.center[1] - at[1];
                let dz = flight.center[2] - at[2];
                (index, dx * dx + dy * dy + dz * dz)
            })
            .filter(|(_, d2)| *d2 <= reach2)
            .min_by(|(_, a), (_, b)| a.total_cmp(b))
        else {
            return false;
        };
        self.flights.remove(index);
        true
    }

    pub fn vertices(&self) -> Vec<Vertex> {
        let mut out = Vec::with_capacity(self.flights.len() * 36);
        for flight in &self.flights {
            let half = if flight.stuck { HALF * 0.85 } else { HALF };
            push_box(&mut out, flight.center, half, COLOR, GLOW);
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
    fn weak_charge_does_not_fire() {
        let mut arrows = Arrows::new();
        arrows.fire([0.0, 2.0, 0.0], [0.0, 0.0, 1.0], MIN_CHARGE * 0.5);
        assert_eq!(arrows.count(), 0);
        arrows.fire([0.0, 2.0, 0.0], [0.0, 0.0, 1.0], 1.0);
        assert_eq!(arrows.count(), 1);
        assert_eq!(arrows.vertices().len() % 36, 0);
    }

    #[test]
    fn take_near_removes_a_flying_tip() {
        let mut arrows = Arrows::new();
        arrows.pin([8.0, 2.0, 10.0], [0.0, 0.0, 4.0]);
        assert!(arrows.take_near([8.1, 2.0, 10.1], 0.5));
        assert_eq!(arrows.count(), 0);
        assert!(!arrows.take_near([8.0, 2.0, 10.0], 0.5));
    }
}
