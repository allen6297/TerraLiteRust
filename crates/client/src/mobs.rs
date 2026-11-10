//! A pig, a zombie, a breeze, a copper golem, a wolf, a cat, a parrot, a frog,
//! an axolotl, a chicken, a bee, a fox, and a goat.
//!
//! The zombie body is `models/entity/zombie.bbmodel` with `textures/entity/zombie.png`,
//! playing `idle`, `walk`, and `attack` the same way the chicken and player do.
//! Night and dark-cave spawns reuse that same mesh and chase/bite AI; the trial
//! spawner zombie stays a separate demo body.
//!
//! The entity tick runs `"pig"`, `"zombie"`, `"breeze"`, `"golem"`, `"wolf"`,
//! `"cat"`, `"parrot"`, `"frog"`, `"bee"`, `"fox"`, and `"goat"` every 50 ms,
//! the same way it runs `"sheep"`. The pig walks a square. The zombie bites.
//! The breeze fires a gust. The golem wakes on a copper bulb. The wolf sits or
//! follows, bites, and turns to face the step. The cat sits or follows, hisses,
//! and turns with the step. The parrot echoes a note, perches, or follows, and
//! turns with the step. The frog hops and eats a bug. The axolotl pauses with its gills open, swims one
//! side of the pool, and follows feet that stay over water, steering around a
//! dry cell. The chicken pecks, walks its yard, steps around a full block, and
//! lays an egg. The zombie walks around a full block to bite. The bee carries
//! pollen to a hive. The fox sits or follows, fetches, and turns with the step.
//! A standing pet appears beside you past twelve blocks. A wild one pauses and
//! picks a new heading instead of a square. The goat paces the hill and
//! rams. None of the bodies are saved.

use rebuild_world::Cluster;

use crate::body::EYE_HEIGHT;
use crate::mesh::Vertex;

/// Horizontal speed while riding, in blocks per second.
pub const RIDE_SPEED: f32 = 6.0;
const BOARD: f32 = 2.5;

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
        crate::blockbench::parse(include_str!(
            "../../../resourcepacks/terralite/assets/terralite/models/entity/zombie.bbmodel"
        ))
        .expect("zombie.bbmodel")
    })
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
        let mut out = Vec::new();
        push_box(
            &mut out,
            [x, y + 0.45, z],
            [0.28, 0.32, 0.22],
            [0.72, 0.90, 0.98],
        );
        push_box(
            &mut out,
            [x, y + 0.15, z],
            [0.16, 0.1, 0.16],
            [0.55, 0.78, 0.92],
        );
        out
    }
}

/// A block a copper bulb can stand on to wake the golem.
pub fn copper_base(id: &str) -> bool {
    matches!(
        id,
        "copper_grate" | "copper_chest" | "copper_bulb" | "copper_door" | "copper_trapdoor"
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
        let mut out = Vec::new();
        push_box(
            &mut out,
            [x, y + 0.32, z],
            [0.2, 0.22, 0.14],
            [0.78, 0.46, 0.26],
        );
        push_box(
            &mut out,
            [x, y + 0.66, z],
            [0.14, 0.12, 0.12],
            [0.86, 0.62, 0.34],
        );
        push_box(
            &mut out,
            [x, y + 0.34, z + 0.12],
            [0.06, 0.08, 0.04],
            [0.42, 0.64, 0.44],
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

    /// Sit or stand. False when the wolf is still wild. True when it sits.
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
        let drop = if self.sitting { 0.12 } else { 0.0 };
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
        push_box(
            &mut out,
            [x - 0.24, y + 0.32 - drop, z],
            [0.08, 0.06, 0.05],
            [0.28, 0.28, 0.32],
        );
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

    /// Sit or stand. False when the cat is still wild. True when it sits.
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
        let body_y = if self.sitting { 0.16 } else { 0.22 };
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
        push_box(
            &mut out,
            [x - 0.18, y + body_y + 0.08, z],
            [0.06, 0.04, 0.04],
            [0.42, 0.22, 0.1],
        );
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

    /// Perch on a shoulder, or hop off. False when the parrot is still wild.
    /// True when it is perched.
    pub fn perch(&mut self) -> bool {
        if !self.tamed {
            return false;
        }
        self.shoulder = !self.shoulder;
        self.shoulder
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
    /// `Some` when an echo is due.
    pub fn tick(&mut self, count: u32, player: [f32; 3], world: &Cluster) -> Option<Mimic> {
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
                self.origin = [player[0] + 0.4, player[1] + 1.35, player[2]];
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
        if self.shoulder {
            return ([0.0, -8.0, 0.0], [0.0, -8.0, 0.0]);
        }
        let [x, y, z] = self.origin;
        ([x - 0.18, y, z - 0.18], [x + 0.18, y + 0.5, z + 0.18])
    }

    pub fn vertices(&self) -> Vec<Vertex> {
        let [x, y, z] = self.origin;
        let mut out = Vec::new();
        push_box(
            &mut out,
            [x, y + 0.22, z],
            [0.12, 0.1, 0.1],
            [0.16, 0.72, 0.28],
        );
        push_box(
            &mut out,
            [x + 0.1, y + 0.34, z],
            [0.07, 0.07, 0.07],
            [0.86, 0.2, 0.16],
        );
        push_box(
            &mut out,
            [x + 0.16, y + 0.32, z],
            [0.04, 0.03, 0.03],
            [0.95, 0.82, 0.2],
        );
        yaw_mesh(&mut out, self.origin, self.yaw);
        out
    }
}

/// Honey is the treat. Feeding the frog asks for a slimeball.
pub fn frog_food(id: &str) -> bool {
    id == "honey"
}

const FROG_HOME: [f32; 3] = [8.0, 4.0, 33.0];
const FROG_LEG: f32 = 1.2;
const FROG_REST: u32 = 8;
const FROG_STEPS: u32 = 6;
const BUG_BACK: u32 = 80;

pub struct Frog {
    origin: [f32; 3],
    yaw: f32,
    steps: u32,
    bug: bool,
    bug_back: u32,
    loose: u32,
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
        }
    }

    pub fn origin(&self) -> [f32; 3] {
        self.origin
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

    /// Pause with the gills open, then swim one side. Follow feet that stay over water.
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
                let held = self.step_water(player[0], player[1], world);
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

    /// Step toward a point when that cell is water. True when already close.
    fn step_water(&mut self, x: f32, z: f32, world: &Cluster) -> bool {
        let dx = x - self.origin[0];
        let dz = z - self.origin[2];
        let dist = (dx * dx + dz * dz).sqrt();
        if dist <= HEEL {
            return true;
        }
        let feet_y = self.origin[1].floor() as i32;
        let Some([next_x, next_z]) = water_step(
            world,
            [self.origin[0], self.origin[2]],
            [x, z],
            STEP.min(dist - HEEL),
            feet_y,
        ) else {
            return false;
        };
        self.yaw = (next_x - self.origin[0]).atan2(next_z - self.origin[2]);
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
    let via = first_open_step(here, aim, &open)?;
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

fn is_fluid(id: &str) -> bool {
    id == "water" || id == "lava"
}

/// Plants, switches, and other shapes a body walks through. Water is a fluid, not one of these.
fn is_soft(id: &str) -> bool {
    matches!(
        id,
        "flower"
            | "button"
            | "lever"
            | "tripwire"
            | "painting"
            | "item_frame"
            | "flower_pot"
            | "cobweb"
            | "sugar_cane"
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
    let next = route_step(from, goal, step, |x, z| {
        fluid_at(world, x, feet_y, z) && !cell_blocks(world, x, feet_y + 1, z)
    })?;
    let ix = next[0].floor() as i32;
    let iz = next[1].floor() as i32;
    fluid_at(world, ix, feet_y, iz).then_some(next)
}

/// The neighbor that starts a path of at most eight blocks toward `aim`.
fn first_open_step(
    start: (i32, i32),
    aim: (i32, i32),
    open: &impl Fn(i32, i32) -> bool,
) -> Option<(i32, i32)> {
    const REACH_CELLS: i32 = 8;
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
        if (cell.0 - start.0).abs() >= REACH_CELLS || (cell.1 - start.1).abs() >= REACH_CELLS {
            continue;
        }
        for (ox, oz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            let next = (cell.0 + ox, cell.1 + oz);
            if (next.0 - start.0).abs() > REACH_CELLS || (next.1 - start.1).abs() > REACH_CELLS {
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
const BEE_HOME: [f32; 3] = [14.0, 2.6, 6.0];
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
                        block.id == "flower"
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

const GUEST_HEARTS: i32 = 4;

/// A mob a script registered. Default pink boxes; optional Blockbench model and hostile AI.
pub struct Guest {
    id: String,
    origin: [f32; 3],
    model: Option<String>,
    hostile: bool,
    hearts: i32,
    yaw: f32,
    pose: f32,
    swing: u32,
    moving: bool,
    attacking: bool,
}

impl Guest {
    fn bare(id: String, origin: [f32; 3], model: Option<String>, hostile: bool) -> Self {
        Self {
            id,
            origin,
            model,
            hostile,
            hearts: GUEST_HEARTS,
            yaw: 0.0,
            pose: 0.0,
            swing: 0,
            moving: false,
            attacking: false,
        }
    }

    /// One body per id. The first stands at x = 10.5, z = 12.5. The next ones step east.
    pub fn line(ids: &[String]) -> Vec<Self> {
        Self::line_defs(
            &ids.iter()
                .map(|id| (id.clone(), None, false))
                .collect::<Vec<_>>(),
        )
    }

    /// Same as [`Self::line`], with per-id model path and hostile flag from the pack.
    pub fn line_defs(defs: &[(String, Option<String>, bool)]) -> Vec<Self> {
        defs.iter()
            .enumerate()
            .map(|(index, (id, model, hostile))| {
                Self::bare(
                    id.clone(),
                    [10.5 + index as f32 * 1.25, 1.0, 12.5],
                    model.clone(),
                    *hostile,
                )
            })
            .collect()
    }

    /// The registered line, then each body from `entity.spawn`. A spawn stands in the middle of its cell.
    pub fn placed(ids: &[String], spawns: &[(String, i32, i32, i32)]) -> Vec<Self> {
        Self::placed_defs(
            &ids.iter()
                .map(|id| (id.clone(), None, false))
                .collect::<Vec<_>>(),
            spawns,
        )
    }

    /// Same as [`Self::placed`], carrying model/hostile from `Pack::script_entity_defs`.
    pub fn placed_defs(
        defs: &[(String, Option<String>, bool)],
        spawns: &[(String, i32, i32, i32)],
    ) -> Vec<Self> {
        let mut guests = Self::line_defs(defs);
        for (id, x, y, z) in spawns {
            let (model, hostile) = defs
                .iter()
                .find(|(known, _, _)| known == id)
                .map(|(_, model, hostile)| (model.clone(), *hostile))
                .unwrap_or((None, false));
            guests.push(Self::bare(
                id.clone(),
                [*x as f32 + 0.5, *y as f32, *z as f32 + 0.5],
                model,
                hostile,
            ));
        }
        guests
    }

    /// Append one live body from a hook-time `entity.spawn`.
    pub fn push_live(
        guests: &mut Vec<Self>,
        id: String,
        x: i32,
        y: i32,
        z: i32,
        model: Option<String>,
        hostile: bool,
    ) {
        let origin = [x as f32 + 0.5, y as f32, z as f32 + 0.5];
        if guests
            .iter()
            .any(|guest| guest.id == id && (guest.origin[0] - origin[0]).abs() < 0.01 && (guest.origin[2] - origin[2]).abs() < 0.01)
        {
            return;
        }
        guests.push(Self::bare(id, origin, model, hostile));
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
        dx * dx + dy * dy + dz * dz < 0.81
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
        assert!(parrot.tick(ECHO_AFTER, [0.0, 1.0, 0.0], &ground).is_none());
        assert!(parrot.feed());
        parrot.hear(Mimic::Note(3));
        parrot.hear(Mimic::Horn);
        assert_eq!(
            parrot.tick(ECHO_AFTER, [home[0] + 8.0, 1.0, home[2]], &ground),
            Some(Mimic::Note(3))
        );
        assert!(
            parrot
                .tick(1, [home[0] + 8.0, 1.0, home[2]], &ground)
                .is_none()
        );
        assert!(parrot.origin()[0] > home[0]);
        assert!(parrot.perch());
        assert!(parrot.shoulder());
        assert!(parrot.tick(1, [2.0, 1.0, 3.0], &ground).is_none());
        assert_eq!(parrot.origin(), [2.4, 2.35, 3.0]);
        parrot.hear(Mimic::Horn);
        assert_eq!(
            parrot.tick(ECHO_AFTER, [2.0, 1.0, 3.0], &ground),
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
        assert!((home[1] - 4.0).abs() < 0.01);
        assert!((home[2] - 33.0).abs() < 0.01);
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
        assert!(frog.tick(3, &ground));
        assert!(frog.origin()[0] > 9.0);
        assert!(
            !frog
                .vertices()
                .iter()
                .any(|vertex| vertex.color[1] > 0.8 && vertex.color[0] < 0.4)
        );
        assert!(frog.feed());
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
            parrot.tick(1, [14.0, 1.0, 11.0], &world);
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
        parrot.tick(1, [nest[0] + 20.0, nest[1], nest[2]], &ground);
        assert!((parrot.origin()[0] - (nest[0] + 20.0)).abs() < 3.0);
        assert!(parrot.perch());
        parrot.tick(1, [nest[0] + 30.0, nest[1], nest[2]], &ground);
        assert!((parrot.origin()[0] - (nest[0] + 30.4)).abs() < 0.01);
        let perched = parrot.origin();
        parrot.tick(4, [nest[0] + 30.0, nest[1], nest[2]], &ground);
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
}
