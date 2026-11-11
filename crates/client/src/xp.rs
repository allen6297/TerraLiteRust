//! Collectible XP orbs and level progress.
//!
//! Mobs and ore mining drop small lime orbs that bob, then home toward the
//! player. Numbers stay tiny and tunable via the consts below.

use crate::mesh::Vertex;

/// XP needed to advance one level. Keep small so a short fight reads.
pub const XP_PER_LEVEL: i32 = 7;
/// Orbs spawned when a mob drops.
pub const MOB_ORBS: i32 = 3;
/// XP value of each mob orb.
pub const MOB_ORB_XP: i32 = 1;
/// XP from one ore break orb.
pub const MINE_ORB_XP: i32 = 1;

const ORB_HALF: f32 = 0.16;
const HOME_SPEED: f32 = 8.0;
const PICK_DIST: f32 = 0.95;
const DELAY: f32 = 0.28;
const LIFE: f32 = 12.0;
const COLOR: [f32; 3] = [0.62, 1.0, 0.28];
const GLOW: [f32; 3] = [0.85, 1.0, 0.4];

struct Orb {
    center: [f32; 3],
    velocity: [f32; 3],
    value: i32,
    age: f32,
    life: f32,
}

#[derive(Default)]
pub struct Xp {
    pub level: i32,
    /// Progress toward the next level (`0..XP_PER_LEVEL`).
    pub progress: i32,
    orbs: Vec<Orb>,
}

impl Xp {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn fraction(&self) -> f32 {
        (self.progress as f32 / XP_PER_LEVEL as f32).clamp(0.0, 1.0)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn orb_count(&self) -> usize {
        self.orbs.len()
    }

    /// Burst of orbs from a fallen mob.
    pub fn spawn_mob(&mut self, origin: [f32; 3]) {
        self.spawn_burst(origin, MOB_ORBS, MOB_ORB_XP);
    }

    /// One orb from mining ore (or a demo source).
    pub fn spawn_mine(&mut self, origin: [f32; 3]) {
        self.spawn_burst(origin, 1, MINE_ORB_XP);
    }

    /// Pin a still-frame orb at an exact center (skip bob / home).
    pub fn pin(&mut self, center: [f32; 3], value: i32) {
        self.orbs.push(Orb {
            center,
            velocity: [0.0, 0.0, 0.0],
            value: value.max(1),
            age: DELAY + 0.4,
            life: LIFE,
        });
    }

    fn spawn_burst(&mut self, origin: [f32; 3], count: i32, value: i32) {
        const DIRS: [[f32; 3]; 5] = [
            [0.55, 1.4, 0.15],
            [-0.6, 1.2, 0.35],
            [0.2, 1.6, -0.55],
            [-0.25, 1.1, -0.4],
            [0.45, 1.5, 0.5],
        ];
        let n = count.max(0) as usize;
        for (i, dir) in DIRS.into_iter().take(n).enumerate() {
            let jitter = i as f32 * 0.07;
            self.orbs.push(Orb {
                center: [
                    origin[0] + jitter * 0.2,
                    origin[1] + 0.2,
                    origin[2] - jitter * 0.15,
                ],
                velocity: dir,
                value: value.max(1),
                age: 0.0,
                life: LIFE,
            });
        }
    }

    /// True when ore mining should drop XP.
    pub fn is_ore(id: &str) -> bool {
        id.ends_with("_ore") || matches!(id, "coal_ore" | "iron_ore" | "ancient_debris")
    }

    /// Advance orbs. Returns true when the mesh should rebuild.
    /// Collected XP is applied immediately (may level up).
    pub fn tick(&mut self, dt: f32, eye: [f32; 3]) -> bool {
        if self.orbs.is_empty() {
            return false;
        }
        let step = dt.max(0.0);
        let mut gained = 0_i32;
        let mut dirty = false;
        self.orbs.retain_mut(|orb| {
            dirty = true;
            orb.age += step;
            orb.life -= step;
            if orb.life <= 0.0 {
                return false;
            }
            if orb.age < DELAY {
                orb.velocity[1] -= 2.8 * step;
                orb.center[0] += orb.velocity[0] * step;
                orb.center[1] += orb.velocity[1] * step;
                orb.center[2] += orb.velocity[2] * step;
                if orb.center[1] < eye[1] - 1.6 {
                    orb.center[1] = eye[1] - 1.6;
                    orb.velocity[1] *= -0.35;
                }
                return true;
            }
            let dx = eye[0] - orb.center[0];
            let dy = (eye[1] - 0.35) - orb.center[1];
            let dz = eye[2] - orb.center[2];
            let dist = (dx * dx + dy * dy + dz * dz).sqrt();
            if dist < PICK_DIST {
                gained += orb.value;
                return false;
            }
            if dist > 1.0e-4 {
                let inv = HOME_SPEED / dist;
                orb.velocity = [dx * inv, dy * inv, dz * inv];
            }
            orb.center[0] += orb.velocity[0] * step;
            orb.center[1] += orb.velocity[1] * step;
            orb.center[2] += orb.velocity[2] * step;
            true
        });
        if gained > 0 {
            let _ = self.gain(gained);
        }
        dirty
    }

    /// Tick orbs and report whether any level-ups happened this frame.
    pub fn tick_collect(&mut self, dt: f32, eye: [f32; 3]) -> (bool, i32) {
        let before = self.level;
        let dirty = self.tick(dt, eye);
        (dirty, self.level - before)
    }

    /// Apply XP. Returns how many levels were gained (for a chat tip).
    pub fn gain(&mut self, amount: i32) -> i32 {
        if amount <= 0 {
            return 0;
        }
        let before = self.level;
        self.progress += amount;
        while self.progress >= XP_PER_LEVEL {
            self.progress -= XP_PER_LEVEL;
            self.level = self.level.saturating_add(1);
        }
        self.level - before
    }

    /// Spend whole levels (anvil rename / repair). Leaves progress alone.
    pub fn spend_levels(&mut self, n: i32) -> bool {
        if n <= 0 {
            return true;
        }
        if self.level < n {
            return false;
        }
        self.level -= n;
        true
    }

    /// Soft lime cubes for the drop pass.
    pub fn vertices(&self) -> Vec<Vertex> {
        let mut out = Vec::with_capacity(self.orbs.len() * 36);
        for orb in &self.orbs {
            let pulse = 0.85 + 0.15 * (orb.age * 7.0).sin().abs();
            let color = [
                (COLOR[0] * pulse).min(1.0),
                (COLOR[1] * pulse).min(1.0),
                (COLOR[2] * pulse).min(1.0),
            ];
            let half = ORB_HALF * (0.9 + 0.15 * (orb.age * 5.0).sin().abs());
            push_box(&mut out, orb.center, half, color, GLOW);
        }
        out
    }
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
    fn mob_orbs_home_and_level_up() {
        let mut xp = Xp::new();
        xp.spawn_mob([8.0, 2.0, 10.0]);
        assert_eq!(xp.orb_count(), MOB_ORBS as usize);
        assert!(Xp::is_ore("coal_ore"));
        assert!(!Xp::is_ore("dirt"));
        // Skip delay, pull straight into the eye.
        let _ = xp.tick(DELAY + 0.05, [8.0, 2.0, 10.0]);
        let _ = xp.tick(0.2, [8.0, 2.0, 10.0]);
        assert!(xp.progress > 0 || xp.level > 0 || xp.orb_count() < MOB_ORBS as usize);
        assert!(xp.gain(XP_PER_LEVEL) >= 1);
        assert!(xp.level >= 1);
        xp.spawn_mine([1.0, 1.0, 1.0]);
        assert_eq!(xp.vertices().len() % 36, 0);
        assert!(!xp.vertices().is_empty());
        assert!(xp.fraction() < 1.0 || xp.progress == 0);
        assert!(!xp.vertices().is_empty() || xp.orb_count() == 0);
        let mut still = Xp::new();
        still.pin([2.0, 3.0, 4.0], 1);
        assert_eq!(still.orb_count(), 1);
        assert_eq!(still.vertices().len(), 36);
    }
}
