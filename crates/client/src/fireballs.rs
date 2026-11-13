//! Blaze / ghast fireball projectiles.
//!
//! Slow orange bolts with light gravity. Burst on a solid hit or when they
//! clip the player. Ghast balls are larger, hit harder, and can be punched back.

use rebuild_world::Cluster;

use crate::mesh::Vertex;

/// Flight speed (blocks / second).
pub const SPEED: f32 = 9.0;
/// Ghast fireball flight speed.
pub const GHAST_SPEED: f32 = 7.5;
const GRAVITY: f32 = 2.5;
const FLY_LIFE: f32 = 5.0;
const GHAST_LIFE: f32 = 7.0;
/// Hearts dealt when a blaze fireball clips the player.
pub const HIT_HEARTS: i32 = 1;
/// Hearts dealt when a ghast fireball clips the player.
pub const GHAST_HIT_HEARTS: i32 = 3;
const HALF: f32 = 0.12;
const GHAST_HALF: f32 = 0.38;
const COLOR: [f32; 3] = [1.0, 0.55, 0.12];
const GLOW: [f32; 3] = [0.85, 0.22, 0.05];
const GHAST_COLOR: [f32; 3] = [1.0, 0.72, 0.35];
const GHAST_GLOW: [f32; 3] = [0.95, 0.35, 0.08];
/// Player hit radius around the torso under the eye.
pub const PLAYER_REACH: f32 = 0.85;
pub const GHAST_PLAYER_REACH: f32 = 1.15;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Blaze,
    Ghast,
}

struct Flight {
    center: [f32; 3],
    velocity: [f32; 3],
    life: f32,
    kind: Kind,
    /// True after the player punched this ball back at the ghast.
    punched: bool,
}

#[derive(Default)]
pub struct Fireballs {
    flights: Vec<Flight>,
}

impl Fireballs {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn count(&self) -> usize {
        self.flights.len()
    }

    /// Launch from the blaze toward a world point (usually the player eye).
    pub fn shoot(&mut self, from: [f32; 3], toward: [f32; 3]) {
        self.launch(from, toward, Kind::Blaze);
    }

    /// Launch a large ghast fireball.
    pub fn shoot_ghast(&mut self, from: [f32; 3], toward: [f32; 3]) {
        self.launch(from, toward, Kind::Ghast);
    }

    fn launch(&mut self, from: [f32; 3], toward: [f32; 3], kind: Kind) {
        let dx = toward[0] - from[0];
        let dy = toward[1] - from[1];
        let dz = toward[2] - from[2];
        let len = (dx * dx + dy * dy + dz * dz).sqrt().max(1.0e-4);
        let dir = [dx / len, dy / len, dz / len];
        let speed = match kind {
            Kind::Blaze => SPEED,
            Kind::Ghast => GHAST_SPEED,
        };
        let life = match kind {
            Kind::Blaze => FLY_LIFE,
            Kind::Ghast => GHAST_LIFE,
        };
        let lift = match kind {
            Kind::Blaze => 0.8,
            Kind::Ghast => 0.35,
        };
        self.flights.push(Flight {
            center: [
                from[0] + dir[0] * 0.55,
                from[1] + dir[1] * 0.55,
                from[2] + dir[2] * 0.55,
            ],
            velocity: [dir[0] * speed, dir[1] * speed + lift, dir[2] * speed],
            life,
            kind,
            punched: false,
        });
    }

    /// Pin a mid-flight fireball for still shots.
    pub fn pin(&mut self, center: [f32; 3], velocity: [f32; 3]) {
        self.flights.push(Flight {
            center,
            velocity,
            life: FLY_LIFE,
            kind: Kind::Blaze,
            punched: false,
        });
    }

    /// Pin a large ghast ball for still shots.
    pub fn pin_ghast(&mut self, center: [f32; 3], velocity: [f32; 3]) {
        self.flights.push(Flight {
            center,
            velocity,
            life: GHAST_LIFE,
            kind: Kind::Ghast,
            punched: false,
        });
    }

    /// Punch the nearest ghast fireball along the look ray back toward `toward`.
    /// True when a ball flipped.
    pub fn try_punch(&mut self, eye: [f32; 3], forward: [f32; 3], toward: [f32; 3]) -> bool {
        let mut best: Option<(usize, f32)> = None;
        for (index, flight) in self.flights.iter().enumerate() {
            if flight.kind != Kind::Ghast || flight.punched {
                continue;
            }
            let to = [
                flight.center[0] - eye[0],
                flight.center[1] - eye[1],
                flight.center[2] - eye[2],
            ];
            let along = to[0] * forward[0] + to[1] * forward[1] + to[2] * forward[2];
            if !(0.4..=5.5).contains(&along) {
                continue;
            }
            let closest = [
                eye[0] + forward[0] * along,
                eye[1] + forward[1] * along,
                eye[2] + forward[2] * along,
            ];
            let dx = closest[0] - flight.center[0];
            let dy = closest[1] - flight.center[1];
            let dz = closest[2] - flight.center[2];
            let dist2 = dx * dx + dy * dy + dz * dz;
            if dist2 > 0.85 {
                continue;
            }
            if best.is_none_or(|(_, so_far)| along < so_far) {
                best = Some((index, along));
            }
        }
        let Some((index, _)) = best else {
            return false;
        };
        let flight = &mut self.flights[index];
        let dx = toward[0] - flight.center[0];
        let dy = toward[1] - flight.center[1];
        let dz = toward[2] - flight.center[2];
        let len = (dx * dx + dy * dy + dz * dz).sqrt().max(1.0e-4);
        flight.velocity = [
            dx / len * GHAST_SPEED * 1.15,
            dy / len * GHAST_SPEED * 1.15,
            dz / len * GHAST_SPEED * 1.15,
        ];
        flight.punched = true;
        flight.life = GHAST_LIFE;
        true
    }

    /// Advance flights. Returns mesh dirty, burst centers, player-hit hearts
    /// (0 if none), and whether a punched ball clipped `ghast_hit` (ghast origin).
    pub fn tick(
        &mut self,
        dt: f32,
        world: &Cluster,
        eye: [f32; 3],
        ghast_hit: Option<[f32; 3]>,
    ) -> (bool, Vec<[f32; 3]>, i32, bool) {
        if self.flights.is_empty() {
            return (false, Vec::new(), 0, false);
        }
        let step = dt.max(0.0);
        let mut dirty = false;
        let mut bursts = Vec::new();
        let mut hit_hearts = 0;
        let mut hit_ghast = false;
        let reach2 = PLAYER_REACH * PLAYER_REACH;
        let ghast_reach2 = GHAST_PLAYER_REACH * GHAST_PLAYER_REACH;
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
            if flight.punched {
                if let Some(target) = ghast_hit {
                    let dx = next[0] - target[0];
                    let dy = next[1] - (target[1] + 1.0);
                    let dz = next[2] - target[2];
                    if dx * dx + dy * dy + dz * dz <= 2.2 {
                        bursts.push(next);
                        hit_ghast = true;
                        return false;
                    }
                }
            } else {
                let dx = next[0] - eye[0];
                let dy = next[1] - (eye[1] - 0.7);
                let dz = next[2] - eye[2];
                let r2 = match flight.kind {
                    Kind::Blaze => reach2,
                    Kind::Ghast => ghast_reach2,
                };
                if dx * dx + dy * dy + dz * dz <= r2 {
                    bursts.push(next);
                    hit_hearts = hit_hearts.max(match flight.kind {
                        Kind::Blaze => HIT_HEARTS,
                        Kind::Ghast => GHAST_HIT_HEARTS,
                    });
                    return false;
                }
            }
            if blocked(world, next) {
                bursts.push(flight.center);
                return false;
            }
            flight.center = next;
            true
        });
        (dirty, bursts, hit_hearts, hit_ghast)
    }

    pub fn vertices(&self) -> Vec<Vertex> {
        let mut out = Vec::with_capacity(self.flights.len() * 72);
        for flight in &self.flights {
            let (half, color, glow) = match flight.kind {
                Kind::Blaze => (HALF, COLOR, GLOW),
                Kind::Ghast => (GHAST_HALF, GHAST_COLOR, GHAST_GLOW),
            };
            push_box(&mut out, flight.center, half, color, glow);
            push_box(
                &mut out,
                [
                    flight.center[0],
                    flight.center[1] + half * 0.35,
                    flight.center[2],
                ],
                half * 0.55,
                [1.0, 0.9, 0.45],
                glow,
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

    fn empty_world() -> Cluster {
        Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, Vec::new()).unwrap()]).unwrap()
    }

    #[test]
    fn a_fireball_flies_toward_the_target() {
        let mut balls = Fireballs::new();
        balls.shoot([0.0, 2.0, 0.0], [6.0, 2.0, 0.0]);
        assert_eq!(balls.count(), 1);
        let world = empty_world();
        let (dirty, bursts, hit, _) = balls.tick(0.05, &world, [100.0, 2.0, 0.0], None);
        assert!(dirty);
        assert!(bursts.is_empty());
        assert_eq!(hit, 0);
        assert!(balls.flights[0].center[0] > 0.0);
    }

    #[test]
    fn a_fireball_clips_the_player() {
        let mut balls = Fireballs::new();
        balls.pin([1.0, 1.6, 1.0], [0.0, 0.0, 0.0]);
        let world = empty_world();
        let (_dirty, bursts, hit, _) = balls.tick(0.01, &world, [1.0, 1.6, 1.0], None);
        assert_eq!(hit, HIT_HEARTS);
        assert!(!bursts.is_empty());
        assert_eq!(balls.count(), 0);
    }

    #[test]
    fn a_ghast_fireball_punches_back() {
        let mut balls = Fireballs::new();
        balls.pin_ghast([2.0, 2.0, 2.0], [1.0, 0.0, 0.0]);
        assert!(balls.try_punch(
            [0.5, 2.0, 2.0],
            [1.0, 0.0, 0.0],
            [8.0, 3.0, 2.0]
        ));
        assert!(balls.flights[0].punched);
        assert!(balls.flights[0].velocity[0] > 0.0);
        let world = empty_world();
        let (_d, bursts, hearts, hit_ghast) =
            balls.tick(0.2, &world, [0.0, 2.0, 0.0], Some([8.0, 3.0, 2.0]));
        assert_eq!(hearts, 0);
        // May or may not reach in one step; punched flag is the contract.
        let _ = (bursts, hit_ghast);
    }
}
