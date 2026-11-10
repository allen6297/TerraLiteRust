//! Throwable splash potions.
//!
//! A right-click throw arcs with gravity, bursts on the first solid hit, then
//! applies a short AOE (heal the thrower or hurt nearby mobs).

use rebuild_world::Cluster;

use crate::mesh::Vertex;

/// Flight speed at throw (blocks / second).
pub const THROW_SPEED: f32 = 14.0;
const GRAVITY: f32 = 18.0;
const FLY_LIFE: f32 = 5.0;
/// AOE radius around the burst, in blocks.
pub const RADIUS: f32 = 2.4;
/// Hearts dealt to each mob inside a harming splash.
pub const HARM_HEARTS: i32 = 2;
const HALF: f32 = 0.09;

/// Effect carried by a thrown bottle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Effect {
    Heal,
    Harm,
}

impl Effect {
    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "splash_healing_potion" => Some(Self::Heal),
            "splash_harming_potion" => Some(Self::Harm),
            _ => None,
        }
    }

    pub fn color(self) -> [f32; 3] {
        match self {
            Self::Heal => [0.92, 0.42, 0.58],
            Self::Harm => [0.55, 0.18, 0.72],
        }
    }

    pub fn glow(self) -> [f32; 3] {
        match self {
            Self::Heal => [0.55, 0.18, 0.28],
            Self::Harm => [0.28, 0.08, 0.42],
        }
    }
}

/// One burst that just landed — center + effect for AOE apply + mote spawn.
#[derive(Clone, Copy, Debug)]
pub struct Burst {
    pub center: [f32; 3],
    pub effect: Effect,
}

struct Flight {
    center: [f32; 3],
    velocity: [f32; 3],
    life: f32,
    effect: Effect,
}

#[derive(Default)]
pub struct Splash {
    flights: Vec<Flight>,
}

impl Splash {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn count(&self) -> usize {
        self.flights.len()
    }

    /// Throw from the eye along `forward`.
    pub fn throw(&mut self, eye: [f32; 3], forward: [f32; 3], effect: Effect) {
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
                dir[1] * THROW_SPEED + 2.5,
                dir[2] * THROW_SPEED,
            ],
            life: FLY_LIFE,
            effect,
        });
    }

    /// Pin a mid-flight bottle for still shots.
    pub fn pin(&mut self, center: [f32; 3], velocity: [f32; 3], effect: Effect) {
        self.flights.push(Flight {
            center,
            velocity,
            life: FLY_LIFE,
            effect,
        });
    }

    /// Advance flights. Returns mesh dirty + bursts that just hit a block.
    pub fn tick(&mut self, dt: f32, world: &Cluster) -> (bool, Vec<Burst>) {
        if self.flights.is_empty() {
            return (false, Vec::new());
        }
        let step = dt.max(0.0);
        let mut dirty = false;
        let mut bursts = Vec::new();
        self.flights.retain_mut(|flight| {
            dirty = true;
            flight.life -= step;
            if flight.life <= 0.0 {
                bursts.push(Burst {
                    center: flight.center,
                    effect: flight.effect,
                });
                return false;
            }
            flight.velocity[1] -= GRAVITY * step;
            let next = [
                flight.center[0] + flight.velocity[0] * step,
                flight.center[1] + flight.velocity[1] * step,
                flight.center[2] + flight.velocity[2] * step,
            ];
            if blocked(world, next) {
                bursts.push(Burst {
                    center: flight.center,
                    effect: flight.effect,
                });
                return false;
            }
            flight.center = next;
            true
        });
        (dirty, bursts)
    }

    pub fn vertices(&self) -> Vec<Vertex> {
        let mut out = Vec::with_capacity(self.flights.len() * 36);
        for flight in &self.flights {
            push_box(
                &mut out,
                flight.center,
                HALF,
                flight.effect.color(),
                flight.effect.glow(),
            );
        }
        out
    }
}

/// True when `at` is inside the AOE sphere around `center`.
pub fn in_radius(center: [f32; 3], at: [f32; 3]) -> bool {
    let dx = at[0] - center[0];
    let dy = at[1] - center[1];
    let dz = at[2] - center[2];
    dx * dx + dy * dy + dz * dz <= RADIUS * RADIUS
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
    fn throw_spawns_a_flight() {
        let mut splash = Splash::new();
        splash.throw([0.0, 2.0, 0.0], [0.0, 0.0, 1.0], Effect::Heal);
        assert_eq!(splash.count(), 1);
        assert_eq!(splash.vertices().len() % 36, 0);
    }

    #[test]
    fn radius_covers_nearby_points() {
        let center = [8.0, 1.0, 10.0];
        assert!(in_radius(center, [8.5, 1.2, 10.3]));
        assert!(!in_radius(center, [14.0, 1.0, 10.0]));
    }
}
