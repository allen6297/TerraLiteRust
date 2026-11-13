//! A pig, a horse, a zombie, a breeze, a copper golem, a wolf, a cat, a parrot,
//! a frog, an axolotl, a chicken, a bee, a fox, a goat, a village trader,
//! an iron golem that guards the village green, and a blaze over the nether
//! fortress cage.
//!
//! The zombie body is `models/entity/zombie.bbmodel` with `textures/entity/zombie.png`,
//! playing `idle`, `walk`, and `attack` the same way the chicken and player do.
//! Night and dark-cave spawns reuse that same mesh and chase/bite AI; the trial
//! spawner zombie stays a separate demo body.
//!
//! The entity tick runs `"pig"`, `"horse"`, `"zombie"`, `"breeze"`, `"golem"`,
//! `"wolf"`, `"cat"`, `"parrot"`, `"frog"`, `"bee"`, `"fox"`, and `"goat"` every
//! 50 ms, the same way it runs `"sheep"`. The pig walks a square. The horse
//! paces the east meadow until wheat and a saddle put you on its back. The
//! zombie bites.
//! The breeze fires a gust. The golem wakes on a copper bulb. The wolf sits or
//! follows, bites, and turns to face the step. The cat sits or follows, hisses,
//! and turns with the step. The parrot echoes a note, perches, or follows, and
//! turns with the step. The frog hops and eats a bug. The axolotl pauses with its gills open, swims one
//! side of the pool, and as a companion pathfinds through water to stay near
//! your feet — hanging at the wet edge when you step onto land, and appearing
//! beside you in water past twelve blocks. The chicken pecks, walks its yard, steps around a full block, and
//! lays an egg. The zombie walks around a full block to bite. The bee carries
//! pollen to a hive. The fox sits or follows, fetches, and turns with the step.
//! A standing pet appears beside you past twelve blocks. A wild one pauses and
//! picks a new heading instead of a square. The goat paces the hill and
//! rams. None of the bodies are saved.

use rebuild_world::Cluster;

use crate::body::EYE_HEIGHT;
use crate::mesh::Vertex;

/// Horizontal speed while riding a pig, in blocks per second.
pub const RIDE_SPEED: f32 = 6.0;
/// Horizontal speed while riding a strider across lava, in blocks per second.
pub const STRIDER_SPEED: f32 = 5.5;
/// Horizontal speed while riding a horse, in blocks per second.
pub const HORSE_SPEED: f32 = 9.0;
const BOARD: f32 = 2.5;

/// East meadow where the horse paces. Walk east from the stairs.
pub const HORSE_PAD: (i32, i32) = (20, 6);
/// Fence hitch west of the meadow horse. Same cell as `rebuild_world::HORSE_HITCH`.
pub const HORSE_HITCH: (i32, i32) = (18, 6);

const LEG: f32 = 2.0;
const STEPS_PER_LEG: u32 = 8;
const AVOID: f32 = 1.6;
/// The zombie stops this far from the player's feet.
const REACH: f32 = 1.2;
const STEP: f32 = 0.12;
/// Entity ticks between bites.
const BITE_EVERY: u32 = 20;
/// A standing pet appears beside you past this distance, in blocks.
const LEASH: f32 = 12.0;

/// A short walk, a pause, then a new heading. Not one side of a square.
struct Roam {
    aim: [f32; 2],
    left: u32,
    walking: bool,
    heading: u8,
}

impl Roam {
    fn around(home: [f32; 3]) -> Self {
        Self {
            aim: [home[0] + 1.6, home[2] + 0.45],
            left: 10,
            walking: true,
            heading: 0,
        }
    }
}

const ROAM_HEAD: [(f32, f32); 5] = [
    (1.0, 0.35),
    (0.15, 1.0),
    (-0.9, 0.4),
    (-0.25, -1.0),
    (0.75, -0.6),
];

/// Box bodies are built facing +X. This yaw sends that face along `(dx, dz)`.
fn box_yaw(dx: f32, dz: f32) -> Option<f32> {
    if dx * dx + dz * dz < 1.0e-8 {
        return None;
    }
    Some((-dz).atan2(dx))
}

fn face_box(yaw: &mut f32, dx: f32, dz: f32) {
    if let Some(next) = box_yaw(dx, dz) {
        *yaw = next;
    }
}

/// Bodies built facing +Z. This is the same turn a Blockbench model uses.
fn face_forward(yaw: &mut f32, dx: f32, dz: f32) {
    if dx * dx + dz * dz < 1.0e-8 {
        return;
    }
    *yaw = dx.atan2(dz);
}

/// Turn every box of a body around its feet, the way a Blockbench model yaws.
fn yaw_mesh(vertices: &mut [Vertex], feet: [f32; 3], yaw: f32) {
    if yaw.abs() < 1.0e-5 {
        return;
    }
    let (s, c) = yaw.sin_cos();
    for vertex in vertices {
        let x = vertex.position[0] - feet[0];
        let z = vertex.position[2] - feet[2];
        vertex.position[0] = feet[0] + x * c + z * s;
        vertex.position[2] = feet[2] - x * s + z * c;
        let nx = vertex.normal[0];
        let nz = vertex.normal[2];
        vertex.normal[0] = nx * c + nz * s;
        vertex.normal[2] = -nx * s + nz * c;
    }
}

fn pet_far(origin: [f32; 3], player: [f32; 2]) -> bool {
    let dx = player[0] - origin[0];
    let dz = player[1] - origin[2];
    dx * dx + dz * dz > LEASH * LEASH
}

fn appear_beside(origin: &mut [f32; 3], yaw: &mut f32, player: [f32; 2], world: &Cluster) {
    let feet_y = origin[1].floor() as i32;
    let spots = [
        [player[0] + 1.15, player[1] + 0.35],
        [player[0] - 1.15, player[1] + 0.2],
        [player[0] + 0.3, player[1] + 1.15],
        [player[0] + 0.2, player[1] - 1.15],
    ];
    for spot in spots {
        let x = spot[0].floor() as i32;
        let z = spot[1].floor() as i32;
        let open = (0..1).all(|rise| !cell_blocks(world, x, feet_y + rise, z));
        if !open {
            continue;
        }
        face_box(yaw, player[0] - spot[0], player[1] - spot[1]);
        origin[0] = spot[0];
        origin[2] = spot[1];
        return;
    }
    face_box(yaw, -1.0, 0.0);
    origin[0] = player[0] + 1.0;
    origin[2] = player[1];
}

/// Step toward `goal` until within `stop`. True when already that close.
fn heel_step(
    origin: &mut [f32; 3],
    yaw: &mut f32,
    goal: [f32; 2],
    stop: f32,
    world: &Cluster,
) -> bool {
    let dx = goal[0] - origin[0];
    let dz = goal[1] - origin[2];
    let dist = (dx * dx + dz * dz).sqrt();
    if dist <= stop || dist <= 0.05 {
        face_box(yaw, dx, dz);
        settle_land(origin, world);
        return true;
    }
    let from = [origin[0], origin[2]];
    let feet_y = origin[1].floor() as i32;
    if let Some(next) = land_step(world, from, goal, STEP.min(dist - stop), feet_y, 1) {
        face_box(yaw, next[0] - from[0], next[1] - from[1]);
        origin[0] = next[0];
        origin[2] = next[1];
    }
    settle_land(origin, world);
    false
}

fn roam_step(
    origin: &mut [f32; 3],
    yaw: &mut f32,
    roam: &mut Roam,
    home: [f32; 3],
    world: &Cluster,
) {
    if roam.left == 0 {
        roam.walking = !roam.walking;
        roam.left = if roam.walking { 14 } else { 8 };
        if roam.walking {
            roam.heading = (roam.heading + 1) % ROAM_HEAD.len() as u8;
            let (mut hx, mut hz) = ROAM_HEAD[roam.heading as usize];
            let away_x = origin[0] - home[0];
            let away_z = origin[2] - home[2];
            if away_x * away_x + away_z * away_z > 9.0 {
                hx = -away_x;
                hz = -away_z;
            }
            let len = (hx * hx + hz * hz).sqrt().max(0.001);
            roam.aim = [origin[0] + hx / len * 2.4, origin[2] + hz / len * 2.4];
        }
    }
    roam.left -= 1;
    if roam.walking {
        let _ = heel_step(origin, yaw, roam.aim, 0.2, world);
    }
}

pub struct Pig {
    steps: u32,
    origin: [f32; 3],
    yaw: f32,
    saddled: bool,
    ridden: bool,
    /// Entity ticks left before the square path resumes.
    loose: u32,
}

impl Pig {
    pub fn new() -> Self {
        Self {
            steps: 0,
            origin: pig_point(0),
            yaw: 0.0,
            saddled: false,
            ridden: false,
            loose: 0,
        }
    }

    /// Shove the pig away from a wind burst. True when it was inside.
    pub fn shove(&mut self, burst: [f32; 3]) -> bool {
        let Some(gust) = crate::body::wind_push(burst, self.origin) else {
            return false;
        };
        self.origin[0] += gust.dx;
        self.origin[2] += gust.dz;
        self.loose = 40;
        true
    }

    pub fn bump(&mut self, from: [f32; 3]) -> bool {
        bump_origin(&mut self.origin, &mut self.loose, from)
    }

    pub fn saddled(&self) -> bool {
        self.saddled
    }

    pub fn ridden(&self) -> bool {
        self.ridden
    }

    /// Put a saddle on. False when one is already there.
    pub fn saddle(&mut self) -> bool {
        if self.saddled {
            return false;
        }
        self.saddled = true;
        true
    }

    pub fn mount(&mut self) -> bool {
        if !self.saddled {
            return false;
        }
        self.ridden = true;
        true
    }

    pub fn dismount(&mut self) {
        self.ridden = false;
    }

    pub fn near(&self, eye: [f32; 3]) -> bool {
        let [x, y, z] = self.origin;
        let dx = eye[0] - x;
        let dy = eye[1] - (y + 0.5);
        let dz = eye[2] - z;
        dx * dx + dy * dy + dz * dz < BOARD * BOARD
    }

    /// True when the look ray passes close to the body.
    pub fn aimed(&self, eye: [f32; 3], forward: [f32; 3]) -> bool {
        let [x, y, z] = self.origin;
        let center = [x, y + 0.5, z];
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

    /// Eye position while riding.
    pub fn seat(&self) -> Option<[f32; 3]> {
        if !self.ridden {
            return None;
        }
        let [x, y, z] = self.origin;
        Some([x, y + 0.55 + EYE_HEIGHT, z])
    }

    /// Slide by `vx` and `vz`. A step onto air, water, or a solid cell is dropped.
    pub fn steer(&mut self, dt: f32, vx: f32, vz: f32, world: &Cluster) -> bool {
        if !self.ridden || dt <= 0.0 {
            return false;
        }
        let [x, y, z] = self.origin;
        let mut next = self.origin;
        let nx = x + vx * dt;
        if pig_ground(world, nx, z, y) {
            next[0] = nx;
        }
        let nz = z + vz * dt;
        if pig_ground(world, next[0], nz, y) {
            next[2] = nz;
        }
        let moved = next != self.origin;
        face_box(&mut self.yaw, next[0] - x, next[2] - z);
        self.origin = next;
        moved
    }

    /// Walk the square, then step away when `player` (x, z) is inside [`AVOID`].
    ///
    /// A saddle makes the pig wait where it is.
    pub fn tick_near(&mut self, count: u32, player: [f32; 2], world: &Cluster) {
        if self.loose > 0 {
            self.loose = self.loose.saturating_sub(count);
            return;
        }
        if self.saddled {
            return;
        }
        let from_x = self.origin[0];
        let from_z = self.origin[2];
        self.steps = self.steps.wrapping_add(count);
        let [x, y, z] = pig_point(self.steps);
        let dx = x - player[0];
        let dz = z - player[1];
        let dist = (dx * dx + dz * dz).sqrt();
        let goal = if dist < AVOID {
            let (ux, uz) = if dist > 1.0e-4 {
                (dx / dist, dz / dist)
            } else {
                (1.0, 0.0)
            };
            let push = AVOID - dist;
            [x + ux * push, y, z + uz * push]
        } else {
            [x, y, z]
        };
        slide_land(&mut self.origin, goal, world, 2);
        face_box(
            &mut self.yaw,
            self.origin[0] - from_x,
            self.origin[2] - from_z,
        );
    }

    pub fn bounds(&self) -> ([f32; 3], [f32; 3]) {
        let [x, y, z] = self.origin;
        ([x - 0.42, y, z - 0.24], [x + 0.42, y + 1.05, z + 0.24])
    }

    pub fn vertices(&self) -> Vec<Vertex> {
        let [x, y, z] = self.origin;
        let mut out = Vec::new();
        push_box(
            &mut out,
            [x, y + 0.42, z],
            [0.42, 0.22, 0.22],
            [0.93, 0.62, 0.68],
        );
        push_box(
            &mut out,
            [x + 0.38, y + 0.48, z],
            [0.14, 0.12, 0.12],
            [0.84, 0.45, 0.52],
        );
        if self.saddled {
            push_box(
                &mut out,
                [x, y + 0.68, z],
                [0.28, 0.06, 0.18],
                [0.48, 0.30, 0.16],
            );
        }
        yaw_mesh(&mut out, self.origin, self.yaw);
        out
    }
}

/// A lava-striding mount for the nether pocket. Mount with F, steer with WASD.
pub struct Strider {
    origin: [f32; 3],
    yaw: f32,
    ridden: bool,
    alive: bool,
}

impl Strider {
    pub fn new() -> Self {
        Self {
            origin: [0.0, 0.0, 0.0],
            yaw: 0.0,
            ridden: false,
            alive: false,
        }
    }

    pub fn alive(&self) -> bool {
        self.alive
    }

    pub fn ridden(&self) -> bool {
        self.ridden
    }

    pub fn origin(&self) -> [f32; 3] {
        self.origin
    }

    /// Place on a lava surface. False when the cell is not lava.
    pub fn place_at(&mut self, at: [f32; 3], world: &Cluster) -> bool {
        let Some(y) = lava_surface_y(world, at[0], at[2]) else {
            return false;
        };
        self.origin = [at[0], y, at[2]];
        self.yaw = 0.0;
        self.ridden = false;
        self.alive = true;
        true
    }

    pub fn mount(&mut self) -> bool {
        if !self.alive || self.ridden {
            return false;
        }
        self.ridden = true;
        true
    }

    pub fn dismount(&mut self) {
        self.ridden = false;
    }

    pub fn near(&self, eye: [f32; 3]) -> bool {
        if !self.alive {
            return false;
        }
        let [x, y, z] = self.origin;
        let dx = eye[0] - x;
        let dy = eye[1] - (y + 0.55);
        let dz = eye[2] - z;
        dx * dx + dy * dy + dz * dz < BOARD * BOARD
    }

    pub fn aimed(&self, eye: [f32; 3], forward: [f32; 3]) -> bool {
        if !self.alive {
            return false;
        }
        let [x, y, z] = self.origin;
        let center = [x, y + 0.55, z];
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
        dx * dx + dy * dy + dz * dz < 0.9
    }

    pub fn seat(&self) -> Option<[f32; 3]> {
        if !self.ridden {
            return None;
        }
        let [x, y, z] = self.origin;
        Some([x, y + 0.7 + EYE_HEIGHT, z])
    }

    /// Slide across lava by `vx` / `vz`. Land without lava is refused.
    pub fn steer(&mut self, dt: f32, vx: f32, vz: f32, world: &Cluster) -> bool {
        if !self.ridden || !self.alive || dt <= 0.0 {
            return false;
        }
        let [x, y, z] = self.origin;
        let mut next = self.origin;
        let nx = x + vx * dt;
        if let Some(ny) = lava_surface_y(world, nx, z) {
            next[0] = nx;
            next[1] = ny;
        }
        let nz = z + vz * dt;
        if let Some(ny) = lava_surface_y(world, next[0], nz) {
            next[2] = nz;
            next[1] = ny;
        }
        let moved = next != self.origin;
        face_box(&mut self.yaw, next[0] - x, next[2] - z);
        // Keep y from an unused previous surface if neither axis moved.
        let _ = y;
        self.origin = next;
        moved
    }

    /// Idle sway on the lava pad when not ridden.
    pub fn tick_near(&mut self, count: u32, world: &Cluster) {
        if !self.alive || self.ridden || count == 0 {
            return;
        }
        if let Some(y) = lava_surface_y(world, self.origin[0], self.origin[2]) {
            self.origin[1] = y;
        }
        self.yaw += 0.04 * count as f32;
    }

    pub fn bounds(&self) -> ([f32; 3], [f32; 3]) {
        if !self.alive {
            return ([0.0, -64.0, 0.0], [0.0, -64.0, 0.0]);
        }
        let [x, y, z] = self.origin;
        ([x - 0.45, y, z - 0.35], [x + 0.45, y + 1.15, z + 0.35])
    }

    pub fn vertices(&self) -> Vec<Vertex> {
        if !self.alive {
            return Vec::new();
        }
        let [x, y, z] = self.origin;
        let mut out = Vec::new();
        // Long maroon-magenta body — reads against netherrack.
        push_box(
            &mut out,
            [x, y + 0.55, z],
            [0.52, 0.36, 0.32],
            [0.78, 0.28, 0.42],
        );
        // Hair tufts.
        push_box(
            &mut out,
            [x - 0.12, y + 0.98, z],
            [0.12, 0.22, 0.12],
            [0.92, 0.62, 0.72],
        );
        push_box(
            &mut out,
            [x + 0.18, y + 1.02, z + 0.05],
            [0.10, 0.20, 0.10],
            [0.88, 0.55, 0.65],
        );
        // Stilt legs into the lava.
        for &(ox, oz) in &[(-0.22f32, -0.16), (0.22, -0.16), (-0.22, 0.16), (0.22, 0.16)] {
            push_box(
                &mut out,
                [x + ox, y + 0.18, z + oz],
                [0.06, 0.22, 0.06],
                [0.45, 0.14, 0.18],
            );
        }
        // Soft saddle read when mounted.
        if self.ridden {
            push_box(
                &mut out,
                [x, y + 0.82, z],
                [0.26, 0.05, 0.18],
                [0.35, 0.55, 0.42],
            );
        }
        yaw_mesh(&mut out, self.origin, self.yaw);
        out
    }
}

/// Top of a lava cell under `(x, z)`, if any within a short vertical scan.
fn lava_surface_y(world: &Cluster, x: f32, z: f32) -> Option<f32> {
    let ix = x.floor() as i32;
    let iz = z.floor() as i32;
    for iy in (0..48).rev() {
        if world.block_at(ix, iy, iz).id == "lava" {
            return Some(iy as f32 + 1.0);
        }
    }
    None
}

/// Wheat tames it; a saddle then lets you mount and gallop. A lead hitches it to a fence.
pub struct Horse {
    steps: u32,
    origin: [f32; 3],
    yaw: f32,
    tamed: bool,
    saddled: bool,
    ridden: bool,
    /// Fence foot cell when hitched with a lead.
    hitch: Option<[i32; 3]>,
    loose: u32,
}

impl Horse {
    pub fn new() -> Self {
        Self {
            steps: 0,
            origin: horse_point(0),
            yaw: 0.0,
            tamed: false,
            saddled: false,
            ridden: false,
            hitch: None,
            loose: 0,
        }
    }

    pub fn origin(&self) -> [f32; 3] {
        self.origin
    }

    pub fn tamed(&self) -> bool {
        self.tamed
    }

    pub fn saddled(&self) -> bool {
        self.saddled
    }

    pub fn ridden(&self) -> bool {
        self.ridden
    }

    pub fn hitched(&self) -> bool {
        self.hitch.is_some()
    }

    pub fn hitch_at(&self) -> Option<[i32; 3]> {
        self.hitch
    }

    pub fn shove(&mut self, burst: [f32; 3]) -> bool {
        if self.hitch.is_some() {
            return false;
        }
        let Some(gust) = crate::body::wind_push(burst, self.origin) else {
            return false;
        };
        self.origin[0] += gust.dx;
        self.origin[2] += gust.dz;
        self.loose = 40;
        true
    }

    pub fn bump(&mut self, from: [f32; 3]) -> bool {
        if self.hitch.is_some() {
            return false;
        }
        bump_origin(&mut self.origin, &mut self.loose, from)
    }

    /// Feed wheat. True when this bite tames a wild horse.
    pub fn feed(&mut self) -> bool {
        if self.tamed {
            return false;
        }
        self.tamed = true;
        true
    }

    /// Put a saddle on a tamed horse. False when wild or already saddled.
    pub fn saddle(&mut self) -> bool {
        if !self.tamed || self.saddled {
            return false;
        }
        self.saddled = true;
        true
    }

    /// Tie a tamed horse to a fence foot cell. False when wild, ridden, or already hitched.
    pub fn hitch(&mut self, fence: [i32; 3]) -> bool {
        if !self.tamed || self.ridden || self.hitch.is_some() {
            return false;
        }
        self.hitch = Some(fence);
        self.origin = [
            fence[0] as f32 + 1.35,
            fence[1] as f32,
            fence[2] as f32 + 0.5,
        ];
        self.yaw = 0.0;
        true
    }

    /// Untie from the hitch. Returns the fence cell when one was set.
    pub fn untie(&mut self) -> Option<[i32; 3]> {
        self.hitch.take()
    }

    pub fn mount(&mut self) -> bool {
        if !self.saddled || self.hitch.is_some() {
            return false;
        }
        self.ridden = true;
        true
    }

    pub fn dismount(&mut self) {
        self.ridden = false;
    }

    pub fn near(&self, eye: [f32; 3]) -> bool {
        let [x, y, z] = self.origin;
        let dx = eye[0] - x;
        let dy = eye[1] - (y + 0.9);
        let dz = eye[2] - z;
        dx * dx + dy * dy + dz * dz < BOARD * BOARD
    }

    pub fn aimed(&self, eye: [f32; 3], forward: [f32; 3]) -> bool {
        let [x, y, z] = self.origin;
        let center = [x, y + 0.85, z];
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
        dx * dx + dy * dy + dz * dz < 1.0
    }

    pub fn seat(&self) -> Option<[f32; 3]> {
        if !self.ridden {
            return None;
        }
        let [x, y, z] = self.origin;
        Some([x, y + 1.15 + EYE_HEIGHT, z])
    }

    pub fn steer(&mut self, dt: f32, vx: f32, vz: f32, world: &Cluster) -> bool {
        if !self.ridden || dt <= 0.0 {
            return false;
        }
        let [x, y, z] = self.origin;
        let mut next = self.origin;
        let nx = x + vx * dt;
        if pig_ground(world, nx, z, y) {
            next[0] = nx;
        }
        let nz = z + vz * dt;
        if pig_ground(world, next[0], nz, y) {
            next[2] = nz;
        }
        let moved = next != self.origin;
        face_box(&mut self.yaw, next[0] - x, next[2] - z);
        self.origin = next;
        moved
    }

    /// Pace the meadow until tamed, saddled, or hitched.
    pub fn tick_near(&mut self, count: u32, player: [f32; 2], world: &Cluster) {
        if self.loose > 0 {
            self.loose = self.loose.saturating_sub(count);
            return;
        }
        if self.saddled || self.ridden || self.hitch.is_some() {
            return;
        }
        let from_x = self.origin[0];
        let from_z = self.origin[2];
        self.steps = self.steps.wrapping_add(count);
        let [x, y, z] = horse_point(self.steps);
        let dx = x - player[0];
        let dz = z - player[1];
        let dist = (dx * dx + dz * dz).sqrt();
        let goal = if dist < AVOID {
            let (ux, uz) = if dist > 1.0e-4 {
                (dx / dist, dz / dist)
            } else {
                (1.0, 0.0)
            };
            let push = AVOID - dist;
            [x + ux * push, y, z + uz * push]
        } else {
            [x, y, z]
        };
        slide_land(&mut self.origin, goal, world, 2);
        face_box(
            &mut self.yaw,
            self.origin[0] - from_x,
            self.origin[2] - from_z,
        );
    }

    pub fn bounds(&self) -> ([f32; 3], [f32; 3]) {
        let [x, y, z] = self.origin;
        ([x - 0.48, y, z - 0.28], [x + 0.48, y + 1.55, z + 0.28])
    }

    pub fn vertices(&self) -> Vec<Vertex> {
        let [x, y, z] = self.origin;
        let mut out = Vec::new();
        let coat = if self.tamed {
            [0.62, 0.42, 0.28]
        } else {
            [0.55, 0.38, 0.24]
        };
        push_box(&mut out, [x, y + 0.85, z], [0.48, 0.32, 0.26], coat);
        push_box(
            &mut out,
            [x + 0.42, y + 1.15, z],
            [0.18, 0.16, 0.14],
            coat,
        );
        push_box(
            &mut out,
            [x + 0.55, y + 1.05, z],
            [0.1, 0.06, 0.06],
            [0.35, 0.22, 0.16],
        );
        // Legs
        for &(dx, dz) in &[
            (0.28f32, 0.16f32),
            (0.28, -0.16),
            (-0.28, 0.16),
            (-0.28, -0.16),
        ] {
            push_box(
                &mut out,
                [x + dx, y + 0.32, z + dz],
                [0.08, 0.32, 0.08],
                [0.4, 0.28, 0.18],
            );
        }
        if self.saddled {
            push_box(
                &mut out,
                [x, y + 1.12, z],
                [0.3, 0.07, 0.2],
                [0.42, 0.26, 0.14],
            );
        }
        if let Some(fence) = self.hitch {
            let neck = [x - 0.35, y + 1.05, z];
            let post = [
                fence[0] as f32 + 0.5,
                fence[1] as f32 + 1.05,
                fence[2] as f32 + 0.5,
            ];
            let mid = [
                (neck[0] + post[0]) * 0.5,
                (neck[1] + post[1]) * 0.5 - 0.08,
                (neck[2] + post[2]) * 0.5,
            ];
            let dx = post[0] - neck[0];
            let dy = post[1] - neck[1];
            let dz = post[2] - neck[2];
            let len = (dx * dx + dy * dy + dz * dz).sqrt().max(0.2);
            let half = [len * 0.5, 0.03, 0.03];
            let rope = [0.72, 0.58, 0.28];
            push_box(&mut out, mid, half, rope);
            push_box(&mut out, neck, [0.05, 0.05, 0.05], rope);
            push_box(&mut out, post, [0.05, 0.05, 0.05], rope);
        }
        yaw_mesh(&mut out, self.origin, self.yaw);
        out
    }
}

/// True when `id` tames a wild horse.
pub fn horse_food(id: &str) -> bool {
    id == "wheat"
}

const ZOMBIE_HEARTS: i32 = 4;
const ZOMBIE_HOME: [f32; 3] = [11.0, 1.0, 13.0];
/// Seconds the mesh stays red after a mace hit.
const ZOMBIE_FLASH: f32 = 0.35;

pub struct Zombie {
    origin: [f32; 3],
    yaw: f32,
    swing: u32,
    hearts: i32,
    /// Seconds left on the ground after a mace drops it.
    down: f32,
    /// Entity ticks left before it walks again.
    loose: u32,
    /// Blockbench clip clock, in seconds.
    pose: f32,
    /// True while the last step walked toward the player.
    moving: bool,
    /// True while in bite range with a clear line.
    attacking: bool,
    /// Seconds of red hit flash left on the mesh.
    flash: f32,
    /// Night/cave spawn: knockdown ends in removal instead of home revive.
    wild: bool,
}

impl Zombie {
    pub fn new() -> Self {
        Self {
            origin: ZOMBIE_HOME,
            yaw: 0.0,
            swing: 0,
            hearts: ZOMBIE_HEARTS,
            down: 0.0,
            loose: 0,
            pose: 0.0,
            moving: false,
            attacking: false,
            flash: 0.0,
            wild: false,
        }
    }

    /// Feet of a living body.
    pub fn origin(&self) -> [f32; 3] {
        self.origin
    }

    pub fn living(&self) -> bool {
        self.hearts > 0
    }

    /// Wild body finished its knockdown and should leave the horde.
    pub fn spent(&self) -> bool {
        self.wild && self.hearts <= 0 && self.down <= 0.0
    }

    /// Shove the zombie away from a wind burst. True when it was inside.
    pub fn shove(&mut self, burst: [f32; 3]) -> bool {
        if self.hearts <= 0 {
            return false;
        }
        let Some(gust) = crate::body::wind_push(burst, self.origin) else {
            return false;
        };
        self.origin[0] += gust.dx;
        self.origin[2] += gust.dz;
        self.loose = 40;
        true
    }

    pub fn bump(&mut self, from: [f32; 3]) -> bool {
        if self.hearts <= 0 {
            return false;
        }
        bump_origin(&mut self.origin, &mut self.loose, from)
    }

    pub fn hearts(&self) -> i32 {
        self.hearts
    }

    /// Step back from `from` and freeze for a short beat.
    pub fn spook(&mut self, from: [f32; 2]) {
        if self.hearts <= 0 {
            return;
        }
        let dx = self.origin[0] - from[0];
        let dz = self.origin[2] - from[1];
        let dist = (dx * dx + dz * dz).sqrt();
        if dist > 0.05 {
            self.origin[0] += dx / dist * 1.2;
            self.origin[2] += dz / dist * 1.2;
        } else {
            self.origin[0] += 1.2;
        }
        self.loose = 40;
    }

    /// Feet on the ground, when the zombie is still up.
    pub fn feet(&self) -> Option<[f32; 2]> {
        if self.hearts <= 0 {
            None
        } else {
            Some([self.origin[0], self.origin[2]])
        }
    }

    /// True when the look ray passes close to the body.
    pub fn aimed(&self, eye: [f32; 3], forward: [f32; 3]) -> bool {
        if self.hearts <= 0 {
            return false;
        }
        let [x, y, z] = self.origin;
        let center = [x, y + 0.9, z];
        let to = [center[0] - eye[0], center[1] - eye[1], center[2] - eye[2]];
        let along = to[0] * forward[0] + to[1] * forward[1] + to[2] * forward[2];
        if !(0.0..=4.0).contains(&along) {
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

    /// Spend hearts. True when this hit drops the zombie.
    pub fn hurt(&mut self, amount: i32) -> bool {
        if self.hearts <= 0 || amount <= 0 {
            return false;
        }
        self.flash = ZOMBIE_FLASH;
        self.hearts -= amount;
        if self.hearts > 0 {
            return false;
        }
        self.hearts = 0;
        self.down = 8.0;
        true
    }

    /// Stand the zombie up beside a trial spawner, ready to fight.
    pub fn rouse(&mut self, at: [f32; 3]) {
        self.origin = at;
        self.hearts = ZOMBIE_HEARTS;
        self.down = 0.0;
        self.swing = 0;
        self.loose = 0;
        self.pose = 0.0;
        self.moving = false;
        self.attacking = false;
        self.flash = 0.0;
        self.wild = false;
    }

    /// Stand a night/cave spawn at `at`. Knockdown despawns instead of home revive.
    pub fn rouse_wild(&mut self, at: [f32; 3]) {
        self.rouse(at);
        self.wild = true;
    }

    /// Freeze in place (proof shots / shove recovery).
    pub fn hold(&mut self) {
        self.loose = 10_000;
        self.moving = false;
        self.attacking = false;
    }

    /// Point the Blockbench +Z face along yaw (radians).
    pub fn face(&mut self, yaw: f32) {
        self.yaw = yaw;
    }

    /// Decay the hit flash, or stand back up after a knockdown.
    ///
    /// True while the mesh should rebuild: flash frames, the rise frame, or a
    /// wild body finishing its knockdown (caller removes it).
    pub fn rest(&mut self, dt: f32) -> bool {
        if self.hearts > 0 {
            if self.flash <= 0.0 {
                return false;
            }
            self.flash = (self.flash - dt).max(0.0);
            return true;
        }
        self.down -= dt;
        if self.down > 0.0 {
            return false;
        }
        if self.wild {
            self.down = 0.0;
            return true;
        }
        self.hearts = ZOMBIE_HEARTS;
        self.origin = ZOMBIE_HOME;
        self.swing = 0;
        self.down = 0.0;
        self.pose = 0.0;
        self.moving = false;
        self.attacking = false;
        self.flash = 0.0;
        true
    }

    /// Step toward `player` (x, z). True when a bite is due and `clear` is open.
    ///
    /// A full block sends the step around it. The bite still waits for a clear line.
    pub fn tick_near(
        &mut self,
        count: u32,
        player: [f32; 2],
        clear: bool,
        world: &Cluster,
    ) -> bool {
        if self.loose > 0 {
            self.loose = self.loose.saturating_sub(count);
            self.moving = false;
            self.attacking = false;
            self.pose += 0.05 * count as f32;
            return false;
        }
        if self.hearts <= 0 {
            return false;
        }
        let mut bite = false;
        let mut stepped = false;
        let mut swinging = false;
        for _ in 0..count {
            self.pose += 0.05;
            let feet_y = self.origin[1].floor() as i32;
            let dx = player[0] - self.origin[0];
            let dz = player[1] - self.origin[2];
            let dist = (dx * dx + dz * dz).sqrt();
            if dist > REACH {
                let from = [self.origin[0], self.origin[2]];
                if let Some(next) =
                    land_step(world, from, player, STEP.min(dist - REACH), feet_y, 2)
                {
                    face_forward(&mut self.yaw, next[0] - from[0], next[1] - from[1]);
                    self.origin[0] = next[0];
                    self.origin[2] = next[1];
                    stepped = true;
                }
            } else if clear {
                face_forward(&mut self.yaw, dx, dz);
                swinging = true;
                self.swing = self.swing.wrapping_add(1);
                if self.swing % BITE_EVERY == 0 {
                    bite = true;
                }
            }
            settle_land(&mut self.origin, world);
        }
        self.moving = stepped;
        self.attacking = swinging;
        bite
    }

    pub fn bounds(&self) -> ([f32; 3], [f32; 3]) {
        if self.hearts <= 0 {
            return ([0.0, -8.0, 0.0], [0.0, -8.0, 0.0]);
        }
        let [x, y, z] = self.origin;
        ([x - 0.28, y, z - 0.28], [x + 0.28, y + 1.95, z + 0.28])
    }

    fn clip(&self) -> &'static str {
        if self.attacking {
            "attack"
        } else if self.moving {
            "walk"
        } else {
            "idle"
        }
    }

    pub fn vertices(&self) -> Vec<Vertex> {
        if self.hearts <= 0 {
            return Vec::new();
        }
        let mut out = crate::blockbench::mesh(
            zombie_figure(),
            self.clip(),
            self.pose,
            self.yaw,
            self.origin,
            &[],
        );
        if self.flash > 0.0 {
            let heat = (self.flash / ZOMBIE_FLASH).clamp(0.0, 1.0);
            for vertex in &mut out {
                vertex.color[0] = (vertex.color[0] * (1.0 - heat) + heat).min(1.0);
                vertex.color[1] *= 1.0 - 0.55 * heat;
                vertex.color[2] *= 1.0 - 0.55 * heat;
            }
        }
        out
    }
}

fn zombie_figure() -> &'static crate::blockbench::Figure {
    use std::sync::OnceLock;
    static FIGURE: OnceLock<crate::blockbench::Figure> = OnceLock::new();
    FIGURE.get_or_init(|| {
        let mut figure = crate::blockbench::parse(include_str!(
            "../../../resourcepacks/terralite/assets/terralite/models/entity/zombie.bbmodel"
        ))
        .expect("zombie.bbmodel");
        // Prefer an enabled pack sheet (Whimscape) over the embedded stub.
        if let Some(skin) = crate::blockbench::pack_entity_skin("zombie") {
            figure.set_skin(skin);
        }
        figure
    })
}

fn piglin_figure() -> &'static crate::blockbench::Figure {
    use std::sync::OnceLock;
    static FIGURE: OnceLock<crate::blockbench::Figure> = OnceLock::new();
    FIGURE.get_or_init(|| {
        let mut figure = crate::blockbench::parse(include_str!(
            "../../../resourcepacks/terralite/assets/terralite/models/entity/piglin.bbmodel"
        ))
        .expect("piglin.bbmodel");
        if let Some(skin) = crate::blockbench::pack_entity_skin("piglin") {
            figure.set_skin(skin);
        }
        figure
    })
}

fn villager_figure() -> &'static crate::blockbench::Figure {
    use std::sync::OnceLock;
    static FIGURE: OnceLock<crate::blockbench::Figure> = OnceLock::new();
    FIGURE.get_or_init(|| {
        let mut figure = crate::blockbench::parse(include_str!(
            "../../../resourcepacks/terralite/assets/terralite/models/entity/villager.bbmodel"
        ))
        .expect("villager.bbmodel");
        if let Some(skin) = crate::blockbench::pack_entity_skin("villager") {
            figure.set_skin(skin);
        }
        figure
    })
}

fn ghast_figure() -> &'static crate::blockbench::Figure {
    use std::sync::OnceLock;
    static FIGURE: OnceLock<crate::blockbench::Figure> = OnceLock::new();
    FIGURE.get_or_init(|| {
        let mut figure = crate::blockbench::parse(include_str!(
            "../../../resourcepacks/terralite/assets/terralite/models/entity/ghast.bbmodel"
        ))
        .expect("ghast.bbmodel");
        if let Some(skin) = crate::blockbench::pack_entity_skin("ghast") {
            figure.set_skin(skin);
        }
        figure
    })
}

fn warden_figure() -> &'static crate::blockbench::Figure {
    use std::sync::OnceLock;
    static FIGURE: OnceLock<crate::blockbench::Figure> = OnceLock::new();
    FIGURE.get_or_init(|| {
        let mut figure = crate::blockbench::parse(include_str!(
            "../../../resourcepacks/terralite/assets/terralite/models/entity/warden.bbmodel"
        ))
        .expect("warden.bbmodel");
        if let Some(skin) = crate::blockbench::pack_entity_skin("warden") {
            figure.set_skin(skin);
        }
        figure
    })
}

/// True when a Whimscape (or other pack) entity sheet is available for `id`.
fn pack_entity_painted(id: &str) -> bool {
    use std::sync::OnceLock;
    match id {
        "piglin" => {
            static READY: OnceLock<bool> = OnceLock::new();
            *READY.get_or_init(|| crate::blockbench::pack_entity_skin("piglin").is_some())
        }
        "villager" => {
            static READY: OnceLock<bool> = OnceLock::new();
            *READY.get_or_init(|| crate::blockbench::pack_entity_skin("villager").is_some())
        }
        "ghast" => {
            static READY: OnceLock<bool> = OnceLock::new();
            *READY.get_or_init(|| crate::blockbench::pack_entity_skin("ghast").is_some())
        }
        "warden" => {
            static READY: OnceLock<bool> = OnceLock::new();
            *READY.get_or_init(|| crate::blockbench::pack_entity_skin("warden").is_some())
        }
        other => crate::blockbench::pack_entity_skin(other).is_some(),
    }
}

/// Sample pack entity-sheet colors for procedural box mobs when present.
fn pack_mob_colors(id: &str, regions: &[(u32, u32, u32, u32)]) -> Option<Vec<[f32; 3]>> {
    let skin = crate::blockbench::pack_entity_skin(id)?;
    let mut out = Vec::with_capacity(regions.len());
    for &(x0, y0, x1, y1) in regions {
        out.push(crate::blockbench::skin_average(&skin, x0, y0, x1, y1)?);
    }
    Some(out)
}

/// Cap how many natural zombies stand near the player at once.
const NIGHT_CAP: usize = 6;
/// Seconds between spawn attempts while it is dark enough.
const NIGHT_COOLDOWN: f32 = 2.4;
/// Closest spawn ring, in blocks from the player.
const NIGHT_SPAWN_MIN: f32 = 8.0;
/// Hostile bodies this close block bed sleep (same radius as the night spawn min ring).
pub const SLEEP_MONSTER_RANGE: f32 = NIGHT_SPAWN_MIN;
/// Farthest spawn ring, in blocks from the player.
const NIGHT_SPAWN_MAX: f32 = 22.0;
/// Despawn when farther than this from the player.
const NIGHT_DESPAWN: f32 = 40.0;
/// Block-light peak at or below this can host a spawn (Minecraft-ish ≤7).
const NIGHT_DARK: u8 = 7;
/// Block-light peak at or above this burns a wild zombie off.
const NIGHT_BRIGHT: u8 = 12;

/// Natural Blockbench zombies that rise in the dark near the player.
///
/// Reuses [`Zombie`] chase/bite. The trial-line demo zombie stays separate.
pub struct NightHorde {
    pack: Vec<Zombie>,
    wait: f32,
    salt: u32,
}

impl NightHorde {
    pub fn new() -> Self {
        Self {
            pack: Vec::new(),
            wait: 0.8,
            salt: 1,
        }
    }

    pub fn len(&self) -> usize {
        self.pack.len()
    }

    pub fn iter(&self) -> impl Iterator<Item = &Zombie> {
        self.pack.iter()
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut Zombie> {
        self.pack.iter_mut()
    }

    pub fn get_mut(&mut self, index: usize) -> Option<&mut Zombie> {
        self.pack.get_mut(index)
    }

    /// Closest living wild body under the look ray, if any.
    pub fn aimed(&self, eye: [f32; 3], forward: [f32; 3]) -> Option<usize> {
        let mut best: Option<(usize, f32)> = None;
        for (index, zombie) in self.pack.iter().enumerate() {
            if !zombie.aimed(eye, forward) {
                continue;
            }
            let [x, y, z] = zombie.origin();
            let dx = x - eye[0];
            let dy = y + 0.9 - eye[1];
            let dz = z - eye[2];
            let dist = (dx * dx + dy * dy + dz * dz).sqrt();
            if best.is_none_or(|(_, so_far)| dist < so_far) {
                best = Some((index, dist));
            }
        }
        best.map(|(index, _)| index)
    }

    pub fn vertices(&self) -> Vec<Vertex> {
        let mut out = Vec::new();
        for zombie in &self.pack {
            out.extend(zombie.vertices());
        }
        out
    }

    /// Place one wild body at `at` for proof shots. Ignores light and the cap.
    pub fn force_at(&mut self, at: [f32; 3]) {
        let mut zombie = Zombie::new();
        zombie.rouse_wild(at);
        self.pack.push(zombie);
    }

    /// True when any living wild body sits within `range` blocks of `at`.
    pub fn any_living_within(&self, at: [f32; 3], range: f32) -> bool {
        let r2 = range * range;
        self.pack.iter().any(|zombie| {
            if !zombie.living() {
                return false;
            }
            let [x, y, z] = zombie.origin();
            let dx = x - at[0];
            let dy = y - at[1];
            let dz = z - at[2];
            dx * dx + dy * dy + dz * dz <= r2
        })
    }

    /// Drop every wild body. Used after sleeping through the night.
    pub fn clear_wild(&mut self) -> bool {
        let before = self.pack.len();
        self.pack.clear();
        before > 0
    }

    /// Decay knockdowns, cull bright/day/far bodies, try one spawn, run chase AI.
    ///
    /// `night` is true after dark. `held` is the player's torch (or similar) so
    /// lit air near the hand stays clear. Returns whether any bite landed and
    /// whether the mesh should rebuild.
    pub fn tick(
        &mut self,
        dt: f32,
        steps: u32,
        player: [f32; 3],
        night: bool,
        held: Option<rebuild_world::Emitter>,
        world: &Cluster,
        clear_of: &mut dyn FnMut([f32; 3]) -> bool,
    ) -> (bool, bool) {
        let mut dirty = false;
        let mut bite = false;
        for zombie in &mut self.pack {
            if zombie.rest(dt) {
                dirty = true;
            }
        }
        let before = self.pack.len();
        self.pack.retain(|zombie| !zombie.spent());
        if self.pack.len() != before {
            dirty = true;
        }

        let feet = [player[0], player[2]];
        self.wait = (self.wait - dt).max(0.0);
        if self.wait <= 0.0 {
            self.wait = NIGHT_COOLDOWN;
            // Light / day / range cull shares the spawn cadence so torch checks stay cheap.
            let mut kept = Vec::with_capacity(self.pack.len());
            for zombie in self.pack.drain(..) {
                if !zombie.living() {
                    kept.push(zombie);
                    continue;
                }
                let origin = zombie.origin();
                let dx = origin[0] - player[0];
                let dz = origin[2] - player[2];
                let dist = (dx * dx + dz * dz).sqrt();
                let cell = [
                    origin[0].floor() as i32,
                    origin[1].floor() as i32,
                    origin[2].floor() as i32,
                ];
                let light = cheap_block_light(world, cell[0], cell[1], cell[2], held);
                let open = sky_open(world, cell[0], cell[1], cell[2]);
                if dist > NIGHT_DESPAWN || light >= NIGHT_BRIGHT || (!night && open) {
                    dirty = true;
                    continue;
                }
                kept.push(zombie);
            }
            self.pack = kept;
            if self.try_spawn(player, night, held, world) {
                dirty = true;
            }
        }

        if steps > 0 {
            for zombie in &mut self.pack {
                if !zombie.living() {
                    continue;
                }
                let (min, max) = zombie.bounds();
                let from = [
                    (min[0] + max[0]) * 0.5,
                    min[1] + 1.4,
                    (min[2] + max[2]) * 0.5,
                ];
                let clear = clear_of(from);
                if zombie.tick_near(steps, feet, clear, world) {
                    bite = true;
                }
                dirty = true;
            }
        }
        (bite, dirty)
    }

    fn try_spawn(
        &mut self,
        player: [f32; 3],
        night: bool,
        held: Option<rebuild_world::Emitter>,
        world: &Cluster,
    ) -> bool {
        if self.pack.iter().filter(|z| z.living()).count() >= NIGHT_CAP {
            return false;
        }
        for _ in 0..8 {
            self.salt = self.salt.wrapping_add(1);
            let Some(at) = spawn_spot(world, player, self.salt) else {
                continue;
            };
            let cell = [at[0].floor() as i32, at[1].floor() as i32, at[2].floor() as i32];
            if !can_host_zombie(world, cell[0], cell[1], cell[2], night, held) {
                continue;
            }
            if self.pack.iter().any(|zombie| {
                let o = zombie.origin();
                (o[0] - at[0]).abs() < 1.1 && (o[2] - at[2]).abs() < 1.1
            }) {
                continue;
            }
            let mut zombie = Zombie::new();
            zombie.rouse_wild(at);
            self.pack.push(zombie);
            return true;
        }
        false
    }
}

fn spawn_spot(world: &Cluster, player: [f32; 3], salt: u32) -> Option<[f32; 3]> {
    let angle = salt as f32 * 2.399_963;
    let span = NIGHT_SPAWN_MAX - NIGHT_SPAWN_MIN;
    let dist = NIGHT_SPAWN_MIN + (salt % 11) as f32 / 10.0 * span;
    let x = (player[0] + angle.cos() * dist).floor() as i32;
    let z = (player[2] + angle.sin() * dist).floor() as i32;
    let py = player[1].floor() as i32;
    let top = (py + 6).min(250);
    let bottom = (py - 10).max(1);
    for y in (bottom..=top).rev() {
        if spawn_footing(world, x, y, z) {
            return Some([x as f32 + 0.5, y as f32, z as f32 + 0.5]);
        }
    }
    None
}

fn spawn_footing(world: &Cluster, x: i32, y: i32, z: i32) -> bool {
    if !(1..255).contains(&y) {
        return false;
    }
    let floor = world.block_at(x, y - 1, z);
    if floor.is_air() || is_fluid(floor.id) || !rebuild_world::shape_of(floor).is_full_cube() {
        return false;
    }
    !cell_blocks(world, x, y, z) && !cell_blocks(world, x, y + 1, z)
}

fn can_host_zombie(
    world: &Cluster,
    x: i32,
    y: i32,
    z: i32,
    night: bool,
    held: Option<rebuild_world::Emitter>,
) -> bool {
    if !spawn_footing(world, x, y, z) {
        return false;
    }
    let light = cheap_block_light(world, x, y, z, held);
    if light > NIGHT_DARK {
        return false;
    }
    let open = sky_open(world, x, y, z);
    // Open sky after dark, or a dark cave / roofed pocket any time.
    night || !open
}

fn sky_open(world: &Cluster, x: i32, y: i32, z: i32) -> bool {
    for yy in (y + 1)..256 {
        let block = world.block_at(x, yy, z);
        if block.is_air() {
            continue;
        }
        if rebuild_world::shape_of(block).is_full_cube() {
            return false;
        }
    }
    true
}

/// Peak block-light channel at a cell, ignoring walls (cheap torch / held check).
fn cheap_block_light(
    world: &Cluster,
    x: i32,
    y: i32,
    z: i32,
    held: Option<rebuild_world::Emitter>,
) -> u8 {
    let mut level = 0u8;
    let reach = rebuild_world::LIGHT_REACH;
    for dy in -reach..=reach {
        for dz in -reach..=reach {
            for dx in -reach..=reach {
                let man = dx.abs() + dy.abs() + dz.abs();
                if man > reach {
                    continue;
                }
                let ny = y + dy;
                if !(0..256).contains(&ny) {
                    continue;
                }
                let Some(light) = rebuild_world::light_of(world.block_at(x + dx, ny, z + dz)) else {
                    continue;
                };
                let peak = light
                    .channels()
                    .into_iter()
                    .max()
                    .unwrap_or(0);
                level = level.max(peak.saturating_sub(man as u8));
            }
        }
    }
    if let Some(emitter) = held {
        let man = (emitter.x - x).abs() + (emitter.y - y).abs() + (emitter.z - z).abs();
        if man <= reach {
            let peak = emitter
                .light
                .channels()
                .into_iter()
                .max()
                .unwrap_or(0);
            level = level.max(peak.saturating_sub(man as u8));
        }
    }
    level
}

const BREEZE_HEARTS: i32 = 4;
const BREEZE_HOME: [f32; 3] = [6.0, 3.2, 10.0];
/// Entity ticks between gusts.
const GUST_EVERY: u32 = 40;
/// The breeze holds about this far from the player, in blocks.
const HOVER: f32 = 4.0;

pub struct Breeze {
    origin: [f32; 3],
    swing: u32,
    hearts: i32,
    down: f32,
    loose: u32,
}

impl Breeze {
    pub fn new() -> Self {
        Self {
            origin: BREEZE_HOME,
            swing: 0,
            hearts: BREEZE_HEARTS,
            down: 0.0,
            loose: 0,
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn origin(&self) -> [f32; 3] {
        self.origin
    }

    /// Shove the breeze away from a wind burst. True when it was inside.
    pub fn shove(&mut self, burst: [f32; 3]) -> bool {
        if self.hearts <= 0 {
            return false;
        }
        let Some(gust) = crate::body::wind_push(burst, self.origin) else {
            return false;
        };
        self.origin[0] += gust.dx;
        self.origin[2] += gust.dz;
        self.loose = 40;
        true
    }

    pub fn bump(&mut self, from: [f32; 3]) -> bool {
        if self.hearts <= 0 {
            return false;
        }
        bump_origin(&mut self.origin, &mut self.loose, from)
    }

    /// True when the look ray passes close to the floating body.
    pub fn aimed(&self, eye: [f32; 3], forward: [f32; 3]) -> bool {
        if self.hearts <= 0 {
            return false;
        }
        let [x, y, z] = self.origin;
        let center = [x, y + 0.5, z];
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
        dx * dx + dy * dy + dz * dz < 0.64
    }

    /// Spend hearts. True when this hit drops the breeze.
    pub fn hurt(&mut self, amount: i32) -> bool {
        if self.hearts <= 0 || amount <= 0 {
            return false;
        }
        self.hearts -= amount;
        if self.hearts > 0 {
            return false;
        }
        self.hearts = 0;
        self.down = 8.0;
        true
    }

    /// Float the breeze up beside a trial spawner.
    pub fn rouse(&mut self, at: [f32; 3]) {
        self.origin = [at[0], at[1] + 1.2, at[2]];
        self.hearts = BREEZE_HEARTS;
        self.down = 0.0;
        self.swing = 0;
        self.loose = 0;
    }

    /// Return home after a knockdown. True on the frame it rises.
    pub fn rest(&mut self, dt: f32) -> bool {
        if self.hearts > 0 {
            return false;
        }
        self.down -= dt;
        if self.down > 0.0 {
            return false;
        }
        self.hearts = BREEZE_HEARTS;
        self.origin = BREEZE_HOME;
        self.swing = 0;
        self.down = 0.0;
        true
    }

    /// Drift toward a hover range. True when a gust is due.
    pub fn tick_near(&mut self, count: u32, player: [f32; 2]) -> bool {
        if self.loose > 0 {
            self.loose = self.loose.saturating_sub(count);
            return false;
        }
        if self.hearts <= 0 {
            return false;
        }
        let mut shot = false;
        for _ in 0..count {
            let dx = player[0] - self.origin[0];
            let dz = player[1] - self.origin[2];
            let dist = (dx * dx + dz * dz).sqrt();
            if dist > HOVER + 1.0 && dist > 0.05 {
                let step = STEP.min(dist - HOVER);
                self.origin[0] += dx / dist * step;
                self.origin[2] += dz / dist * step;
            } else if dist < HOVER - 1.0 && dist > 0.05 {
                self.origin[0] -= dx / dist * STEP;
                self.origin[2] -= dz / dist * STEP;
            }
            self.swing = self.swing.wrapping_add(1);
            if (1.0..14.0).contains(&dist) && self.swing % GUST_EVERY == 0 {
                shot = true;
            }
        }
        shot
    }

    pub fn bounds(&self) -> ([f32; 3], [f32; 3]) {
        if self.hearts <= 0 {
            return ([0.0, -8.0, 0.0], [0.0, -8.0, 0.0]);
        }
        let [x, y, z] = self.origin;
        ([x - 0.3, y, z - 0.3], [x + 0.3, y + 1.1, z + 0.3])
    }

    pub fn vertices(&self) -> Vec<Vertex> {
        if self.hearts <= 0 {
            return Vec::new();
        }
        let [x, y, z] = self.origin;
        let (body, base) = match pack_mob_colors("breeze", &[(8, 8, 24, 24), (4, 24, 12, 32)]) {
            Some(c) => (c[0], c[1]),
            None => ([0.72, 0.90, 0.98], [0.55, 0.78, 0.92]),
        };
        let mut out = Vec::new();
        push_box(
            &mut out,
            [x, y + 0.45, z],
            [0.28, 0.32, 0.22],
            body,
        );
        push_box(
            &mut out,
            [x, y + 0.15, z],
            [0.16, 0.1, 0.16],
            base,
        );
        out
    }
}

/// A block a copper bulb can stand on to wake the golem.
pub fn copper_base(id: &str) -> bool {
    matches!(
        id,
        "copper_grate"
            | "copper_chest"
            | "copper_bulb"
            | "copper_door"
            | "copper_trapdoor"
            | "copper_block"
            | "exposed_copper"
            | "weathered_copper"
            | "oxidized_copper"
    )
}

/// Feet position beside the bulb, standing on the copper block's layer.
pub fn summon_feet(bulb: [i32; 3]) -> [f32; 3] {
    [bulb[0] as f32 + 1.5, bulb[1] as f32, bulb[2] as f32 + 0.5]
}

const GOLEM_LEG: f32 = 1.5;
const GOLEM_STEPS: u32 = 6;
/// Entity ticks between chores.
const GOLEM_WAIT: u32 = 80;

/// Something the golem found next to itself.
pub enum Chore {
    Press([i32; 3]),
    Open([i32; 3]),
}

pub struct Golem {
    origin: [f32; 3],
    home: [f32; 3],
    steps: u32,
    awake: bool,
    wait: u32,
}

impl Golem {
    pub fn new() -> Self {
        Self {
            origin: [0.0, -8.0, 0.0],
            home: [0.0, -8.0, 0.0],
            steps: 0,
            awake: false,
            wait: 0,
        }
    }

    pub fn shove(&mut self, burst: [f32; 3]) -> bool {
        if !self.awake {
            return false;
        }
        let Some(gust) = crate::body::wind_push(burst, self.origin) else {
            return false;
        };
        self.origin[0] += gust.dx;
        self.origin[2] += gust.dz;
        self.home[0] += gust.dx;
        self.home[2] += gust.dz;
        true
    }

    pub fn bump(&mut self, from: [f32; 3]) -> bool {
        if !self.awake {
            return false;
        }
        let Some(butt) = goat_shove(from, self.origin) else {
            return false;
        };
        self.origin[0] += butt.dx;
        self.origin[2] += butt.dz;
        self.home[0] += butt.dx;
        self.home[2] += butt.dz;
        true
    }

    /// Wake the golem at `at`. Calling it again moves the same body.
    pub fn summon(&mut self, at: [f32; 3]) {
        self.origin = at;
        self.home = at;
        self.steps = 0;
        self.awake = true;
        self.wait = 20;
    }

    /// Wander the square, and now and then notice a button or copper chest.
    pub fn tick(&mut self, count: u32, world: &Cluster) -> Option<Chore> {
        if !self.awake {
            return None;
        }
        let mut chore = None;
        for _ in 0..count {
            self.steps = self.steps.wrapping_add(1);
            let next = golem_point(self.home, self.steps);
            slide_land(&mut self.origin, next, world, 1);
            if self.wait > 0 {
                self.wait -= 1;
                continue;
            }
            if chore.is_none() {
                if let Some(found) = chore_near(world, self.origin) {
                    self.wait = GOLEM_WAIT;
                    chore = Some(found);
                }
            }
        }
        chore
    }

    pub fn bounds(&self) -> ([f32; 3], [f32; 3]) {
        if !self.awake {
            return ([0.0, -8.0, 0.0], [0.0, -8.0, 0.0]);
        }
        let [x, y, z] = self.origin;
        ([x - 0.22, y, z - 0.22], [x + 0.22, y + 0.85, z + 0.22])
    }

    pub fn vertices(&self) -> Vec<Vertex> {
        if !self.awake {
            return Vec::new();
        }
        let [x, y, z] = self.origin;
        let (body, head, accent) = match pack_mob_colors(
            "copper_golem",
            &[(8, 8, 16, 16), (8, 0, 16, 8), (20, 20, 28, 28)],
        ) {
            Some(c) => (c[0], c[1], c[2]),
            None => (
                [0.78, 0.46, 0.26],
                [0.86, 0.62, 0.34],
                [0.42, 0.64, 0.44],
            ),
        };
        let mut out = Vec::new();
        push_box(
            &mut out,
            [x, y + 0.32, z],
            [0.2, 0.22, 0.14],
            body,
        );
        push_box(
            &mut out,
            [x, y + 0.66, z],
            [0.14, 0.12, 0.12],
            head,
        );
        push_box(
            &mut out,
            [x, y + 0.34, z + 0.12],
            [0.06, 0.08, 0.04],
            accent,
        );
        out
    }
}

fn golem_point(home: [f32; 3], steps: u32) -> [f32; 3] {
    let leg = (steps / GOLEM_STEPS) % 4;
    let along = (steps % GOLEM_STEPS) as f32 / GOLEM_STEPS as f32 * GOLEM_LEG;
    let (x, z) = match leg {
        0 => (home[0] + along, home[2]),
        1 => (home[0] + GOLEM_LEG, home[2] + along),
        2 => (home[0] + GOLEM_LEG - along, home[2] + GOLEM_LEG),
        _ => (home[0], home[2] + GOLEM_LEG - along),
    };
    [x, home[1], z]
}

fn chore_near(world: &Cluster, origin: [f32; 3]) -> Option<Chore> {
    let ix = origin[0].floor() as i32;
    let iy = origin[1].floor() as i32;
    let iz = origin[2].floor() as i32;
    for y in (iy - 1)..=(iy + 2) {
        for z in (iz - 2)..=(iz + 2) {
            for x in (ix - 2)..=(ix + 2) {
                let id = world.block_at(x, y, z).id;
                if id == "button" {
                    return Some(Chore::Press([x, y, z]));
                }
                if id == "copper_chest" {
                    return Some(Chore::Open([x, y, z]));
                }
            }
        }
    }
    None
}

/// Bone meal, raw meat, or cooked meat. Any of these tames the wolf.
pub fn wolf_food(id: &str) -> bool {
    matches!(id, "bone_meal" | "raw" | "cooked")
}

const WOLF_HOME: [f32; 3] = [8.0, 1.0, 8.0];
/// The tamed wolf starts a bite when the zombie is this close, in blocks.
const HUNT: f32 = 8.0;
/// How far behind the player a tamed wolf is content to walk.
const HEEL: f32 = 2.0;
/// Pad home for tips and shots.
pub const WOLF_PAD: (i32, i32) = (8, 8);

pub struct Wolf {
    origin: [f32; 3],
    yaw: f32,
    swing: u32,
    tamed: bool,
    sitting: bool,
    loose: u32,
    roam: Roam,
}

impl Wolf {
    pub fn new() -> Self {
        Self {
            origin: WOLF_HOME,
            yaw: 0.0,
            swing: 0,
            tamed: false,
            sitting: false,
            loose: 0,
            roam: Roam::around(WOLF_HOME),
        }
    }

    pub fn origin(&self) -> [f32; 3] {
        self.origin
    }

    /// Put the body somewhere for shots and tests.
    pub fn place_at(&mut self, at: [f32; 3]) {
        self.origin = at;
        self.loose = 0;
    }

    pub fn tamed(&self) -> bool {
        self.tamed
    }

    /// Tame a wild wolf. False when it is already a friend.
    pub fn feed(&mut self) -> bool {
        if self.tamed {
            return false;
        }
        self.tamed = true;
        self.sitting = false;
        self.swing = 0;
        true
    }

    pub fn sitting(&self) -> bool {
        self.sitting
    }

    /// Empty-hand companion toggle: sit or stand-and-follow.
    /// False when the wolf is still wild. True when it sits.
    pub fn sit(&mut self) -> bool {
        if !self.tamed {
            return false;
        }
        self.sitting = !self.sitting;
        self.sitting
    }

    pub fn shove(&mut self, burst: [f32; 3]) -> bool {
        let Some(gust) = crate::body::wind_push(burst, self.origin) else {
            return false;
        };
        self.origin[0] += gust.dx;
        self.origin[2] += gust.dz;
        self.loose = 40;
        true
    }

    pub fn bump(&mut self, from: [f32; 3]) -> bool {
        bump_origin(&mut self.origin, &mut self.loose, from)
    }

    pub fn aimed(&self, eye: [f32; 3], forward: [f32; 3]) -> bool {
        let [x, y, z] = self.origin;
        let center = [x, y + 0.4, z];
        let to = [center[0] - eye[0], center[1] - eye[1], center[2] - eye[2]];
        let along = to[0] * forward[0] + to[1] * forward[1] + to[2] * forward[2];
        if !(0.0..=5.0).contains(&along) {
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
        dx * dx + dy * dy + dz * dz < 0.64
    }

    /// Wander, sit, follow, or bite. A full block sends the step around it.
    /// Past twelve blocks a standing wolf appears beside you.
    /// True when a bite lands on `foe`.
    pub fn tick(
        &mut self,
        count: u32,
        player: [f32; 2],
        foe: Option<[f32; 2]>,
        world: &Cluster,
    ) -> bool {
        let mut bite = false;
        for _ in 0..count {
            if self.loose > 0 {
                self.loose -= 1;
                continue;
            }
            if !self.tamed {
                roam_step(
                    &mut self.origin,
                    &mut self.yaw,
                    &mut self.roam,
                    WOLF_HOME,
                    world,
                );
                continue;
            }
            if self.sitting {
                face_box(
                    &mut self.yaw,
                    player[0] - self.origin[0],
                    player[1] - self.origin[2],
                );
                continue;
            }
            if pet_far(self.origin, player) {
                appear_beside(&mut self.origin, &mut self.yaw, player, world);
                continue;
            }
            let hunt = foe.filter(|at| {
                let dx = at[0] - self.origin[0];
                let dz = at[1] - self.origin[2];
                let foe_d = dx * dx + dz * dz;
                if foe_d >= HUNT * HUNT {
                    return false;
                }
                let px = player[0] - self.origin[0];
                let pz = player[1] - self.origin[2];
                foe_d + 1.0 < px * px + pz * pz
            });
            let (goal, stop) = if let Some(at) = hunt {
                (at, REACH)
            } else {
                (player, HEEL)
            };
            if heel_step(&mut self.origin, &mut self.yaw, goal, stop, world) && hunt.is_some() {
                self.swing = self.swing.wrapping_add(1);
                if self.swing % BITE_EVERY == 0 {
                    bite = true;
                }
            }
        }
        bite
    }

    pub fn bounds(&self) -> ([f32; 3], [f32; 3]) {
        let [x, y, z] = self.origin;
        ([x - 0.3, y, z - 0.3], [x + 0.3, y + 0.7, z + 0.3])
    }

    pub fn vertices(&self) -> Vec<Vertex> {
        let [x, y, z] = self.origin;
        let drop = if self.sitting { 0.18 } else { 0.0 };
        let mut out = Vec::new();
        push_box(
            &mut out,
            [x, y + 0.28 - drop, z],
            [0.28, 0.16, 0.14],
            [0.46, 0.46, 0.50],
        );
        push_box(
            &mut out,
            [x + 0.22, y + 0.42 - drop, z],
            [0.12, 0.12, 0.1],
            [0.58, 0.56, 0.54],
        );
        // Ears perk when following; flop a little when sitting.
        let ear_y = if self.sitting { 0.06 } else { 0.1 };
        push_box(
            &mut out,
            [x + 0.18, y + 0.48 - drop, z - 0.06],
            [0.04, ear_y, 0.03],
            [0.4, 0.4, 0.44],
        );
        push_box(
            &mut out,
            [x + 0.18, y + 0.48 - drop, z + 0.06],
            [0.04, ear_y, 0.03],
            [0.4, 0.4, 0.44],
        );
        push_box(
            &mut out,
            [x - 0.24, y + 0.32 - drop, z],
            [0.08, 0.06, 0.05],
            [0.28, 0.28, 0.32],
        );
        if self.sitting {
            // Front paws tucked under the chest while sitting.
            push_box(
                &mut out,
                [x + 0.14, y + 0.08, z - 0.09],
                [0.07, 0.06, 0.05],
                [0.4, 0.4, 0.44],
            );
            push_box(
                &mut out,
                [x + 0.14, y + 0.08, z + 0.09],
                [0.07, 0.06, 0.05],
                [0.4, 0.4, 0.44],
            );
            // Haunches drop onto the grass.
            push_box(
                &mut out,
                [x - 0.12, y + 0.1, z],
                [0.12, 0.08, 0.12],
                [0.42, 0.42, 0.46],
            );
        } else {
            for &(dx, dz) in &[
                (0.16f32, 0.1f32),
                (0.16, -0.1),
                (-0.16, 0.1),
                (-0.16, -0.1),
            ] {
                push_box(
                    &mut out,
                    [x + dx, y + 0.1, z + dz],
                    [0.05, 0.1, 0.05],
                    [0.4, 0.4, 0.44],
                );
            }
        }
        if self.tamed {
            push_box(
                &mut out,
                [x + 0.1, y + 0.36 - drop, z],
                [0.06, 0.05, 0.08],
                [0.75, 0.18, 0.16],
            );
        }
        yaw_mesh(&mut out, self.origin, self.yaw);
        out
    }
}

/// Raw meat tames the cat.
pub fn cat_food(id: &str) -> bool {
    id == "raw"
}

const CAT_HOME: [f32; 3] = [12.0, 1.0, 10.0];
/// Pad home for tips and shots.
pub const CAT_PAD: (i32, i32) = (12, 10);
/// The tamed cat hisses when the zombie is this close, in blocks.
const SCARE: f32 = 4.0;
/// Entity ticks between hisses.
const SCARE_WAIT: u32 = 60;

pub struct Cat {
    origin: [f32; 3],
    yaw: f32,
    tamed: bool,
    sitting: bool,
    wait: u32,
    loose: u32,
    roam: Roam,
}

impl Cat {
    pub fn new() -> Self {
        Self {
            origin: CAT_HOME,
            yaw: 0.0,
            tamed: false,
            sitting: false,
            wait: 0,
            loose: 0,
            roam: Roam::around(CAT_HOME),
        }
    }

    pub fn origin(&self) -> [f32; 3] {
        self.origin
    }

    /// Put the body somewhere for shots and tests.
    pub fn place_at(&mut self, at: [f32; 3]) {
        self.origin = at;
        self.loose = 0;
    }

    pub fn tamed(&self) -> bool {
        self.tamed
    }

    pub fn sitting(&self) -> bool {
        self.sitting
    }

    /// Tame a wild cat. False when it is already a friend.
    pub fn feed(&mut self) -> bool {
        if self.tamed {
            return false;
        }
        self.tamed = true;
        self.sitting = false;
        true
    }

    /// Empty-hand companion toggle: sit or stand-and-follow.
    /// False when the cat is still wild. True when it sits.
    pub fn sit(&mut self) -> bool {
        if !self.tamed {
            return false;
        }
        self.sitting = !self.sitting;
        self.sitting
    }

    pub fn shove(&mut self, burst: [f32; 3]) -> bool {
        let Some(gust) = crate::body::wind_push(burst, self.origin) else {
            return false;
        };
        self.origin[0] += gust.dx;
        self.origin[2] += gust.dz;
        self.loose = 40;
        true
    }

    pub fn bump(&mut self, from: [f32; 3]) -> bool {
        bump_origin(&mut self.origin, &mut self.loose, from)
    }

    pub fn aimed(&self, eye: [f32; 3], forward: [f32; 3]) -> bool {
        let [x, y, z] = self.origin;
        let center = [x, y + 0.3, z];
        let to = [center[0] - eye[0], center[1] - eye[1], center[2] - eye[2]];
        let along = to[0] * forward[0] + to[1] * forward[1] + to[2] * forward[2];
        if !(0.0..=5.0).contains(&along) {
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
        dx * dx + dy * dy + dz * dz < 0.49
    }

    /// Wander, sit, follow, or hiss. A full block sends the step around it.
    /// Past twelve blocks a standing cat appears beside you.
    /// True when the zombie should step back.
    pub fn tick(
        &mut self,
        count: u32,
        player: [f32; 2],
        foe: Option<[f32; 2]>,
        world: &Cluster,
    ) -> bool {
        let mut scare = false;
        for _ in 0..count {
            if self.loose > 0 {
                self.loose -= 1;
                continue;
            }
            if self.wait > 0 {
                self.wait -= 1;
            }
            if !self.tamed {
                roam_step(
                    &mut self.origin,
                    &mut self.yaw,
                    &mut self.roam,
                    CAT_HOME,
                    world,
                );
                continue;
            }
            if self.sitting {
                face_box(
                    &mut self.yaw,
                    player[0] - self.origin[0],
                    player[1] - self.origin[2],
                );
                continue;
            }
            let hiss = foe.filter(|at| {
                let dx = at[0] - self.origin[0];
                let dz = at[1] - self.origin[2];
                let foe_d = dx * dx + dz * dz;
                if foe_d >= SCARE * SCARE {
                    return false;
                }
                let px = player[0] - self.origin[0];
                let pz = player[1] - self.origin[2];
                foe_d + 1.0 < px * px + pz * pz
            });
            if let Some(at) = hiss {
                face_box(
                    &mut self.yaw,
                    at[0] - self.origin[0],
                    at[1] - self.origin[2],
                );
                if self.wait == 0 {
                    self.wait = SCARE_WAIT;
                    scare = true;
                }
                continue;
            }
            if pet_far(self.origin, player) {
                appear_beside(&mut self.origin, &mut self.yaw, player, world);
                continue;
            }
            let _ = heel_step(&mut self.origin, &mut self.yaw, player, HEEL, world);
        }
        scare
    }

    pub fn bounds(&self) -> ([f32; 3], [f32; 3]) {
        let [x, y, z] = self.origin;
        let top = if self.sitting { 0.4 } else { 0.55 };
        ([x - 0.22, y, z - 0.22], [x + 0.22, y + top, z + 0.22])
    }

    pub fn vertices(&self) -> Vec<Vertex> {
        let [x, y, z] = self.origin;
        let body_y = if self.sitting { 0.14 } else { 0.22 };
        let mut out = Vec::new();
        push_box(
            &mut out,
            [x, y + body_y, z],
            [0.2, 0.12, 0.12],
            [0.86, 0.48, 0.16],
        );
        push_box(
            &mut out,
            [x + 0.16, y + body_y + 0.1, z],
            [0.08, 0.08, 0.08],
            [0.94, 0.82, 0.62],
        );
        // Pointed ears; flop a little when sitting.
        let ear_y = if self.sitting { 0.05 } else { 0.08 };
        push_box(
            &mut out,
            [x + 0.14, y + body_y + 0.16, z - 0.05],
            [0.03, ear_y, 0.025],
            [0.9, 0.55, 0.2],
        );
        push_box(
            &mut out,
            [x + 0.14, y + body_y + 0.16, z + 0.05],
            [0.03, ear_y, 0.025],
            [0.9, 0.55, 0.2],
        );
        push_box(
            &mut out,
            [x - 0.18, y + body_y + 0.08, z],
            [0.06, 0.04, 0.04],
            [0.42, 0.22, 0.1],
        );
        if self.sitting {
            push_box(
                &mut out,
                [x + 0.1, y + 0.06, z - 0.07],
                [0.05, 0.05, 0.04],
                [0.8, 0.42, 0.14],
            );
            push_box(
                &mut out,
                [x + 0.1, y + 0.06, z + 0.07],
                [0.05, 0.05, 0.04],
                [0.8, 0.42, 0.14],
            );
            push_box(
                &mut out,
                [x - 0.08, y + 0.08, z],
                [0.1, 0.06, 0.1],
                [0.78, 0.4, 0.12],
            );
        } else {
            for &(dx, dz) in &[
                (0.12f32, 0.08f32),
                (0.12, -0.08),
                (-0.12, 0.08),
                (-0.12, -0.08),
            ] {
                push_box(
                    &mut out,
                    [x + dx, y + 0.08, z + dz],
                    [0.04, 0.08, 0.04],
                    [0.8, 0.42, 0.14],
                );
            }
        }
        yaw_mesh(&mut out, self.origin, self.yaw);
        out
    }
}

/// A hay bale stands in for seeds. It tames the parrot.
pub fn parrot_food(id: &str) -> bool {
    id == "hay_bale"
}

/// What the parrot repeats a moment after it hears it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mimic {
    Note(u8),
    Horn,
}

const PARROT_HOME: [f32; 3] = [4.0, 1.0, 11.0];
/// Pad home for tips and shots.
pub const PARROT_PAD: (i32, i32) = (4, 11);
/// Entity ticks after a note or horn before the parrot repeats it.
pub const ECHO_AFTER: u32 = 15;

pub struct Parrot {
    origin: [f32; 3],
    yaw: f32,
    tamed: bool,
    shoulder: bool,
    loose: u32,
    echo: Option<(u32, Mimic)>,
    roam: Roam,
}

impl Parrot {
    pub fn new() -> Self {
        Self {
            origin: PARROT_HOME,
            yaw: 0.0,
            tamed: false,
            shoulder: false,
            loose: 0,
            echo: None,
            roam: Roam::around(PARROT_HOME),
        }
    }

    pub fn origin(&self) -> [f32; 3] {
        self.origin
    }

    /// Put the body somewhere for shots and tests.
    pub fn place_at(&mut self, at: [f32; 3]) {
        self.origin = at;
        self.loose = 0;
    }

    pub fn tamed(&self) -> bool {
        self.tamed
    }

    pub fn shoulder(&self) -> bool {
        self.shoulder
    }

    /// Tame a wild parrot. False when it is already a friend.
    pub fn feed(&mut self) -> bool {
        if self.tamed {
            return false;
        }
        self.tamed = true;
        self.shoulder = false;
        true
    }

    /// Empty-hand companion toggle: perch on the shoulder, or hop off.
    /// False when the parrot is still wild. True when it is perched.
    pub fn perch(&mut self) -> bool {
        if !self.tamed {
            return false;
        }
        self.shoulder = !self.shoulder;
        self.shoulder
    }

    /// Stick to the player's right shoulder, facing their look.
    pub fn ride_shoulder(&mut self, feet: [f32; 3], yaw: f32) {
        let (sy, cy) = yaw.sin_cos();
        // Same axes as `Camera::walk_axes`: forward (cy, sy), right (-sy, cy).
        let right_x = -sy;
        let right_z = cy;
        let forward_x = cy;
        let forward_z = sy;
        self.origin = [
            feet[0] + right_x * 0.38 + forward_x * 0.12,
            feet[1] + 1.38,
            feet[2] + right_z * 0.38 + forward_z * 0.12,
        ];
        self.yaw = yaw;
    }

    /// Remember a sound to repeat. A wild bird, or one already echoing, ignores it.
    pub fn hear(&mut self, sound: Mimic) {
        if !self.tamed || self.echo.is_some() {
            return;
        }
        self.echo = Some((ECHO_AFTER, sound));
    }

    pub fn shove(&mut self, burst: [f32; 3]) -> bool {
        if self.shoulder {
            return false;
        }
        let Some(gust) = crate::body::wind_push(burst, self.origin) else {
            return false;
        };
        self.origin[0] += gust.dx;
        self.origin[2] += gust.dz;
        self.loose = 40;
        true
    }

    pub fn bump(&mut self, from: [f32; 3]) -> bool {
        if self.shoulder {
            return false;
        }
        bump_origin(&mut self.origin, &mut self.loose, from)
    }

    pub fn aimed(&self, eye: [f32; 3], forward: [f32; 3]) -> bool {
        let [x, y, z] = self.origin;
        let center = [x, y + 0.35, z];
        let to = [center[0] - eye[0], center[1] - eye[1], center[2] - eye[2]];
        let along = to[0] * forward[0] + to[1] * forward[1] + to[2] * forward[2];
        if !(0.0..=5.0).contains(&along) {
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
        dx * dx + dy * dy + dz * dz < 0.36
    }

    /// Wander, follow, or ride a shoulder. A full block sends the step around it.
    /// Past twelve blocks a free parrot appears beside you.
    /// `player` is feet; `yaw` turns the perched bird with the look.
    /// `Some` when an echo is due.
    pub fn tick(
        &mut self,
        count: u32,
        player: [f32; 3],
        yaw: f32,
        world: &Cluster,
    ) -> Option<Mimic> {
        let mut heard = None;
        let feet = [player[0], player[2]];
        for _ in 0..count {
            if let Some((left, sound)) = self.echo.take() {
                if left <= 1 {
                    heard = Some(sound);
                } else {
                    self.echo = Some((left - 1, sound));
                }
            }
            if self.loose > 0 {
                self.loose -= 1;
                continue;
            }
            if self.shoulder {
                self.ride_shoulder(player, yaw);
                continue;
            }
            if !self.tamed {
                roam_step(
                    &mut self.origin,
                    &mut self.yaw,
                    &mut self.roam,
                    PARROT_HOME,
                    world,
                );
                continue;
            }
            if pet_far(self.origin, feet) {
                let y = self.origin[1];
                appear_beside(&mut self.origin, &mut self.yaw, feet, world);
                self.origin[1] = y;
                continue;
            }
            let _ = heel_step(&mut self.origin, &mut self.yaw, feet, HEEL, world);
        }
        heard
    }

    pub fn bounds(&self) -> ([f32; 3], [f32; 3]) {
        let [x, y, z] = self.origin;
        ([x - 0.18, y, z - 0.18], [x + 0.18, y + 0.5, z + 0.18])
    }

    pub fn vertices(&self) -> Vec<Vertex> {
        let [x, y, z] = self.origin;
        let mut out = Vec::new();
        // Slightly smaller when perched so the shoulder read stays tidy.
        let scale = if self.shoulder { 0.85 } else { 1.0 };
        push_box(
            &mut out,
            [x, y + 0.22 * scale, z],
            [0.12 * scale, 0.1 * scale, 0.1 * scale],
            [0.16, 0.72, 0.28],
        );
        push_box(
            &mut out,
            [x + 0.1 * scale, y + 0.34 * scale, z],
            [0.07 * scale, 0.07 * scale, 0.07 * scale],
            [0.86, 0.2, 0.16],
        );
        push_box(
            &mut out,
            [x + 0.16 * scale, y + 0.32 * scale, z],
            [0.04 * scale, 0.03 * scale, 0.03 * scale],
            [0.95, 0.82, 0.2],
        );
        // Wing flashes when perched so the shoulder bird reads against the body.
        if self.shoulder {
            push_box(
                &mut out,
                [x, y + 0.26 * scale, z + 0.12 * scale],
                [0.03, 0.06 * scale, 0.08 * scale],
                [0.12, 0.55, 0.22],
            );
            push_box(
                &mut out,
                [x, y + 0.26 * scale, z - 0.12 * scale],
                [0.03, 0.06 * scale, 0.08 * scale],
                [0.12, 0.55, 0.22],
            );
        }
        yaw_mesh(&mut out, self.origin, self.yaw);
        out
    }
}

/// Honey is the treat. Feeding the frog asks for a slimeball.
pub fn frog_food(id: &str) -> bool {
    id == "honey"
}

/// Thin lily top above marsh water at y = 3. Frog feet rest here.
const FROG_PAD_Y: f32 = 4.0 + 2.0 / 16.0;
const FROG_HOME: [f32; 3] = [8.0, FROG_PAD_Y, 33.0];
const FROG_LEG: f32 = 1.2;
const FROG_REST: u32 = 8;
const FROG_STEPS: u32 = 6;
const BUG_BACK: u32 = 80;
/// World cells that hold lily pads under the frog hop square.
pub const FROG_PAD: (i32, i32) = (8, 33);
pub const FROG_PADS: [(i32, i32); 4] = [(8, 33), (9, 33), (9, 34), (8, 34)];

pub struct Frog {
    origin: [f32; 3],
    yaw: f32,
    steps: u32,
    bug: bool,
    bug_back: u32,
    loose: u32,
    /// Set when a hop lands back onto a lily; cleared by [`Frog::take_landed`].
    landed: bool,
}

impl Frog {
    pub fn new() -> Self {
        Self {
            origin: FROG_HOME,
            yaw: 0.0,
            steps: 0,
            bug: true,
            bug_back: 0,
            loose: 0,
            landed: false,
        }
    }

    pub fn origin(&self) -> [f32; 3] {
        self.origin
    }

    /// Put the body on a pad for shots and tests.
    pub fn place_at(&mut self, at: [f32; 3]) {
        self.origin = at;
        self.loose = 0;
    }

    /// True while the frog is resting between hops (perched on a lily).
    pub fn perched(&self) -> bool {
        let cycle = FROG_REST + FROG_STEPS;
        (self.steps % cycle) < FROG_REST
    }

    /// True once after a hop lands on a lily. Clears the flag.
    pub fn take_landed(&mut self) -> bool {
        let hit = self.landed;
        self.landed = false;
        hit
    }

    /// Accept a treat. The caller hands over the slimeball.
    pub fn feed(&mut self) -> bool {
        self.loose = 10;
        true
    }

    pub fn shove(&mut self, burst: [f32; 3]) -> bool {
        let Some(gust) = crate::body::wind_push(burst, self.origin) else {
            return false;
        };
        self.origin[0] += gust.dx;
        self.origin[2] += gust.dz;
        self.loose = 40;
        true
    }

    pub fn bump(&mut self, from: [f32; 3]) -> bool {
        bump_origin(&mut self.origin, &mut self.loose, from)
    }

    pub fn aimed(&self, eye: [f32; 3], forward: [f32; 3]) -> bool {
        let [x, y, z] = self.origin;
        let center = [x, y + 0.2, z];
        let to = [center[0] - eye[0], center[1] - eye[1], center[2] - eye[2]];
        let along = to[0] * forward[0] + to[1] * forward[1] + to[2] * forward[2];
        if !(0.0..=5.0).contains(&along) {
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
        dx * dx + dy * dy + dz * dz < 0.5
    }

    /// Hop the marsh square. True on the tick a bug is swallowed.
    pub fn tick(&mut self, count: u32, world: &Cluster) -> bool {
        let mut ate = false;
        for _ in 0..count {
            if self.bug_back > 0 {
                self.bug_back -= 1;
                if self.bug_back == 0 {
                    self.bug = true;
                }
            }
            if self.loose > 0 {
                self.loose -= 1;
                continue;
            }
            let from_x = self.origin[0];
            let from_z = self.origin[2];
            let was_hopping = !self.perched();
            self.steps = self.steps.wrapping_add(1);
            let next = frog_spot(self.steps);
            slide_land(&mut self.origin, next, world, 1);
            face_forward(
                &mut self.yaw,
                self.origin[0] - from_x,
                self.origin[2] - from_z,
            );
            if self.bug && self.on_bug() {
                self.bug = false;
                self.bug_back = BUG_BACK;
                ate = true;
            }
            if was_hopping && self.perched() {
                self.landed = true;
            }
        }
        ate
    }

    fn on_bug(&self) -> bool {
        let bug = frog_bug();
        let dx = self.origin[0] - bug[0];
        let dy = self.origin[1] - bug[1];
        let dz = self.origin[2] - bug[2];
        dx * dx + dy * dy + dz * dz < 0.04
    }

    pub fn bounds(&self) -> ([f32; 3], [f32; 3]) {
        let [x, y, z] = self.origin;
        ([x - 0.32, y, z - 0.28], [x + 0.32, y + 0.4, z + 0.28])
    }

    pub fn vertices(&self) -> Vec<Vertex> {
        let [x, y, z] = self.origin;
        let mut out = Vec::new();
        push_box(
            &mut out,
            [x, y + 0.1, z],
            [0.28, 0.08, 0.18],
            [0.22, 0.72, 0.28],
        );
        push_box(
            &mut out,
            [x - 0.12, y + 0.24, z + 0.08],
            [0.06, 0.08, 0.06],
            [0.94, 0.96, 0.72],
        );
        push_box(
            &mut out,
            [x + 0.12, y + 0.24, z + 0.08],
            [0.06, 0.08, 0.06],
            [0.94, 0.96, 0.72],
        );
        push_box(
            &mut out,
            [x - 0.12, y + 0.26, z + 0.12],
            [0.025, 0.03, 0.025],
            [0.08, 0.1, 0.08],
        );
        push_box(
            &mut out,
            [x + 0.12, y + 0.26, z + 0.12],
            [0.025, 0.03, 0.025],
            [0.08, 0.1, 0.08],
        );
        yaw_mesh(&mut out, self.origin, self.yaw);
        if self.bug {
            let bug = frog_bug();
            // A body about half a block across. The old cube was 8 cm and
            // disappeared against the marsh.
            push_box(
                &mut out,
                [bug[0], bug[1] + 0.32, bug[2]],
                [0.32, 0.32, 0.32],
                [0.28, 0.86, 0.32],
            );
            push_box(
                &mut out,
                [bug[0] - 0.1, bug[1] + 0.42, bug[2] + 0.28],
                [0.06, 0.06, 0.04],
                [0.92, 0.98, 0.86],
            );
            push_box(
                &mut out,
                [bug[0] + 0.1, bug[1] + 0.42, bug[2] + 0.28],
                [0.06, 0.06, 0.04],
                [0.92, 0.98, 0.86],
            );
        }
        out
    }
}

fn frog_bug() -> [f32; 3] {
    [FROG_HOME[0] + FROG_LEG, FROG_HOME[1], FROG_HOME[2]]
}

/// Marsh water just south of the stairs. The cell is (8, 32). The water block is y = 3.
pub const AXOLOTL_HOME: [f32; 3] = [8.5, 3.72, 32.5];
const AXOLOTL_LEG: f32 = 1.0;
/// Kept so a short swim still crosses the pool in the marsh test.
#[cfg_attr(not(test), allow(dead_code))]
pub const AXOLOTL_STEPS: u32 = 10;
const AXOLOTL_IDLE: u32 = 12;
const AXOLOTL_SWIM: f32 = 0.12;
/// Companion swim pace — a bit quicker so it keeps up.
const AXOLOTL_FOLLOW: f32 = 0.18;
/// How close a following axolotl hangs beside your feet.
const AXOLOTL_HEEL: f32 = 1.6;
/// Water search radius when hanging near a dry player.
const AXOLOTL_NEAR: i32 = 4;
/// Path cells an axolotl will BFS through water while following.
const AXOLOTL_REACH: i32 = 16;

#[derive(Clone, Copy)]
enum AxolotlMood {
    Idle { left: u32 },
    Swim,
}

pub struct Axolotl {
    origin: [f32; 3],
    mood: AxolotlMood,
    leg: u32,
    yaw: f32,
    pose: f32,
    /// 0 is the idle clip, 1 is the swim. It eases so the clips do not pop.
    fade: f32,
    following: bool,
    loose: u32,
}

impl Axolotl {
    pub fn new() -> Self {
        Self {
            origin: AXOLOTL_HOME,
            mood: AxolotlMood::Swim,
            leg: 0,
            yaw: 0.0,
            pose: 0.0,
            fade: 1.0,
            following: false,
            loose: 0,
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn origin(&self) -> [f32; 3] {
        self.origin
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn following(&self) -> bool {
        self.following
    }

    /// Start following, or go back to the swim. True when it is now following.
    pub fn follow(&mut self) -> bool {
        self.following = !self.following;
        self.following
    }

    /// Put the body in a pool for shots and tests.
    pub fn place_at(&mut self, at: [f32; 3]) {
        self.origin = at;
        self.loose = 0;
        self.mood = AxolotlMood::Swim;
    }

    pub fn shove(&mut self, burst: [f32; 3]) -> bool {
        let Some(gust) = crate::body::wind_push(burst, self.origin) else {
            return false;
        };
        self.origin[0] += gust.dx;
        self.origin[2] += gust.dz;
        self.loose = 40;
        true
    }

    pub fn bump(&mut self, from: [f32; 3]) -> bool {
        bump_origin(&mut self.origin, &mut self.loose, from)
    }

    pub fn aimed(&self, eye: [f32; 3], forward: [f32; 3]) -> bool {
        let [x, y, z] = self.origin;
        let center = [x, y + 0.4, z];
        let to = [center[0] - eye[0], center[1] - eye[1], center[2] - eye[2]];
        let along = to[0] * forward[0] + to[1] * forward[1] + to[2] * forward[2];
        if !(0.0..=5.0).contains(&along) {
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
        dx * dx + dy * dy + dz * dz < 0.8
    }

    /// Pause with the gills open, then swim one side. As a companion, pathfind
    /// through water to stay near the player (wet edge when they step onto land).
    pub fn tick(&mut self, count: u32, player: [f32; 2], world: &Cluster) {
        for _ in 0..count {
            self.pose += 0.03;
            let toward = if self.clip() == "walk" { 1.0 } else { 0.0 };
            self.fade += (toward - self.fade).clamp(-0.04, 0.04);
            if self.loose > 0 {
                self.loose -= 1;
                continue;
            }
            if self.following {
                if pet_far(self.origin, player) {
                    appear_in_water(&mut self.origin, &mut self.yaw, player, world);
                    self.mood = AxolotlMood::Idle { left: AXOLOTL_IDLE / 2 };
                    settle_swim(&mut self.origin, world);
                    continue;
                }
                let Some(goal) = companion_water_goal(player, world) else {
                    face_box(
                        &mut self.yaw,
                        player[0] - self.origin[0],
                        player[1] - self.origin[2],
                    );
                    self.mood = AxolotlMood::Idle { left: AXOLOTL_IDLE };
                    settle_swim(&mut self.origin, world);
                    continue;
                };
                let held = self.follow_water(goal[0], goal[1], world);
                self.mood = if held {
                    AxolotlMood::Idle { left: AXOLOTL_IDLE }
                } else {
                    AxolotlMood::Swim
                };
                settle_swim(&mut self.origin, world);
                continue;
            }
            match self.mood {
                AxolotlMood::Idle { left } => {
                    self.mood = if left == 0 {
                        AxolotlMood::Swim
                    } else {
                        AxolotlMood::Idle { left: left - 1 }
                    };
                }
                AxolotlMood::Swim => {
                    let goal = axolotl_corner(self.leg);
                    if self.swim_toward(goal[0], goal[1], world) {
                        self.leg = (self.leg + 1) % 4;
                        self.mood = AxolotlMood::Idle { left: AXOLOTL_IDLE };
                    }
                }
            }
            settle_swim(&mut self.origin, world);
        }
    }

    fn clip(&self) -> &'static str {
        if self.loose > 0 {
            return "idle";
        }
        match self.mood {
            AxolotlMood::Idle { .. } => "idle",
            AxolotlMood::Swim => "walk",
        }
    }

    /// Step toward a yard corner through water. True when that corner is reached.
    fn swim_toward(&mut self, x: f32, z: f32, world: &Cluster) -> bool {
        let dx = x - self.origin[0];
        let dz = z - self.origin[2];
        let dist = (dx * dx + dz * dz).sqrt();
        if dist <= 0.08 {
            return true;
        }
        let feet_y = self.origin[1].floor() as i32;
        let Some([next_x, next_z]) = water_step(
            world,
            [self.origin[0], self.origin[2]],
            [x, z],
            AXOLOTL_SWIM.min(dist),
            feet_y,
        ) else {
            return false;
        };
        self.yaw = (next_x - self.origin[0]).atan2(next_z - self.origin[2]);
        self.origin[0] = next_x;
        self.origin[2] = next_z;
        false
    }

    /// Step toward a wet goal while following. True when already close.
    fn follow_water(&mut self, x: f32, z: f32, world: &Cluster) -> bool {
        let dx = x - self.origin[0];
        let dz = z - self.origin[2];
        let dist = (dx * dx + dz * dz).sqrt();
        if dist <= AXOLOTL_HEEL {
            face_box(&mut self.yaw, dx, dz);
            return true;
        }
        let feet_y = self.origin[1].floor() as i32;
        let Some([next_x, next_z]) = water_follow_step(
            world,
            [self.origin[0], self.origin[2]],
            [x, z],
            AXOLOTL_FOLLOW.min(dist - AXOLOTL_HEEL),
            feet_y,
        ) else {
            face_box(&mut self.yaw, dx, dz);
            return false;
        };
        face_box(
            &mut self.yaw,
            next_x - self.origin[0],
            next_z - self.origin[2],
        );
        self.origin[0] = next_x;
        self.origin[2] = next_z;
        false
    }

    pub fn bounds(&self) -> ([f32; 3], [f32; 3]) {
        let [x, y, z] = self.origin;
        ([x - 0.55, y, z - 0.4], [x + 0.55, y + 0.7, z + 0.4])
    }

    pub fn vertices(&self) -> Vec<Vertex> {
        crate::blockbench::mesh_blend(
            axolotl_figure(),
            "idle",
            "walk",
            self.pose,
            self.yaw,
            self.origin,
            &[],
            self.fade,
        )
    }

    /// Soft lead beads from the axolotl neck to the player's hand while following.
    pub fn leash_rope(&self, hand: [f32; 3]) -> Vec<Vertex> {
        if !self.following {
            return Vec::new();
        }
        let [x, y, z] = self.origin;
        let neck = [x, y + 0.38, z];
        let mut out = Vec::new();
        let rope = [0.72, 0.58, 0.28];
        let segs = 8;
        for i in 0..=segs {
            let t = i as f32 / segs as f32;
            let sag = 0.08 * (4.0 * t * (1.0 - t));
            let p = [
                neck[0] + (hand[0] - neck[0]) * t,
                neck[1] + (hand[1] - neck[1]) * t - sag,
                neck[2] + (hand[2] - neck[2]) * t,
            ];
            let half = if i == 0 || i == segs { 0.045 } else { 0.028 };
            push_box(&mut out, p, [half, half, half], rope);
        }
        out
    }
}

fn axolotl_figure() -> &'static crate::blockbench::Figure {
    use std::sync::OnceLock;
    static FIGURE: OnceLock<crate::blockbench::Figure> = OnceLock::new();
    FIGURE.get_or_init(|| {
        crate::blockbench::parse(include_str!(
            "../../../resourcepacks/terralite/assets/terralite/models/entity/axolotl.bbmodel"
        ))
        .expect("axolotl.bbmodel")
    })
}

fn axolotl_corner(leg: u32) -> [f32; 2] {
    let x = AXOLOTL_HOME[0];
    let z = AXOLOTL_HOME[2];
    match leg % 4 {
        0 => [x + AXOLOTL_LEG, z],
        1 => [x + AXOLOTL_LEG, z + AXOLOTL_LEG],
        2 => [x, z + AXOLOTL_LEG],
        _ => [x, z],
    }
}

/// Grass just west of the oak by the stairs.
const CHICKEN_HOME: [f32; 3] = [3.0, 1.0, 8.0];
const CHICKEN_LEG: f32 = 1.0;
/// Entity ticks spent pecking between walks.
const CHICKEN_IDLE: u32 = 16;
/// The chicken steps back when the player is this close, in blocks.
const CHICKEN_NEAR: f32 = 2.6;
const CHICKEN_FLEE: u32 = 10;
const CHICKEN_WALK: f32 = 0.14;
const CHICKEN_BOLT: f32 = 0.16;
/// Entity ticks between eggs.
const LAY_EVERY: u32 = 40;

#[derive(Clone, Copy)]
enum ChickenMood {
    Idle { left: u32 },
    Walk,
    Flee { left: u32 },
}

pub struct Chicken {
    origin: [f32; 3],
    mood: ChickenMood,
    leg: u32,
    yaw: f32,
    pose: f32,
    egg: bool,
    lay: u32,
    loose: u32,
}

impl Chicken {
    pub fn new() -> Self {
        Self {
            origin: CHICKEN_HOME,
            mood: ChickenMood::Idle { left: CHICKEN_IDLE },
            leg: 0,
            yaw: -std::f32::consts::FRAC_PI_2,
            pose: 0.0,
            egg: true,
            lay: 0,
            loose: 0,
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn origin(&self) -> [f32; 3] {
        self.origin
    }

    pub fn egg(&self) -> bool {
        self.egg
    }

    /// Take the ready egg. False when it is still pecking.
    pub fn take_egg(&mut self) -> bool {
        if !self.egg {
            return false;
        }
        self.egg = false;
        self.lay = LAY_EVERY;
        true
    }

    pub fn shove(&mut self, burst: [f32; 3]) -> bool {
        let Some(gust) = crate::body::wind_push(burst, self.origin) else {
            return false;
        };
        self.origin[0] += gust.dx;
        self.origin[2] += gust.dz;
        self.loose = 40;
        true
    }

    pub fn bump(&mut self, from: [f32; 3]) -> bool {
        bump_origin(&mut self.origin, &mut self.loose, from)
    }

    pub fn aimed(&self, eye: [f32; 3], forward: [f32; 3]) -> bool {
        let [x, y, z] = self.origin;
        let center = [x, y + 0.4, z];
        let to = [center[0] - eye[0], center[1] - eye[1], center[2] - eye[2]];
        let along = to[0] * forward[0] + to[1] * forward[1] + to[2] * forward[2];
        if !(0.0..=5.0).contains(&along) {
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
        dx * dx + dy * dy + dz * dz < 0.8
    }

    /// Peck, walk one side of the yard, or step back from `player` (x, z).
    /// A full block sends the step around it. True on the tick an egg is ready.
    pub fn tick(&mut self, count: u32, player: [f32; 2], world: &Cluster) -> bool {
        let mut laid = false;
        for _ in 0..count {
            self.pose += 0.05;
            if !self.egg {
                self.lay = self.lay.saturating_sub(1);
                if self.lay == 0 {
                    self.egg = true;
                    laid = true;
                }
            }
            if self.loose > 0 {
                self.loose -= 1;
                settle_land(&mut self.origin, world);
                continue;
            }
            let dx = player[0] - self.origin[0];
            let dz = player[1] - self.origin[2];
            if dx * dx + dz * dz < CHICKEN_NEAR * CHICKEN_NEAR {
                self.mood = ChickenMood::Flee { left: CHICKEN_FLEE };
            }
            match self.mood {
                ChickenMood::Idle { left } => {
                    self.mood = if left == 0 {
                        ChickenMood::Walk
                    } else {
                        ChickenMood::Idle { left: left - 1 }
                    };
                }
                ChickenMood::Walk => {
                    let goal = chicken_corner(self.leg);
                    if self.face_step(goal[0], goal[1], CHICKEN_WALK, world) {
                        self.leg = (self.leg + 1) % 4;
                        self.mood = ChickenMood::Idle { left: CHICKEN_IDLE };
                    }
                }
                ChickenMood::Flee { left } => {
                    self.step_away(player, world);
                    let left = left.saturating_sub(1);
                    self.mood = if left == 0 {
                        ChickenMood::Idle { left: CHICKEN_IDLE }
                    } else {
                        ChickenMood::Flee { left }
                    };
                }
            }
            settle_land(&mut self.origin, world);
        }
        laid
    }

    fn clip(&self) -> &'static str {
        if self.loose > 0 {
            return "idle";
        }
        match self.mood {
            ChickenMood::Idle { .. } => "idle",
            ChickenMood::Walk | ChickenMood::Flee { .. } => "walk",
        }
    }

    /// Step toward a yard corner. True when that corner is reached.
    fn face_step(&mut self, x: f32, z: f32, step: f32, world: &Cluster) -> bool {
        let dx = x - self.origin[0];
        let dz = z - self.origin[2];
        let dist = (dx * dx + dz * dz).sqrt();
        if dist <= 0.08 {
            if place_clear(world, x, self.origin[1], z, 1) {
                self.origin[0] = x;
                self.origin[2] = z;
                return true;
            }
            return false;
        }
        let from = [self.origin[0], self.origin[2]];
        let feet_y = self.origin[1].floor() as i32;
        let Some(next) = land_step(world, from, [x, z], step.min(dist), feet_y, 1) else {
            return false;
        };
        self.yaw = (next[0] - from[0]).atan2(next[1] - from[1]);
        self.origin[0] = next[0];
        self.origin[2] = next[1];
        false
    }

    fn step_away(&mut self, player: [f32; 2], world: &Cluster) {
        let dx = self.origin[0] - player[0];
        let dz = self.origin[2] - player[1];
        let dist = (dx * dx + dz * dz).sqrt();
        let (away_x, away_z) = if dist < 0.05 {
            (1.0, 0.0)
        } else {
            (dx / dist, dz / dist)
        };
        let from = [self.origin[0], self.origin[2]];
        let goal = [from[0] + away_x * 2.0, from[1] + away_z * 2.0];
        let feet_y = self.origin[1].floor() as i32;
        let Some(next) = land_step(world, from, goal, CHICKEN_BOLT, feet_y, 1) else {
            return;
        };
        self.yaw = (next[0] - from[0]).atan2(next[1] - from[1]);
        self.origin[0] = next[0];
        self.origin[2] = next[1];
    }

    pub fn bounds(&self) -> ([f32; 3], [f32; 3]) {
        let [x, y, z] = self.origin;
        ([x - 0.4, y, z - 0.45], [x + 0.85, y + 0.7, z + 0.45])
    }

    pub fn vertices(&self) -> Vec<Vertex> {
        let hide: &[&str] = if self.egg { &[] } else { &["egg"] };
        crate::blockbench::mesh(
            chicken_figure(),
            self.clip(),
            self.pose,
            self.yaw,
            self.origin,
            hide,
        )
    }
}

fn chicken_figure() -> &'static crate::blockbench::Figure {
    use std::sync::OnceLock;
    static FIGURE: OnceLock<crate::blockbench::Figure> = OnceLock::new();
    FIGURE.get_or_init(|| {
        crate::blockbench::parse(include_str!(
            "../../../resourcepacks/terralite/assets/terralite/models/entity/chicken.bbmodel"
        ))
        .expect("chicken.bbmodel")
    })
}

fn chicken_corner(leg: u32) -> [f32; 2] {
    let x = CHICKEN_HOME[0];
    let z = CHICKEN_HOME[2];
    match leg % 4 {
        0 => [x + CHICKEN_LEG, z],
        1 => [x + CHICKEN_LEG, z + CHICKEN_LEG],
        2 => [x, z + CHICKEN_LEG],
        _ => [x, z],
    }
}

#[cfg(test)]
fn water_cell(world: &Cluster, x: f32, z: f32) -> bool {
    world.block_at(x.floor() as i32, 3, z.floor() as i32).id == "water"
}

/// One step toward `goal` that stays on open cells. A straight step is used
/// when that cell is open. Otherwise the first cell of a short path is used.
fn route_step(
    from: [f32; 2],
    goal: [f32; 2],
    step: f32,
    open: impl Fn(i32, i32) -> bool,
) -> Option<[f32; 2]> {
    route_step_reach(from, goal, step, 8, open)
}

fn is_fluid(id: &str) -> bool {
    id == "water" || id == "lava"
}

/// Plants, switches, and other shapes a body walks through. Water is a fluid, not one of these.
fn is_soft(id: &str) -> bool {
    matches!(
        id,
        "flower"
            | "azalea"
            | "moss_carpet"
            | "lily_pad"
            | "button"
            | "lever"
            | "tripwire"
            | "painting"
            | "item_frame"
            | "flower_pot"
            | "cobweb"
            | "sugar_cane"
            | "bamboo"
            | "scaffolding"
            | "cave_vines"
            | "cave_vines_berries"
            | "powder_snow"
            | "sculk_vein"
            | "wheat_crop"
            | "chorus_plant"
            | "chorus_flower"
            | "copper_grate"
    )
}

/// A cell whose collider stops a mob. Fluids and soft shapes do not.
fn cell_blocks(world: &Cluster, x: i32, y: i32, z: i32) -> bool {
    if !(0..256).contains(&y) {
        return false;
    }
    let block = world.block_at(x, y, z);
    if block.is_air() || is_fluid(block.id) || is_soft(block.id) {
        return false;
    }
    !rebuild_world::shape_of(block).is_empty()
}

fn fluid_at(world: &Cluster, x: i32, y: i32, z: i32) -> bool {
    (0..256).contains(&y) && is_fluid(world.block_at(x, y, z).id)
}

/// Top of the fluid column that contains `y`, in blocks. `None` when that cell is dry.
fn fluid_top(world: &Cluster, x: i32, z: i32, y: i32) -> Option<f32> {
    if !fluid_at(world, x, y, z) {
        return None;
    }
    let mut top = y;
    while fluid_at(world, x, top + 1, z) {
        top += 1;
    }
    Some(top as f32 + 1.0)
}

fn place_clear(world: &Cluster, x: f32, y: f32, z: f32, headroom: i32) -> bool {
    let iy = y.floor() as i32;
    let samples = [
        [x, z],
        [x - 0.3, z],
        [x + 0.3, z],
        [x, z - 0.3],
        [x, z + 0.3],
    ];
    samples.iter().all(|sample| {
        let ix = sample[0].floor() as i32;
        let iz = sample[1].floor() as i32;
        (0..headroom).all(|rise| !cell_blocks(world, ix, iy + rise, iz))
    })
}

/// Walk toward `to` and stop at the first solid. In water, feet rise to the surface.
pub(crate) fn slide_land(origin: &mut [f32; 3], to: [f32; 3], world: &Cluster, headroom: i32) {
    let from = *origin;
    let dx = to[0] - from[0];
    let dy = to[1] - from[1];
    let dz = to[2] - from[2];
    let dist = (dx * dx + dz * dz).sqrt();
    let steps = ((dist / 0.25).ceil() as i32).max(1);
    let mut last = from;
    for index in 1..=steps {
        let t = index as f32 / steps as f32;
        let next = [from[0] + dx * t, from[1] + dy * t, from[2] + dz * t];
        if !place_clear(world, next[0], next[1], next[2], headroom) {
            break;
        }
        last = next;
    }
    *origin = last;
    settle_land(origin, world);
}

/// A land mob in water floats so the body sits in the surface. A body inside a solid is pushed up.
fn settle_land(origin: &mut [f32; 3], world: &Cluster) {
    let x = origin[0].floor() as i32;
    let z = origin[2].floor() as i32;
    let y = origin[1].floor() as i32;
    if let Some(pad) = lily_pad_top(world, x, z, y) {
        // Keep hops above the pad; only settle while near the leaf.
        if origin[1] <= pad + 0.2 {
            origin[1] = pad;
        }
        return;
    }
    if let Some(top) = fluid_top(world, x, z, y) {
        if origin[1] < top - 0.05 {
            origin[1] = top;
        }
        return;
    }
    if cell_blocks(world, x, y, z) {
        origin[1] = y as f32 + 1.0;
    }
}

/// Top of a lily pad near `y`, in blocks. Pads sit one cell above marsh water.
fn lily_pad_top(world: &Cluster, x: i32, z: i32, y: i32) -> Option<f32> {
    for dy in [0, -1, 1] {
        let yy = y + dy;
        if (0..256).contains(&yy) && world.block_at(x, yy, z).id == "lily_pad" {
            return Some(yy as f32 + 2.0 / 16.0);
        }
    }
    None
}

/// A water mob holds just under the surface instead of resting on the floor.
fn settle_swim(origin: &mut [f32; 3], world: &Cluster) {
    let x = origin[0].floor() as i32;
    let z = origin[2].floor() as i32;
    let mut y = origin[1].floor() as i32;
    if !fluid_at(world, x, y, z) {
        y -= 1;
        if !fluid_at(world, x, y, z) {
            return;
        }
    }
    let Some(top) = fluid_top(world, x, z, y) else {
        return;
    };
    let mut bottom = y;
    while bottom > 0 && fluid_at(world, x, bottom - 1, z) {
        bottom -= 1;
    }
    origin[1] = (top - 0.28).max(bottom as f32);
}

fn land_step(
    world: &Cluster,
    from: [f32; 2],
    goal: [f32; 2],
    step: f32,
    feet_y: i32,
    headroom: i32,
) -> Option<[f32; 2]> {
    route_step(from, goal, step, |x, z| {
        (0..headroom).all(|rise| !cell_blocks(world, x, feet_y + rise, z))
    })
}

fn water_step(
    world: &Cluster,
    from: [f32; 2],
    goal: [f32; 2],
    step: f32,
    feet_y: i32,
) -> Option<[f32; 2]> {
    water_step_reach(world, from, goal, step, feet_y, 8)
}

/// Companion pathfind through water — longer BFS so it can snake around dry cells.
fn water_follow_step(
    world: &Cluster,
    from: [f32; 2],
    goal: [f32; 2],
    step: f32,
    feet_y: i32,
) -> Option<[f32; 2]> {
    water_step_reach(world, from, goal, step, feet_y, AXOLOTL_REACH)
}

fn water_open(world: &Cluster, x: i32, z: i32, feet_y: i32) -> bool {
    for dy in [-1, 0, 1] {
        let y = feet_y + dy;
        if fluid_at(world, x, y, z) && !cell_blocks(world, x, y + 1, z) {
            return true;
        }
    }
    false
}

fn water_step_reach(
    world: &Cluster,
    from: [f32; 2],
    goal: [f32; 2],
    step: f32,
    feet_y: i32,
    reach: i32,
) -> Option<[f32; 2]> {
    let next = route_step_reach(from, goal, step, reach, |x, z| water_open(world, x, z, feet_y))?;
    let ix = next[0].floor() as i32;
    let iz = next[1].floor() as i32;
    water_open(world, ix, iz, feet_y).then_some(next)
}

/// Aim for the player when any water is within [`AXOLOTL_NEAR`] — pathfind keeps
/// the body in water and hangs at the wet edge beside dry feet. `None` when the
/// player has left the water entirely.
fn companion_water_goal(player: [f32; 2], world: &Cluster) -> Option<[f32; 2]> {
    let px = player[0].floor() as i32;
    let pz = player[1].floor() as i32;
    for dz in -AXOLOTL_NEAR..=AXOLOTL_NEAR {
        for dx in -AXOLOTL_NEAR..=AXOLOTL_NEAR {
            let x = px + dx;
            let z = pz + dz;
            if water_open(world, x, z, 3)
                || water_open(world, x, z, 2)
                || water_open(world, x, z, 1)
            {
                return Some(player);
            }
        }
    }
    None
}

/// Nearest wet cell center around `player`, if any within [`AXOLOTL_NEAR`].
fn nearest_water_cell(player: [f32; 2], world: &Cluster) -> Option<[f32; 2]> {
    let px = player[0].floor() as i32;
    let pz = player[1].floor() as i32;
    let mut best: Option<(i32, i32, i32)> = None;
    for dz in -AXOLOTL_NEAR..=AXOLOTL_NEAR {
        for dx in -AXOLOTL_NEAR..=AXOLOTL_NEAR {
            let x = px + dx;
            let z = pz + dz;
            if !(water_open(world, x, z, 3)
                || water_open(world, x, z, 2)
                || water_open(world, x, z, 1))
            {
                continue;
            }
            let dist = dx.abs() + dz.abs();
            if best.map_or(true, |(_, _, d)| dist < d) {
                best = Some((x, z, dist));
            }
        }
    }
    best.map(|(x, z, _)| [x as f32 + 0.5, z as f32 + 0.5])
}

/// Warp into water beside the player when the leash snaps (same spirit as land pets).
fn appear_in_water(origin: &mut [f32; 3], yaw: &mut f32, player: [f32; 2], world: &Cluster) {
    let spots = [
        [player[0] + 1.15, player[1] + 0.35],
        [player[0] - 1.15, player[1] + 0.2],
        [player[0] + 0.3, player[1] + 1.15],
        [player[0] + 0.2, player[1] - 1.15],
        [player[0] + 1.4, player[1] - 0.4],
        [player[0] - 0.5, player[1] + 1.4],
    ];
    for spot in spots {
        let x = spot[0].floor() as i32;
        let z = spot[1].floor() as i32;
        if !(water_open(world, x, z, 3) || water_open(world, x, z, 2) || water_open(world, x, z, 1))
        {
            continue;
        }
        face_box(yaw, player[0] - spot[0], player[1] - spot[1]);
        origin[0] = spot[0];
        origin[2] = spot[1];
        settle_swim(origin, world);
        return;
    }
    if let Some(goal) = nearest_water_cell(player, world) {
        face_box(yaw, player[0] - goal[0], player[1] - goal[1]);
        origin[0] = goal[0];
        origin[2] = goal[1];
        settle_swim(origin, world);
        return;
    }
    face_box(yaw, -1.0, 0.0);
}

/// One step toward `goal` that stays on open cells. A straight step is used
/// when that cell is open. Otherwise the first cell of a short path is used.
fn route_step_reach(
    from: [f32; 2],
    goal: [f32; 2],
    step: f32,
    reach: i32,
    open: impl Fn(i32, i32) -> bool,
) -> Option<[f32; 2]> {
    let dx = goal[0] - from[0];
    let dz = goal[1] - from[1];
    let dist = (dx * dx + dz * dz).sqrt();
    if dist <= 1.0e-4 || step <= 0.0 {
        return None;
    }
    let step = step.min(dist);
    let straight = [from[0] + dx / dist * step, from[1] + dz / dist * step];
    let here = (from[0].floor() as i32, from[1].floor() as i32);
    let into = (straight[0].floor() as i32, straight[1].floor() as i32);
    if open(into.0, into.1) {
        return Some(straight);
    }
    let aim = (goal[0].floor() as i32, goal[1].floor() as i32);
    let via = first_open_step(here, aim, reach, &open)?;
    let center = [via.0 as f32 + 0.5, via.1 as f32 + 0.5];
    let vx = center[0] - from[0];
    let vz = center[1] - from[1];
    let along = (vx * vx + vz * vz).sqrt();
    if along <= 1.0e-4 {
        return None;
    }
    let step = step.min(along);
    Some([from[0] + vx / along * step, from[1] + vz / along * step])
}

/// The neighbor that starts a path of at most `reach` blocks toward `aim`.
fn first_open_step(
    start: (i32, i32),
    aim: (i32, i32),
    reach: i32,
    open: &impl Fn(i32, i32) -> bool,
) -> Option<(i32, i32)> {
    let mut queue = std::collections::VecDeque::new();
    let mut seen = Vec::new();
    let mut parent = Vec::new();
    queue.push_back(start);
    seen.push(start);
    let mut found = None;
    while let Some(cell) = queue.pop_front() {
        if cell == aim {
            found = Some(cell);
            break;
        }
        if (cell.0 - start.0).abs() >= reach || (cell.1 - start.1).abs() >= reach {
            continue;
        }
        for (ox, oz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            let next = (cell.0 + ox, cell.1 + oz);
            if (next.0 - start.0).abs() > reach || (next.1 - start.1).abs() > reach {
                continue;
            }
            if seen.contains(&next) || !open(next.0, next.1) {
                continue;
            }
            seen.push(next);
            parent.push((next, cell));
            queue.push_back(next);
        }
    }
    let target = found.or_else(|| {
        seen.iter()
            .copied()
            .filter(|cell| *cell != start)
            .min_by(|a, b| {
                let da = (a.0 - aim.0).abs() + (a.1 - aim.1).abs();
                let db = (b.0 - aim.0).abs() + (b.1 - aim.1).abs();
                da.cmp(&db)
            })
    })?;
    let mut cell = target;
    if cell == start {
        return None;
    }
    while let Some((_, previous)) = parent.iter().find(|(next, _)| *next == cell).copied() {
        if previous == start {
            return Some(cell);
        }
        cell = previous;
    }
    None
}

fn frog_spot(steps: u32) -> [f32; 3] {
    let cycle = FROG_REST + FROG_STEPS;
    let lap = steps % (cycle * 4);
    let leg = lap / cycle;
    let into = lap % cycle;
    let (along, lift) = if into < FROG_REST {
        (0.0, 0.0)
    } else {
        let t = (into - FROG_REST) as f32 / FROG_STEPS as f32;
        (t * FROG_LEG, (t * std::f32::consts::PI).sin() * 0.45)
    };
    let (x, z) = match leg {
        0 => (FROG_HOME[0] + along, FROG_HOME[2]),
        1 => (FROG_HOME[0] + FROG_LEG, FROG_HOME[2] + along),
        2 => (FROG_HOME[0] + FROG_LEG - along, FROG_HOME[2] + FROG_LEG),
        _ => (FROG_HOME[0], FROG_HOME[2] + FROG_LEG - along),
    };
    [x, FROG_HOME[1] + lift, z]
}

/// A beehive at this level is full. The bee looks for a lower one.
const HIVE_FULL: u8 = 3;
/// Feet the bee drifts around. Flower and hive sit just west on the demo pad.
pub const BEE_HOME: [f32; 3] = [14.0, 2.6, 6.0];
const BEE_SCAN: i32 = 8;
const BEE_STEP: f32 = 0.22;
const BEE_ANGER: u32 = 120;
const STING_WAIT: u32 = 20;

/// What the bee finished on this tick.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BeeWork {
    /// It just left a flower with pollen.
    Pollen,
    /// Pour one level into this beehive.
    Fill([i32; 3], u8),
    /// It is close enough to sting.
    Sting,
}

#[derive(Clone, Copy)]
enum Aim {
    Flower([i32; 3]),
    Hive([i32; 3]),
    Drift,
}

pub struct Bee {
    origin: [f32; 3],
    goal: Option<[f32; 3]>,
    aim: Aim,
    pollen: bool,
    angry: u32,
    sting_wait: u32,
    wait: u32,
    loose: u32,
    lap: u32,
}

impl Bee {
    pub fn new() -> Self {
        Self {
            origin: BEE_HOME,
            goal: None,
            aim: Aim::Drift,
            pollen: false,
            angry: 0,
            sting_wait: 0,
            wait: 0,
            loose: 0,
            lap: 0,
        }
    }

    pub fn origin(&self) -> [f32; 3] {
        self.origin
    }

    /// Put the body somewhere for shots and tests.
    pub fn place_at(&mut self, at: [f32; 3]) {
        self.origin = at;
        self.goal = None;
        self.loose = 0;
        self.wait = 0;
    }

    pub fn pollen(&self) -> bool {
        self.pollen
    }

    /// The hive edit did not land. Keep the pollen for another pass.
    pub fn keep_pollen(&mut self) {
        self.pollen = true;
    }

    /// A hive was broken with an empty hand.
    pub fn anger(&mut self) {
        self.angry = BEE_ANGER;
        self.goal = None;
        self.wait = 0;
    }

    pub fn shove(&mut self, burst: [f32; 3]) -> bool {
        let Some(gust) = crate::body::wind_push(burst, self.origin) else {
            return false;
        };
        self.origin[0] += gust.dx;
        self.origin[2] += gust.dz;
        self.loose = 40;
        self.goal = None;
        true
    }

    pub fn bump(&mut self, from: [f32; 3]) -> bool {
        bump_origin(&mut self.origin, &mut self.loose, from)
    }

    /// Fly, pollinate, fill, or chase. `player` is the feet position.
    pub fn tick(&mut self, count: u32, player: [f32; 3], world: &Cluster) -> Option<BeeWork> {
        let mut work = None;
        for _ in 0..count {
            if self.loose > 0 {
                self.loose -= 1;
                continue;
            }
            if self.angry > 0 {
                self.angry -= 1;
                let chase = [player[0], player[1] + 1.1, player[2]];
                let _ = self.step_toward(chase);
                if self.sting_wait > 0 {
                    self.sting_wait -= 1;
                } else if flat_distance(self.origin, chase) < 1.5 {
                    self.sting_wait = STING_WAIT;
                    work = Some(BeeWork::Sting);
                }
                continue;
            }
            if self.wait > 0 {
                self.wait -= 1;
                continue;
            }
            if self.goal.is_none() {
                self.choose(world);
            }
            let Some(goal) = self.goal else {
                continue;
            };
            if self.step_toward(goal) {
                if let Some(found) = self.arrive(world) {
                    work = Some(found);
                }
                self.goal = None;
                self.wait = 8;
            }
        }
        work
    }

    fn choose(&mut self, world: &Cluster) {
        if self.pollen {
            if let Some(cell) = self.nearest(world, true) {
                self.aim = Aim::Hive(cell);
                self.goal = Some(hover(cell));
                return;
            }
        } else if let Some(cell) = self.nearest(world, false) {
            self.aim = Aim::Flower(cell);
            self.goal = Some(hover(cell));
            return;
        }
        self.lap = self.lap.wrapping_add(1);
        self.aim = Aim::Drift;
        let d = 1.4;
        self.goal = Some(match self.lap % 4 {
            0 => [BEE_HOME[0] + d, BEE_HOME[1], BEE_HOME[2]],
            1 => [BEE_HOME[0] + d, BEE_HOME[1], BEE_HOME[2] + d],
            2 => [BEE_HOME[0], BEE_HOME[1], BEE_HOME[2] + d],
            _ => BEE_HOME,
        });
    }

    fn nearest(&self, world: &Cluster, hive: bool) -> Option<[i32; 3]> {
        let ox = self.origin[0].floor() as i32;
        let oy = self.origin[1].floor() as i32;
        let oz = self.origin[2].floor() as i32;
        let mut best: Option<(i32, [i32; 3])> = None;
        for dz in -BEE_SCAN..=BEE_SCAN {
            for dx in -BEE_SCAN..=BEE_SCAN {
                for dy in -3..=2 {
                    let cell = [ox + dx, oy + dy, oz + dz];
                    let block = world.block_at(cell[0], cell[1], cell[2]);
                    let ok = if hive {
                        block.id == "beehive" && block.metadata < HIVE_FULL
                    } else {
                        matches!(block.id, "flower" | "azalea" | "moss_carpet")
                    };
                    if !ok {
                        continue;
                    }
                    let score = dx * dx + dy * dy + dz * dz;
                    if best.map(|(have, _)| score < have).unwrap_or(true) {
                        best = Some((score, cell));
                    }
                }
            }
        }
        best.map(|(_, cell)| cell)
    }

    fn arrive(&mut self, world: &Cluster) -> Option<BeeWork> {
        match self.aim {
            Aim::Flower(cell) => {
                if !self.pollen && world.block_at(cell[0], cell[1], cell[2]).id == "flower" {
                    self.pollen = true;
                    return Some(BeeWork::Pollen);
                }
                None
            }
            Aim::Hive(cell) => {
                let block = world.block_at(cell[0], cell[1], cell[2]);
                if self.pollen && block.id == "beehive" && block.metadata < HIVE_FULL {
                    let next = block.metadata.saturating_add(1);
                    self.pollen = false;
                    return Some(BeeWork::Fill(cell, next));
                }
                None
            }
            Aim::Drift => None,
        }
    }

    /// Move toward `goal`. True when this step lands on it.
    fn step_toward(&mut self, goal: [f32; 3]) -> bool {
        let dx = goal[0] - self.origin[0];
        let dy = goal[1] - self.origin[1];
        let dz = goal[2] - self.origin[2];
        let dist = (dx * dx + dy * dy + dz * dz).sqrt();
        if dist < 0.25 {
            self.origin = goal;
            return true;
        }
        let step = BEE_STEP.min(dist);
        self.origin[0] += dx / dist * step;
        self.origin[1] += dy / dist * step;
        self.origin[2] += dz / dist * step;
        false
    }

    pub fn bounds(&self) -> ([f32; 3], [f32; 3]) {
        let [x, y, z] = self.origin;
        ([x - 0.2, y, z - 0.16], [x + 0.2, y + 0.28, z + 0.16])
    }

    pub fn vertices(&self) -> Vec<Vertex> {
        let [x, y, z] = self.origin;
        let mut out = Vec::new();
        push_box(
            &mut out,
            [x, y + 0.1, z],
            [0.1, 0.07, 0.08],
            [0.93, 0.78, 0.16],
        );
        push_box(
            &mut out,
            [x, y + 0.1, z],
            [0.035, 0.075, 0.085],
            [0.08, 0.08, 0.08],
        );
        push_box(
            &mut out,
            [x - 0.16, y + 0.14, z],
            [0.08, 0.01, 0.05],
            [0.9, 0.93, 0.84],
        );
        push_box(
            &mut out,
            [x + 0.16, y + 0.14, z],
            [0.08, 0.01, 0.05],
            [0.9, 0.93, 0.84],
        );
        let eye = if self.angry > 0 {
            [0.86, 0.12, 0.1]
        } else {
            [0.1, 0.1, 0.1]
        };
        push_box(&mut out, [x, y + 0.14, z + 0.06], [0.03, 0.025, 0.02], eye);
        if self.pollen {
            push_box(
                &mut out,
                [x, y + 0.16, z - 0.04],
                [0.04, 0.03, 0.03],
                [0.95, 0.5, 0.12],
            );
        }
        out
    }
}

fn hover(cell: [i32; 3]) -> [f32; 3] {
    [
        cell[0] as f32 + 0.5,
        cell[1] as f32 + 1.2,
        cell[2] as f32 + 0.5,
    ]
}

fn flat_distance(from: [f32; 3], to: [f32; 3]) -> f32 {
    let dx = from[0] - to[0];
    let dz = from[2] - to[2];
    (dx * dx + dz * dz).sqrt()
}

/// Sweet food stands in for berries. It earns the fox's trust.
pub fn fox_food(id: &str) -> bool {
    matches!(id, "apple" | "honey" | "melon_slice")
}

const FOX_HOME: [f32; 3] = [6.0, 1.0, 6.0];
const GRAB: f32 = 0.75;

pub struct Fox {
    origin: [f32; 3],
    yaw: f32,
    trusted: bool,
    sitting: bool,
    held: Option<&'static str>,
    loose: u32,
    roam: Roam,
}

impl Fox {
    pub fn new() -> Self {
        Self {
            origin: FOX_HOME,
            yaw: 0.0,
            trusted: false,
            sitting: false,
            held: None,
            loose: 0,
            roam: Roam::around(FOX_HOME),
        }
    }

    pub fn origin(&self) -> [f32; 3] {
        self.origin
    }

    pub fn trusted(&self) -> bool {
        self.trusted
    }

    pub fn holding(&self) -> Option<&'static str> {
        self.held
    }

    /// Trust a wild fox. False when it already follows.
    pub fn trust(&mut self) -> bool {
        if self.trusted {
            return false;
        }
        self.trusted = true;
        self.sitting = false;
        true
    }

    pub fn sitting(&self) -> bool {
        self.sitting
    }

    /// Sit or stand. False when the fox does not follow yet. True when it sits.
    pub fn sit(&mut self) -> bool {
        if !self.trusted {
            return false;
        }
        self.sitting = !self.sitting;
        self.sitting
    }

    /// Pick up one food. A full mouth keeps what it already has.
    pub fn hold(&mut self, id: &'static str) {
        if self.held.is_none() {
            self.held = Some(id);
        }
    }

    pub fn shove(&mut self, burst: [f32; 3]) -> bool {
        let Some(gust) = crate::body::wind_push(burst, self.origin) else {
            return false;
        };
        self.origin[0] += gust.dx;
        self.origin[2] += gust.dz;
        self.loose = 40;
        true
    }

    pub fn bump(&mut self, from: [f32; 3]) -> bool {
        bump_origin(&mut self.origin, &mut self.loose, from)
    }

    pub fn aimed(&self, eye: [f32; 3], forward: [f32; 3]) -> bool {
        let [x, y, z] = self.origin;
        let center = [x, y + 0.35, z];
        let to = [center[0] - eye[0], center[1] - eye[1], center[2] - eye[2]];
        let along = to[0] * forward[0] + to[1] * forward[1] + to[2] * forward[2];
        if !(0.0..=5.0).contains(&along) {
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
        dx * dx + dy * dy + dz * dz < 0.45
    }

    /// Wander, sit, fetch, or follow. A full block sends the step around it.
    /// Past twelve blocks a standing fox appears beside you.
    /// True when the mouth is empty and `treat` is in reach.
    pub fn tick(
        &mut self,
        count: u32,
        player: [f32; 2],
        treat: Option<[f32; 3]>,
        world: &Cluster,
    ) -> bool {
        let mut grab = false;
        for _ in 0..count {
            if self.loose > 0 {
                self.loose -= 1;
                continue;
            }
            if self.sitting {
                face_box(
                    &mut self.yaw,
                    player[0] - self.origin[0],
                    player[1] - self.origin[2],
                );
                continue;
            }
            if self.held.is_none() {
                if let Some(spot) = treat {
                    if heel_step(
                        &mut self.origin,
                        &mut self.yaw,
                        [spot[0], spot[2]],
                        GRAB,
                        world,
                    ) {
                        grab = true;
                    }
                    continue;
                }
            }
            if self.trusted {
                if pet_far(self.origin, player) {
                    appear_beside(&mut self.origin, &mut self.yaw, player, world);
                } else {
                    let _ = heel_step(&mut self.origin, &mut self.yaw, player, HEEL, world);
                }
                continue;
            }
            roam_step(
                &mut self.origin,
                &mut self.yaw,
                &mut self.roam,
                FOX_HOME,
                world,
            );
        }
        grab
    }

    pub fn bounds(&self) -> ([f32; 3], [f32; 3]) {
        let [x, y, z] = self.origin;
        ([x - 0.28, y, z - 0.22], [x + 0.28, y + 0.55, z + 0.22])
    }

    pub fn vertices(&self) -> Vec<Vertex> {
        let [x, y, z] = self.origin;
        let drop = if self.sitting { 0.08 } else { 0.0 };
        let mut out = Vec::new();
        push_box(
            &mut out,
            [x, y + 0.22 - drop, z],
            [0.26, 0.1, 0.1],
            [0.74, 0.26, 0.12],
        );
        push_box(
            &mut out,
            [x + 0.08, y + 0.24 - drop, z + 0.04],
            [0.12, 0.06, 0.06],
            [0.95, 0.92, 0.84],
        );
        push_box(
            &mut out,
            [x + 0.22, y + 0.38 - drop, z],
            [0.05, 0.08, 0.04],
            [0.16, 0.1, 0.08],
        );
        push_box(
            &mut out,
            [x - 0.28, y + 0.28 - drop, z],
            [0.08, 0.05, 0.05],
            [0.74, 0.26, 0.12],
        );
        push_box(
            &mut out,
            [x - 0.36, y + 0.3 - drop, z],
            [0.04, 0.04, 0.04],
            [0.96, 0.94, 0.88],
        );
        if let Some(id) = self.held {
            push_box(
                &mut out,
                [x + 0.28, y + 0.22 - drop, z],
                [0.05, 0.05, 0.05],
                crate::palette::color(id, 0),
            );
        }
        yaw_mesh(&mut out, self.origin, self.yaw);
        out
    }
}

/// A knock from a goat ram.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Butt {
    pub dx: f32,
    pub dz: f32,
    pub lift: f32,
}

/// Shove `at` away from a ram. Misses when the body is far or on top of the goat.
pub fn goat_shove(from: [f32; 3], at: [f32; 3]) -> Option<Butt> {
    let dx = at[0] - from[0];
    let dy = at[1] - from[1];
    let dz = at[2] - from[2];
    let dist2 = dx * dx + dy * dy + dz * dz;
    if !(0.04..=6.25).contains(&dist2) {
        return None;
    }
    let dist = dist2.sqrt();
    Some(Butt {
        dx: dx / dist * 0.85,
        dz: dz / dist * 0.85,
        lift: 0.28,
    })
}

fn bump_origin(origin: &mut [f32; 3], loose: &mut u32, from: [f32; 3]) -> bool {
    let Some(butt) = goat_shove(from, *origin) else {
        return false;
    };
    origin[0] += butt.dx;
    origin[2] += butt.dz;
    *loose = 15;
    true
}

/// One finished charge.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ram {
    pub from: [f32; 3],
    pub horn: bool,
}

const GOAT_HOME: [f32; 3] = [-23.0, 8.0, 30.0];
const RAM_COOL: u32 = 24;
const RAM_RUN: u32 = 6;
/// Solid hits between goat horns.
const HORN_EVERY: u32 = 4;

pub struct Goat {
    origin: [f32; 3],
    yaw: f32,
    steps: u32,
    charge: u32,
    cool: u32,
    horns: u32,
    loose: u32,
}

impl Goat {
    pub fn new() -> Self {
        Self {
            origin: GOAT_HOME,
            yaw: 0.0,
            steps: 0,
            charge: 0,
            cool: 0,
            horns: 0,
            loose: 0,
        }
    }

    pub fn origin(&self) -> [f32; 3] {
        self.origin
    }

    pub fn shove(&mut self, burst: [f32; 3]) -> bool {
        let Some(gust) = crate::body::wind_push(burst, self.origin) else {
            return false;
        };
        self.origin[0] += gust.dx;
        self.origin[2] += gust.dz;
        self.loose = 40;
        true
    }

    /// Pace the ridge, then charge. `player` is the feet position.
    pub fn tick(&mut self, count: u32, player: [f32; 3], world: &Cluster) -> Option<Ram> {
        let mut ram = None;
        for _ in 0..count {
            if self.loose > 0 {
                self.loose -= 1;
                continue;
            }
            if self.charge > 0 {
                self.rush(player[0], player[2], world);
                self.charge -= 1;
                if self.charge == 0 {
                    let horn = self.struck(player, world);
                    ram = Some(Ram {
                        from: self.origin,
                        horn,
                    });
                    self.cool = RAM_COOL;
                }
                continue;
            }
            if self.cool > 0 {
                self.cool -= 1;
                self.pace(world);
                continue;
            }
            let dx = player[0] - self.origin[0];
            let dz = player[2] - self.origin[2];
            let dist = (dx * dx + dz * dz).sqrt();
            if dist < 8.0 {
                self.charge = RAM_RUN;
            } else {
                self.pace(world);
            }
        }
        ram
    }

    fn pace(&mut self, world: &Cluster) {
        let from_x = self.origin[0];
        let from_z = self.origin[2];
        self.steps = self.steps.wrapping_add(1);
        let next = goat_point(self.steps);
        slide_land(&mut self.origin, next, world, 2);
        face_box(
            &mut self.yaw,
            self.origin[0] - from_x,
            self.origin[2] - from_z,
        );
    }

    fn rush(&mut self, x: f32, z: f32, world: &Cluster) {
        let dx = x - self.origin[0];
        let dz = z - self.origin[2];
        let dist = (dx * dx + dz * dz).sqrt();
        if dist < 0.05 {
            return;
        }
        let step = 0.55_f32.min(dist);
        face_box(&mut self.yaw, dx, dz);
        let next = [
            self.origin[0] + dx / dist * step,
            self.origin[1],
            self.origin[2] + dz / dist * step,
        ];
        slide_land(&mut self.origin, next, world, 2);
    }

    /// A full cube at head height ahead of the charge. Every fourth one drops a horn.
    fn struck(&mut self, player: [f32; 3], world: &Cluster) -> bool {
        let dx = player[0] - self.origin[0];
        let dz = player[2] - self.origin[2];
        let dist = (dx * dx + dz * dz).sqrt();
        let (ux, uz) = if dist < 0.05 {
            (1.0, 0.0)
        } else {
            (dx / dist, dz / dist)
        };
        let ix = (self.origin[0] + ux * 0.8).floor() as i32;
        let iy = self.origin[1].floor() as i32;
        let iz = (self.origin[2] + uz * 0.8).floor() as i32;
        let block = world.block_at(ix, iy, iz);
        if block.is_air() || block.id == "water" || !rebuild_world::shape_of(block).is_full_cube() {
            return false;
        }
        self.horns = self.horns.saturating_add(1);
        self.horns % HORN_EVERY == 0
    }

    pub fn bounds(&self) -> ([f32; 3], [f32; 3]) {
        let [x, y, z] = self.origin;
        ([x - 0.3, y, z - 0.22], [x + 0.3, y + 0.9, z + 0.22])
    }

    pub fn vertices(&self) -> Vec<Vertex> {
        let [x, y, z] = self.origin;
        let mut out = Vec::new();
        push_box(
            &mut out,
            [x, y + 0.38, z],
            [0.22, 0.16, 0.12],
            [0.86, 0.84, 0.78],
        );
        push_box(
            &mut out,
            [x + 0.16, y + 0.62, z],
            [0.1, 0.1, 0.08],
            [0.8, 0.78, 0.72],
        );
        push_box(
            &mut out,
            [x + 0.22, y + 0.78, z - 0.04],
            [0.03, 0.1, 0.03],
            [0.52, 0.4, 0.26],
        );
        push_box(
            &mut out,
            [x + 0.22, y + 0.78, z + 0.04],
            [0.03, 0.1, 0.03],
            [0.52, 0.4, 0.26],
        );
        push_box(
            &mut out,
            [x + 0.2, y + 0.5, z],
            [0.04, 0.06, 0.03],
            [0.22, 0.18, 0.14],
        );
        yaw_mesh(&mut out, self.origin, self.yaw);
        out
    }
}

fn goat_point(steps: u32) -> [f32; 3] {
    let cycle = 16u32;
    let t = (steps % cycle) as f32 / cycle as f32;
    let along = if t < 0.5 { t * 2.0 } else { 2.0 * (1.0 - t) };
    [
        GOAT_HOME[0] - along * 0.7,
        GOAT_HOME[1],
        GOAT_HOME[2] + along * 0.5,
    ]
}

fn pig_ground(world: &Cluster, x: f32, z: f32, feet_y: f32) -> bool {
    let ix = x.floor() as i32;
    let iz = z.floor() as i32;
    let iy = (feet_y - 0.05).floor() as i32;
    if iy < 0 {
        return false;
    }
    let below = world.block_at(ix, iy, iz);
    if below.is_air() || below.id == "water" {
        return false;
    }
    world.block_at(ix, iy + 1, iz).is_air()
}

const GUEST_HEARTS: i32 = rebuild_world::ScriptEntityDef::DEFAULT_HEARTS;
const GUEST_HURTBOX: f32 = rebuild_world::ScriptEntityDef::DEFAULT_HURTBOX;
const GUEST_SPEED: f32 = rebuild_world::ScriptEntityDef::DEFAULT_SPEED;

/// A mob a script registered. Default pink boxes; optional Blockbench model and hostile AI.
pub struct Guest {
    id: String,
    origin: [f32; 3],
    model: Option<String>,
    hostile: bool,
    hearts: i32,
    max_hearts: i32,
    hurtbox: f32,
    speed: f32,
    yaw: f32,
    pose: f32,
    swing: u32,
    moving: bool,
    attacking: bool,
}

impl Guest {
    fn bare(def: rebuild_world::ScriptEntityDef, origin: [f32; 3]) -> Self {
        let hearts = def.hearts.max(1);
        Self {
            id: def.id,
            origin,
            model: def.model,
            hostile: def.hostile,
            hearts,
            max_hearts: hearts,
            hurtbox: def.hurtbox.max(0.2),
            speed: def.speed.max(0.25),
            yaw: 0.0,
            pose: 0.0,
            swing: 0,
            moving: false,
            attacking: false,
        }
    }

    fn stub_def(id: String) -> rebuild_world::ScriptEntityDef {
        rebuild_world::ScriptEntityDef {
            id,
            model: None,
            hostile: false,
            hearts: GUEST_HEARTS,
            hurtbox: GUEST_HURTBOX,
            speed: GUEST_SPEED,
        }
    }

    /// One body per id. The first stands at x = 10.5, z = 12.5. The next ones step east.
    pub fn line(ids: &[String]) -> Vec<Self> {
        Self::line_defs(
            &ids.iter()
                .map(|id| Self::stub_def(id.clone()))
                .collect::<Vec<_>>(),
        )
    }

    /// Same as [`Self::line`], with per-id attrs from the pack.
    pub fn line_defs(defs: &[rebuild_world::ScriptEntityDef]) -> Vec<Self> {
        defs.iter()
            .enumerate()
            .map(|(index, def)| {
                Self::bare(
                    def.clone(),
                    [10.5 + index as f32 * 1.25, 1.0, 12.5],
                )
            })
            .collect()
    }

    /// The registered line, then each body from `entity.spawn`. A spawn stands in the middle of its cell.
    pub fn placed(ids: &[String], spawns: &[(String, i32, i32, i32)]) -> Vec<Self> {
        Self::placed_defs(
            &ids.iter()
                .map(|id| Self::stub_def(id.clone()))
                .collect::<Vec<_>>(),
            spawns,
        )
    }

    /// Same as [`Self::placed`], carrying attrs from `Pack::script_entity_defs`.
    pub fn placed_defs(
        defs: &[rebuild_world::ScriptEntityDef],
        spawns: &[(String, i32, i32, i32)],
    ) -> Vec<Self> {
        let mut guests = Self::line_defs(defs);
        for (id, x, y, z) in spawns {
            let def = defs
                .iter()
                .find(|known| known.id == *id)
                .cloned()
                .unwrap_or_else(|| Self::stub_def(id.clone()));
            guests.push(Self::bare(
                def,
                [*x as f32 + 0.5, *y as f32, *z as f32 + 0.5],
            ));
        }
        guests
    }

    /// Append one live body from a hook-time `entity.spawn`.
    pub fn push_live(
        guests: &mut Vec<Self>,
        def: rebuild_world::ScriptEntityDef,
        x: i32,
        y: i32,
        z: i32,
    ) {
        let origin = [x as f32 + 0.5, y as f32, z as f32 + 0.5];
        if guests.iter().any(|guest| {
            guest.id == def.id
                && (guest.origin[0] - origin[0]).abs() < 0.01
                && (guest.origin[2] - origin[2]).abs() < 0.01
        }) {
            return;
        }
        guests.push(Self::bare(def, origin));
    }

    /// Remove one live body from a hook-time `entity.despawn`.
    pub fn take_live(guests: &mut Vec<Self>, id: &str, x: i32, y: i32, z: i32) -> bool {
        let origin = [x as f32 + 0.5, y as f32, z as f32 + 0.5];
        if let Some(index) = guests.iter().position(|guest| {
            guest.id == id
                && (guest.origin[0] - origin[0]).abs() < 0.01
                && (guest.origin[2] - origin[2]).abs() < 0.01
        }) {
            guests.remove(index);
            true
        } else {
            false
        }
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn origin(&self) -> [f32; 3] {
        self.origin
    }

    pub fn hostile(&self) -> bool {
        self.hostile
    }

    pub fn alive(&self) -> bool {
        self.hearts > 0
    }

    /// True when the look ray passes close to the body.
    pub fn aimed(&self, eye: [f32; 3], forward: [f32; 3]) -> bool {
        if self.hearts <= 0 {
            return false;
        }
        let [x, y, z] = self.origin;
        let center = [x, y + 1.0, z];
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
        let radius = self.hurtbox;
        dx * dx + dy * dy + dz * dz < radius * radius
    }

    /// Spend hearts. True when this hit drops the guest.
    pub fn hurt(&mut self, amount: i32) -> bool {
        if self.hearts <= 0 || amount <= 0 {
            return false;
        }
        self.hearts -= amount;
        if self.hearts > 0 {
            return false;
        }
        self.hearts = 0;
        true
    }

    /// Restore hearts after a cancelled `entity_death` (keep the body).
    pub fn revive(&mut self) {
        self.hearts = self.max_hearts.max(1);
        self.moving = false;
        self.attacking = false;
    }

    /// Chase the player when hostile. True when a bite is due.
    pub fn tick_near(&mut self, count: u32, player: [f32; 2], clear: bool, world: &Cluster) -> bool {
        if self.hearts <= 0 {
            return false;
        }
        if !self.hostile {
            self.pose += 0.05 * count as f32;
            self.moving = false;
            self.attacking = false;
            return false;
        }
        let step = STEP * self.speed;
        let mut bite = false;
        let mut stepped = false;
        let mut swinging = false;
        for _ in 0..count {
            self.pose += 0.05;
            let feet_y = self.origin[1].floor() as i32;
            let dx = player[0] - self.origin[0];
            let dz = player[1] - self.origin[2];
            let dist = (dx * dx + dz * dz).sqrt();
            if dist > REACH {
                let from = [self.origin[0], self.origin[2]];
                if let Some(next) =
                    land_step(world, from, player, step.min(dist - REACH), feet_y, 2)
                {
                    face_forward(&mut self.yaw, next[0] - from[0], next[1] - from[1]);
                    self.origin[0] = next[0];
                    self.origin[2] = next[1];
                    stepped = true;
                }
            } else if clear {
                face_forward(&mut self.yaw, dx, dz);
                swinging = true;
                self.swing = self.swing.wrapping_add(1);
                if self.swing % BITE_EVERY == 0 {
                    bite = true;
                }
            }
            settle_land(&mut self.origin, world);
        }
        self.moving = stepped;
        self.attacking = swinging;
        bite
    }

    fn clip(&self) -> &'static str {
        if self.attacking {
            "attack"
        } else if self.moving {
            "walk"
        } else {
            "idle"
        }
    }

    pub fn vertices(&self) -> Vec<Vertex> {
        if self.hearts <= 0 {
            return Vec::new();
        }
        if let Some(path) = &self.model {
            if let Some(figure) = guest_figure(path) {
                return crate::blockbench::mesh(
                    figure,
                    self.clip(),
                    self.pose,
                    self.yaw,
                    self.origin,
                    &[],
                );
            }
        }
        let [x, y, z] = self.origin;
        let mut out = Vec::new();
        push_box(
            &mut out,
            [x, y + 0.45, z],
            [0.22, 0.28, 0.22],
            [0.86, 0.28, 0.42],
        );
        push_box(
            &mut out,
            [x, y + 0.86, z],
            [0.12, 0.12, 0.12],
            [0.96, 0.62, 0.70],
        );
        out
    }
}

fn guest_figure(path: &str) -> Option<&'static crate::blockbench::Figure> {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    static FIGURES: OnceLock<Mutex<HashMap<String, &'static crate::blockbench::Figure>>> =
        OnceLock::new();
    let map = FIGURES.get_or_init(|| Mutex::new(HashMap::new()));
    let mut guard = map.lock().ok()?;
    if let Some(figure) = guard.get(path) {
        return Some(*figure);
    }
    let text = rebuild_world::asset_text(path)?;
    let figure = crate::blockbench::parse(&text).ok()?;
    let leaked: &'static crate::blockbench::Figure = Box::leak(Box::new(figure));
    guard.insert(path.to_string(), leaked);
    Some(leaked)
}

fn pig_point(steps: u32) -> [f32; 3] {
    let leg = (steps / STEPS_PER_LEG) % 4;
    let along = (steps % STEPS_PER_LEG) as f32 / STEPS_PER_LEG as f32 * LEG;
    let (x, z) = match leg {
        0 => (5.0 + along, 9.0),
        1 => (7.0, 9.0 + along),
        2 => (7.0 - along, 11.0),
        _ => (5.0, 11.0 - along),
    };
    [x, 1.0, z]
}

fn horse_point(steps: u32) -> [f32; 3] {
    let leg = (steps / STEPS_PER_LEG) % 4;
    let along = (steps % STEPS_PER_LEG) as f32 / STEPS_PER_LEG as f32 * LEG;
    let (hx, hz) = (HORSE_PAD.0 as f32, HORSE_PAD.1 as f32);
    let (x, z) = match leg {
        0 => (hx - 1.0 + along, hz - 1.0),
        1 => (hx + 1.0, hz - 1.0 + along),
        2 => (hx + 1.0 - along, hz + 1.0),
        _ => (hx - 1.0, hz + 1.0 - along),
    };
    [x, 1.0, z]
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

/// Village green pad — same cell as `rebuild_world::VILLAGE_GREEN`.
pub const TRADER_PAD: (i32, i32) = (8, -22);

/// One emerald trade row. Cost then payout.
#[derive(Clone, Copy, Debug)]
pub struct TradeOffer {
    pub cost: &'static str,
    pub cost_n: u16,
    pub give: &'static str,
    pub give_n: u16,
    pub stock: u16,
}

/// Stock recipes the village trader keeps.
pub const TRADER_OFFERS: [TradeOffer; 3] = [
    TradeOffer {
        cost: "emerald",
        cost_n: 1,
        give: "torch",
        give_n: 8,
        stock: 12,
    },
    TradeOffer {
        cost: "emerald",
        cost_n: 1,
        give: "apple",
        give_n: 4,
        stock: 8,
    },
    TradeOffer {
        cost: "emerald",
        cost_n: 1,
        give: "map",
        give_n: 1,
        stock: 4,
    },
];

/// Idle boxy villager on the village green. Right-click opens trades.
pub struct Trader {
    origin: [f32; 3],
    yaw: f32,
    pose: f32,
    stock: [u16; 3],
}

impl Trader {
    pub fn new() -> Self {
        let (x, z) = TRADER_PAD;
        Self {
            origin: [x as f32 + 0.5, 4.0, z as f32 + 0.5],
            yaw: std::f32::consts::PI,
            pose: 0.0,
            stock: [
                TRADER_OFFERS[0].stock,
                TRADER_OFFERS[1].stock,
                TRADER_OFFERS[2].stock,
            ],
        }
    }

    pub fn origin(&self) -> [f32; 3] {
        self.origin
    }

    pub fn stock(&self) -> [u16; 3] {
        self.stock
    }

    pub fn set_stock(&mut self, stock: [u16; 3]) {
        self.stock = stock;
    }

    /// True when looking at the trader within interact range.
    pub fn aimed(&self, eye: [f32; 3], forward: [f32; 3]) -> bool {
        let [x, y, z] = self.origin;
        let center = [x, y + 0.9, z];
        let to = [center[0] - eye[0], center[1] - eye[1], center[2] - eye[2]];
        let along = to[0] * forward[0] + to[1] * forward[1] + to[2] * forward[2];
        if !(0.0..=5.0).contains(&along) {
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
        dx * dx + dy * dy + dz * dz < 1.1
    }

    /// Idle bob; face the player when nearby. Settle onto solid ground.
    pub fn tick(&mut self, count: u32, player: [f32; 2], world: &Cluster) {
        for _ in 0..count {
            self.pose += 0.04;
            settle_land(&mut self.origin, world);
            let dx = player[0] - self.origin[0];
            let dz = player[1] - self.origin[2];
            if dx * dx + dz * dz < 64.0 {
                face_forward(&mut self.yaw, dx, dz);
            }
        }
    }

    pub fn bounds(&self) -> ([f32; 3], [f32; 3]) {
        let [x, y, z] = self.origin;
        ([x - 0.35, y, z - 0.35], [x + 0.35, y + 1.85, z + 0.35])
    }

    pub fn vertices(&self) -> Vec<Vertex> {
        // Prefer Whimscape villager sheet paint; fall back to average-tint boxes.
        if pack_entity_painted("villager") {
            return crate::blockbench::mesh(
                villager_figure(),
                "idle",
                self.pose,
                self.yaw,
                self.origin,
                &[],
            );
        }
        let [x, y, z] = self.origin;
        let bob = (self.pose * 2.0).sin() * 0.02;
        let mut out = Vec::new();
        // Villager sheet: head face / body / leg (64×64 humanoid UV).
        let (robe, skin, belt, leg) = match pack_mob_colors(
            "villager",
            &[(8, 8, 16, 16), (20, 20, 28, 32), (4, 20, 8, 32), (4, 20, 8, 32)],
        ) {
            Some(c) => (c[1], c[0], [c[1][0] * 0.7, c[1][1] * 0.65, c[1][2] * 0.5], c[2]),
            None => (
                [0.42, 0.38, 0.55],
                [0.86, 0.68, 0.52],
                [0.55, 0.42, 0.18],
                [0.28, 0.24, 0.36],
            ),
        };
        // Legs.
        push_box(
            &mut out,
            [x - 0.1, y + 0.35 + bob, z],
            [0.1, 0.35, 0.1],
            leg,
        );
        push_box(
            &mut out,
            [x + 0.1, y + 0.35 + bob, z],
            [0.1, 0.35, 0.1],
            leg,
        );
        // Body / robe.
        push_box(
            &mut out,
            [x, y + 0.95 + bob, z],
            [0.28, 0.4, 0.18],
            robe,
        );
        push_box(
            &mut out,
            [x, y + 0.7 + bob, z],
            [0.3, 0.06, 0.2],
            belt,
        );
        // Arms.
        push_box(
            &mut out,
            [x - 0.38, y + 0.95 + bob, z],
            [0.08, 0.32, 0.08],
            robe,
        );
        push_box(
            &mut out,
            [x + 0.38, y + 0.95 + bob, z],
            [0.08, 0.32, 0.08],
            robe,
        );
        // Head + nose.
        push_box(
            &mut out,
            [x, y + 1.5 + bob, z],
            [0.22, 0.22, 0.2],
            skin,
        );
        push_box(
            &mut out,
            [x, y + 1.42 + bob, z + 0.22],
            [0.06, 0.06, 0.08],
            skin,
        );
        yaw_mesh(&mut out, self.origin, self.yaw);
        out
    }
}

/// Village green iron golem pad — east of the trader on the plaza.
pub const IRON_GOLEM_PAD: (i32, i32) = (10, -21);

const IRON_HUNT: f32 = 10.0;
const IRON_REACH: f32 = 1.55;
const IRON_PUNCH_EVERY: u32 = 28;

/// Slow iron guardian on the village green. Punches nearby zombies.
pub struct IronGolem {
    origin: [f32; 3],
    home: [f32; 3],
    yaw: f32,
    pose: f32,
    swing: u32,
    roam: Roam,
    loose: u32,
}

impl IronGolem {
    pub fn new() -> Self {
        let (x, z) = IRON_GOLEM_PAD;
        let home = [x as f32 + 0.5, 4.0, z as f32 + 0.5];
        Self {
            origin: home,
            home,
            yaw: std::f32::consts::PI * 0.25,
            pose: 0.0,
            swing: 0,
            roam: Roam::around(home),
            loose: 0,
        }
    }

    pub fn origin(&self) -> [f32; 3] {
        self.origin
    }

    /// Place the body for shots and tests.
    pub fn place_at(&mut self, at: [f32; 3]) {
        self.origin = at;
        self.home = at;
        self.roam = Roam::around(at);
        self.loose = 0;
        self.swing = 0;
    }

    /// Wander the green slowly; chase and punch a nearby zombie.
    /// True when a punch lands on `foe`.
    pub fn tick(&mut self, count: u32, foe: Option<[f32; 2]>, world: &Cluster) -> bool {
        let mut punch = false;
        for n in 0..count {
            self.pose += 0.03;
            if self.loose > 0 {
                self.loose -= 1;
                settle_land(&mut self.origin, world);
                continue;
            }
            let hunt = foe.filter(|at| {
                let dx = at[0] - self.origin[0];
                let dz = at[1] - self.origin[2];
                dx * dx + dz * dz < IRON_HUNT * IRON_HUNT
            });
            if let Some(at) = hunt {
                if heel_step(&mut self.origin, &mut self.yaw, at, IRON_REACH, world) {
                    self.swing = self.swing.wrapping_add(1);
                    if self.swing % IRON_PUNCH_EVERY == 0 {
                        punch = true;
                    }
                }
            } else if n % 2 == 0 {
                // Half-speed roam so the guard reads as heavy.
                roam_step(
                    &mut self.origin,
                    &mut self.yaw,
                    &mut self.roam,
                    self.home,
                    world,
                );
            } else {
                settle_land(&mut self.origin, world);
            }
        }
        punch
    }

    pub fn bounds(&self) -> ([f32; 3], [f32; 3]) {
        let [x, y, z] = self.origin;
        ([x - 0.45, y, z - 0.45], [x + 0.45, y + 2.35, z + 0.45])
    }

    pub fn vertices(&self) -> Vec<Vertex> {
        let [x, y, z] = self.origin;
        let bob = (self.pose * 1.4).sin() * 0.015;
        // Iron golem sheet is 128×128 — sample torso / leg / vine tuft.
        let (iron, dark, vine) = match pack_mob_colors(
            "iron_golem",
            &[(8, 20, 24, 40), (48, 72, 64, 96), (56, 0, 72, 16)],
        ) {
            Some(c) => (c[0], c[1], c[2]),
            None => (
                [0.46, 0.48, 0.52],
                [0.22, 0.24, 0.28],
                [0.16, 0.58, 0.22],
            ),
        };
        let mut out = Vec::new();
        // Legs.
        push_box(
            &mut out,
            [x - 0.18, y + 0.48 + bob, z],
            [0.16, 0.48, 0.16],
            dark,
        );
        push_box(
            &mut out,
            [x + 0.18, y + 0.48 + bob, z],
            [0.16, 0.48, 0.16],
            dark,
        );
        // Wide torso.
        push_box(
            &mut out,
            [x, y + 1.25 + bob, z],
            [0.48, 0.46, 0.26],
            iron,
        );
        // Long arms.
        push_box(
            &mut out,
            [x - 0.62, y + 1.1 + bob, z],
            [0.14, 0.52, 0.14],
            iron,
        );
        push_box(
            &mut out,
            [x + 0.62, y + 1.1 + bob, z],
            [0.14, 0.52, 0.14],
            iron,
        );
        // Vine trail on the left arm.
        push_box(
            &mut out,
            [x - 0.62, y + 0.72 + bob, z + 0.12],
            [0.06, 0.28, 0.06],
            vine,
        );
        // Squared head + nose.
        push_box(
            &mut out,
            [x, y + 2.05 + bob, z],
            [0.32, 0.32, 0.28],
            iron,
        );
        push_box(
            &mut out,
            [x, y + 1.95 + bob, z + 0.3],
            [0.1, 0.1, 0.1],
            dark,
        );
        // Vine flower tuft.
        push_box(
            &mut out,
            [x + 0.14, y + 2.42 + bob, z],
            [0.1, 0.1, 0.1],
            vine,
        );
        yaw_mesh(&mut out, self.origin, self.yaw);
        out
    }
}

/// Shallow water south of the shipwreck — guardian patrol pad.
pub const GUARDIAN_PAD: (i32, i32) = (
    rebuild_world::SHIPWRECK.0,
    rebuild_world::SHIPWRECK.1 + 3,
);

const GUARDIAN_HEARTS: i32 = 6;
const GUARDIAN_LASER_EVERY: u32 = 32;
const GUARDIAN_SPIKE_EVERY: u32 = 18;
const GUARDIAN_HOVER: f32 = 4.5;
const GUARDIAN_STEP: f32 = 0.07;
const GUARDIAN_HOME_Y: f32 = 3.4;
const GUARDIAN_SPIKE_REACH: f32 = 1.6;

/// What a guardian does this tick when in range.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuardHit {
    Laser,
    Spike,
}

/// Floating underwater hostile south of the shipwreck. Lasers at range, spikes up close.
pub struct Guardian {
    origin: [f32; 3],
    home: [f32; 3],
    swing: u32,
    hearts: i32,
    down: f32,
    loose: u32,
}

impl Guardian {
    pub fn new() -> Self {
        let (x, z) = GUARDIAN_PAD;
        let home = [x as f32 + 0.5, GUARDIAN_HOME_Y, z as f32 + 0.5];
        Self {
            origin: home,
            home,
            swing: 0,
            hearts: GUARDIAN_HEARTS,
            down: 0.0,
            loose: 0,
        }
    }

    pub fn origin(&self) -> [f32; 3] {
        self.origin
    }

    pub fn alive(&self) -> bool {
        self.hearts > 0
    }

    /// Place for shots and tests.
    pub fn place_at(&mut self, at: [f32; 3]) {
        self.origin = at;
        self.home = at;
        self.hearts = GUARDIAN_HEARTS;
        self.down = 0.0;
        self.swing = 0;
        self.loose = 0;
    }

    pub fn aimed(&self, eye: [f32; 3], forward: [f32; 3]) -> bool {
        if self.hearts <= 0 {
            return false;
        }
        let [x, y, z] = self.origin;
        let center = [x, y + 0.45, z];
        let to = [center[0] - eye[0], center[1] - eye[1], center[2] - eye[2]];
        let along = to[0] * forward[0] + to[1] * forward[1] + to[2] * forward[2];
        if !(0.0..=8.0).contains(&along) {
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
        dx * dx + dy * dy + dz * dz < 0.85
    }

    /// Spend hearts. True when this hit drops the guardian.
    pub fn hurt(&mut self, amount: i32) -> bool {
        if self.hearts <= 0 || amount <= 0 {
            return false;
        }
        self.hearts -= amount;
        if self.hearts > 0 {
            return false;
        }
        self.hearts = 0;
        self.down = 9.0;
        true
    }

    /// Return home after a knockdown. True on the frame it rises.
    pub fn rest(&mut self, dt: f32) -> bool {
        if self.hearts > 0 {
            return false;
        }
        self.down -= dt;
        if self.down > 0.0 {
            return false;
        }
        self.hearts = GUARDIAN_HEARTS;
        self.origin = self.home;
        self.swing = 0;
        self.down = 0.0;
        self.loose = 0;
        true
    }

    /// Drift in the shallows. Laser at mid range, spike when close.
    pub fn tick_near(&mut self, count: u32, player: [f32; 3]) -> Option<GuardHit> {
        if self.loose > 0 {
            self.loose = self.loose.saturating_sub(count);
            return None;
        }
        if self.hearts <= 0 {
            return None;
        }
        let mut hit = None;
        for _ in 0..count {
            let dx = player[0] - self.origin[0];
            let dy = player[1] - self.origin[1];
            let dz = player[2] - self.origin[2];
            let dist = (dx * dx + dz * dz).sqrt();
            self.origin[1] = self.home[1] + (self.swing as f32 * 0.12).sin() * 0.14;
            if dist > GUARDIAN_HOVER + 1.0 && dist > 0.05 {
                let step = GUARDIAN_STEP.min(dist - GUARDIAN_HOVER);
                self.origin[0] += dx / dist * step;
                self.origin[2] += dz / dist * step;
            } else if dist < GUARDIAN_HOVER - 1.2 && dist > 0.05 {
                self.origin[0] -= dx / dist * GUARDIAN_STEP;
                self.origin[2] -= dz / dist * GUARDIAN_STEP;
            } else if dist > 16.0 {
                let hx = self.home[0] - self.origin[0];
                let hz = self.home[2] - self.origin[2];
                let hd = (hx * hx + hz * hz).sqrt();
                if hd > 0.05 {
                    self.origin[0] += hx / hd * GUARDIAN_STEP;
                    self.origin[2] += hz / hd * GUARDIAN_STEP;
                }
            }
            // Bob toward the player's eye height a touch.
            if dy.abs() > 0.4 {
                self.origin[1] += dy.signum() * 0.03;
            }
            self.swing = self.swing.wrapping_add(1);
            if dist <= GUARDIAN_SPIKE_REACH && self.swing % GUARDIAN_SPIKE_EVERY == 0 {
                hit = Some(GuardHit::Spike);
            } else if (2.5..12.0).contains(&dist) && self.swing % GUARDIAN_LASER_EVERY == 0 {
                hit = Some(GuardHit::Laser);
            }
        }
        hit
    }

    pub fn bounds(&self) -> ([f32; 3], [f32; 3]) {
        if self.hearts <= 0 {
            return ([0.0, -8.0, 0.0], [0.0, -8.0, 0.0]);
        }
        let [x, y, z] = self.origin;
        ([x - 0.55, y, z - 0.55], [x + 0.55, y + 1.05, z + 0.55])
    }

    pub fn vertices(&self) -> Vec<Vertex> {
        if self.hearts <= 0 {
            return Vec::new();
        }
        let [x, y, z] = self.origin;
        let bob = (self.swing as f32 * 0.15).sin() * 0.04;
        let body = [0.35, 0.62, 0.55];
        let dark = [0.18, 0.38, 0.35];
        let eye = [0.95, 0.85, 0.25];
        let spike = [0.55, 0.78, 0.7];
        let mut out = Vec::new();
        // Fat disc body.
        push_box(
            &mut out,
            [x, y + 0.45 + bob, z],
            [0.48, 0.28, 0.48],
            body,
        );
        // Tail fin.
        push_box(
            &mut out,
            [x, y + 0.45 + bob, z - 0.55],
            [0.08, 0.22, 0.22],
            dark,
        );
        // Spines.
        for (ox, oz) in [(-0.35f32, 0.2), (0.35, 0.2), (0.0, 0.4), (-0.25, -0.25), (0.25, -0.25)]
        {
            push_box(
                &mut out,
                [x + ox, y + 0.85 + bob, z + oz],
                [0.05, 0.22, 0.05],
                spike,
            );
        }
        // Big yellow eye.
        push_box(
            &mut out,
            [x, y + 0.5 + bob, z + 0.42],
            [0.16, 0.16, 0.08],
            eye,
        );
        push_box(
            &mut out,
            [x, y + 0.5 + bob, z + 0.5],
            [0.06, 0.06, 0.04],
            dark,
        );
        out
    }
}

/// Fortress blaze pad — floats above [`rebuild_world::NETHER_FORTRESS_BLAZE`].
pub const BLAZE_PAD: (i32, i32) = (-29, -41);

/// Ghast hover pad — over the open nether pocket chamber (`NETHER_PAD`).
pub const GHAST_PAD: (i32, i32) = rebuild_world::NETHER_PAD;

const BLAZE_HEARTS: i32 = 5;
const BLAZE_SHOOT_EVERY: u32 = 36;
const BLAZE_HOVER: f32 = 5.0;
const BLAZE_STEP: f32 = 0.08;
const BLAZE_HOME_Y: f32 = 5.6;

const GHAST_HEARTS: i32 = 8;
const GHAST_SHOOT_EVERY: u32 = 48;
const GHAST_HOVER: f32 = 8.0;
const GHAST_STEP: f32 = 0.06;
/// Hover just under the pocket ceiling (`NETHER_FORTRESS_FLOOR` + ~2).
const GHAST_HOME_Y: f32 = 6.15;

/// Floating hostile over the nether fortress blaze cage. Shoots fireballs.
pub struct Blaze {
    origin: [f32; 3],
    home: [f32; 3],
    swing: u32,
    hearts: i32,
    down: f32,
    loose: u32,
}

impl Blaze {
    pub fn new() -> Self {
        let (x, z) = BLAZE_PAD;
        let home = [x as f32 + 0.5, BLAZE_HOME_Y, z as f32 + 0.5];
        Self {
            origin: home,
            home,
            swing: 0,
            hearts: BLAZE_HEARTS,
            down: 0.0,
            loose: 0,
        }
    }

    pub fn origin(&self) -> [f32; 3] {
        self.origin
    }

    pub fn alive(&self) -> bool {
        self.hearts > 0
    }

    /// Place for shots and tests.
    pub fn place_at(&mut self, at: [f32; 3]) {
        self.origin = at;
        self.home = at;
        self.hearts = BLAZE_HEARTS;
        self.down = 0.0;
        self.swing = 0;
        self.loose = 0;
    }

    pub fn bump(&mut self, from: [f32; 3]) -> bool {
        if self.hearts <= 0 {
            return false;
        }
        bump_origin(&mut self.origin, &mut self.loose, from)
    }

    /// True when the look ray passes close to the floating body.
    pub fn aimed(&self, eye: [f32; 3], forward: [f32; 3]) -> bool {
        if self.hearts <= 0 {
            return false;
        }
        let [x, y, z] = self.origin;
        let center = [x, y + 0.55, z];
        let to = [center[0] - eye[0], center[1] - eye[1], center[2] - eye[2]];
        let along = to[0] * forward[0] + to[1] * forward[1] + to[2] * forward[2];
        if !(0.0..=7.0).contains(&along) {
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
        dx * dx + dy * dy + dz * dz < 0.7
    }

    /// Spend hearts. True when this hit drops the blaze.
    pub fn hurt(&mut self, amount: i32) -> bool {
        if self.hearts <= 0 || amount <= 0 {
            return false;
        }
        self.hearts -= amount;
        if self.hearts > 0 {
            return false;
        }
        self.hearts = 0;
        self.down = 10.0;
        true
    }

    /// Float up beside the fortress cage.
    pub fn rouse(&mut self, at: [f32; 3]) {
        self.origin = [at[0], at[1] + 1.5, at[2]];
        self.home = self.origin;
        self.hearts = BLAZE_HEARTS;
        self.down = 0.0;
        self.swing = 0;
        self.loose = 0;
    }

    /// Return home after a knockdown. True on the frame it rises.
    pub fn rest(&mut self, dt: f32) -> bool {
        if self.hearts > 0 {
            return false;
        }
        self.down -= dt;
        if self.down > 0.0 {
            return false;
        }
        self.hearts = BLAZE_HEARTS;
        self.origin = self.home;
        self.swing = 0;
        self.down = 0.0;
        true
    }

    /// Drift toward a hover range. True when a fireball is due.
    pub fn tick_near(&mut self, count: u32, player: [f32; 3]) -> bool {
        if self.loose > 0 {
            self.loose = self.loose.saturating_sub(count);
            return false;
        }
        if self.hearts <= 0 {
            return false;
        }
        let mut shot = false;
        for _ in 0..count {
            let dx = player[0] - self.origin[0];
            let dz = player[2] - self.origin[2];
            let dist = (dx * dx + dz * dz).sqrt();
            // Bob gently while holding altitude near the cage.
            self.origin[1] = self.home[1] + (self.swing as f32 * 0.11).sin() * 0.12;
            if dist > BLAZE_HOVER + 1.0 && dist > 0.05 {
                let step = BLAZE_STEP.min(dist - BLAZE_HOVER);
                self.origin[0] += dx / dist * step;
                self.origin[2] += dz / dist * step;
            } else if dist < BLAZE_HOVER - 1.5 && dist > 0.05 {
                self.origin[0] -= dx / dist * BLAZE_STEP;
                self.origin[2] -= dz / dist * BLAZE_STEP;
            } else if dist > 14.0 {
                // Drift home when the player is far.
                let hx = self.home[0] - self.origin[0];
                let hz = self.home[2] - self.origin[2];
                let hd = (hx * hx + hz * hz).sqrt();
                if hd > 0.05 {
                    self.origin[0] += hx / hd * BLAZE_STEP;
                    self.origin[2] += hz / hd * BLAZE_STEP;
                }
            }
            self.swing = self.swing.wrapping_add(1);
            if (2.0..16.0).contains(&dist) && self.swing % BLAZE_SHOOT_EVERY == 0 {
                shot = true;
            }
        }
        shot
    }

    pub fn bounds(&self) -> ([f32; 3], [f32; 3]) {
        if self.hearts <= 0 {
            return ([0.0, -8.0, 0.0], [0.0, -8.0, 0.0]);
        }
        let [x, y, z] = self.origin;
        ([x - 0.35, y, z - 0.35], [x + 0.35, y + 1.35, z + 0.35])
    }

    pub fn vertices(&self) -> Vec<Vertex> {
        if self.hearts <= 0 {
            return Vec::new();
        }
        let [x, y, z] = self.origin;
        let bob = (self.swing as f32 * 0.14).sin() * 0.04;
        let body = [0.92, 0.72, 0.18];
        let dark = [0.55, 0.28, 0.08];
        let flame = [1.0, 0.42, 0.08];
        let mut out = Vec::new();
        // Core body.
        push_box(
            &mut out,
            [x, y + 0.55 + bob, z],
            [0.28, 0.38, 0.22],
            body,
        );
        // Head nub.
        push_box(
            &mut out,
            [x, y + 1.05 + bob, z],
            [0.22, 0.18, 0.2],
            dark,
        );
        // Outer spinning rods (four yellow sticks).
        let spin = self.swing as f32 * 0.18;
        for i in 0..4 {
            let ang = spin + i as f32 * std::f32::consts::FRAC_PI_2;
            let rx = ang.cos() * 0.42;
            let rz = ang.sin() * 0.42;
            push_box(
                &mut out,
                [x + rx, y + 0.55 + bob, z + rz],
                [0.05, 0.32, 0.05],
                flame,
            );
        }
        // Ember underglow (blaze).
        push_box(
            &mut out,
            [x, y + 0.12 + bob, z],
            [0.18, 0.08, 0.18],
            flame,
        );
        out
    }
}

/// Large floating ghast over the nether pocket. Shoots impact fireballs.
pub struct Ghast {
    origin: [f32; 3],
    home: [f32; 3],
    swing: u32,
    hearts: i32,
    down: f32,
    loose: u32,
}

impl Ghast {
    pub fn new() -> Self {
        let (x, z) = GHAST_PAD;
        let home = [x as f32 + 0.5, GHAST_HOME_Y, z as f32 + 0.5];
        Self {
            origin: home,
            home,
            swing: 0,
            hearts: GHAST_HEARTS,
            down: 0.0,
            loose: 0,
        }
    }

    pub fn origin(&self) -> [f32; 3] {
        self.origin
    }

    pub fn alive(&self) -> bool {
        self.hearts > 0
    }

    pub fn place_at(&mut self, at: [f32; 3]) {
        self.origin = at;
        self.home = at;
        self.hearts = GHAST_HEARTS;
        self.down = 0.0;
        self.swing = 0;
        self.loose = 0;
    }

    pub fn bump(&mut self, from: [f32; 3]) -> bool {
        if self.hearts <= 0 {
            return false;
        }
        bump_origin(&mut self.origin, &mut self.loose, from)
    }

    pub fn aimed(&self, eye: [f32; 3], forward: [f32; 3]) -> bool {
        if self.hearts <= 0 {
            return false;
        }
        let [x, y, z] = self.origin;
        let center = [x, y + 1.1, z];
        let to = [center[0] - eye[0], center[1] - eye[1], center[2] - eye[2]];
        let along = to[0] * forward[0] + to[1] * forward[1] + to[2] * forward[2];
        if !(0.0..=14.0).contains(&along) {
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
        dx * dx + dy * dy + dz * dz < 2.4
    }

    /// Spend hearts. True when this hit drops the ghast.
    pub fn hurt(&mut self, amount: i32) -> bool {
        if self.hearts <= 0 || amount <= 0 {
            return false;
        }
        self.hearts -= amount;
        if self.hearts > 0 {
            return false;
        }
        self.hearts = 0;
        self.down = 14.0;
        true
    }

    pub fn rest(&mut self, dt: f32) -> bool {
        if self.hearts > 0 {
            return false;
        }
        self.down -= dt;
        if self.down > 0.0 {
            return false;
        }
        self.hearts = GHAST_HEARTS;
        self.origin = self.home;
        self.swing = 0;
        self.down = 0.0;
        true
    }

    /// Drift and bob. True when a fireball is due.
    pub fn tick_near(&mut self, count: u32, player: [f32; 3]) -> bool {
        if self.loose > 0 {
            self.loose = self.loose.saturating_sub(count);
            return false;
        }
        if self.hearts <= 0 {
            return false;
        }
        let mut shot = false;
        for _ in 0..count {
            let dx = player[0] - self.origin[0];
            let dz = player[2] - self.origin[2];
            let dist = (dx * dx + dz * dz).sqrt();
            self.origin[1] = self.home[1] + (self.swing as f32 * 0.08).sin() * 0.22;
            if dist > GHAST_HOVER + 2.0 && dist > 0.05 {
                let step = GHAST_STEP.min(dist - GHAST_HOVER);
                self.origin[0] += dx / dist * step;
                self.origin[2] += dz / dist * step;
            } else if dist < GHAST_HOVER - 2.5 && dist > 0.05 {
                self.origin[0] -= dx / dist * GHAST_STEP;
                self.origin[2] -= dz / dist * GHAST_STEP;
            } else if dist > 22.0 {
                let hx = self.home[0] - self.origin[0];
                let hz = self.home[2] - self.origin[2];
                let hd = (hx * hx + hz * hz).sqrt();
                if hd > 0.05 {
                    self.origin[0] += hx / hd * GHAST_STEP;
                    self.origin[2] += hz / hd * GHAST_STEP;
                }
            }
            self.swing = self.swing.wrapping_add(1);
            if (4.0..20.0).contains(&dist) && self.swing % GHAST_SHOOT_EVERY == 0 {
                shot = true;
            }
        }
        shot
    }

    pub fn bounds(&self) -> ([f32; 3], [f32; 3]) {
        if self.hearts <= 0 {
            return ([0.0, -8.0, 0.0], [0.0, -8.0, 0.0]);
        }
        let [x, y, z] = self.origin;
        ([x - 0.9, y, z - 0.9], [x + 0.9, y + 1.9, z + 0.9])
    }

    pub fn vertices(&self) -> Vec<Vertex> {
        if self.hearts <= 0 {
            return Vec::new();
        }
        // Prefer Whimscape ghast sheet paint; fall back to cream procedural face.
        if pack_entity_painted("ghast") {
            let bob = (self.swing as f32 * 0.1).sin() * 0.06;
            let feet = [self.origin[0], self.origin[1] + bob, self.origin[2]];
            return crate::blockbench::mesh(ghast_figure(), "idle", 0.0, 0.0, feet, &[]);
        }
        let [x, y, z] = self.origin;
        let bob = (self.swing as f32 * 0.1).sin() * 0.06;
        // Parchment body + cool block light (avoids lantern warm-pulse dimming).
        let body = [1.0, 0.95, 0.88];
        let body_glow = [0.55, 0.6, 0.85];
        let shade = [0.78, 0.7, 0.58];
        let shade_glow = [0.4, 0.42, 0.55];
        let tear = [0.45, 0.88, 1.0];
        let eye = [0.08, 0.06, 0.06];
        let mouth = [0.7, 0.18, 0.2];
        let mut out = Vec::new();
        // Big soft cube body (fits under the pocket ceiling).
        push_box_lit(
            &mut out,
            [x, y + 0.95 + bob, z],
            [0.82, 0.82, 0.82],
            body,
            body_glow,
        );
        // Face — eyes + open mouth toward +Z.
        push_box_lit(
            &mut out,
            [x - 0.28, y + 1.15 + bob, z + 0.8],
            [0.12, 0.16, 0.06],
            eye,
            eye,
        );
        push_box_lit(
            &mut out,
            [x + 0.28, y + 1.15 + bob, z + 0.8],
            [0.12, 0.16, 0.06],
            eye,
            eye,
        );
        push_box_lit(
            &mut out,
            [x, y + 0.72 + bob, z + 0.82],
            [0.24, 0.14, 0.06],
            mouth,
            mouth,
        );
        // Tentacles.
        for (ox, oz, len) in [
            (-0.48f32, -0.3, 0.5),
            (-0.18, -0.48, 0.62),
            (0.18, -0.48, 0.58),
            (0.48, -0.3, 0.45),
            (0.0, 0.12, 0.4),
        ] {
            push_box_lit(
                &mut out,
                [x + ox, y + 0.12 + bob - len * 0.35, z + oz],
                [0.09, len * 0.5, 0.09],
                shade,
                shade_glow,
            );
        }
        // Tear glint under the eye when about to shoot.
        if self.swing % GHAST_SHOOT_EVERY > GHAST_SHOOT_EVERY - 8 {
            push_box_lit(
                &mut out,
                [x, y + 0.92 + bob, z + 0.86],
                [0.08, 0.1, 0.05],
                tear,
                tear,
            );
        }
        out
    }
}

/// Like [`push_box`], but keeps a separate block-light channel (fireball-style).
fn push_box_lit(
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

/// Magma cube pad — hops near the SE nether-pocket lava.
pub const MAGMA_CUBE_PAD: (i32, i32) = (
    rebuild_world::NETHER_PAD.0 + 2,
    rebuild_world::NETHER_PAD.1 - 2,
);

const MAGMA_BIG: u8 = 2;
const MAGMA_SMALL: u8 = 1;
const MAGMA_BIG_HEARTS: i32 = 6;
const MAGMA_SMALL_HEARTS: i32 = 3;
const MAGMA_HOP_EVERY: u32 = 18;
const MAGMA_HUNT: f32 = 10.0;
const MAGMA_REACH: f32 = 1.35;
const MAGMA_STEP: f32 = 0.55;

/// Result of damaging a magma cube under the crosshair.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MagmaHit {
    /// Cubes took damage but still live.
    Hurt,
    /// A big cube died and split into smaller ones.
    Split,
    /// A small cube died — hand over magma cream.
    Cream,
}

struct MagmaBody {
    origin: [f32; 3],
    size: u8,
    hearts: i32,
    hop: u32,
    loose: u32,
}

/// Hopping nether cubes. A big one splits once into smalls; smalls drop cream.
pub struct MagmaCubes {
    cubes: Vec<MagmaBody>,
    home: [f32; 3],
    down: f32,
}

impl MagmaCubes {
    pub fn new() -> Self {
        let (x, z) = MAGMA_CUBE_PAD;
        let floor = f32::from(rebuild_world::NETHER_FORTRESS_FLOOR);
        let home = [x as f32 + 0.5, floor, z as f32 + 0.5];
        let mut pack = Self {
            cubes: Vec::new(),
            home,
            down: 0.0,
        };
        pack.spawn_big(home);
        pack
    }

    fn spawn_big(&mut self, at: [f32; 3]) {
        self.cubes.clear();
        self.cubes.push(MagmaBody {
            origin: at,
            size: MAGMA_BIG,
            hearts: MAGMA_BIG_HEARTS,
            hop: 0,
            loose: 0,
        });
        self.down = 0.0;
    }

    pub fn place_at(&mut self, at: [f32; 3]) {
        self.home = at;
        self.spawn_big(at);
    }

    pub fn alive(&self) -> bool {
        !self.cubes.is_empty()
    }

    pub fn origin(&self) -> [f32; 3] {
        self.cubes
            .first()
            .map(|c| c.origin)
            .unwrap_or(self.home)
    }

    /// Closest living cube under the look ray.
    pub fn aimed_index(&self, eye: [f32; 3], forward: [f32; 3]) -> Option<usize> {
        let mut best: Option<(usize, f32)> = None;
        for (index, cube) in self.cubes.iter().enumerate() {
            let half = magma_half(cube.size);
            let center = [
                cube.origin[0],
                cube.origin[1] + half,
                cube.origin[2],
            ];
            let to = [
                center[0] - eye[0],
                center[1] - eye[1],
                center[2] - eye[2],
            ];
            let along = to[0] * forward[0] + to[1] * forward[1] + to[2] * forward[2];
            if !(0.0..=8.0).contains(&along) {
                continue;
            }
            let closest = [
                eye[0] + forward[0] * along,
                eye[1] + forward[1] * along,
                eye[2] + forward[2] * along,
            ];
            let dx = closest[0] - center[0];
            let dy = closest[1] - center[1];
            let dz = closest[2] - center[2];
            let reach = half + 0.35;
            let dist2 = dx * dx + dy * dy + dz * dz;
            if dist2 > reach * reach {
                continue;
            }
            if best.is_none_or(|(_, so_far)| along < so_far) {
                best = Some((index, along));
            }
        }
        best.map(|(index, _)| index)
    }

    pub fn aimed(&self, eye: [f32; 3], forward: [f32; 3]) -> bool {
        self.aimed_index(eye, forward).is_some()
    }

    /// Spend hearts on the aimed cube. Splits a big one; cream from a small.
    pub fn hurt_aimed(
        &mut self,
        eye: [f32; 3],
        forward: [f32; 3],
        amount: i32,
    ) -> Option<MagmaHit> {
        let index = self.aimed_index(eye, forward)?;
        self.hurt_at(index, amount)
    }

    pub fn hurt_at(&mut self, index: usize, amount: i32) -> Option<MagmaHit> {
        if amount <= 0 || index >= self.cubes.len() {
            return None;
        }
        let size = self.cubes[index].size;
        let origin = self.cubes[index].origin;
        self.cubes[index].hearts -= amount;
        if self.cubes[index].hearts > 0 {
            self.cubes[index].loose = 8;
            return Some(MagmaHit::Hurt);
        }
        self.cubes.remove(index);
        if size >= MAGMA_BIG {
            // One split level — two smalls beside the corpse.
            self.cubes.push(MagmaBody {
                origin: [origin[0] - 0.55, origin[1], origin[2] + 0.2],
                size: MAGMA_SMALL,
                hearts: MAGMA_SMALL_HEARTS,
                hop: 4,
                loose: 6,
            });
            self.cubes.push(MagmaBody {
                origin: [origin[0] + 0.55, origin[1], origin[2] - 0.15],
                size: MAGMA_SMALL,
                hearts: MAGMA_SMALL_HEARTS,
                hop: 10,
                loose: 6,
            });
            if self.cubes.is_empty() {
                self.down = 12.0;
            }
            Some(MagmaHit::Split)
        } else {
            if self.cubes.is_empty() {
                self.down = 12.0;
            }
            Some(MagmaHit::Cream)
        }
    }

    /// Hop toward the player. True when a landing slap clips them.
    pub fn tick_near(&mut self, count: u32, player: [f32; 3]) -> bool {
        if self.cubes.is_empty() {
            return false;
        }
        let floor_y = self.home[1];
        let mut slap = false;
        for _ in 0..count {
            for cube in &mut self.cubes {
                if cube.loose > 0 {
                    cube.loose = cube.loose.saturating_sub(1);
                    continue;
                }
                cube.hop = cube.hop.wrapping_add(1);
                let half = magma_half(cube.size);
                // Squash/stretch hop arc.
                let phase = (cube.hop % MAGMA_HOP_EVERY) as f32 / MAGMA_HOP_EVERY as f32;
                let lift = (phase * std::f32::consts::PI).sin() * (0.35 + half * 0.25);
                cube.origin[1] = floor_y + lift;
                if cube.hop % MAGMA_HOP_EVERY != 0 {
                    continue;
                }
                let dx = player[0] - cube.origin[0];
                let dz = player[2] - cube.origin[2];
                let dist = (dx * dx + dz * dz).sqrt();
                if dist > 0.05 && dist < MAGMA_HUNT {
                    let step = MAGMA_STEP.min(dist);
                    cube.origin[0] += dx / dist * step;
                    cube.origin[2] += dz / dist * step;
                }
                let px = player[0] - cube.origin[0];
                let pz = player[2] - cube.origin[2];
                if px * px + pz * pz <= MAGMA_REACH * MAGMA_REACH {
                    slap = true;
                }
            }
        }
        slap
    }

    /// Respawn a big cube at home after knockdown. True on the frame it rises.
    pub fn rest(&mut self, dt: f32) -> bool {
        if !self.cubes.is_empty() {
            return false;
        }
        self.down -= dt;
        if self.down > 0.0 {
            return false;
        }
        self.spawn_big(self.home);
        true
    }

    pub fn bounds(&self) -> ([f32; 3], [f32; 3]) {
        let Some(cube) = self.cubes.first() else {
            return ([0.0, -8.0, 0.0], [0.0, -8.0, 0.0]);
        };
        magma_bounds(cube)
    }

    /// All living AABB pairs for look / projectile tests.
    pub fn all_bounds(&self) -> Vec<([f32; 3], [f32; 3])> {
        self.cubes.iter().map(magma_bounds).collect()
    }

    pub fn vertices(&self) -> Vec<Vertex> {
        let mut out = Vec::new();
        for cube in &self.cubes {
            let half = magma_half(cube.size);
            let [x, y, z] = cube.origin;
            let phase = (cube.hop % MAGMA_HOP_EVERY) as f32 / MAGMA_HOP_EVERY as f32;
            let squash = 1.0 - (phase * std::f32::consts::PI).sin() * 0.12;
            let stretch = 1.0 + (phase * std::f32::consts::PI).sin() * 0.18;
            let body = [0.92, 0.38, 0.12];
            let glow = [0.95, 0.45, 0.08];
            let dark = [0.35, 0.12, 0.05];
            let eye = [0.08, 0.05, 0.04];
            push_box_lit(
                &mut out,
                [x, y + half * stretch * 0.55, z],
                [half * squash, half * stretch, half * squash],
                body,
                glow,
            );
            // Core glow.
            push_box_lit(
                &mut out,
                [x, y + half * stretch * 0.55, z],
                [half * squash * 0.45, half * stretch * 0.45, half * squash * 0.45],
                [1.0, 0.72, 0.25],
                [1.0, 0.55, 0.1],
            );
            // Eyes on +Z.
            let eye_y = y + half * stretch * 0.7;
            let eye_z = z + half * squash * 0.92;
            let eye_x = half * squash * 0.35;
            let eye_h = half * 0.18;
            push_box_lit(
                &mut out,
                [x - eye_x, eye_y, eye_z],
                [eye_h, eye_h, eye_h * 0.4],
                eye,
                dark,
            );
            push_box_lit(
                &mut out,
                [x + eye_x, eye_y, eye_z],
                [eye_h, eye_h, eye_h * 0.4],
                eye,
                dark,
            );
        }
        out
    }
}

fn magma_half(size: u8) -> f32 {
    if size >= MAGMA_BIG {
        0.72
    } else {
        0.38
    }
}

fn magma_bounds(cube: &MagmaBody) -> ([f32; 3], [f32; 3]) {
    let half = magma_half(cube.size);
    let [x, y, z] = cube.origin;
    (
        [x - half, y, z - half],
        [x + half, y + half * 2.0, z + half],
    )
}

/// Stronghold silverfish pad — emerges from the infested hall.
pub const SILVERFISH_PAD: (i32, i32) = rebuild_world::STRONGHOLD_SILVERFISH;

const SILVERFISH_HEARTS: i32 = 4;
const SILVERFISH_HUNT: f32 = 10.0;
const SILVERFISH_REACH: f32 = 1.05;
const SILVERFISH_STEP: f32 = 0.16;
const SILVERFISH_BITE_EVERY: u32 = 16;
const SILVERFISH_MAX: usize = 8;

struct SilverBody {
    origin: [f32; 3],
    yaw: f32,
    hearts: i32,
    swing: u32,
    loose: u32,
}

/// Nest of silverfish that erupt from infested stronghold bricks.
pub struct Silverfish {
    bugs: Vec<SilverBody>,
    home: [f32; 3],
}

impl Silverfish {
    pub fn new() -> Self {
        let (x, z) = SILVERFISH_PAD;
        let floor = f32::from(rebuild_world::STRONGHOLD_LIBRARY_FLOOR);
        Self {
            bugs: Vec::new(),
            home: [x as f32 + 0.5, floor, z as f32 + 0.5],
        }
    }

    pub fn alive(&self) -> bool {
        !self.bugs.is_empty()
    }

    pub fn count(&self) -> usize {
        self.bugs.len()
    }

    pub fn origin(&self) -> [f32; 3] {
        self.bugs
            .first()
            .map(|bug| bug.origin)
            .unwrap_or(self.home)
    }

    /// Place one demo bug for shots / tests.
    pub fn place_at(&mut self, at: [f32; 3]) {
        self.home = at;
        self.bugs.clear();
        self.emerge(at);
    }

    /// Freeze skitter (proof shots).
    pub fn hold_pose(&mut self) {
        for bug in &mut self.bugs {
            bug.loose = 10_000;
        }
    }

    /// Burst a silverfish out of a broken infested brick.
    pub fn emerge(&mut self, at: [f32; 3]) {
        if self.bugs.len() >= SILVERFISH_MAX {
            return;
        }
        let n = self.bugs.len() as f32;
        self.bugs.push(SilverBody {
            origin: [
                at[0] + (n * 0.37).sin() * 0.25,
                at[1],
                at[2] + (n * 0.51).cos() * 0.25,
            ],
            yaw: n * 0.9,
            hearts: SILVERFISH_HEARTS,
            swing: n as u32 * 3,
            loose: 4,
        });
    }

    pub fn aimed_index(&self, eye: [f32; 3], forward: [f32; 3]) -> Option<usize> {
        let mut best: Option<(usize, f32)> = None;
        for (index, bug) in self.bugs.iter().enumerate() {
            let center = [bug.origin[0], bug.origin[1] + 0.2, bug.origin[2]];
            let to = [
                center[0] - eye[0],
                center[1] - eye[1],
                center[2] - eye[2],
            ];
            let along = to[0] * forward[0] + to[1] * forward[1] + to[2] * forward[2];
            if !(0.0..=6.0).contains(&along) {
                continue;
            }
            let closest = [
                eye[0] + forward[0] * along,
                eye[1] + forward[1] * along,
                eye[2] + forward[2] * along,
            ];
            let dx = closest[0] - center[0];
            let dy = closest[1] - center[1];
            let dz = closest[2] - center[2];
            if dx * dx + dy * dy + dz * dz > 0.55 {
                continue;
            }
            if best.is_none_or(|(_, so_far)| along < so_far) {
                best = Some((index, along));
            }
        }
        best.map(|(index, _)| index)
    }

    pub fn aimed(&self, eye: [f32; 3], forward: [f32; 3]) -> bool {
        self.aimed_index(eye, forward).is_some()
    }

    /// Spend hearts on the aimed bug. True when that bug dies.
    pub fn hurt_aimed(&mut self, eye: [f32; 3], forward: [f32; 3], amount: i32) -> bool {
        let Some(index) = self.aimed_index(eye, forward) else {
            return false;
        };
        if amount <= 0 || index >= self.bugs.len() {
            return false;
        }
        self.bugs[index].hearts -= amount;
        if self.bugs[index].hearts > 0 {
            self.bugs[index].loose = 6;
            return false;
        }
        self.bugs.remove(index);
        true
    }

    /// Skitter toward the player. True when a bite is due.
    pub fn tick_near(&mut self, count: u32, player: [f32; 3]) -> bool {
        if self.bugs.is_empty() {
            return false;
        }
        let mut bite = false;
        for _ in 0..count {
            for bug in &mut self.bugs {
                if bug.loose > 0 {
                    bug.loose = bug.loose.saturating_sub(1);
                    continue;
                }
                bug.swing = bug.swing.wrapping_add(1);
                let dx = player[0] - bug.origin[0];
                let dz = player[2] - bug.origin[2];
                let dist = (dx * dx + dz * dz).sqrt();
                if dist > 0.05 && dist < SILVERFISH_HUNT {
                    let step = SILVERFISH_STEP.min(dist);
                    bug.origin[0] += dx / dist * step;
                    bug.origin[2] += dz / dist * step;
                    face_box(&mut bug.yaw, dx, dz);
                }
                if dist <= SILVERFISH_REACH && bug.swing % SILVERFISH_BITE_EVERY == 0 {
                    bite = true;
                }
            }
        }
        bite
    }

    pub fn bounds(&self) -> ([f32; 3], [f32; 3]) {
        let Some(bug) = self.bugs.first() else {
            return ([0.0, -8.0, 0.0], [0.0, -8.0, 0.0]);
        };
        silver_bounds(bug)
    }

    pub fn all_bounds(&self) -> Vec<([f32; 3], [f32; 3])> {
        self.bugs.iter().map(silver_bounds).collect()
    }

    pub fn vertices(&self) -> Vec<Vertex> {
        let mut out = Vec::new();
        for bug in &self.bugs {
            let [x, y, z] = bug.origin;
            let bob = (bug.swing as f32 * 0.45).sin() * 0.03;
            let body = [0.72, 0.74, 0.78];
            let dark = [0.38, 0.4, 0.44];
            let eye = [0.12, 0.08, 0.08];
            // Segmented silver body.
            push_box(
                &mut out,
                [x, y + 0.14 + bob, z],
                [0.22, 0.1, 0.14],
                body,
            );
            push_box(
                &mut out,
                [x + 0.18, y + 0.12 + bob, z],
                [0.12, 0.08, 0.1],
                dark,
            );
            push_box(
                &mut out,
                [x - 0.18, y + 0.12 + bob, z],
                [0.1, 0.07, 0.09],
                dark,
            );
            // Tiny eyes on +Z.
            push_box(
                &mut out,
                [x - 0.06, y + 0.18 + bob, z + 0.12],
                [0.03, 0.03, 0.02],
                eye,
            );
            push_box(
                &mut out,
                [x + 0.06, y + 0.18 + bob, z + 0.12],
                [0.03, 0.03, 0.02],
                eye,
            );
            let _ = bug.yaw;
        }
        out
    }
}

fn silver_bounds(bug: &SilverBody) -> ([f32; 3], [f32; 3]) {
    let [x, y, z] = bug.origin;
    ([x - 0.28, y, z - 0.22], [x + 0.28, y + 0.32, z + 0.22])
}

#[cfg(test)]
mod silverfish_tests {
    use super::*;

    #[test]
    fn silverfish_emerge_from_infested_breaks() {
        let mut nest = Silverfish::new();
        assert!(!nest.alive());
        nest.emerge([30.5, 3.0, -22.5]);
        nest.emerge([31.5, 3.0, -22.5]);
        assert_eq!(nest.count(), 2);
        assert!(nest.alive());
        let eye = [29.5, 3.2, -22.5];
        let forward = [1.0, 0.0, 0.0];
        assert!(nest.aimed(eye, forward));
        assert!(nest.hurt_aimed(eye, forward, 99));
        assert_eq!(nest.count(), 1);
    }
}

/// Nether pocket piglin — gold-ingot barter; hostile without gold armor, angry when struck.
pub const PIGLIN_PAD: (i32, i32) = rebuild_world::NETHER_PIGLIN;

const PIGLIN_HEARTS: i32 = 8;
const PIGLIN_REACH: f32 = 1.4;
const PIGLIN_STEP: f32 = 0.1;
const PIGLIN_SWING_EVERY: u32 = 24;
const PIGLIN_HUNT: f32 = 12.0;
/// Sight range where missing gold armor turns a calm piglin hostile.
const PIGLIN_AGGRO: f32 = 10.0;

/// Loot a gold ingot buys from a calm piglin.
pub const PIGLIN_BARTER: &[&str] = &[
    "ender_pearl",
    "gravel",
    "leather_cap",
    "soul_sand",
    "quartz",
    "blackstone",
    "nether_bricks",
    "fire_charge",
    "obsidian",
    "iron_ore",
];

/// Gold-hungry piglin: neutral in gold armor, hostile without it, enraged when struck.
pub struct Piglin {
    origin: [f32; 3],
    home: [f32; 3],
    yaw: f32,
    pose: f32,
    swing: u32,
    hearts: i32,
    down: f32,
    loose: u32,
    /// Provoked by a player hit — chases even when the player wears gold.
    angry: bool,
    /// Last tick: chasing because provoked or the player lacked gold armor.
    hunting: bool,
    roll: u32,
}

impl Piglin {
    pub fn new() -> Self {
        let (x, z) = PIGLIN_PAD;
        let floor = f32::from(rebuild_world::NETHER_FORTRESS_FLOOR);
        let home = [x as f32 + 0.5, floor, z as f32 + 0.5];
        Self {
            origin: home,
            home,
            yaw: std::f32::consts::PI * 0.75,
            pose: 0.0,
            swing: 0,
            hearts: PIGLIN_HEARTS,
            down: 0.0,
            loose: 0,
            angry: false,
            hunting: false,
            roll: 0,
        }
    }

    pub fn origin(&self) -> [f32; 3] {
        self.origin
    }

    pub fn alive(&self) -> bool {
        self.hearts > 0
    }

    /// Provoked by a hit (blocks barter until rest).
    pub fn angry(&self) -> bool {
        self.angry
    }

    /// Chasing this frame (provoked or player has no gold armor).
    pub fn hunting(&self) -> bool {
        self.hunting
    }

    /// Place for shots and tests.
    pub fn place_at(&mut self, at: [f32; 3]) {
        self.origin = at;
        self.home = at;
        self.hearts = PIGLIN_HEARTS;
        self.down = 0.0;
        self.swing = 0;
        self.loose = 0;
        self.angry = false;
        self.hunting = false;
    }

    /// Spend one gold for the next barter roll. `None` when angry or dead.
    pub fn barter(&mut self) -> Option<&'static str> {
        if self.angry || self.hearts <= 0 || PIGLIN_BARTER.is_empty() {
            return None;
        }
        let id = PIGLIN_BARTER[self.roll as usize % PIGLIN_BARTER.len()];
        self.roll = self.roll.wrapping_add(1);
        Some(id)
    }

    pub fn aimed(&self, eye: [f32; 3], forward: [f32; 3]) -> bool {
        if self.hearts <= 0 {
            return false;
        }
        let [x, y, z] = self.origin;
        let center = [x, y + 0.95, z];
        let to = [center[0] - eye[0], center[1] - eye[1], center[2] - eye[2]];
        let along = to[0] * forward[0] + to[1] * forward[1] + to[2] * forward[2];
        if !(0.0..=5.0).contains(&along) {
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
        dx * dx + dy * dy + dz * dz < 1.0
    }

    /// Spend hearts and turn angry. True when this hit drops the piglin.
    pub fn hurt(&mut self, amount: i32) -> bool {
        if self.hearts <= 0 || amount <= 0 {
            return false;
        }
        self.angry = true;
        self.hunting = true;
        self.hearts -= amount;
        if self.hearts > 0 {
            return false;
        }
        self.hearts = 0;
        self.down = 8.0;
        true
    }

    /// Return home calm after a knockdown. True on the frame it rises.
    pub fn rest(&mut self, dt: f32) -> bool {
        if self.hearts > 0 {
            return false;
        }
        self.down -= dt;
        if self.down > 0.0 {
            return false;
        }
        self.hearts = PIGLIN_HEARTS;
        self.origin = self.home;
        self.swing = 0;
        self.down = 0.0;
        self.loose = 0;
        self.angry = false;
        self.hunting = false;
        true
    }

    pub fn bump(&mut self, from: [f32; 3]) -> bool {
        if self.hearts <= 0 {
            return false;
        }
        bump_origin(&mut self.origin, &mut self.loose, from)
    }

    /// Idle when pacified by gold; chase when provoked or the player lacks gold.
    /// True on a melee hit.
    pub fn tick_near(
        &mut self,
        count: u32,
        player: [f32; 2],
        clear: bool,
        world: &Cluster,
        wearing_gold: bool,
    ) -> bool {
        if self.loose > 0 {
            self.loose = self.loose.saturating_sub(count);
            return false;
        }
        if self.hearts <= 0 {
            self.hunting = false;
            return false;
        }
        let mut hit = false;
        for _ in 0..count {
            self.pose += 0.05;
            settle_land(&mut self.origin, world);
            let dx = player[0] - self.origin[0];
            let dz = player[1] - self.origin[2];
            let dist = (dx * dx + dz * dz).sqrt();
            // Sight aggro: no gold within range → hostile. Gold keeps them calm
            // unless already provoked by a hit.
            self.hunting = self.angry || (!wearing_gold && dist <= PIGLIN_AGGRO);
            if !self.hunting {
                if dist < 8.0 {
                    face_forward(&mut self.yaw, dx, dz);
                }
                continue;
            }
            if dist > PIGLIN_HUNT {
                continue;
            }
            let feet_y = self.origin[1].floor() as i32;
            if dist > PIGLIN_REACH {
                let from = [self.origin[0], self.origin[2]];
                if let Some(next) =
                    land_step(world, from, player, PIGLIN_STEP.min(dist - PIGLIN_REACH), feet_y, 2)
                {
                    face_forward(&mut self.yaw, next[0] - from[0], next[1] - from[1]);
                    self.origin[0] = next[0];
                    self.origin[2] = next[1];
                }
            } else if clear {
                face_forward(&mut self.yaw, dx, dz);
                self.swing = self.swing.wrapping_add(1);
                if self.swing % PIGLIN_SWING_EVERY == 0 {
                    hit = true;
                }
            }
            settle_land(&mut self.origin, world);
        }
        hit
    }

    pub fn bounds(&self) -> ([f32; 3], [f32; 3]) {
        if self.hearts <= 0 {
            return ([0.0, -8.0, 0.0], [0.0, -8.0, 0.0]);
        }
        let [x, y, z] = self.origin;
        ([x - 0.32, y, z - 0.32], [x + 0.32, y + 1.9, z + 0.32])
    }

    pub fn vertices(&self) -> Vec<Vertex> {
        if self.hearts <= 0 {
            return Vec::new();
        }
        // Prefer Whimscape piglin sheet paint; fall back to average-tint boxes.
        if pack_entity_painted("piglin") {
            let clip = if self.hunting { "attack" } else { "idle" };
            let mut out = crate::blockbench::mesh(
                piglin_figure(),
                clip,
                self.pose,
                self.yaw,
                self.origin,
                &[],
            );
            if self.hunting {
                for vertex in &mut out {
                    vertex.color[0] = (vertex.color[0] * 0.85 + 0.2).min(1.0);
                    vertex.color[1] *= 0.75;
                    vertex.color[2] *= 0.7;
                }
            }
            return out;
        }
        let [x, y, z] = self.origin;
        let bob = (self.pose * 2.2).sin() * 0.02;
        // Piglin 64×64: head / body / ear tip.
        let (mut hide, cloth, snout, ear) = match pack_mob_colors(
            "piglin",
            &[(8, 8, 16, 16), (20, 20, 28, 32), (8, 8, 12, 12), (32, 0, 40, 8)],
        ) {
            Some(c) => (c[0], [c[1][0] * 0.55, c[1][1] * 0.5, c[1][2] * 0.45], c[2], c[3]),
            None => (
                if self.hunting {
                    [0.62, 0.32, 0.28]
                } else {
                    [0.72, 0.48, 0.38]
                },
                [0.28, 0.22, 0.18],
                [0.78, 0.42, 0.36],
                [0.55, 0.28, 0.24],
            ),
        };
        if self.hunting {
            hide = [
                (hide[0] * 0.85 + 0.2).min(1.0),
                hide[1] * 0.75,
                hide[2] * 0.7,
            ];
        }
        let mut out = Vec::new();
        // Legs.
        push_box(
            &mut out,
            [x - 0.12, y + 0.4 + bob, z],
            [0.1, 0.4, 0.1],
            hide,
        );
        push_box(
            &mut out,
            [x + 0.12, y + 0.4 + bob, z],
            [0.1, 0.4, 0.1],
            hide,
        );
        // Body + belt.
        push_box(
            &mut out,
            [x, y + 1.0 + bob, z],
            [0.28, 0.38, 0.18],
            hide,
        );
        push_box(
            &mut out,
            [x, y + 0.78 + bob, z],
            [0.3, 0.06, 0.2],
            cloth,
        );
        // Arms.
        let reach = if self.hunting { 0.22 } else { 0.06 };
        push_box(
            &mut out,
            [x - 0.36, y + 1.0 + bob, z + reach],
            [0.08, 0.32, 0.08],
            hide,
        );
        push_box(
            &mut out,
            [x + 0.36, y + 1.0 + bob, z + reach],
            [0.08, 0.32, 0.08],
            hide,
        );
        // Head + snout + ears.
        push_box(
            &mut out,
            [x, y + 1.52 + bob, z],
            [0.24, 0.22, 0.22],
            hide,
        );
        push_box(
            &mut out,
            [x, y + 1.42 + bob, z + 0.24],
            [0.1, 0.08, 0.1],
            snout,
        );
        push_box(
            &mut out,
            [x - 0.22, y + 1.68 + bob, z],
            [0.06, 0.12, 0.04],
            ear,
        );
        push_box(
            &mut out,
            [x + 0.22, y + 1.68 + bob, z],
            [0.06, 0.12, 0.04],
            ear,
        );
        yaw_mesh(&mut out, self.origin, self.yaw);
        out
    }
}

/// Fortress corridor pad — stands on [`rebuild_world::NETHER_FORTRESS`].
pub const WITHER_SKELETON_PAD: (i32, i32) = rebuild_world::NETHER_FORTRESS;

const WITHER_HEARTS: i32 = 6;
const WITHER_HEIGHT: f32 = 2.35;
const WITHER_REACH: f32 = 1.35;
const WITHER_STEP: f32 = 0.11;
const WITHER_SWING_EVERY: u32 = 22;

/// Tall dark skeleton that patrols the nether fortress corridor and melee-swings.
pub struct WitherSkeleton {
    origin: [f32; 3],
    home: [f32; 3],
    yaw: f32,
    swing: u32,
    hearts: i32,
    down: f32,
    loose: u32,
    attacking: bool,
}

impl WitherSkeleton {
    pub fn new() -> Self {
        let (x, z) = WITHER_SKELETON_PAD;
        let floor = f32::from(rebuild_world::NETHER_FORTRESS_FLOOR);
        let home = [x as f32 + 0.5, floor, z as f32 + 0.5];
        Self {
            origin: home,
            home,
            yaw: 0.0,
            swing: 0,
            hearts: WITHER_HEARTS,
            down: 0.0,
            loose: 0,
            attacking: false,
        }
    }

    pub fn origin(&self) -> [f32; 3] {
        self.origin
    }

    pub fn alive(&self) -> bool {
        self.hearts > 0
    }

    /// Place for shots and tests.
    pub fn place_at(&mut self, at: [f32; 3]) {
        self.origin = at;
        self.home = at;
        self.hearts = WITHER_HEARTS;
        self.down = 0.0;
        self.swing = 0;
        self.loose = 0;
        self.attacking = false;
    }

    pub fn aimed(&self, eye: [f32; 3], forward: [f32; 3]) -> bool {
        if self.hearts <= 0 {
            return false;
        }
        let [x, y, z] = self.origin;
        let center = [x, y + 1.2, z];
        let to = [center[0] - eye[0], center[1] - eye[1], center[2] - eye[2]];
        let along = to[0] * forward[0] + to[1] * forward[1] + to[2] * forward[2];
        if !(0.0..=5.0).contains(&along) {
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
        dx * dx + dy * dy + dz * dz < 0.95
    }

    /// Spend hearts. True when this hit drops the skeleton.
    pub fn hurt(&mut self, amount: i32) -> bool {
        if self.hearts <= 0 || amount <= 0 {
            return false;
        }
        self.hearts -= amount;
        if self.hearts > 0 {
            return false;
        }
        self.hearts = 0;
        self.down = 9.0;
        self.attacking = false;
        true
    }

    /// Return home after a knockdown. True on the frame it rises.
    pub fn rest(&mut self, dt: f32) -> bool {
        if self.hearts > 0 {
            return false;
        }
        self.down -= dt;
        if self.down > 0.0 {
            return false;
        }
        self.hearts = WITHER_HEARTS;
        self.origin = self.home;
        self.swing = 0;
        self.down = 0.0;
        self.loose = 0;
        self.attacking = false;
        true
    }

    /// Chase the player along the corridor. True when a melee swing is due.
    pub fn tick_near(
        &mut self,
        count: u32,
        player: [f32; 2],
        clear: bool,
        world: &Cluster,
    ) -> bool {
        if self.loose > 0 {
            self.loose = self.loose.saturating_sub(count);
            self.attacking = false;
            return false;
        }
        if self.hearts <= 0 {
            return false;
        }
        let mut hit = false;
        for _ in 0..count {
            let feet_y = self.origin[1].floor() as i32;
            let dx = player[0] - self.origin[0];
            let dz = player[1] - self.origin[2];
            let dist = (dx * dx + dz * dz).sqrt();
            if dist > WITHER_REACH {
                let from = [self.origin[0], self.origin[2]];
                if let Some(next) =
                    land_step(world, from, player, WITHER_STEP.min(dist - WITHER_REACH), feet_y, 2)
                {
                    face_forward(&mut self.yaw, next[0] - from[0], next[1] - from[1]);
                    self.origin[0] = next[0];
                    self.origin[2] = next[1];
                }
                self.attacking = false;
            } else if clear {
                face_forward(&mut self.yaw, dx, dz);
                self.attacking = true;
                self.swing = self.swing.wrapping_add(1);
                if self.swing % WITHER_SWING_EVERY == 0 {
                    hit = true;
                }
            } else {
                self.attacking = false;
            }
            settle_land(&mut self.origin, world);
        }
        hit
    }

    pub fn bounds(&self) -> ([f32; 3], [f32; 3]) {
        if self.hearts <= 0 {
            return ([0.0, -8.0, 0.0], [0.0, -8.0, 0.0]);
        }
        let [x, y, z] = self.origin;
        (
            [x - 0.28, y, z - 0.28],
            [x + 0.28, y + WITHER_HEIGHT, z + 0.28],
        )
    }

    pub fn vertices(&self) -> Vec<Vertex> {
        if self.hearts <= 0 {
            return Vec::new();
        }
        let [x, y, z] = self.origin;
        let bone = [0.22, 0.20, 0.18];
        let dark = [0.10, 0.09, 0.08];
        let rib = [0.16, 0.14, 0.13];
        let mut out = Vec::new();
        // Tall legs.
        push_box(&mut out, [x - 0.12, y + 0.55, z], [0.07, 0.55, 0.07], bone);
        push_box(&mut out, [x + 0.12, y + 0.55, z], [0.07, 0.55, 0.07], bone);
        // Torso.
        push_box(&mut out, [x, y + 1.35, z], [0.18, 0.38, 0.12], dark);
        // Ribs.
        push_box(&mut out, [x, y + 1.45, z + 0.1], [0.2, 0.22, 0.04], rib);
        // Arms — forward when swinging.
        let reach = if self.attacking { 0.28 } else { 0.08 };
        push_box(
            &mut out,
            [x - 0.28, y + 1.45, z + reach],
            [0.06, 0.42, 0.06],
            bone,
        );
        push_box(
            &mut out,
            [x + 0.28, y + 1.45, z + reach],
            [0.06, 0.42, 0.06],
            bone,
        );
        // Oversized black skull.
        push_box(&mut out, [x, y + 2.05, z], [0.24, 0.22, 0.22], dark);
        // Eye sockets.
        push_box(
            &mut out,
            [x - 0.1, y + 2.08, z + 0.2],
            [0.05, 0.05, 0.03],
            [0.85, 0.55, 0.12],
        );
        push_box(
            &mut out,
            [x + 0.1, y + 2.08, z + 0.2],
            [0.05, 0.05, 0.03],
            [0.85, 0.55, 0.12],
        );
        yaw_mesh(&mut out, self.origin, self.yaw);
        out
    }
}

const WITHER_BOSS_HEARTS: i32 = 20;
const WITHER_BOSS_HOVER: f32 = 4.5;
const WITHER_BOSS_STEP: f32 = 0.07;
const WITHER_BOSS_SHOOT_EVERY: u32 = 28;
const WITHER_BOSS_HOME_Y: f32 = 3.2;

const DRAGON_HEARTS: i32 = 20;
const DRAGON_RADIUS: f32 = 7.5;
const DRAGON_HOME_Y: f32 = 6.5;
const DRAGON_CIRCLE_STEP: f32 = 0.035;
const DRAGON_DIVE_EVERY: u32 = 90;
const DRAGON_DIVE_SPEED: f32 = 0.22;
const DRAGON_HIT_RANGE: f32 = 2.4;
/// Crystal beam restores one heart on this swing cadence while pillars stand.
const DRAGON_CRYSTAL_HEAL_EVERY: u32 = 40;

/// Three skull cells that complete a soul-sand summon row, when the last skull
/// sits at `at`. Soul sand must sit under each skull. Row runs along X or Z.
pub fn wither_ritual_row(
    world: &Cluster,
    at: [i32; 3],
) -> Option<[[i32; 3]; 3]> {
    let [x, y, z] = at;
    if world.block_at(x, y, z).id != "wither_skull" {
        return None;
    }
    for axis in 0..2 {
        for start in -2..=0 {
            let cells = if axis == 0 {
                [
                    [x + start, y, z],
                    [x + start + 1, y, z],
                    [x + start + 2, y, z],
                ]
            } else {
                [
                    [x, y, z + start],
                    [x, y, z + start + 1],
                    [x, y, z + start + 2],
                ]
            };
            if !cells.contains(&at) {
                continue;
            }
            let ready = cells.iter().all(|c| {
                world.block_at(c[0], c[1], c[2]).id == "wither_skull"
                    && world.block_at(c[0], c[1] - 1, c[2]).id == "soul_sand"
            });
            if ready {
                return Some(cells);
            }
        }
    }
    None
}

/// Floating three-headed wither boss. Shoots black skulls; does not revive.
pub struct Wither {
    origin: [f32; 3],
    home: [f32; 3],
    swing: u32,
    hearts: i32,
    /// True after a kill until the next ritual summons it again.
    spent: bool,
}

impl Wither {
    pub fn new() -> Self {
        Self {
            origin: [0.0, -8.0, 0.0],
            home: [0.0, -8.0, 0.0],
            swing: 0,
            hearts: 0,
            spent: true,
        }
    }

    pub fn origin(&self) -> [f32; 3] {
        self.origin
    }

    pub fn alive(&self) -> bool {
        self.hearts > 0 && !self.spent
    }

    /// Rise above a ritual center.
    pub fn summon(&mut self, at: [f32; 3]) {
        let home = [at[0], at[1] + WITHER_BOSS_HOME_Y, at[2]];
        self.origin = home;
        self.home = home;
        self.hearts = WITHER_BOSS_HEARTS;
        self.swing = 0;
        self.spent = false;
    }

    /// Place for shots and tests.
    pub fn place_at(&mut self, at: [f32; 3]) {
        self.summon([at[0], at[1] - WITHER_BOSS_HOME_Y, at[2]]);
        self.origin = at;
        self.home = at;
    }

    pub fn aimed(&self, eye: [f32; 3], forward: [f32; 3]) -> bool {
        if !self.alive() {
            return false;
        }
        let [x, y, z] = self.origin;
        let center = [x, y + 0.9, z];
        let to = [center[0] - eye[0], center[1] - eye[1], center[2] - eye[2]];
        let along = to[0] * forward[0] + to[1] * forward[1] + to[2] * forward[2];
        if !(0.0..=9.0).contains(&along) {
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
        dx * dx + dy * dy + dz * dz < 1.4
    }

    /// Spend hearts. True when this hit drops the wither.
    pub fn hurt(&mut self, amount: i32) -> bool {
        if !self.alive() || amount <= 0 {
            return false;
        }
        self.hearts -= amount;
        if self.hearts > 0 {
            return false;
        }
        self.hearts = 0;
        self.spent = true;
        true
    }

    /// Drift and queue skull shots. True when a projectile is due.
    pub fn tick_near(&mut self, count: u32, player: [f32; 3]) -> bool {
        if !self.alive() {
            return false;
        }
        let mut shot = false;
        for _ in 0..count {
            let dx = player[0] - self.origin[0];
            let dz = player[2] - self.origin[2];
            let dist = (dx * dx + dz * dz).sqrt();
            self.origin[1] = self.home[1] + (self.swing as f32 * 0.09).sin() * 0.18;
            if dist > WITHER_BOSS_HOVER + 1.5 && dist > 0.05 {
                let step = WITHER_BOSS_STEP.min(dist - WITHER_BOSS_HOVER);
                self.origin[0] += dx / dist * step;
                self.origin[2] += dz / dist * step;
            } else if dist < WITHER_BOSS_HOVER - 1.0 && dist > 0.05 {
                self.origin[0] -= dx / dist * WITHER_BOSS_STEP;
                self.origin[2] -= dz / dist * WITHER_BOSS_STEP;
            } else if dist > 18.0 {
                let hx = self.home[0] - self.origin[0];
                let hz = self.home[2] - self.origin[2];
                let hd = (hx * hx + hz * hz).sqrt();
                if hd > 0.05 {
                    self.origin[0] += hx / hd * WITHER_BOSS_STEP;
                    self.origin[2] += hz / hd * WITHER_BOSS_STEP;
                }
            }
            self.swing = self.swing.wrapping_add(1);
            if (3.0..20.0).contains(&dist) && self.swing % WITHER_BOSS_SHOOT_EVERY == 0 {
                shot = true;
            }
        }
        shot
    }

    pub fn bounds(&self) -> ([f32; 3], [f32; 3]) {
        if !self.alive() {
            return ([0.0, -8.0, 0.0], [0.0, -8.0, 0.0]);
        }
        let [x, y, z] = self.origin;
        ([x - 0.85, y, z - 0.45], [x + 0.85, y + 2.1, z + 0.45])
    }

    pub fn vertices(&self) -> Vec<Vertex> {
        if !self.alive() {
            return Vec::new();
        }
        let [x, y, z] = self.origin;
        let bob = (self.swing as f32 * 0.11).sin() * 0.05;
        let body = [0.14, 0.12, 0.11];
        let dark = [0.08, 0.07, 0.06];
        let eye = [0.9, 0.55, 0.12];
        let mut out = Vec::new();
        // Wide torso.
        push_box(
            &mut out,
            [x, y + 0.85 + bob, z],
            [0.55, 0.55, 0.28],
            body,
        );
        // Three skulls across the shoulders.
        for (ox, oy) in [(-0.55f32, 1.55), (0.0, 1.75), (0.55, 1.55)] {
            push_box(
                &mut out,
                [x + ox, y + oy + bob, z],
                [0.22, 0.2, 0.2],
                dark,
            );
            push_box(
                &mut out,
                [x + ox - 0.08, y + oy + 0.02 + bob, z + 0.18],
                [0.04, 0.04, 0.03],
                eye,
            );
            push_box(
                &mut out,
                [x + ox + 0.08, y + oy + 0.02 + bob, z + 0.18],
                [0.04, 0.04, 0.03],
                eye,
            );
        }
        // Stub legs / underglow.
        push_box(
            &mut out,
            [x - 0.28, y + 0.2 + bob, z],
            [0.12, 0.22, 0.12],
            dark,
        );
        push_box(
            &mut out,
            [x + 0.28, y + 0.2 + bob, z],
            [0.12, 0.22, 0.12],
            dark,
        );
        out
    }
}

const WARDEN_HEARTS: i32 = 12;
const WARDEN_BITE_EVERY: u32 = 28;
const WARDEN_REACH: f32 = 1.45;
const WARDEN_STEP: f32 = 0.11;

/// Deep-dark warden: tall biped that seeks vibration, then the player.
pub struct Warden {
    origin: [f32; 3],
    home: [f32; 3],
    yaw: f32,
    swing: u32,
    hearts: i32,
    /// True after a kill until the next shrieker summons it again.
    spent: bool,
    attacking: bool,
}

impl Warden {
    pub fn new() -> Self {
        Self {
            origin: [0.0, -8.0, 0.0],
            home: [0.0, -8.0, 0.0],
            yaw: 0.0,
            swing: 0,
            hearts: 0,
            spent: true,
            attacking: false,
        }
    }

    pub fn origin(&self) -> [f32; 3] {
        self.origin
    }

    pub fn alive(&self) -> bool {
        self.hearts > 0 && !self.spent
    }

    /// Rise beside a shrieker.
    pub fn summon(&mut self, at: [f32; 3]) {
        self.origin = at;
        self.home = at;
        self.hearts = WARDEN_HEARTS;
        self.swing = 0;
        self.spent = false;
        self.attacking = false;
        self.yaw = 0.0;
    }

    /// Place for shots and tests.
    pub fn place_at(&mut self, at: [f32; 3]) {
        self.summon(at);
    }

    pub fn aimed(&self, eye: [f32; 3], forward: [f32; 3]) -> bool {
        if !self.alive() {
            return false;
        }
        let [x, y, z] = self.origin;
        let center = [x, y + 1.4, z];
        let to = [center[0] - eye[0], center[1] - eye[1], center[2] - eye[2]];
        let along = to[0] * forward[0] + to[1] * forward[1] + to[2] * forward[2];
        if !(0.0..=8.0).contains(&along) {
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
        dx * dx + dy * dy + dz * dz < 1.2
    }

    /// Spend hearts. True when this hit drops the warden.
    pub fn hurt(&mut self, amount: i32) -> bool {
        if !self.alive() || amount <= 0 {
            return false;
        }
        self.hearts -= amount;
        if self.hearts > 0 {
            return false;
        }
        self.hearts = 0;
        self.spent = true;
        self.attacking = false;
        true
    }

    /// Seek `lure` (vibration) when set, else the player. True when a bite is due.
    pub fn tick_near(
        &mut self,
        count: u32,
        player: [f32; 2],
        lure: Option<[f32; 2]>,
        clear: bool,
        world: &Cluster,
    ) -> bool {
        if !self.alive() {
            return false;
        }
        let goal = lure.unwrap_or(player);
        let mut bite = false;
        let mut swinging = false;
        for _ in 0..count {
            let feet_y = self.origin[1].floor() as i32;
            let dx = goal[0] - self.origin[0];
            let dz = goal[1] - self.origin[2];
            let dist = (dx * dx + dz * dz).sqrt();
            if dist > WARDEN_REACH {
                let from = [self.origin[0], self.origin[2]];
                if let Some(next) =
                    land_step(world, from, goal, WARDEN_STEP.min(dist - WARDEN_REACH), feet_y, 3)
                {
                    face_forward(&mut self.yaw, next[0] - from[0], next[1] - from[1]);
                    self.origin[0] = next[0];
                    self.origin[2] = next[1];
                }
            } else if clear {
                face_forward(&mut self.yaw, dx, dz);
                swinging = true;
                self.swing = self.swing.wrapping_add(1);
                if self.swing % WARDEN_BITE_EVERY == 0 {
                    bite = true;
                }
            }
            settle_land(&mut self.origin, world);
        }
        self.attacking = swinging;
        bite
    }

    pub fn bounds(&self) -> ([f32; 3], [f32; 3]) {
        if !self.alive() {
            return ([0.0, -8.0, 0.0], [0.0, -8.0, 0.0]);
        }
        let [x, y, z] = self.origin;
        ([x - 0.45, y, z - 0.45], [x + 0.45, y + 2.55, z + 0.45])
    }

    pub fn vertices(&self) -> Vec<Vertex> {
        if !self.alive() {
            return Vec::new();
        }
        // Prefer Whimscape warden sheet paint; fall back to dark procedural boxes.
        if pack_entity_painted("warden") {
            let bob = (self.swing as f32 * 0.14).sin() * 0.04;
            let feet = [self.origin[0], self.origin[1] + bob, self.origin[2]];
            return crate::blockbench::mesh(
                warden_figure(),
                "idle",
                0.0,
                self.yaw,
                feet,
                &[],
            );
        }
        let [x, y, z] = self.origin;
        let bob = (self.swing as f32 * 0.14).sin() * 0.04;
        let body = [0.06, 0.1, 0.12];
        let dark = [0.03, 0.05, 0.06];
        let glow = [0.2, 0.95, 0.95];
        let mut out = Vec::new();
        // Legs.
        push_box(
            &mut out,
            [x - 0.18, y + 0.55 + bob, z],
            [0.14, 0.55, 0.14],
            dark,
        );
        push_box(
            &mut out,
            [x + 0.18, y + 0.55 + bob, z],
            [0.14, 0.55, 0.14],
            dark,
        );
        // Torso.
        push_box(
            &mut out,
            [x, y + 1.45 + bob, z],
            [0.42, 0.55, 0.28],
            body,
        );
        // Rib glow.
        push_box(
            &mut out,
            [x, y + 1.5 + bob, z + 0.22],
            [0.22, 0.28, 0.04],
            glow,
        );
        // Head + horn buds.
        push_box(
            &mut out,
            [x, y + 2.2 + bob, z],
            [0.28, 0.28, 0.28],
            dark,
        );
        push_box(
            &mut out,
            [x - 0.32, y + 2.45 + bob, z],
            [0.08, 0.18, 0.08],
            body,
        );
        push_box(
            &mut out,
            [x + 0.32, y + 2.45 + bob, z],
            [0.08, 0.18, 0.08],
            body,
        );
        // Tendril sensors.
        push_box(
            &mut out,
            [x - 0.12, y + 2.55 + bob, z + 0.05],
            [0.04, 0.16, 0.04],
            glow,
        );
        push_box(
            &mut out,
            [x + 0.12, y + 2.55 + bob, z + 0.05],
            [0.04, 0.16, 0.04],
            glow,
        );
        // Arms.
        let swing = if self.attacking {
            (self.swing as f32 * 0.35).sin() * 0.25
        } else {
            0.0
        };
        push_box(
            &mut out,
            [x - 0.55, y + 1.35 + bob + swing, z],
            [0.12, 0.55, 0.12],
            dark,
        );
        push_box(
            &mut out,
            [x + 0.55, y + 1.35 + bob - swing, z],
            [0.12, 0.55, 0.12],
            dark,
        );
        out
    }
}

const SHULKER_HEARTS: i32 = 8;
const SHULKER_SHOOT_EVERY: u32 = 48;
const SHULKER_OPEN_FOR: u32 = 18;
const SHULKER_HOME_Y: f32 = 5.0;

/// Boxy shulker clinging to the End city tower. Opens and fires levitation bullets.
pub struct Shulker {
    origin: [f32; 3],
    home: [f32; 3],
    swing: u32,
    hearts: i32,
    down: f32,
    /// Lid openness 0..=1 for the mesh.
    open: f32,
    /// True while the shell is raised and a shot may fire.
    peeking: bool,
}

impl Shulker {
    pub fn new() -> Self {
        let (x, z) = rebuild_world::END_CITY;
        let floor = f32::from(rebuild_world::END_CITY_FLOOR);
        // Sit on the south face looking toward the End pad.
        let home = [
            x as f32 + 0.5,
            floor + SHULKER_HOME_Y,
            z as f32 + 1.55,
        ];
        Self {
            origin: home,
            home,
            swing: 0,
            hearts: SHULKER_HEARTS,
            down: 0.0,
            open: 0.0,
            peeking: false,
        }
    }

    pub fn origin(&self) -> [f32; 3] {
        self.origin
    }

    pub fn alive(&self) -> bool {
        self.hearts > 0
    }

    pub fn openness(&self) -> f32 {
        self.open
    }

    /// Place for shots and tests.
    pub fn place_at(&mut self, at: [f32; 3]) {
        self.origin = at;
        self.home = at;
        self.hearts = SHULKER_HEARTS;
        self.down = 0.0;
        self.swing = SHULKER_SHOOT_EVERY - 4;
        self.open = 1.0;
        self.peeking = true;
    }

    pub fn aimed(&self, eye: [f32; 3], forward: [f32; 3]) -> bool {
        if self.hearts <= 0 {
            return false;
        }
        let [x, y, z] = self.origin;
        let center = [x, y + 0.45, z];
        let to = [center[0] - eye[0], center[1] - eye[1], center[2] - eye[2]];
        let along = to[0] * forward[0] + to[1] * forward[1] + to[2] * forward[2];
        if !(0.0..=10.0).contains(&along) {
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
        dx * dx + dy * dy + dz * dz < 0.9
    }

    /// Spend hearts. True when this hit drops the shulker.
    pub fn hurt(&mut self, amount: i32) -> bool {
        if self.hearts <= 0 || amount <= 0 {
            return false;
        }
        self.hearts -= amount;
        if self.hearts > 0 {
            return false;
        }
        self.hearts = 0;
        self.down = 10.0;
        self.open = 0.0;
        self.peeking = false;
        true
    }

    /// Return home after a knockdown. True on the frame it rises.
    pub fn rest(&mut self, dt: f32) -> bool {
        if self.hearts > 0 {
            return false;
        }
        self.down -= dt;
        if self.down > 0.0 {
            return false;
        }
        self.hearts = SHULKER_HEARTS;
        self.origin = self.home;
        self.swing = 0;
        self.down = 0.0;
        self.open = 0.0;
        self.peeking = false;
        true
    }

    /// Open / close and queue a bullet. True when a shot is due.
    pub fn tick_near(&mut self, count: u32, player: [f32; 3]) -> bool {
        if self.hearts <= 0 {
            return false;
        }
        let mut shot = false;
        for _ in 0..count {
            self.swing = self.swing.wrapping_add(1);
            let dx = player[0] - self.origin[0];
            let dz = player[2] - self.origin[2];
            let dist = (dx * dx + dz * dz).sqrt();
            let in_range = (2.0..18.0).contains(&dist);
            let phase = self.swing % SHULKER_SHOOT_EVERY;
            if in_range && phase < SHULKER_OPEN_FOR {
                self.peeking = true;
                self.open = (self.open + 0.18).min(1.0);
                if phase == SHULKER_OPEN_FOR / 2 {
                    shot = true;
                }
            } else {
                self.peeking = false;
                self.open = (self.open - 0.14).max(0.0);
            }
            let _ = self.peeking;
        }
        shot
    }

    pub fn bounds(&self) -> ([f32; 3], [f32; 3]) {
        if self.hearts <= 0 {
            return ([0.0, -8.0, 0.0], [0.0, -8.0, 0.0]);
        }
        let [x, y, z] = self.origin;
        let top = 0.55 + self.open * 0.55;
        ([x - 0.45, y, z - 0.45], [x + 0.45, y + top, z + 0.45])
    }

    pub fn vertices(&self) -> Vec<Vertex> {
        if self.hearts <= 0 {
            return Vec::new();
        }
        let [x, y, z] = self.origin;
        let shell = [0.62, 0.32, 0.78];
        let dark = [0.28, 0.12, 0.38];
        let eye = [0.95, 0.55, 1.0];
        let mut out = Vec::new();
        // Closed box body.
        push_box(&mut out, [x, y + 0.35, z], [0.42, 0.35, 0.42], shell);
        // Lid rises when peeking.
        let lid_y = y + 0.72 + self.open * 0.55;
        push_box(&mut out, [x, lid_y, z], [0.44, 0.1, 0.44], dark);
        if self.open > 0.35 {
            push_box(
                &mut out,
                [x, y + 0.55 + self.open * 0.2, z + 0.28],
                [0.12, 0.12, 0.1],
                dark,
            );
            push_box(
                &mut out,
                [x - 0.05, y + 0.58 + self.open * 0.2, z + 0.36],
                [0.03, 0.03, 0.02],
                eye,
            );
            push_box(
                &mut out,
                [x + 0.05, y + 0.58 + self.open * 0.2, z + 0.36],
                [0.03, 0.03, 0.02],
                eye,
            );
        }
        out
    }
}

/// Large flying ender dragon. Circles the End pad, dives at the player, drops an egg.
pub struct EnderDragon {
    origin: [f32; 3],
    home: [f32; 3],
    angle: f32,
    yaw: f32,
    swing: u32,
    hearts: i32,
    /// True after the first awaken on the End pad.
    awakened: bool,
    /// True after a kill — stays down for the session.
    spent: bool,
    diving: bool,
    dive_left: u32,
}

impl EnderDragon {
    pub fn new() -> Self {
        Self {
            origin: [0.0, -8.0, 0.0],
            home: [0.0, -8.0, 0.0],
            angle: 0.0,
            yaw: 0.0,
            swing: 0,
            hearts: 0,
            awakened: false,
            spent: false,
            diving: false,
            dive_left: 0,
        }
    }

    pub fn origin(&self) -> [f32; 3] {
        self.origin
    }

    pub fn alive(&self) -> bool {
        self.hearts > 0 && !self.spent
    }

    pub fn spent(&self) -> bool {
        self.spent
    }

    pub fn hearts(&self) -> i32 {
        self.hearts
    }

    /// Rise over the End pad once per session.
    pub fn awaken(&mut self, pad: [f32; 3]) -> bool {
        if self.awakened || self.spent {
            return false;
        }
        self.home = [pad[0], pad[1] + DRAGON_HOME_Y, pad[2]];
        self.angle = 0.4;
        self.origin = [
            self.home[0] + self.angle.cos() * DRAGON_RADIUS,
            self.home[1],
            self.home[2] + self.angle.sin() * DRAGON_RADIUS,
        ];
        self.yaw = self.angle + std::f32::consts::FRAC_PI_2;
        self.hearts = DRAGON_HEARTS;
        self.swing = 0;
        self.diving = false;
        self.dive_left = 0;
        self.awakened = true;
        true
    }

    /// Place for shots and tests.
    pub fn place_at(&mut self, at: [f32; 3]) {
        self.home = [at[0], at[1], at[2]];
        self.origin = at;
        self.angle = 0.0;
        self.yaw = 0.0;
        self.hearts = DRAGON_HEARTS;
        self.swing = 20;
        self.diving = false;
        self.dive_left = 0;
        self.awakened = true;
        self.spent = false;
    }

    /// Aim the silhouette for a still shot.
    pub fn face_yaw(&mut self, yaw: f32) {
        self.yaw = yaw;
    }

    pub fn aimed(&self, eye: [f32; 3], forward: [f32; 3]) -> bool {
        if !self.alive() {
            return false;
        }
        let [x, y, z] = self.origin;
        let center = [x, y + 0.4, z];
        let to = [center[0] - eye[0], center[1] - eye[1], center[2] - eye[2]];
        let along = to[0] * forward[0] + to[1] * forward[1] + to[2] * forward[2];
        if !(0.0..=14.0).contains(&along) {
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
        dx * dx + dy * dy + dz * dz < 2.8
    }

    /// Spend hearts. True when this hit drops the dragon.
    pub fn hurt(&mut self, amount: i32) -> bool {
        if !self.alive() || amount <= 0 {
            return false;
        }
        self.hearts -= amount;
        if self.hearts > 0 {
            return false;
        }
        self.hearts = 0;
        self.spent = true;
        self.diving = false;
        self.dive_left = 0;
        true
    }

    /// Crystal gate: take damage but never drop while pillars still heal.
    /// True when the hit changed hearts.
    pub fn hurt_shielded(&mut self, amount: i32) -> bool {
        if !self.alive() || amount <= 0 || self.hearts <= 1 {
            return false;
        }
        let next = (self.hearts - amount).max(1);
        if next == self.hearts {
            return false;
        }
        self.hearts = next;
        true
    }

    /// Restore hearts from living End crystals. Caps at full.
    pub fn heal(&mut self, amount: i32) -> bool {
        if !self.alive() || amount <= 0 || self.hearts >= DRAGON_HEARTS {
            return false;
        }
        self.hearts = (self.hearts + amount).min(DRAGON_HEARTS);
        true
    }

    /// After [`Self::tick_near`], beam-heal when a heal cadence crossed.
    pub fn crystal_beam_heal(&mut self, steps: u32) -> bool {
        if !self.alive() || self.hearts >= DRAGON_HEARTS || steps == 0 {
            return false;
        }
        let before = self.swing.wrapping_sub(steps);
        let crossed =
            before / DRAGON_CRYSTAL_HEAL_EVERY != self.swing / DRAGON_CRYSTAL_HEAL_EVERY;
        if !crossed {
            return false;
        }
        self.heal(1)
    }

    /// Circle / dive. True when a dive scrapes the player.
    pub fn tick_near(&mut self, count: u32, player: [f32; 3]) -> bool {
        if !self.alive() {
            return false;
        }
        let mut hit = false;
        for _ in 0..count {
            self.swing = self.swing.wrapping_add(1);
            if self.diving {
                let dx = player[0] - self.origin[0];
                let dy = (player[1] + 0.4) - self.origin[1];
                let dz = player[2] - self.origin[2];
                let dist = (dx * dx + dy * dy + dz * dz).sqrt().max(0.05);
                self.origin[0] += dx / dist * DRAGON_DIVE_SPEED;
                self.origin[1] += dy / dist * DRAGON_DIVE_SPEED;
                self.origin[2] += dz / dist * DRAGON_DIVE_SPEED;
                self.yaw = dx.atan2(dz);
                self.dive_left = self.dive_left.saturating_sub(1);
                if dist < DRAGON_HIT_RANGE {
                    hit = true;
                    self.diving = false;
                    self.dive_left = 0;
                } else if self.dive_left == 0 || self.origin[1] < self.home[1] - 5.0 {
                    self.diving = false;
                }
            } else {
                self.angle = (self.angle + DRAGON_CIRCLE_STEP) % (std::f32::consts::TAU);
                let bob = (self.swing as f32 * 0.08).sin() * 0.35;
                self.origin[0] = self.home[0] + self.angle.cos() * DRAGON_RADIUS;
                self.origin[1] = self.home[1] + bob;
                self.origin[2] = self.home[2] + self.angle.sin() * DRAGON_RADIUS;
                self.yaw = self.angle + std::f32::consts::FRAC_PI_2;
                let pdx = player[0] - self.home[0];
                let pdz = player[2] - self.home[2];
                let near_pad = pdx * pdx + pdz * pdz < 22.0 * 22.0;
                if near_pad && self.swing % DRAGON_DIVE_EVERY == 0 {
                    self.diving = true;
                    self.dive_left = 56;
                }
            }
        }
        hit
    }

    pub fn bounds(&self) -> ([f32; 3], [f32; 3]) {
        if !self.alive() {
            return ([0.0, -8.0, 0.0], [0.0, -8.0, 0.0]);
        }
        let [x, y, z] = self.origin;
        ([x - 1.8, y - 0.4, z - 1.8], [x + 1.8, y + 1.4, z + 1.8])
    }

    pub fn vertices(&self) -> Vec<Vertex> {
        if !self.alive() {
            return Vec::new();
        }
        let [x, y, z] = self.origin;
        let flap = (self.swing as f32 * 0.22).sin();
        // Mid purple so the silhouette holds against the void sky.
        let body = [0.28, 0.12, 0.36];
        let wing = [0.42, 0.18, 0.52];
        let belly = [0.16, 0.08, 0.2];
        let eye = [0.95, 0.35, 1.0];
        let mut out = Vec::new();
        // Long body + neck + head facing +Z before yaw.
        push_box(&mut out, [x, y + 0.35, z], [0.42, 0.32, 1.25], body);
        push_box(&mut out, [x, y + 0.2, z], [0.3, 0.18, 0.9], belly);
        push_box(&mut out, [x, y + 0.5, z + 1.45], [0.26, 0.22, 0.4], body);
        push_box(&mut out, [x, y + 0.55, z + 2.05], [0.34, 0.26, 0.34], body);
        push_box(
            &mut out,
            [x - 0.14, y + 0.62, z + 2.3],
            [0.05, 0.05, 0.04],
            eye,
        );
        push_box(
            &mut out,
            [x + 0.14, y + 0.62, z + 2.3],
            [0.05, 0.05, 0.04],
            eye,
        );
        // Wings.
        let wing_y = y + 0.6 + flap * 0.4;
        push_box(
            &mut out,
            [x - 1.55, wing_y, z],
            [1.3, 0.07, 0.65],
            wing,
        );
        push_box(
            &mut out,
            [x + 1.55, wing_y, z],
            [1.3, 0.07, 0.65],
            wing,
        );
        // Tail stubs.
        push_box(&mut out, [x, y + 0.28, z - 1.55], [0.18, 0.16, 0.4], body);
        push_box(&mut out, [x, y + 0.18, z - 2.25], [0.12, 0.12, 0.32], body);
        yaw_mesh(&mut out, self.origin, self.yaw);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span(vertices: &[Vertex]) -> (f32, f32, f32, f32) {
        let mut min_x = f32::MAX;
        let mut max_x = f32::MIN;
        let mut min_y = f32::MAX;
        let mut max_y = f32::MIN;
        for vertex in vertices {
            min_x = min_x.min(vertex.position[0]);
            max_x = max_x.max(vertex.position[0]);
            min_y = min_y.min(vertex.position[1]);
            max_y = max_y.max(vertex.position[1]);
        }
        (min_x, max_x, min_y, max_y)
    }
    use rebuild_world::{Biome, Block, Chunk, Cluster, biome_at, generate_chunk};

    fn air_plot() -> Cluster {
        Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, []).unwrap()]).unwrap()
    }

    #[test]
    fn a_registered_mob_stands_beside_the_stairs() {
        let guests = Guest::line(&["rosegold:wisp".to_string()]);
        assert_eq!(guests.len(), 1);
        assert_eq!(guests[0].id(), "rosegold:wisp");
        assert_eq!(guests[0].origin(), [10.5, 1.0, 12.5]);
        assert!(guests[0].vertices().len() >= 12);
        let two = Guest::line(&["rosegold:wisp".into(), "rosegold:second".into()]);
        assert_eq!(two[1].origin()[0], 11.75);
        let spawned = Guest::placed(
            &["rosegold:wisp".into()],
            &[("rosegold:wisp".into(), 12, 1, 14)],
        );
        assert_eq!(spawned.len(), 2);
        assert_eq!(spawned[1].id(), "rosegold:wisp");
        assert_eq!(spawned[1].origin(), [12.5, 1.0, 14.5]);
    }

    #[test]
    fn night_spawns_need_dark_footing_and_respect_the_cap() {
        let mut blocks: Vec<(u8, u16, u8, Block)> = (0..16u8)
            .flat_map(|z| (0..16u8).map(move |x| (x, 0u16, z, Block::new("stone", 0))))
            .collect();
        // Stone roof above (4,1,4) so day cave spawns stay legal.
        blocks.push((4, 4, 4, Block::new("stone", 0)));
        let ground = Cluster::try_from_chunks(
            0,
            0,
            [Chunk::from_blocks(0, 0, blocks).unwrap()],
        )
        .unwrap();
        // Open night surface should host.
        assert!(can_host_zombie(&ground, 8, 1, 8, true, None));
        // Open day surface should not.
        assert!(!can_host_zombie(&ground, 8, 1, 8, false, None));
        // Roofed dark cell can host by day.
        assert!(can_host_zombie(&ground, 4, 1, 4, false, None));
        // A held torch at the cell burns the spot.
        let torch = rebuild_world::Emitter {
            x: 4,
            y: 1,
            z: 4,
            light: rebuild_world::Light {
                color: [1.0, 0.8, 0.4],
                level: 14,
            },
        };
        assert!(!can_host_zombie(&ground, 4, 1, 4, true, Some(torch)));

        let mut horde = NightHorde::new();
        for i in 0..NIGHT_CAP {
            horde.force_at([8.5 + i as f32, 1.0, 10.5]);
        }
        assert_eq!(horde.len(), NIGHT_CAP);
        assert!(!horde.try_spawn([8.0, 2.0, 8.0], true, None, &ground));
        let mut clear = |_from: [f32; 3]| true;
        let (_bite, dirty) = horde.tick(
            0.05,
            1,
            [8.0, 2.0, 8.0],
            true,
            None,
            &ground,
            &mut clear,
        );
        assert!(dirty);
        assert!(!horde.vertices().is_empty());
    }

    #[test]
    fn sleep_blocks_when_a_wild_zombie_is_inside_the_spawn_min_ring() {
        let mut horde = NightHorde::new();
        horde.force_at([10.0, 1.0, 10.0]);
        let feet = [8.0, 1.0, 10.0];
        assert!(horde.any_living_within(feet, SLEEP_MONSTER_RANGE));
        assert!(!horde.any_living_within(feet, 1.5));
        assert!(horde.clear_wild());
        assert_eq!(horde.len(), 0);
        assert!(!horde.any_living_within(feet, SLEEP_MONSTER_RANGE));
        assert!(!horde.clear_wild());
    }

    #[test]
    fn a_falling_mace_drops_the_zombie_sooner() {
        let mut zombie = Zombie::new();
        assert_eq!(zombie.hearts(), 4);
        assert!(!zombie.hurt(1));
        assert_eq!(zombie.hearts(), 3);
        assert!(zombie.vertices().len() > 0);
        assert!(zombie.hurt(3));
        assert_eq!(zombie.hearts(), 0);
        assert!(zombie.vertices().is_empty());
        let ground = air_plot();
        assert!(!zombie.tick_near(40, [11.0, 13.0], true, &ground));
        assert!(!zombie.rest(1.0));
        assert!(zombie.rest(8.0));
        assert_eq!(zombie.hearts(), 4);
        let eye = [11.0, 2.0, 10.0];
        let forward = [0.0, 0.0, 1.0];
        assert!(zombie.aimed(eye, forward));
        assert!(!zombie.aimed(eye, [-1.0, 0.0, 0.0]));
        let home = zombie.origin;
        assert!(zombie.shove([home[0] - 1.0, home[1], home[2]]));
        assert!(zombie.origin[0] > home[0]);
        zombie.tick_near(10, [0.0, 0.0], true, &ground);
        let held = zombie.origin;
        zombie.tick_near(10, [0.0, 0.0], true, &ground);
        assert_eq!(zombie.origin, held);
        zombie.hurt(4);
        zombie.rouse([3.0, 1.0, 4.0]);
        assert_eq!(zombie.hearts(), 4);
        assert_eq!(zombie.origin, [3.0, 1.0, 4.0]);
        assert!(!zombie.vertices().is_empty());
    }

    #[test]
    fn a_breeze_floats_fires_and_drops() {
        let mut breeze = Breeze::new();
        assert!(breeze.origin()[1] > 2.0);
        assert!(!breeze.vertices().is_empty());
        let home = breeze.origin();
        let mut fired = false;
        for _ in 0..GUST_EVERY {
            if breeze.tick_near(1, [home[0] + 6.0, home[2]]) {
                fired = true;
            }
        }
        assert!(fired);
        assert!(!breeze.hurt(1));
        assert!(breeze.hurt(3));
        assert!(breeze.vertices().is_empty());
        assert!(!breeze.rest(1.0));
        assert!(breeze.rest(8.0));
        assert_eq!(breeze.origin(), BREEZE_HOME);
        breeze.rouse([2.0, 1.0, 3.0]);
        assert_eq!(breeze.origin()[1], 2.2);
        let gust = crate::body::breeze_shot([0.0, 3.0, 0.0], [6.0, 1.6, 0.0]).expect("shot");
        assert!(gust.dx > 0.0);
        assert!(gust.lift > 0.0);
        assert!(crate::body::breeze_shot([0.0, 3.0, 0.0], [40.0, 1.0, 0.0]).is_none());
    }

    #[test]
    fn a_guardian_lasers_and_spikes_in_the_shallows() {
        let mut guard = Guardian::new();
        assert_eq!(
            (
                guard.origin()[0].floor() as i32,
                guard.origin()[2].floor() as i32
            ),
            GUARDIAN_PAD
        );
        assert!(!guard.vertices().is_empty());
        let home = guard.origin();
        let mut laser = false;
        let mut spike = false;
        for _ in 0..GUARDIAN_LASER_EVERY {
            match guard.tick_near(1, [home[0] + 5.0, home[1], home[2]]) {
                Some(GuardHit::Laser) => laser = true,
                Some(GuardHit::Spike) => spike = true,
                None => {}
            }
        }
        assert!(laser, "guardian should laser at mid range");
        for _ in 0..GUARDIAN_SPIKE_EVERY {
            if matches!(
                guard.tick_near(1, [home[0] + 0.8, home[1], home[2]]),
                Some(GuardHit::Spike)
            ) {
                spike = true;
            }
        }
        assert!(spike, "guardian should spike up close");
        assert!(guard.hurt(6));
        assert!(!guard.alive());
        assert!(guard.rest(10.0));
        assert!(guard.alive());
    }

    #[test]
    fn a_piglin_barters_gold_then_angers_when_struck() {
        let mut piglin = Piglin::new();
        assert_eq!(
            (
                piglin.origin()[0].floor() as i32,
                piglin.origin()[2].floor() as i32
            ),
            PIGLIN_PAD
        );
        let ground = air_plot();
        let near = [piglin.origin()[0] + 2.0, piglin.origin()[2]];
        // No gold nearby → sight aggro / hunting (not yet attack-provoked).
        assert!(!piglin.angry());
        let _ = piglin.tick_near(4, near, true, &ground, false);
        assert!(piglin.hunting());
        assert!(!piglin.angry());
        // Gold armor pacifies unless provoked.
        piglin.place_at(piglin.origin());
        let _ = piglin.tick_near(4, near, true, &ground, true);
        assert!(!piglin.hunting());
        assert!(!piglin.vertices().is_empty());
        let first = piglin.barter().expect("calm barter");
        assert!(PIGLIN_BARTER.contains(&first));
        let second = piglin.barter().expect("second barter");
        assert_ne!(first, second);
        assert!(!piglin.hurt(1));
        assert!(piglin.angry());
        assert!(piglin.hunting());
        // Provoked piglin still hunts through gold armor.
        let _ = piglin.tick_near(2, near, true, &ground, true);
        assert!(piglin.hunting());
        assert!(piglin.barter().is_none());
        assert!(piglin.hurt(PIGLIN_HEARTS));
        assert!(!piglin.alive());
        assert!(piglin.rest(10.0));
        assert!(piglin.alive());
        assert!(!piglin.angry());
        assert!(piglin.barter().is_some());
    }

    #[test]
    fn a_blaze_floats_fires_and_drops_a_rod() {
        let mut blaze = Blaze::new();
        assert_eq!(
            (blaze.origin()[0].floor() as i32, blaze.origin()[2].floor() as i32),
            BLAZE_PAD
        );
        assert!(blaze.origin()[1] > 5.0);
        assert!(!blaze.vertices().is_empty());
        let home = blaze.origin();
        let mut fired = false;
        for _ in 0..BLAZE_SHOOT_EVERY {
            if blaze.tick_near(1, [home[0] + 5.0, home[1], home[2]]) {
                fired = true;
            }
        }
        assert!(fired, "blaze should queue a fireball in hover range");
        assert!(!blaze.hurt(1));
        assert!(blaze.hurt(4));
        assert!(!blaze.alive());
        assert!(blaze.vertices().is_empty());
        assert!(!blaze.rest(1.0));
        assert!(blaze.rest(10.0));
        assert!(blaze.alive());
        blaze.rouse([2.0, 4.0, 3.0]);
        assert!((blaze.origin()[1] - 5.5).abs() < 0.01);
    }

    #[test]
    fn a_ghast_shoots_when_the_player_is_near() {
        let mut ghast = Ghast::new();
        ghast.place_at([0.0, 8.0, 0.0]);
        let mut fired = false;
        for _ in 0..80 {
            if ghast.tick_near(1, [6.0, 5.0, 0.0]) {
                fired = true;
                break;
            }
        }
        assert!(fired, "ghast should queue a fireball in hover range");
        assert!(ghast.hurt(GHAST_HEARTS));
        assert!(!ghast.alive());
        assert!(ghast.vertices().is_empty());
    }

    #[test]
    fn a_magma_cube_hops_splits_and_drops_cream() {
        let mut pack = MagmaCubes::new();
        pack.place_at([0.0, 4.0, 0.0]);
        assert!(pack.alive());
        assert!(!pack.vertices().is_empty());
        let mut slapped = false;
        for _ in 0..40 {
            if pack.tick_near(1, [0.4, 4.0, 0.2]) {
                slapped = true;
                break;
            }
        }
        assert!(slapped, "magma cube should slap when it lands close");
        let eye = [0.0_f32, 5.0, 2.5];
        let forward = {
            let raw = [0.0_f32, -0.1, -1.0];
            let len = (raw[0] * raw[0] + raw[1] * raw[1] + raw[2] * raw[2]).sqrt();
            [raw[0] / len, raw[1] / len, raw[2] / len]
        };
        assert!(pack.aimed(eye, forward));
        assert_eq!(
            pack.hurt_aimed(eye, forward, MAGMA_BIG_HEARTS),
            Some(MagmaHit::Split)
        );
        assert_eq!(pack.all_bounds().len(), 2, "big cube splits into two smalls");
        // Kill both smalls for cream.
        let mut cream = 0;
        for _ in 0..8 {
            if !pack.alive() {
                break;
            }
            match pack.hurt_at(0, MAGMA_SMALL_HEARTS) {
                Some(MagmaHit::Cream) => cream += 1,
                Some(MagmaHit::Split) | Some(MagmaHit::Hurt) | None => {}
            }
        }
        assert_eq!(cream, 2, "each small cube should drop cream");
        assert!(!pack.alive());
        assert!(pack.rest(12.0));
        assert!(pack.alive());
    }

    #[test]
    fn a_wither_skeleton_swings_in_the_fortress() {
        let mut wither = WitherSkeleton::new();
        assert_eq!(
            (
                wither.origin()[0].floor() as i32,
                wither.origin()[2].floor() as i32
            ),
            WITHER_SKELETON_PAD
        );
        assert!(!wither.vertices().is_empty());
        let (_, _, min_y, max_y) = span(&wither.vertices());
        assert!(max_y - min_y > 2.0, "wither skeleton should read tall");
        let world = air_plot();
        let home = wither.origin();
        let mut swung = false;
        for _ in 0..WITHER_SWING_EVERY {
            if wither.tick_near(1, [home[0] + 0.4, home[2]], true, &world) {
                swung = true;
            }
        }
        assert!(swung, "wither skeleton should melee in reach");
        assert!(!wither.hurt(2));
        assert!(wither.hurt(4));
        assert!(!wither.alive());
        assert!(wither.vertices().is_empty());
        assert!(wither.rest(10.0));
        assert!(wither.alive());
    }

    #[test]
    fn three_skulls_on_soul_sand_summon_a_wither() {
        let mut blocks = Vec::new();
        for x in 4..=6u8 {
            blocks.push((x, 1u16, 5, Block::new("soul_sand", 0)));
            blocks.push((x, 2, 5, Block::new("wither_skull", 0)));
        }
        let world =
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, blocks).unwrap()]).unwrap();
        let row = wither_ritual_row(&world, [5, 2, 5]).expect("ritual row");
        assert_eq!(row.len(), 3);
        assert!(wither_ritual_row(&world, [5, 2, 6]).is_none());

        let mut boss = Wither::new();
        assert!(!boss.alive());
        boss.summon([5.5, 2.0, 5.5]);
        assert!(boss.alive());
        assert!(!boss.vertices().is_empty());
        let home = boss.origin();
        let mut fired = false;
        for _ in 0..WITHER_BOSS_SHOOT_EVERY {
            if boss.tick_near(1, [home[0] + 6.0, home[1], home[2]]) {
                fired = true;
            }
        }
        assert!(fired, "wither should queue a skull shot");
        assert!(boss.hurt(20));
        assert!(!boss.alive());
        assert!(boss.vertices().is_empty());
    }

    #[test]
    fn an_ender_dragon_circles_dives_and_falls() {
        let mut dragon = EnderDragon::new();
        assert!(!dragon.alive());
        assert!(dragon.awaken([24.5, 16.0, -39.5]));
        assert!(dragon.alive());
        assert!(!dragon.vertices().is_empty());
        assert!(!dragon.awaken([24.5, 16.0, -39.5]), "one awaken per session");
        dragon.place_at([10.0, 20.0, 10.0]);
        // Stand on the circle path so the next dive closes the gap.
        let mut scraped = false;
        for _ in 0..(DRAGON_DIVE_EVERY + 80) {
            let o = dragon.origin();
            if dragon.tick_near(1, [o[0], o[1] - 0.5, o[2]]) {
                scraped = true;
                break;
            }
        }
        assert!(scraped, "dragon dive should scrape the player");
        assert!(dragon.hurt(20));
        assert!(!dragon.alive());
        assert!(dragon.spent());
        assert!(dragon.vertices().is_empty());
        assert!(!dragon.awaken([24.5, 16.0, -39.5]), "spent dragon stays down");
    }

    #[test]
    fn end_crystals_heal_and_gate_the_dragon() {
        let mut dragon = EnderDragon::new();
        dragon.place_at([24.5, 22.0, -39.5]);
        assert_eq!(dragon.hearts(), DRAGON_HEARTS);
        assert!(dragon.hurt_shielded(8));
        assert_eq!(dragon.hearts(), DRAGON_HEARTS - 8);
        assert!(dragon.hurt_shielded(100), "gate clamps instead of killing");
        assert_eq!(dragon.hearts(), 1);
        assert!(!dragon.hurt_shielded(5), "already at one heart");
        assert!(dragon.alive());
        assert!(!dragon.spent());
        assert!(dragon.heal(3));
        assert_eq!(dragon.hearts(), 4);
        // Advance swing across a heal cadence while damaged.
        dragon.hurt_shielded(2);
        let mut beamed = false;
        for _ in 0..(DRAGON_CRYSTAL_HEAL_EVERY + 4) {
            let _ = dragon.tick_near(1, [24.5, 16.0, -39.5]);
            if dragon.crystal_beam_heal(1) {
                beamed = true;
                break;
            }
        }
        assert!(beamed, "living crystals restore a heart on cadence");
        assert!(dragon.hurt(20), "ungated kill still drops");
        assert!(!dragon.alive());
    }

    #[test]
    fn a_shulker_opens_and_queues_a_bullet() {
        let mut shulker = Shulker::new();
        assert!(shulker.alive());
        assert!(!shulker.vertices().is_empty());
        let home = shulker.origin();
        let mut fired = false;
        for _ in 0..(SHULKER_SHOOT_EVERY + 4) {
            if shulker.tick_near(1, [home[0], home[1], home[2] + 6.0]) {
                fired = true;
                break;
            }
        }
        assert!(fired, "open shulker should queue a bullet");
        assert!(shulker.openness() > 0.4, "lid should rise while peeking");
        assert!(!shulker.hurt(2));
        assert!(shulker.hurt(8));
        assert!(!shulker.alive());
        assert!(shulker.vertices().is_empty());
        assert!(shulker.rest(12.0));
        assert!(shulker.alive());
    }

    #[test]
    fn a_copper_golem_wakes_beside_a_bulb_and_works() {
        assert!(copper_base("copper_grate"));
        assert!(copper_base("copper_chest"));
        assert!(!copper_base("stone"));
        let feet = summon_feet([2, 2, 2]);
        assert_eq!(feet, [3.5, 2.0, 2.5]);
        let mut golem = Golem::new();
        assert!(golem.vertices().is_empty());
        golem.summon(feet);
        assert!(golem.vertices().iter().any(|vertex| vertex.color[0] > 0.7));
        let (min, max) = golem.bounds();
        assert!(max[1] - min[1] < 1.0);
        let blocks = [
            (3u8, 2, 2, Block::new("button", 0)),
            (6, 1, 6, Block::new("copper_chest", 0)),
        ];
        let world =
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, blocks).unwrap()]).unwrap();
        assert!(golem.tick(10, &world).is_none());
        assert_ne!(golem.origin[0], feet[0]);
        golem.summon([3.2, 2.0, 2.4]);
        assert!(matches!(golem.tick(21, &world), Some(Chore::Press(_))));
        golem.summon([6.2, 1.0, 6.4]);
        assert!(matches!(golem.tick(21, &world), Some(Chore::Open(_))));
    }

    #[test]
    fn an_iron_golem_wanders_the_green_and_punches_zombies() {
        let mut golem = IronGolem::new();
        let (px, pz) = IRON_GOLEM_PAD;
        assert_eq!(golem.origin()[0], px as f32 + 0.5);
        assert_eq!(golem.origin()[2], pz as f32 + 0.5);
        assert!(!golem.vertices().is_empty());
        let (min, max) = golem.bounds();
        assert!(max[1] - min[1] > 2.0, "tall iron body");
        let ground = air_plot();
        golem.place_at([4.5, 1.0, 4.5]);
        let home = golem.origin();
        assert!(!golem.tick(12, None, &ground));
        // Slow roam still leaves home eventually.
        let mut moved = false;
        for _ in 0..40 {
            let _ = golem.tick(4, None, &ground);
            if (golem.origin()[0] - home[0]).abs() > 0.05
                || (golem.origin()[2] - home[2]).abs() > 0.05
            {
                moved = true;
                break;
            }
        }
        assert!(moved, "golem should wander");
        golem.place_at([4.5, 1.0, 4.5]);
        let mut punched = false;
        for _ in 0..80 {
            if golem.tick(1, Some([4.7, 4.7]), &ground) {
                punched = true;
                break;
            }
        }
        assert!(punched, "golem punches a close zombie");
    }

    #[test]
    fn a_wolf_waits_near_home_then_follows_and_bites() {
        assert!(wolf_food("bone_meal"));
        assert!(wolf_food("cooked"));
        assert!(wolf_food("raw"));
        assert!(!wolf_food("apple"));
        let ground = air_plot();
        let mut wolf = Wolf::new();
        assert!(!wolf.tamed());
        assert!(wolf.vertices().iter().any(|vertex| vertex.color[2] > 0.4));
        let home = wolf.origin();
        assert!((home[0] - 8.0).abs() < 0.1 && (home[2] - 8.0).abs() < 0.1);
        let foe = [home[0], home[2]];
        assert!(!wolf.tick(BITE_EVERY, [0.0, 0.0], Some(foe), &ground));
        assert!(wolf.feed());
        assert!(wolf.tamed());
        assert!(!wolf.feed());
        assert!(wolf.vertices().iter().any(|vertex| vertex.color[0] > 0.7));
        // Empty-hand companion: sit stays put, stand follows again.
        assert!(wolf.sit());
        assert!(wolf.sitting());
        let sit_low = wolf
            .vertices()
            .iter()
            .map(|v| v.position[1])
            .fold(f32::MAX, f32::min);
        assert!(
            sit_low < home[1] + 0.12,
            "tucked sit paws rest near the grass, low {sit_low}"
        );
        let parked = wolf.origin();
        wolf.tick(30, [home[0] + 10.0, home[2]], None, &ground);
        assert!((wolf.origin()[0] - parked[0]).abs() < 0.05);
        assert!(!wolf.sit());
        assert!(!wolf.sitting());
        let stand_span = {
            let verts = wolf.vertices();
            let lo = verts.iter().map(|v| v.position[1]).fold(f32::MAX, f32::min);
            let hi = verts.iter().map(|v| v.position[1]).fold(f32::MIN, f32::max);
            hi - lo
        };
        let sit_span = {
            // re-sit briefly to compare body height to the standing pose
            assert!(wolf.sit());
            let verts = wolf.vertices();
            let lo = verts.iter().map(|v| v.position[1]).fold(f32::MAX, f32::min);
            let hi = verts.iter().map(|v| v.position[1]).fold(f32::MIN, f32::max);
            let span = hi - lo;
            assert!(!wolf.sit());
            span
        };
        assert!(
            stand_span > sit_span,
            "standing reads taller than the tucked sit, stand {stand_span} sit {sit_span}"
        );
        wolf.tick(40, [home[0] + 10.0, home[2]], None, &ground);
        assert!(wolf.origin()[0] > home[0] + 2.0);
        let mut bit = false;
        let at = [wolf.origin()[0] + 0.4, wolf.origin()[2]];
        for _ in 0..BITE_EVERY {
            if wolf.tick(1, [home[0] + 10.0, home[2]], Some(at), &ground) {
                bit = true;
            }
        }
        assert!(bit);
    }

    #[test]
    fn a_cat_waits_then_follows_sits_and_hisses() {
        assert!(cat_food("raw"));
        assert!(!cat_food("cooked"));
        assert!(!cat_food("bone_meal"));
        let ground = air_plot();
        let mut cat = Cat::new();
        assert!(!cat.tamed());
        assert!(!cat.sit());
        let home = cat.origin();
        assert!((home[0] - 12.0).abs() < 0.1);
        assert!(cat.vertices().iter().any(|vertex| vertex.color[0] > 0.8));
        let foe = [home[0] + 1.0, home[2]];
        assert!(!cat.tick(1, [0.0, 0.0], Some(foe), &ground));
        assert!(cat.feed());
        assert!(cat.tamed());
        assert!(cat.sit());
        assert!(cat.sitting());
        let sit_low = cat
            .vertices()
            .iter()
            .map(|v| v.position[1])
            .fold(f32::MAX, f32::min);
        assert!(
            sit_low < home[1] + 0.1,
            "tucked sit paws rest near the grass, low {sit_low}"
        );
        assert!(
            cat.vertices()
                .iter()
                .any(|v| v.color[0] > 0.85 && v.color[1] > 0.5),
            "sitting cat keeps pointed ears"
        );
        let parked = cat.origin();
        assert!(!cat.tick(10, [home[0] + 10.0, home[2]], Some(foe), &ground));
        assert_eq!(cat.origin(), parked);
        assert!(!cat.sit());
        cat.tick(40, [home[0] + 10.0, home[2]], None, &ground);
        assert!(cat.origin()[0] > home[0] + 1.0);
        let near = [cat.origin()[0] + 1.0, cat.origin()[2]];
        assert!(cat.tick(1, [home[0] + 20.0, home[2]], Some(near), &ground));
        assert!(!cat.tick(1, [home[0] + 20.0, home[2]], Some(near), &ground));
        let mut zombie = Zombie::new();
        let before = zombie.feet().unwrap();
        zombie.spook([before[0] - 2.0, before[1]]);
        assert!(zombie.feet().unwrap()[0] > before[0]);
    }

    #[test]
    fn a_parrot_waits_then_perches_and_echoes() {
        assert!(parrot_food("hay_bale"));
        assert!(!parrot_food("raw"));
        let ground = air_plot();
        let mut parrot = Parrot::new();
        assert!(!parrot.tamed());
        assert!(!parrot.perch());
        let home = parrot.origin();
        assert!((home[0] - 4.0).abs() < 0.1);
        assert!(parrot.vertices().iter().any(|vertex| vertex.color[1] > 0.6));
        parrot.hear(Mimic::Note(3));
        assert!(parrot.tick(ECHO_AFTER, [0.0, 1.0, 0.0], 0.0, &ground).is_none());
        assert!(parrot.feed());
        parrot.hear(Mimic::Note(3));
        parrot.hear(Mimic::Horn);
        assert_eq!(
            parrot.tick(ECHO_AFTER, [home[0] + 8.0, 1.0, home[2]], 0.0, &ground),
            Some(Mimic::Note(3))
        );
        assert!(
            parrot
                .tick(1, [home[0] + 8.0, 1.0, home[2]], 0.0, &ground)
                .is_none()
        );
        assert!(parrot.origin()[0] > home[0]);
        assert!(parrot.perch());
        assert!(parrot.shoulder());
        assert!(parrot.tick(1, [2.0, 1.0, 3.0], 0.0, &ground).is_none());
        // yaw 0 → forward +X, right +Z
        assert_eq!(parrot.origin(), [2.12, 2.38, 3.38]);
        parrot.hear(Mimic::Horn);
        assert_eq!(
            parrot.tick(ECHO_AFTER, [2.0, 1.0, 3.0], 0.0, &ground),
            Some(Mimic::Horn)
        );
    }

    #[test]
    fn a_frog_hops_eats_a_bug_and_takes_honey() {
        assert!(frog_food("honey"));
        assert!(!frog_food("raw"));
        let mut frog = Frog::new();
        let home = frog.origin();
        assert!((home[0] - 8.0).abs() < 0.01);
        assert!((home[1] - FROG_PAD_Y).abs() < 0.01);
        assert!((home[2] - 33.0).abs() < 0.01);
        assert!(frog.perched());
        assert!(
            frog.vertices()
                .iter()
                .any(|vertex| vertex.color[1] > 0.6 && vertex.color[0] < 0.4)
        );
        let slime: Vec<_> = frog
            .vertices()
            .into_iter()
            .filter(|vertex| vertex.color[1] > 0.8 && vertex.color[0] < 0.4)
            .collect();
        assert!(slime.len() > 12, "the slime has a body");
        let mut min_y = f32::MAX;
        let mut max_y = f32::MIN;
        for vertex in &slime {
            min_y = min_y.min(vertex.position[1]);
            max_y = max_y.max(vertex.position[1]);
        }
        assert!(
            max_y - min_y > 0.5,
            "the slime is tall enough to see, span {}",
            max_y - min_y
        );
        let ground = air_plot();
        assert!(!frog.tick(11, &ground));
        assert!(frog.origin()[1] > 4.3);
        assert!(!frog.perched());
        assert!(frog.tick(3, &ground));
        assert!(frog.origin()[0] > 9.0);
        assert!(frog.take_landed(), "hop should land on a lily");
        assert!(frog.perched());
        assert!(
            !frog
                .vertices()
                .iter()
                .any(|vertex| vertex.color[1] > 0.8 && vertex.color[0] < 0.4)
        );
        assert!(frog.feed());
    }

    #[test]
    fn a_frog_settles_on_a_lily_pad() {
        let mut cells = Vec::new();
        for &(wx, wz) in &FROG_PADS {
            let x = wx as u8;
            let z = (wz - 32) as u8;
            cells.push((x, 2u16, z, Block::new("dirt", 0)));
            cells.push((x, 3, z, Block::new("water", 0)));
            cells.push((x, 4, z, Block::new("lily_pad", 0)));
        }
        let marsh = Cluster::try_from_chunks(
            0,
            0,
            [Chunk::from_blocks(0, 2, cells).unwrap()],
        )
        .unwrap();
        assert_eq!(marsh.block_at(8, 4, 33).id, "lily_pad");
        let mut frog = Frog::new();
        frog.place_at([8.5, 4.5, 33.5]);
        frog.tick(1, &marsh);
        assert!(
            (frog.origin()[1] - FROG_PAD_Y).abs() < 0.05,
            "settles onto the lily top, y {}",
            frog.origin()[1]
        );
    }

    #[test]
    fn an_axolotl_swims_the_marsh_and_follows_through_water() {
        let mut axolotl = Axolotl::new();
        let home = axolotl.origin();
        assert!((home[0] - 8.5).abs() < 0.01);
        assert!((home[1] - 3.72).abs() < 0.01);
        assert!((home[2] - 32.5).abs() < 0.01);
        assert_eq!(biome_at(8, 32), Biome::Marsh);
        let marsh = Cluster::try_from_chunks(0, 0, [generate_chunk(0, 2)]).unwrap();
        assert_eq!(marsh.block_at(8, 3, 32).id, "water");
        let body: Vec<_> = axolotl
            .vertices()
            .into_iter()
            .filter(|vertex| vertex.color[0] > 0.9 && vertex.color[1] < 0.5)
            .collect();
        assert!(body.len() > 12, "the axolotl has a body");
        let mut min_x = f32::MAX;
        let mut max_x = f32::MIN;
        let mut min_y = f32::MAX;
        let mut max_y = f32::MIN;
        for vertex in &body {
            min_x = min_x.min(vertex.position[0]);
            max_x = max_x.max(vertex.position[0]);
            min_y = min_y.min(vertex.position[1]);
            max_y = max_y.max(vertex.position[1]);
        }
        assert!(
            max_x - min_x > 0.7,
            "wide enough to see, span {}",
            max_x - min_x
        );
        assert!(
            max_y - min_y > 0.45,
            "tall enough to see, span {}",
            max_y - min_y
        );
        assert!(max_y > 4.0, "the back clears the water, top {max_y}");
        axolotl.tick(AXOLOTL_STEPS, [home[0], home[2]], &marsh);
        assert!(
            (axolotl.origin()[0] - home[0]).abs() > 0.2
                || (axolotl.origin()[2] - home[2]).abs() > 0.2
        );
        assert!(axolotl.follow());
        assert!(axolotl.following());
        let before = axolotl.origin()[2];
        axolotl.tick(40, [8.5, 36.0], &marsh);
        assert!(axolotl.origin()[2] > before);
        let pond = Cluster::try_from_chunks(
            0,
            0,
            [Chunk::from_blocks(0, 2, [(8, 3, 0, Block::new("water", 0))]).unwrap()],
        )
        .unwrap();
        let mut one = Axolotl::new();
        assert!(one.follow());
        one.tick(40, [8.5, 14.0], &pond);
        assert!(one.origin()[2] > 32.0);
        assert!(one.origin()[2] <= home[2]);
        assert!(!one.follow());
    }

    #[test]
    fn a_following_axolotl_draws_a_leash_rope() {
        let mut axolotl = Axolotl::new();
        let hand = [
            axolotl.origin()[0] + 2.0,
            axolotl.origin()[1] + 1.2,
            axolotl.origin()[2] + 1.0,
        ];
        assert!(axolotl.leash_rope(hand).is_empty());
        assert!(axolotl.follow());
        let rope = axolotl.leash_rope(hand);
        assert!(rope.len() > 24, "leash beads should draw, got {}", rope.len());
        assert!(
            rope.iter()
                .any(|v| (v.color[0] - 0.72).abs() < 0.02 && (v.color[1] - 0.58).abs() < 0.02),
            "lead-colored rope"
        );
        let mid_y: f32 = rope.iter().map(|v| v.position[1]).sum::<f32>() / rope.len() as f32;
        assert!(
            mid_y < hand[1] + 0.05,
            "rope should sag below the hand line, mid {mid_y}"
        );
    }

    #[test]
    fn axolotl_companion_pathfinds_around_dry_and_stays_near() {
        // L-shaped pool: along x=5..10 at z=5, then up z at x=10. A dry block
        // sits in the elbow so a straight cut is closed.
        let mut cells = Vec::new();
        for x in 5..=10u8 {
            cells.push((x, 1u16, 5u8, Block::new("stone", 0)));
            cells.push((x, 2, 5, Block::new("water", 0)));
        }
        for z in 6..=10u8 {
            cells.push((10, 1, z, Block::new("stone", 0)));
            cells.push((10, 2, z, Block::new("water", 0)));
        }
        cells.push((8, 1, 8, Block::new("stone", 0)));
        cells.push((8, 2, 8, Block::new("stone", 0)));
        let mut far_cells = cells.clone();
        for x in 1..=3u8 {
            for z in 1..=3u8 {
                far_cells.push((x, 1, z, Block::new("stone", 0)));
                far_cells.push((x, 2, z, Block::new("water", 0)));
            }
        }
        let pool = Cluster::try_from_chunks(
            0,
            0,
            [Chunk::from_blocks(0, 0, cells).unwrap()],
        )
        .unwrap();
        let mut axolotl = Axolotl::new();
        axolotl.place_at([5.5, 2.72, 5.5]);
        assert!(axolotl.follow());
        // Swim the L toward the far arm.
        axolotl.tick(200, [10.5, 10.5], &pool);
        assert!(
            (axolotl.origin()[0] - 10.5).abs() < 1.5,
            "pathfinds to x, at {}",
            axolotl.origin()[0]
        );
        assert!(
            axolotl.origin()[2] > 8.5,
            "pathfinds up the arm, at {}",
            axolotl.origin()[2]
        );
        // Player on dry land beside the pool — hang at the wet edge.
        let shore = axolotl.origin();
        axolotl.tick(40, [12.5, 10.5], &pool);
        let after = axolotl.origin();
        assert!(
            (after[0] - shore[0]).abs() < 2.5 && (after[2] - shore[2]).abs() < 2.5,
            "stays near the shore beside the player"
        );
        assert!(
            pool.block_at(after[0].floor() as i32, 2, after[2].floor() as i32)
                .id
                == "water",
            "still in water"
        );
        // Leash snap: player far in another wet pocket → appear nearby in water.
        let far_pool =
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, far_cells).unwrap()])
                .unwrap();
        axolotl.place_at([10.5, 2.72, 10.5]);
        assert!(axolotl.following());
        // ~12.7 blocks away (> LEASH) with water underfoot at the pocket.
        axolotl.tick(5, [1.5, 1.5], &far_pool);
        let warped = axolotl.origin();
        assert!(
            (warped[0] - 1.5).abs() < 3.0 && (warped[2] - 1.5).abs() < 3.0,
            "appears in water beside the far player, at {warped:?}"
        );
        assert_eq!(
            far_pool
                .block_at(warped[0].floor() as i32, 2, warped[2].floor() as i32)
                .id,
            "water"
        );
    }

    #[test]
    fn a_chicken_pecks_west_of_the_oak_and_lays_an_egg() {
        let mut chicken = Chicken::new();
        let home = chicken.origin();
        assert!((home[0] - 3.0).abs() < 0.01);
        assert!((home[1] - 1.0).abs() < 0.01);
        assert!((home[2] - 8.0).abs() < 0.01);
        assert!(chicken.egg());
        let body: Vec<_> = chicken
            .vertices()
            .into_iter()
            .filter(|vertex| vertex.color[0] > 0.9 && vertex.color[2] > 0.9)
            .collect();
        assert!(body.len() > 12, "the chicken has a white body");
        let (min_x, max_x, min_y, max_y) = span(&body);
        assert!(
            max_x - min_x > 0.5,
            "wide enough to see, span {}",
            max_x - min_x
        );
        assert!(
            max_y - min_y > 0.4,
            "tall enough to see, span {}",
            max_y - min_y
        );
        let egg: Vec<_> = chicken
            .vertices()
            .into_iter()
            .filter(|vertex| {
                vertex.color[0] > 0.95 && vertex.color[1] > 0.85 && vertex.color[2] < 0.7
            })
            .collect();
        let (egg_min_x, egg_max_x, egg_min_y, egg_max_y) = span(&egg);
        assert!(
            egg_max_x - egg_min_x > 0.5,
            "the egg is wide enough to see, span {}",
            egg_max_x - egg_min_x
        );
        assert!(
            egg_max_y - egg_min_y > 0.5,
            "the egg is tall enough to see, span {}",
            egg_max_y - egg_min_y
        );
        assert!(
            chicken
                .vertices()
                .iter()
                .any(|vertex| vertex.color[0] > 0.7 && vertex.color[1] < 0.3)
        );
        assert!(chicken.take_egg());
        assert!(!chicken.egg());
        assert!(chicken.vertices().iter().all(|vertex| {
            !(vertex.color[0] > 0.95 && vertex.color[1] > 0.85 && vertex.color[2] < 0.7)
        }));
        assert!(!chicken.take_egg());
        let ground = air_plot();
        let mut laid = false;
        for _ in 0..80 {
            if chicken.tick(1, [0.0, 0.0], &ground) {
                laid = true;
                break;
            }
        }
        assert!(laid);
        assert!(chicken.egg());
        let mut still = Chicken::new();
        let home = still.origin();
        still.tick(8, [0.0, 0.0], &ground);
        let pecking = still.origin();
        assert!(
            (pecking[0] - home[0]).abs() < 0.01 && (pecking[2] - home[2]).abs() < 0.01,
            "it pecks before it walks"
        );
        still.tick(24, [0.0, 0.0], &ground);
        let walked = still.origin();
        assert!(
            (walked[0] - home[0]).abs() > 0.4 || (walked[2] - home[2]).abs() > 0.4,
            "then it walks the yard"
        );
        let mut shy = Chicken::new();
        let at = shy.origin();
        shy.tick(4, [at[0], at[2]], &ground);
        let fled = shy.origin();
        let away = (fled[0] - at[0]).hypot(fled[2] - at[2]);
        assert!(
            away > 0.4,
            "it steps back when you stand close, moved {away}"
        );
    }

    #[test]
    fn a_bee_pollinates_a_flower_then_fills_the_hive() {
        let world =
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, []).unwrap()]).unwrap();
        let mut bee = Bee::new();
        let home = bee.origin();
        assert!((home[1] - 2.6).abs() < 0.01);
        assert!(bee.vertices().iter().any(|vertex| vertex.color[0] > 0.85
            && vertex.color[1] > 0.7
            && vertex.color[2] < 0.3));
        assert!(bee.tick(1, [0.0, 1.0, 0.0], &world).is_none());
        assert!(bee.origin()[0] > home[0]);
        let blocks = [
            (14u8, 1, 6, Block::new("flower", 0)),
            (12u8, 1, 6, Block::new("beehive", 1)),
            (10u8, 1, 6, Block::new("beehive", 3)),
        ];
        let world =
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, blocks).unwrap()]).unwrap();
        let mut bee = Bee::new();
        let mut pollen = false;
        for _ in 0..12 {
            if matches!(bee.tick(1, [0.0, 1.0, 0.0], &world), Some(BeeWork::Pollen)) {
                pollen = true;
                break;
            }
        }
        assert!(pollen);
        assert!(bee.pollen());
        assert!(
            bee.vertices().iter().any(|vertex| vertex.color[0] > 0.9
                && vertex.color[1] < 0.6
                && vertex.color[2] < 0.2)
        );
        let mut filled = None;
        for _ in 0..40 {
            if let Some(BeeWork::Fill(at, next)) = bee.tick(1, [0.0, 1.0, 0.0], &world) {
                filled = Some((at, next));
                break;
            }
        }
        assert_eq!(filled, Some(([12, 1, 6], 2)));
        assert!(!bee.pollen());
        bee.anger();
        assert!(
            bee.vertices()
                .iter()
                .any(|vertex| vertex.color[0] > 0.8 && vertex.color[1] < 0.2)
        );
        assert!(bee.tick(1, [80.0, 1.0, 80.0], &world).is_none());
        let at = bee.origin();
        assert_eq!(
            bee.tick(1, [at[0], 1.0, at[2]], &world),
            Some(BeeWork::Sting)
        );
    }

    #[test]
    fn a_fox_wanders_holds_food_and_trusts_a_sweet() {
        assert!(fox_food("apple"));
        assert!(fox_food("honey"));
        assert!(fox_food("melon_slice"));
        assert!(!fox_food("raw"));
        let ground = air_plot();
        let mut fox = Fox::new();
        let home = fox.origin();
        assert!((home[0] - 6.0).abs() < 0.01 && (home[2] - 6.0).abs() < 0.01);
        assert!(
            fox.vertices()
                .iter()
                .any(|vertex| vertex.color[0] > 0.6 && vertex.color[1] < 0.35)
        );
        assert!(
            fox.vertices()
                .iter()
                .any(|vertex| vertex.color[0] > 0.9 && vertex.color[2] > 0.8)
        );
        assert!(fox.tick(1, [0.0, 0.0], Some([6.2, 1.2, 6.1]), &ground));
        fox.hold("apple");
        assert_eq!(fox.holding(), Some("apple"));
        let held = crate::palette::color("apple", 0);
        assert!(fox.vertices().iter().any(|vertex| vertex.color == held));
        assert!(!fox.tick(1, [0.0, 0.0], Some([6.2, 1.2, 6.1]), &ground));
        assert!(fox.trust());
        assert!(!fox.trust());
        assert!(fox.trusted());
        let before = fox.origin()[0];
        assert!(!fox.tick(30, [20.0, 6.0], None, &ground));
        assert!(fox.origin()[0] > before);
        let mut wild = Fox::new();
        assert!(!wild.tick(6, [0.0, 0.0], None, &ground));
        assert!(wild.origin()[0] > home[0]);
    }

    #[test]
    fn pets_follow_around_a_stone() {
        let stone = Block::new("stone", 0);
        let world = Cluster::try_from_chunks(
            0,
            0,
            [Chunk::from_blocks(
                0,
                0,
                [
                    (9, 1, 8, stone),
                    (13, 1, 10, stone),
                    (7, 1, 6, stone),
                    (5, 1, 11, stone),
                ],
            )
            .unwrap()],
        )
        .unwrap();
        let mut wolf = Wolf::new();
        assert!(wolf.feed());
        let mut cat = Cat::new();
        assert!(cat.feed());
        let mut fox = Fox::new();
        assert!(fox.trust());
        let mut parrot = Parrot::new();
        assert!(parrot.feed());
        let mut aside = [0.0_f32; 4];
        let home_z = [8.0, 10.0, 6.0, 11.0];
        let walls = [(9, 8), (13, 10), (7, 6), (5, 11)];
        for _ in 0..40 {
            wolf.tick(1, [18.0, 8.0], None, &world);
            cat.tick(1, [22.0, 10.0], None, &world);
            fox.tick(1, [16.0, 6.0], None, &world);
            parrot.tick(1, [14.0, 1.0, 11.0], 0.0, &world);
            let spots = [wolf.origin(), cat.origin(), fox.origin(), parrot.origin()];
            for i in 0..4 {
                let at = spots[i];
                assert!(
                    !(at[0].floor() as i32 == walls[i].0 && at[2].floor() as i32 == walls[i].1),
                    "stepped into the stone at {}, {}",
                    at[0],
                    at[2]
                );
                aside[i] = aside[i].max((at[2] - home_z[i]).abs());
            }
        }
        assert!(aside[0] > 0.3, "the wolf sidestepped {}", aside[0]);
        assert!(aside[1] > 0.3, "the cat sidestepped {}", aside[1]);
        assert!(aside[2] > 0.3, "the fox sidestepped {}", aside[2]);
        assert!(aside[3] > 0.3, "the parrot sidestepped {}", aside[3]);
    }

    #[test]
    fn pets_sit_follow_and_face_the_step() {
        let ground = air_plot();
        let mut wolf = Wolf::new();
        assert!(!wolf.sit());
        assert!(wolf.feed());
        assert!(wolf.sit());
        let parked = wolf.origin();
        wolf.tick(12, [parked[0] + 6.0, parked[2]], None, &ground);
        assert_eq!(wolf.origin(), parked);
        assert!(!wolf.sit());
        wolf.tick(1, [parked[0] + 20.0, parked[2]], None, &ground);
        let back = wolf.origin();
        assert!(
            (back[0] - (parked[0] + 20.0)).abs() < 3.0,
            "the wolf appears beside you, at {}",
            back[0]
        );

        let mut facing = Wolf::new();
        assert!(facing.feed());
        let home = facing.origin();
        facing.tick(16, [home[0], home[2] + 8.0], None, &ground);
        let snout = |vertices: &[Vertex]| {
            let mut sx = 0.0;
            let mut sz = 0.0;
            let mut n = 0.0;
            for vertex in vertices {
                if (vertex.color[0] - 0.58).abs() < 0.02 && (vertex.color[2] - 0.54).abs() < 0.02 {
                    sx += vertex.position[0];
                    sz += vertex.position[2];
                    n += 1.0;
                }
            }
            (sx / n, sz / n)
        };
        let (sx, sz) = snout(&facing.vertices());
        let at = facing.origin();
        assert!(
            (sz - at[2]).abs() > (sx - at[0]).abs(),
            "the snout follows +Z, offset {}, {}",
            sx - at[0],
            sz - at[2]
        );

        let mut wild = Wolf::new();
        let start = wild.origin();
        wild.tick(4, [0.0, 0.0], None, &ground);
        let mid = wild.origin();
        assert!((mid[0] - start[0]).abs() > 0.05 && (mid[2] - start[2]).abs() > 0.05);
        let mut paused = false;
        let mut prev = mid;
        for _ in 0..24 {
            wild.tick(1, [0.0, 0.0], None, &ground);
            let now = wild.origin();
            if (now[0] - prev[0]).abs() < 1.0e-4 && (now[2] - prev[2]).abs() < 1.0e-4 {
                paused = true;
            }
            prev = now;
        }
        assert!(paused, "a wild wolf pauses between headings");

        let mut fox = Fox::new();
        assert!(!fox.sit());
        assert!(fox.trust());
        assert!(fox.sit());
        let fox_home = fox.origin();
        fox.tick(8, [fox_home[0] + 4.0, fox_home[2]], None, &ground);
        assert_eq!(fox.origin(), fox_home);
        assert!(!fox.sit());
        fox.tick(1, [fox_home[0] + 20.0, fox_home[2]], None, &ground);
        assert!((fox.origin()[0] - (fox_home[0] + 20.0)).abs() < 3.0);

        let mut parrot = Parrot::new();
        assert!(parrot.feed());
        let nest = parrot.origin();
        parrot.tick(1, [nest[0] + 20.0, nest[1], nest[2]], 0.0, &ground);
        assert!((parrot.origin()[0] - (nest[0] + 20.0)).abs() < 3.0);
        assert!(parrot.perch());
        parrot.tick(1, [nest[0] + 30.0, nest[1], nest[2]], 0.0, &ground);
        // yaw 0 → forward +X, right +Z → +0.12 X, +0.38 Z, +1.38 Y
        assert!((parrot.origin()[0] - (nest[0] + 30.12)).abs() < 0.01);
        assert!((parrot.origin()[1] - (nest[1] + 1.38)).abs() < 0.01);
        assert!((parrot.origin()[2] - (nest[2] + 0.38)).abs() < 0.01);
        let perched = parrot.origin();
        parrot.tick(4, [nest[0] + 30.0, nest[1], nest[2]], 0.0, &ground);
        assert_eq!(parrot.origin(), perched);
    }

    #[test]
    fn a_goat_rams_on_the_hill_and_horns_a_wall() {
        let mut goat = Goat::new();
        let home = goat.origin();
        assert!((home[0] + 23.0).abs() < 0.01);
        assert!((home[1] - 8.0).abs() < 0.01);
        assert!((home[2] - 30.0).abs() < 0.01);
        assert!(goat.vertices().iter().any(|vertex| vertex.color[0] > 0.75
            && vertex.color[1] > 0.75
            && vertex.color[2] > 0.7));
        assert!(goat.vertices().iter().any(|vertex| vertex.color[0] > 0.4
            && vertex.color[1] < 0.5
            && vertex.color[2] < 0.35));
        let air = Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, []).unwrap()]).unwrap();
        assert!(goat.tick(4, [80.0, 8.0, 80.0], &air).is_none());
        assert!(goat.origin()[0] < home[0]);
        let mut goat = Goat::new();
        let player = [-19.0, 8.0, 30.0];
        let ram = goat.tick(7, player, &air).expect("charge");
        assert!(!ram.horn);
        let butt = goat_shove(ram.from, [player[0], 9.6, player[2]]).expect("knock");
        assert!(butt.dx > 0.0);
        assert!(butt.lift > 0.0);
        let stone = Block::new("stone", 0);
        let world = Cluster::try_from_chunks(
            -2,
            1,
            [Chunk::from_blocks(
                -2,
                1,
                [
                    (10u8, 8, 14, stone),
                    (11, 8, 14, stone),
                    (12, 8, 14, stone),
                    (13, 8, 14, stone),
                    (14, 8, 14, stone),
                ],
            )
            .unwrap()],
        )
        .unwrap();
        let mut goat = Goat::new();
        let mut horns = 0;
        let mut rams = 0;
        for _ in 0..140 {
            if let Some(hit) = goat.tick(1, player, &world) {
                rams += 1;
                if hit.horn {
                    horns += 1;
                }
            }
        }
        assert!(rams >= 4, "rams {rams}");
        assert_eq!(horns, 1);
    }

    #[test]
    fn the_pig_walks_a_square_and_steps_away() {
        let mut pig = Pig::new();
        assert_eq!(pig.origin, [5.0, 1.0, 9.0]);
        assert_eq!(pig.vertices().len(), 72);
        assert!(
            pig.vertices()
                .iter()
                .any(|vertex| vertex.color[0] > vertex.color[1])
        );
        let plot = air_plot();
        pig.tick_near(STEPS_PER_LEG, [0.0, 0.0], &plot);
        assert_eq!(pig.origin, [7.0, 1.0, 9.0]);
        pig.tick_near(0, [7.0, 9.0], &plot);
        let [x, _, z] = pig.origin;
        let dist = ((x - 7.0).powi(2) + (z - 9.0).powi(2)).sqrt();
        assert!(dist >= AVOID - 1.0e-3, "kept {dist} from the player");
        let (min, max) = pig.bounds();
        assert!(max[1] - min[1] > 1.0);
        assert!(!pig.mount());
        let parked = pig.origin;
        assert!(pig.saddle());
        assert!(!pig.saddle());
        pig.tick_near(STEPS_PER_LEG, [0.0, 0.0], &plot);
        assert_eq!(pig.origin, parked);
        assert!(pig.vertices().len() > 72);
        let mut ride = Pig::new();
        assert!(ride.saddle());
        assert!(ride.mount());
        let mut blocks = Vec::new();
        for z in 0..16u8 {
            for x in 0..8u8 {
                blocks.push((x, 0, z, Block::new("stone", 0)));
            }
        }
        let world =
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, blocks).unwrap()]).unwrap();
        assert!(ride.steer(0.2, RIDE_SPEED, 0.0, &world));
        assert!(ride.origin[0] > 5.0);
        for _ in 0..80 {
            ride.steer(1.0 / 60.0, RIDE_SPEED, 0.0, &world);
        }
        assert!(
            ride.origin[0] > 7.0 && ride.origin[0] < 8.0,
            "stayed on the pad at {}",
            ride.origin[0]
        );
        let stuck = ride.origin[0];
        assert!(!ride.steer(0.5, RIDE_SPEED, 0.0, &world));
        assert_eq!(ride.origin[0], stuck);
        ride.dismount();
        assert!(ride.seat().is_none());
    }

    #[test]
    fn a_strider_mounts_and_steers_on_lava() {
        let mut blocks = Vec::new();
        for z in 0..8u8 {
            for x in 0..8u8 {
                blocks.push((x, 0, z, Block::new("lava", 0)));
            }
        }
        // Dry stone rim the strider must refuse.
        for z in 0..8u8 {
            blocks.push((7, 0, z, Block::new("stone", 0)));
        }
        let world =
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, blocks).unwrap()]).unwrap();
        let mut strider = Strider::new();
        assert!(!strider.place_at([7.5, 0.0, 1.5], &world)); // stone rim
        assert!(strider.place_at([1.5, 0.0, 1.5], &world));
        assert!((strider.origin()[1] - 1.0).abs() < 1.0e-3);
        assert!(strider.mount());
        assert!(!strider.mount());
        assert!(strider.seat().is_some());
        assert!(strider.steer(0.2, STRIDER_SPEED, 0.0, &world));
        assert!(strider.origin()[0] > 1.5);
        let edge = strider.origin()[0];
        // Drive toward the stone rim — should stop on lava.
        for _ in 0..80 {
            strider.steer(1.0 / 60.0, STRIDER_SPEED, 0.0, &world);
        }
        assert!(
            strider.origin()[0] < 7.0,
            "stayed on lava at {}",
            strider.origin()[0]
        );
        assert!(strider.origin()[0] >= edge);
        assert!(!strider.vertices().is_empty());
        strider.dismount();
        assert!(strider.seat().is_none());
    }

    #[test]
    fn soft_render_zombie_preview_png() {
        let path = match std::env::var("ZOMBIE_SHOT") {
            Ok(path) if !path.is_empty() => path,
            _ => return,
        };
        let width = 160usize;
        let height = 240usize;
        let vertices = Zombie::new().vertices();
        assert!(vertices.len() > 36, "zombie mesh for preview");
        let mut rgba = vec![0u8; width * height * 4];
        let mut depth = vec![f32::INFINITY; width * height];
        let yaw = 0.55_f32;
        let (sy, cy) = (yaw.sin(), yaw.cos());
        let scale = height as f32 * 0.42;
        let cx = width as f32 * 0.5;
        let cy_screen = height as f32 * 0.78;
        let project = |p: [f32; 3]| -> (f32, f32, f32) {
            let x = (p[0] - 11.0) * cy + (p[2] - 13.0) * sy;
            let z = -(p[0] - 11.0) * sy + (p[2] - 13.0) * cy;
            let sx = x * scale + cx;
            let sy = (1.05 - (p[1] - 1.0)) * scale + cy_screen;
            (sx, sy, z)
        };
        for tri in vertices.chunks_exact(3) {
            let (ax, ay, az) = project(tri[0].position);
            let (bx, by, bz) = project(tri[1].position);
            let (cxp, cyp, cz) = project(tri[2].position);
            let color = tri[0].color;
            let min_x = ax.min(bx).min(cxp).floor().max(0.0) as usize;
            let max_x = ax.max(bx).max(cxp).ceil().min(width as f32 - 1.0) as usize;
            let min_y = ay.min(by).min(cyp).floor().max(0.0) as usize;
            let max_y = ay.max(by).max(cyp).ceil().min(height as f32 - 1.0) as usize;
            let area = (bx - ax) * (cyp - ay) - (cxp - ax) * (by - ay);
            if area.abs() < 1.0e-5 {
                continue;
            }
            for y in min_y..=max_y {
                for x in min_x..=max_x {
                    let px = x as f32 + 0.5;
                    let py = y as f32 + 0.5;
                    let w0 = ((bx - px) * (cyp - py) - (cxp - px) * (by - py)) / area;
                    let w1 = ((cxp - px) * (ay - py) - (ax - px) * (cyp - py)) / area;
                    let w2 = 1.0 - w0 - w1;
                    if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                        continue;
                    }
                    let z = az * w0 + bz * w1 + cz * w2;
                    let i = y * width + x;
                    if z >= depth[i] {
                        continue;
                    }
                    depth[i] = z;
                    let o = i * 4;
                    rgba[o] = (color[0] * 255.0) as u8;
                    rgba[o + 1] = (color[1] * 255.0) as u8;
                    rgba[o + 2] = (color[2] * 255.0) as u8;
                    rgba[o + 3] = 255;
                }
            }
        }
        let opaque = rgba.chunks(4).filter(|px| px[3] > 0).count();
        assert!(opaque > 200, "preview silhouette has pixels, got {opaque}");
        let mut file = std::fs::File::create(&path).expect("create preview png");
        let mut encoder = png::Encoder::new(&mut file, width as u32, height as u32);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().expect("png header");
        writer.write_image_data(&rgba).expect("png pixels");
    }

    #[test]
    fn the_zombie_plays_attack_and_flashes_on_a_hit() {
        let mut zombie = Zombie::new();
        let ground = air_plot();
        assert_eq!(zombie.clip(), "idle");
        for _ in 0..BITE_EVERY {
            zombie.tick_near(1, [zombie.origin[0], zombie.origin[2]], true, &ground);
        }
        assert_eq!(zombie.clip(), "attack");
        assert!(!zombie.hurt(1));
        assert!(
            zombie
                .vertices()
                .iter()
                .any(|vertex| vertex.color[0] > vertex.color[1]),
            "a hit tints the mesh red"
        );
        assert!(zombie.rest(0.05));
    }

    #[test]
    fn the_zombie_walks_toward_the_player_and_bites() {
        let mut zombie = Zombie::new();
        assert_eq!(zombie.origin, [11.0, 1.0, 13.0]);
        assert!(
            zombie
                .vertices()
                .iter()
                .any(|vertex| vertex.color[1] > vertex.color[0])
        );
        let ground = air_plot();
        assert!(!zombie.tick_near(1, [8.5, 14.4], true, &ground));
        assert!(zombie.origin[0] < 11.0, "stepped toward the player");
        let mut bites = 0;
        for _ in 0..BITE_EVERY {
            if zombie.tick_near(1, [zombie.origin[0], zombie.origin[2]], true, &ground) {
                bites += 1;
            }
        }
        assert_eq!(bites, 1);
        let mut walled = Zombie::new();
        let spot = [walled.origin[0], walled.origin[2]];
        let mut through = 0;
        for _ in 0..BITE_EVERY {
            if walled.tick_near(1, spot, false, &ground) {
                through += 1;
            }
        }
        assert_eq!(through, 0, "a wall stops the bite");
        let (min, max) = zombie.bounds();
        assert!(max[1] - min[1] > 1.5);
    }

    #[test]
    fn a_zombie_walks_around_a_stone_wall() {
        let stone = Block::new("stone", 0);
        assert!(rebuild_world::shape_of(stone).is_full_cube());
        let world = Cluster::try_from_chunks(
            0,
            0,
            [Chunk::from_blocks(0, 0, [(10, 1, 13, stone), (10, 2, 13, stone)]).unwrap()],
        )
        .unwrap();
        let mut zombie = Zombie::new();
        let mut swung = 0.0_f32;
        for _ in 0..40 {
            zombie.tick_near(1, [8.0, 13.0], true, &world);
            let [x, _, z] = zombie.origin;
            assert!(
                !(x.floor() as i32 == 10 && z.floor() as i32 == 13),
                "stepped into the stone at {x}, {z}"
            );
            swung = swung.max((z - 13.0).abs());
        }
        assert!(swung > 0.4, "sidestepped the wall, offset {swung}");
        assert!(
            zombie.origin[0] < 10.2,
            "came around to {}",
            zombie.origin[0]
        );
        assert!(!zombie.vertices().is_empty());
    }

    #[test]
    fn a_chicken_steps_around_a_post() {
        let world = Cluster::try_from_chunks(
            0,
            0,
            [Chunk::from_blocks(0, 0, [(4, 1, 8, Block::new("stone", 0))]).unwrap()],
        )
        .unwrap();
        let mut chicken = Chicken::new();
        let mut aside = 0.0_f32;
        for _ in 0..24 {
            chicken.tick(1, [3.0, 8.0], &world);
            let at = chicken.origin();
            assert!(
                !(at[0].floor() as i32 == 4 && at[2].floor() as i32 == 8),
                "stepped into the post at {}, {}",
                at[0],
                at[2]
            );
            aside = aside.max((at[2] - 8.0).abs());
        }
        assert!(aside > 0.3, "stepped aside, offset {aside}");
        assert!(
            chicken.origin()[0] > 4.0,
            "got past the post at {}",
            chicken.origin()[0]
        );
        assert!(
            chicken.vertices().len() > 12,
            "the blockbench body is still drawn"
        );
    }

    #[test]
    fn an_axolotl_follows_around_a_dry_cell() {
        let water = Block::new("water", 0);
        let world = Cluster::try_from_chunks(
            0,
            2,
            [Chunk::from_blocks(
                0,
                2,
                [
                    (8, 3, 0, water),
                    (9, 3, 0, water),
                    (9, 3, 1, water),
                    (9, 3, 2, water),
                    (8, 3, 2, water),
                ],
            )
            .unwrap()],
        )
        .unwrap();
        let mut axolotl = Axolotl::new();
        assert!(axolotl.follow());
        let mut widest = axolotl.origin()[0];
        for _ in 0..30 {
            axolotl.tick(1, [8.5, 36.0], &world);
            let [x, _, z] = axolotl.origin();
            widest = widest.max(x);
            assert!(water_cell(&world, x, z), "left the water at {x}, {z}");
        }
        let at = axolotl.origin();
        assert!(widest > 8.7, "slid around the dry cell, x {widest}");
        assert!(at[2] > 32.6, "still headed along the pool, z {}", at[2]);
        assert!(
            axolotl.vertices().len() > 12,
            "the blockbench body is still drawn"
        );
    }

    #[test]
    fn a_body_stops_on_a_solid_and_floats_in_water() {
        let water = Block::new("water", 0);
        let deep = Cluster::try_from_chunks(
            0,
            2,
            [
                Chunk::from_blocks(0, 2, [(8, 2, 0, water), (8, 3, 0, water), (8, 4, 0, water)])
                    .unwrap(),
            ],
        )
        .unwrap();
        let mut sunk = [8.5, 2.1, 32.5];
        settle_swim(&mut sunk, &deep);
        assert!(
            (sunk[1] - 4.72).abs() < 0.05,
            "a water mob swims under the surface, y {}",
            sunk[1]
        );
        let mut wading = [8.5, 2.1, 32.5];
        settle_land(&mut wading, &deep);
        assert!(
            (wading[1] - 5.0).abs() < 0.05,
            "a land mob stands on the water, y {}",
            wading[1]
        );

        let pool = Cluster::try_from_chunks(
            0,
            0,
            [Chunk::from_blocks(0, 0, [(5, 1, 9, water)]).unwrap()],
        )
        .unwrap();
        let mut pig = Pig::new();
        pig.tick_near(0, [0.0, 0.0], &pool);
        assert!(
            (pig.origin[1] - 2.0).abs() < 0.05,
            "the pig floats, y {}",
            pig.origin[1]
        );

        let blocked = Cluster::try_from_chunks(
            0,
            0,
            [Chunk::from_blocks(0, 0, [(6, 1, 9, Block::new("slab", 0))]).unwrap()],
        )
        .unwrap();
        let mut walker = Pig::new();
        walker.tick_near(STEPS_PER_LEG, [0.0, 0.0], &blocked);
        assert!(
            walker.origin[0] < 6.0,
            "a slab stops the pig at {}",
            walker.origin[0]
        );

        let stairs = Cluster::try_from_chunks(
            0,
            0,
            [Chunk::from_blocks(0, 0, [(10, 1, 13, Block::new("oak_stairs", 0))]).unwrap()],
        )
        .unwrap();
        let mut zombie = Zombie::new();
        for _ in 0..40 {
            zombie.tick_near(1, [8.0, 13.0], true, &stairs);
            let [x, _, z] = zombie.origin;
            assert!(
                !(x.floor() as i32 == 10 && z.floor() as i32 == 13),
                "walked through the stairs at {x}, {z}"
            );
        }
        assert!(
            (zombie.origin[2] - 13.0).abs() > 0.3,
            "stepped around the stairs, z {}",
            zombie.origin[2]
        );
    }

    #[test]
    fn a_horse_tames_saddles_and_gallops() {
        let mut blocks = Vec::new();
        for z in 4..10u8 {
            for x in 2..10u8 {
                blocks.push((x, 0, z, Block::new("grass", 0)));
            }
        }
        let world =
            Cluster::try_from_chunks(1, 0, [Chunk::from_blocks(1, 0, blocks).unwrap()]).unwrap();
        let mut horse = Horse::new();
        assert!(!horse.tamed());
        assert!(horse_food("wheat"));
        assert!(!horse_food("apple"));
        assert!(horse.feed());
        assert!(horse.tamed());
        assert!(!horse.feed());
        assert!(!horse.mount());
        assert!(horse.saddle());
        assert!(horse.saddled());
        assert!(!horse.saddle());
        assert!(horse.mount());
        assert!(horse.ridden());
        assert_eq!(horse.origin()[0].floor() as i32, HORSE_PAD.0 - 1);
        assert!(horse.steer(0.2, HORSE_SPEED, 0.0, &world));
        let before = horse.origin()[0];
        assert!(horse.steer(0.2, HORSE_SPEED, 0.0, &world));
        assert!(horse.origin()[0] > before);
        assert!(horse.seat().is_some());
        horse.dismount();
        assert!(!horse.ridden());
        assert!(horse.vertices().len() > 100);
        let before = horse.vertices().len();
        let fence = [HORSE_HITCH.0, 1, HORSE_HITCH.1];
        assert!(horse.hitch(fence));
        assert!(horse.hitched());
        assert_eq!(horse.hitch_at(), Some(fence));
        assert!(!horse.mount());
        assert!(!horse.hitch(fence));
        assert!(horse.vertices().len() > before);
        assert_eq!(horse.untie(), Some(fence));
        assert!(!horse.hitched());
    }
}
