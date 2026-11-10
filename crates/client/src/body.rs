//! Player box, gravity, and collision against shape boxes.
//!
//! The body is an axis-aligned box under the camera. Each solid element in
//! the pack stops it, including both boxes of the oak stairs and a model's
//! plane. Movement is one axis at a time, so a wall slides instead of sticking.
//! A rise of at most one block is walked up when the body fits above it and
//! the landing is a real platform. Taller walls stop the walk. Jump still
//! leaves the ground for those. A ceiling the raised body would enter blocks
//! the step. The loaded world's horizontal edge is a wall too, so the player
//! cannot walk past it.

use glam::Vec3;
use rebuild_world::{Cluster, for_each_collider, shape_of};

use crate::camera::{Camera, MoveInput};

pub const HALF_WIDTH: f32 = 0.3;
pub const HEIGHT: f32 = 1.8;
pub const EYE_HEIGHT: f32 = 1.62;
/// Hitbox height while sneaking (Minecraft-like).
pub const CROUCH_HEIGHT: f32 = 1.5;
/// View eye height while sneaking, relative to the feet.
pub const CROUCH_EYE: f32 = 1.27;

const GRAVITY: f32 = 28.0;
const JUMP_SPEED: f32 = 8.6;
/// Horizontal speed while any part of the body is in water.
const WATER_DRAG: f32 = 0.35;
/// Horizontal speed inside a cobweb, as a fraction of a normal walk.
pub const COBWEB_DRAG: f32 = 0.25;
/// Horizontal speed while standing on soul sand, as a fraction of a normal walk.
/// Half speed is a lighter drag than a cobweb's quarter.
pub const SOUL_SAND_DRAG: f32 = 0.5;
/// Horizontal speed while crouching, as a fraction of a normal walk.
pub const CROUCH_DRAG: f32 = 0.3;
/// Fastest fall, in blocks per second, while the body is inside a cobweb.
pub const COBWEB_FALL: f32 = 1.2;
/// How quickly a walk on ice eases toward full speed, in blocks per second squared.
pub const ICE_ACCEL: f32 = 10.0;
/// Coast decay on ice, per second. About half the speed is gone after one second.
pub const ICE_DRAG: f32 = 0.7;
/// Gravity inside a pool at least two blocks deep.
const WATER_GRAVITY: f32 = 6.0;
/// Upward acceleration that holds a swimmer near the surface.
const BUOYANCY: f32 = 14.0;
const SWIM_UP: f32 = 4.5;
const WALK_SPEED: f32 = 6.0;
/// Creative flight horizontal speed (a touch under sprint).
const FLY_SPEED: f32 = 12.0;
/// Creative flight vertical speed while holding Space / Shift.
const FLY_VERT: f32 = 8.0;
/// Vertical speed while the body overlaps a ladder or a vine, in blocks per second.
const CLIMB_SPEED: f32 = 4.0;
const FAST_SPEED: f32 = 16.0;
const SKIN: f32 = 1.0e-3;
/// Highest rise a walk will climb. Anything taller needs a jump.
const STEP_HEIGHT: f32 = 1.0;
/// A post or a sign board is thinner than this, so those stay walls.
const MIN_PLATFORM: f32 = 0.4;

#[derive(Clone, Copy)]
struct Aabb {
    min: Vec3,
    max: Vec3,
}

#[cfg_attr(not(test), allow(dead_code))]
pub fn step(camera: &mut Camera, dt: f32, input: &MoveInput, world: &Cluster) {
    step_past(camera, dt, input, world, &[]);
}

/// `step`, plus solid boxes that are not blocks. A sheep uses this.
pub fn step_past(
    camera: &mut Camera,
    dt: f32,
    input: &MoveInput,
    world: &Cluster,
    obstacles: &[([f32; 3], [f32; 3])],
) {
    step_paced(camera, dt, input, world, obstacles, 1.0);
}

/// `step_past` at `pace`. `1` is a normal walk. Empty hunger uses a smaller pace.
pub fn step_paced(
    camera: &mut Camera,
    dt: f32,
    input: &MoveInput,
    world: &Cluster,
    obstacles: &[([f32; 3], [f32; 3])],
    pace: f32,
) {
    let extras: Vec<Aabb> = obstacles
        .iter()
        .map(|bounds| Aabb {
            min: Vec3::from(bounds.0),
            max: Vec3::from(bounds.1),
        })
        .collect();
    let dt = dt.clamp(0.0, 0.05);
    // Stay crouched under a low ceiling even after Shift is released.
    let crouch = input.crouch || !can_stand_with(camera.eye, world, &extras);
    let body_now = player_box(camera.eye, crouch);
    let wet = overlaps_water(&body_now, world);
    let deep = deep_water(&body_now, world);
    let webbed = inside_cobweb(&body_now, world);
    let soul = feet_on_soul_sand(camera.eye, world);
    let climbing = touches_climb(&body_now, world);
    // A jump frame leaves the ground instead of auto-stepping into the wall.
    let can_step = camera.on_ground && !input.jump && !wet && !climbing;
    // Sneak cling: do not walk off a solid edge into open air.
    let cling = crouch && camera.on_ground && !wet && !climbing;
    if climbing {
        let climb = if input.forward || input.jump {
            CLIMB_SPEED
        } else if input.back {
            -CLIMB_SPEED
        } else {
            0.0
        };
        camera.velocity_y = climb;
    } else if wet && input.jump {
        camera.velocity_y = SWIM_UP;
        camera.on_ground = false;
    } else if camera.on_ground && input.jump {
        camera.velocity_y = JUMP_SPEED;
        camera.on_ground = false;
    }
    if climbing {
        // The ladder or vine holds you. Gravity comes back on the frame you leave it.
    } else if deep {
        camera.velocity_y -= WATER_GRAVITY * dt;
        camera.velocity_y += BUOYANCY * dt;
        camera.velocity_y = camera.velocity_y.clamp(-2.5, 4.0);
    } else {
        camera.velocity_y -= GRAVITY * dt;
        if webbed && camera.velocity_y < -COBWEB_FALL {
            camera.velocity_y = -COBWEB_FALL;
        }
    }

    let (forward, right) = camera.walk_axes();
    let mut wish = Vec3::ZERO;
    if input.forward {
        wish += forward;
    }
    if input.back {
        wish -= forward;
    }
    if input.right {
        wish += right;
    }
    if input.left {
        wish -= right;
    }
    if wish.length_squared() > 0.0 {
        let speed = if crouch {
            WALK_SPEED * CROUCH_DRAG
        } else if input.fast {
            FAST_SPEED
        } else {
            WALK_SPEED
        };
        let speed = if webbed {
            speed * COBWEB_DRAG
        } else if soul {
            speed * SOUL_SAND_DRAG
        } else if wet {
            speed * WATER_DRAG
        } else {
            speed
        };
        let speed = speed * pace.clamp(0.25, 1.0);
        wish = wish.normalize() * speed;
    }

    let icy = feet_on_ice(camera.eye, world);
    let glide = if icy {
        coast_on_ice(&mut camera.slide, wish, dt)
    } else {
        camera.slide = Vec3::ZERO;
        wish
    };
    let mut eye = camera.eye;
    let mut body = player_box(eye, crouch);
    let mut rise_left = if can_step { STEP_HEIGHT } else { 0.0 };
    rise_left = move_horizontal(
        &mut body,
        &mut eye,
        0,
        glide.x * dt,
        rise_left,
        cling,
        world,
        &extras,
    );
    let _rise_left = move_horizontal(
        &mut body,
        &mut eye,
        2,
        glide.z * dt,
        rise_left,
        cling,
        world,
        &extras,
    );
    let wanted_y = camera.velocity_y * dt;
    let dy = clip(&body, 1, wanted_y, world, &extras);
    eye.y += dy;
    if wanted_y < 0.0 && dy > wanted_y + 1.0e-4 {
        camera.on_ground = true;
        camera.velocity_y = 0.0;
    } else if wanted_y > 0.0 && dy < wanted_y - 1.0e-4 {
        camera.on_ground = false;
        camera.velocity_y = 0.0;
    } else {
        camera.on_ground = false;
    }
    if climbing {
        camera.on_ground = true;
    }
    camera.eye = eye;
}

/// Creative flight: no gravity, Space rises, Shift sinks, WASD glides.
///
/// Collision still stops the body against solids so you cannot clip through
/// walls. The hitbox stays standing height (Shift is vertical only).
pub fn step_fly(
    camera: &mut Camera,
    dt: f32,
    input: &MoveInput,
    world: &Cluster,
    obstacles: &[([f32; 3], [f32; 3])],
) {
    let extras: Vec<Aabb> = obstacles
        .iter()
        .map(|bounds| Aabb {
            min: Vec3::from(bounds.0),
            max: Vec3::from(bounds.1),
        })
        .collect();
    let dt = dt.clamp(0.0, 0.05);
    camera.on_ground = false;
    camera.velocity_y = if input.jump {
        FLY_VERT
    } else if input.crouch {
        -FLY_VERT
    } else {
        0.0
    };

    let (forward, right) = camera.walk_axes();
    let mut wish = Vec3::ZERO;
    if input.forward {
        wish += forward;
    }
    if input.back {
        wish -= forward;
    }
    if input.right {
        wish += right;
    }
    if input.left {
        wish -= right;
    }
    if wish.length_squared() > 0.0 {
        let speed = if input.fast {
            FAST_SPEED
        } else {
            FLY_SPEED
        };
        wish = wish.normalize() * speed;
    }
    camera.slide = Vec3::ZERO;

    let mut eye = camera.eye;
    let mut body = player_box(eye, false);
    let _ = move_horizontal(&mut body, &mut eye, 0, wish.x * dt, 0.0, false, world, &extras);
    let _ = move_horizontal(&mut body, &mut eye, 2, wish.z * dt, 0.0, false, world, &extras);
    let wanted_y = camera.velocity_y * dt;
    let dy = clip(&body, 1, wanted_y, world, &extras);
    eye.y += dy;
    if wanted_y > 0.0 && dy < wanted_y - 1.0e-4 {
        camera.velocity_y = 0.0;
    } else if wanted_y < 0.0 && dy > wanted_y + 1.0e-4 {
        camera.velocity_y = 0.0;
    }
    camera.eye = eye;
}

/// Hearts a player starts with, and returns to after a fatal fall.
pub const HEARTS: i32 = 10;

/// Hearts after one healing potion. Two points, and the bar does not pass full.
pub fn potion_hearts(hearts: i32) -> i32 {
    (hearts + 2).min(HEARTS)
}

/// Hunger points a player starts with.
pub const HUNGER: i32 = 10;

/// Seconds of walking that spend one hunger point.
pub const WALK_HUNGER: f32 = 4.0;

/// Seconds of mining that spend one hunger point.
pub const WORK_HUNGER: f32 = 2.0;

/// Seconds at empty hunger before one heart is spent.
pub const STARVE_EVERY: f32 = 4.0;

/// Walk speed while hunger is empty.
pub const STARVE_PACE: f32 = 0.5;

/// Food needed before idle heart regen starts.
pub const REGEN_FOOD: i32 = 8;

/// Seconds of stillness at high food before one heart returns.
pub const REGEN_EVERY: f32 = 4.0;

/// What an empty stomach did this frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HungerTick {
    None,
    Hurt,
    /// Well-fed and idle long enough to restore one heart.
    Heal,
}

/// Food points, plus the timers that spend them and then hearts.
#[derive(Clone, Copy, Debug)]
pub struct Hunger {
    food: i32,
    work: f32,
    starve: f32,
    regen: f32,
}

impl Hunger {
    pub fn full() -> Self {
        Self {
            food: HUNGER,
            work: 0.0,
            starve: 0.0,
            regen: 0.0,
        }
    }

    pub fn food(&self) -> i32 {
        self.food
    }

    /// Force the bar for proof shots and tests. Clamped to `0..=HUNGER`.
    pub fn set_food(&mut self, food: i32) {
        self.food = food.clamp(0, HUNGER);
        if self.food > 0 {
            self.starve = 0.0;
        }
        self.work = 0.0;
        self.regen = 0.0;
    }

    /// `0` when fed enough, up to `1` when empty. Drives the hunger edge veil.
    pub fn hunger_veil(&self) -> f32 {
        if self.food >= 3 {
            0.0
        } else {
            (3 - self.food) as f32 / 3.0
        }
    }

    /// `1` with food left. Half speed when the bar is empty.
    pub fn pace(&self) -> f32 {
        if self.food <= 0 { STARVE_PACE } else { 1.0 }
    }

    /// Drain while moving, mining, or sprinting (`hard`). At zero, hearts drain.
    /// Well-fed, still, and wounded restores a heart and spends one food.
    pub fn tick(&mut self, dt: f32, moving: bool, hard: bool, wounded: bool) -> HungerTick {
        if dt <= 0.0 {
            return HungerTick::None;
        }
        if self.food <= 0 {
            self.work = 0.0;
            self.regen = 0.0;
            self.starve += dt;
            if self.starve >= STARVE_EVERY {
                self.starve -= STARVE_EVERY;
                return HungerTick::Hurt;
            }
            return HungerTick::None;
        }
        self.starve = 0.0;
        if !moving && !hard {
            if wounded && self.food >= REGEN_FOOD {
                self.regen += dt;
                if self.regen >= REGEN_EVERY {
                    self.regen -= REGEN_EVERY;
                    self.food -= 1;
                    self.work = 0.0;
                    return HungerTick::Heal;
                }
                return HungerTick::None;
            }
            self.regen = 0.0;
            return HungerTick::None;
        }
        self.regen = 0.0;
        let per = if hard { WORK_HUNGER } else { WALK_HUNGER };
        self.work += dt;
        while self.work >= per && self.food > 0 {
            self.work -= per;
            self.food -= 1;
        }
        if self.food == 0 {
            self.work = 0.0;
        }
        HungerTick::None
    }

    /// Add food, capped at a full bar. False when the bar is already full.
    pub fn eat(&mut self, amount: i32) -> bool {
        if amount <= 0 || self.food >= HUNGER {
            return false;
        }
        self.food = (self.food + amount).min(HUNGER);
        self.starve = 0.0;
        true
    }
}

/// A short hop onto a nearby solid floor. The same spot always picks the same landing.
pub fn chorus_hop(eye: Vec3, world: &Cluster) -> Option<Vec3> {
    let x = eye.x.floor() as i32;
    let feet = (eye.y - EYE_HEIGHT + 1.0e-3).floor() as i32;
    let z = eye.z.floor() as i32;
    if feet <= 0 {
        return None;
    }
    let start = (x.wrapping_mul(31) ^ z.wrapping_mul(17)).unsigned_abs() as usize;
    const STEPS: [(i32, i32); 8] = [
        (2, 0),
        (-2, 0),
        (0, 2),
        (0, -2),
        (3, 1),
        (-1, 3),
        (2, -2),
        (-3, 1),
    ];
    for step in 0..STEPS.len() {
        let (dx, dz) = STEPS[(start + step) % STEPS.len()];
        let nx = x + dx;
        let nz = z + dz;
        if !world.block_at(nx, feet, nz).is_air() || !world.block_at(nx, feet + 1, nz).is_air() {
            continue;
        }
        if !shape_of(world.block_at(nx, feet - 1, nz)).is_full_cube() {
            continue;
        }
        return Some(Vec3::new(
            nx as f32 + 0.5,
            feet as f32 + EYE_HEIGHT,
            nz as f32 + 0.5,
        ));
    }
    None
}

/// A short hop onto a nearby floor. The same cell always picks the same spot.
pub fn egg_hop(at: [i32; 3], world: &Cluster) -> Option<[i32; 3]> {
    let start = (at[0].wrapping_mul(31) ^ at[2].wrapping_mul(17)).unsigned_abs() as usize;
    const STEPS: [(i32, i32, i32); 8] = [
        (2, 0, 0),
        (-2, 0, 0),
        (0, 0, 2),
        (0, 0, -2),
        (2, 1, 0),
        (-2, 1, 0),
        (0, 1, 2),
        (1, 0, 2),
    ];
    for step in 0..STEPS.len() {
        let (dx, dy, dz) = STEPS[(start + step) % STEPS.len()];
        let nx = at[0] + dx;
        let ny = at[1] + dy;
        let nz = at[2] + dz;
        if ny <= 0 || !world.block_at(nx, ny, nz).is_air() {
            continue;
        }
        if !shape_of(world.block_at(nx, ny - 1, nz)).is_full_cube() {
            continue;
        }
        return Some([nx, ny, nz]);
    }
    None
}

/// True when a punched end crystal's blast reaches the player. Four blocks.
pub fn crystal_reaches(eye: Vec3, at: [i32; 3]) -> bool {
    let dx = eye.x - (at[0] as f32 + 0.5);
    let dy = eye.y - (at[1] as f32 + 0.5);
    let dz = eye.z - (at[2] as f32 + 0.5);
    dx * dx + dy * dy + dz * dz < 16.0
}

/// Where a thrown pearl lands. A solid along the look stops it; open air rides the ray.
pub fn pearl_landing(eye: Vec3, forward: Vec3, world: &Cluster) -> Option<Vec3> {
    if forward.length_squared() < 1.0e-8 {
        return None;
    }
    let forward = forward.normalize();
    let mut last_air: Option<[i32; 3]> = None;
    let mut t = 0.5_f32;
    while t <= 12.0 {
        let at = eye + forward * t;
        let cell = [
            at.x.floor() as i32,
            at.y.floor() as i32,
            at.z.floor() as i32,
        ];
        if shape_of(world.block_at(cell[0], cell[1], cell[2])).is_empty() {
            last_air = Some(cell);
            t += 0.5;
            continue;
        }
        break;
    }
    settle_pearl(last_air?, world)
}

fn settle_pearl(cell: [i32; 3], world: &Cluster) -> Option<Vec3> {
    let (x, mut y, z) = (cell[0], cell[1], cell[2]);
    for _ in 0..16 {
        if y < 1 {
            break;
        }
        if shape_of(world.block_at(x, y - 1, z)).is_full_cube()
            && world.block_at(x, y, z).is_air()
            && world.block_at(x, y + 1, z).is_air()
        {
            return Some(Vec3::new(
                x as f32 + 0.5,
                y as f32 + EYE_HEIGHT,
                z as f32 + 0.5,
            ));
        }
        if !shape_of(world.block_at(x, y, z)).is_empty() {
            break;
        }
        y -= 1;
    }
    if world.block_at(cell[0], cell[1], cell[2]).is_air()
        && world.block_at(cell[0], cell[1] + 1, cell[2]).is_air()
    {
        return Some(Vec3::new(
            cell[0] as f32 + 0.5,
            cell[1] as f32 + EYE_HEIGHT,
            cell[2] as f32 + 0.5,
        ));
    }
    None
}

/// How far a wind burst reaches, in blocks.
pub const WIND_REACH: f32 = 3.0;

/// Old rod lift, kept so the burst math stays comparable. The player is not lifted.
#[cfg_attr(not(test), allow(dead_code))]
pub const ROD_LIFT: f32 = 4.3;

/// Seconds the rod used to wait. The rod no longer launches you.
#[cfg_attr(not(test), allow(dead_code))]
pub const ROD_WAIT: f32 = 0.8;

/// `Some` when a rod cooldown would have elapsed. The player is not lifted.
#[cfg_attr(not(test), allow(dead_code))]
pub fn rod_ready(wait: f32) -> Option<f32> {
    if wait > 0.0 { None } else { Some(ROD_WAIT) }
}

/// A shove away from a burst. Mobs may use it. The player does not.
pub struct Gust {
    pub dx: f32,
    pub dz: f32,
    /// Upward part of a burst. Mobs ignore it, and the player is not moved.
    #[cfg_attr(not(test), allow(dead_code))]
    pub lift: f32,
}

/// Where a thrown wind charge bursts. The last open cell along the look.
pub fn wind_spot(eye: Vec3, forward: Vec3, world: &Cluster) -> Option<[f32; 3]> {
    if forward.length_squared() < 1.0e-8 {
        return None;
    }
    let forward = forward.normalize();
    let mut last = None;
    let mut t = 0.5_f32;
    while t <= 8.0 {
        let at = eye + forward * t;
        let cell = [
            at.x.floor() as i32,
            at.y.floor() as i32,
            at.z.floor() as i32,
        ];
        if shape_of(world.block_at(cell[0], cell[1], cell[2])).is_empty() {
            last = Some([
                cell[0] as f32 + 0.5,
                cell[1] as f32 + 0.5,
                cell[2] as f32 + 0.5,
            ]);
            t += 0.5;
            continue;
        }
        break;
    }
    last
}

/// Push `at` away from `burst` when it stands inside the burst.
pub fn wind_push(burst: [f32; 3], at: [f32; 3]) -> Option<Gust> {
    let dx = at[0] - burst[0];
    let dy = at[1] - burst[1];
    let dz = at[2] - burst[2];
    let dist2 = dx * dx + dy * dy + dz * dz;
    if dist2 > WIND_REACH * WIND_REACH {
        return None;
    }
    let dist = dist2.sqrt();
    let (ux, uz) = if dist < 0.05 {
        (0.0, 0.0)
    } else {
        (dx / dist, dz / dist)
    };
    let power = 1.0 - dist / WIND_REACH;
    Some(Gust {
        dx: ux * 1.6 * power,
        dz: uz * 1.6 * power,
        lift: 8.6 * power,
    })
}

/// A breeze's old shot. Nothing applies this to the player.
#[cfg_attr(not(test), allow(dead_code))]
pub fn breeze_shot(from: [f32; 3], at: [f32; 3]) -> Option<Gust> {
    let dx = at[0] - from[0];
    let dz = at[2] - from[2];
    let dist2 = dx * dx + dz * dz;
    if !(0.25..=256.0).contains(&dist2) {
        return None;
    }
    let dist = dist2.sqrt();
    Some(Gust {
        dx: dx / dist * 1.4,
        dz: dz / dist * 1.4,
        lift: ROD_LIFT,
    })
}

/// Slices in one cake. The last bite removes the block.
pub const CAKE_SLICES: u8 = 6;

/// Hunger restored by one slice.
pub const CAKE_FOOD: i32 = 2;

/// The next bite of a cake.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CakeBite {
    /// Metadata after this slice is eaten.
    Slice(u8),
    /// This was the last slice.
    Finish,
}

/// Six slices, then the cake is gone.
pub fn cake_bite(metadata: u8) -> CakeBite {
    let eaten = metadata.min(CAKE_SLICES - 1);
    if eaten + 1 >= CAKE_SLICES {
        CakeBite::Finish
    } else {
        CakeBite::Slice(eaten + 1)
    }
}

/// Hearts left after `soak` points of armor. Extra soak does not heal.
pub fn soaked(amount: i32, soak: i32) -> i32 {
    (amount - soak.max(0)).max(0)
}

/// Hearts a mace swing takes. A falling swing deals `smash` hearts.
pub fn mace_hearts(falling: bool, smash: i32) -> i32 {
    if falling { smash.max(1) } else { 1 }
}

/// Seconds before the mace can swing again.
pub const MACE_WAIT: f32 = 0.5;

/// `Some` when a swing may land, carrying the cooldown to store.
pub fn mace_ready(wait: f32) -> Option<f32> {
    if wait > 0.0 { None } else { Some(MACE_WAIT) }
}

/// Hearts a zombie bite costs.
///
/// A held shield stops the bite, so armor does not see it. A bite that gets
/// through is soaked by the worn pieces.
pub fn bite_hearts(shield: bool, amount: i32, soak: i32) -> i32 {
    if shield { 0 } else { soaked(amount, soak) }
}

/// Three worn pieces. `0` is the cap, `1` the tunic, `2` the boots.
#[derive(Clone, Copy, Debug, Default)]
pub struct Armor {
    slots: [Option<&'static str>; 3],
}

impl Armor {
    pub fn new() -> Self {
        Self { slots: [None; 3] }
    }

    pub fn worn(self) -> [Option<&'static str>; 3] {
        self.slots
    }

    /// Wear `id` in `slot`. The piece that was there comes back.
    pub fn equip(&mut self, slot: usize, id: &'static str) -> Option<&'static str> {
        let Some(cell) = self.slots.get_mut(slot) else {
            return None;
        };
        let old = *cell;
        *cell = Some(id);
        old
    }

    /// Take off the piece in `slot`.
    pub fn clear(&mut self, slot: usize) -> Option<&'static str> {
        self.slots.get_mut(slot).and_then(Option::take)
    }
}

/// Hearts left, and whether the fall was fatal.
///
/// A drop under four blocks changes nothing. At zero hearts the player
/// respawns with a full set. Play applies armor first, then spends hearts.
#[cfg_attr(not(test), allow(dead_code))]
pub fn survive_fall(hearts: i32, drop: f32) -> (i32, bool) {
    let hurt = fall_hurt(drop);
    if hurt <= 0 {
        return (hearts, false);
    }
    let left = hearts - hurt;
    if left <= 0 {
        (HEARTS, true)
    } else {
        (left, false)
    }
}

/// Hearts of fall damage. A drop under four blocks, including a normal jump, is 0.
pub fn fall_hurt(drop: f32) -> i32 {
    if drop < 4.0 {
        return 0;
    }
    (drop - 3.0).floor() as i32
}

pub fn feet_y(eye_y: f32) -> f32 {
    eye_y - EYE_HEIGHT
}

/// True when any part of the body overlaps a water block.
pub fn in_water(eye: Vec3, world: &Cluster) -> bool {
    overlaps_water(&player_box(eye, false), world)
}

/// True when the camera eye sits inside a water cell (fully submerged view).
pub fn eye_in_water(eye: Vec3, world: &Cluster) -> bool {
    let x = eye.x.floor() as i32;
    let y = eye.y.floor() as i32;
    let z = eye.z.floor() as i32;
    (0..256).contains(&y) && world.block_at(x, y, z).id == "water"
}

/// The block the feet are on. Water at the soles wins over the floor beneath.
pub fn ground_id(world: &Cluster, eye: Vec3) -> &'static str {
    let feet = feet_y(eye.y);
    let x = eye.x.floor() as i32;
    let z = eye.z.floor() as i32;
    let wading = (feet + 0.15).floor() as i32;
    if world.block_at(x, wading, z).id == "water" {
        return "water";
    }
    let sole = (feet - 0.02).floor() as i32;
    world.block_at(x, sole, z).id
}

/// True when Space would launch off the ground this step.
///
/// A swim and a climb also read Space, and those are not a jump.
pub fn ground_jump(camera: &Camera, world: &Cluster) -> bool {
    if !camera.on_ground {
        return false;
    }
    let body = player_box(camera.eye, false);
    !overlaps_water(&body, world) && !touches_climb(&body, world)
}

/// Seconds between cactus pokes while the body stays against one.
pub const CACTUS_POKE: f32 = 0.6;
/// Extra reach so a body stopped against the spines still counts as touching.
const CACTUS_REACH: f32 = 0.05;

/// True when the body, grown a little sideways, overlaps a cactus box.
pub fn touches_cactus(eye: Vec3, world: &Cluster) -> bool {
    let (body_min, body_max) = player_box_bounds(eye);
    let min = [
        body_min[0] - CACTUS_REACH,
        body_min[1],
        body_min[2] - CACTUS_REACH,
    ];
    let max = [
        body_max[0] + CACTUS_REACH,
        body_max[1],
        body_max[2] + CACTUS_REACH,
    ];
    let x0 = min[0].floor() as i32;
    let y0 = min[1].floor() as i32;
    let z0 = min[2].floor() as i32;
    let x1 = (max[0] - 1.0e-4).floor() as i32;
    let y1 = (max[1] - 1.0e-4).floor() as i32;
    let z1 = (max[2] - 1.0e-4).floor() as i32;
    let mut x = x0;
    while x <= x1 {
        let mut y = y0;
        while y <= y1 {
            if (0..256).contains(&y) {
                let mut z = z0;
                while z <= z1 {
                    let block = world.block_at(x, y, z);
                    if block.id == "cactus" && cactus_box_hits(x, y, z, min, max) {
                        return true;
                    }
                    z += 1;
                }
            }
            y += 1;
        }
        x += 1;
    }
    false
}

fn cactus_box_hits(x: i32, y: i32, z: i32, min: [f32; 3], max: [f32; 3]) -> bool {
    let shape = shape_of(rebuild_world::Block::new("cactus", 0));
    shape.boxes().iter().any(|cell| {
        let bmin = [
            x as f32 + f32::from(cell.min[0]) / 16.0,
            y as f32 + f32::from(cell.min[1]) / 16.0,
            z as f32 + f32::from(cell.min[2]) / 16.0,
        ];
        let bmax = [
            x as f32 + f32::from(cell.max[0]) / 16.0,
            y as f32 + f32::from(cell.max[1]) / 16.0,
            z as f32 + f32::from(cell.max[2]) / 16.0,
        ];
        min[0] < bmax[0]
            && max[0] > bmin[0]
            && min[1] < bmax[1]
            && max[1] > bmin[1]
            && min[2] < bmax[2]
            && max[2] > bmin[2]
    })
}

/// The next cooldown and whether this tick pokes. Leaving the cactus clears the wait.
pub fn cactus_poke(touching: bool, left: f32, dt: f32) -> (f32, bool) {
    if !touching {
        return (0.0, false);
    }
    let left = left - dt;
    if left <= 0.0 {
        (CACTUS_POKE, true)
    } else {
        (left, false)
    }
}

pub fn player_box_bounds(eye: Vec3) -> ([f32; 3], [f32; 3]) {
    player_box_bounds_at(eye, false)
}

/// Standing or crouched body extents under the camera eye.
pub fn player_box_bounds_at(eye: Vec3, crouch: bool) -> ([f32; 3], [f32; 3]) {
    let body = player_box(eye, crouch);
    (
        [body.min.x, body.min.y, body.min.z],
        [body.max.x, body.max.y, body.max.z],
    )
}

/// True when a standing body fits where a crouched one already stands.
pub fn can_stand(eye: Vec3, world: &Cluster, obstacles: &[([f32; 3], [f32; 3])]) -> bool {
    let extras: Vec<Aabb> = obstacles
        .iter()
        .map(|bounds| Aabb {
            min: Vec3::from(bounds.0),
            max: Vec3::from(bounds.1),
        })
        .collect();
    can_stand_with(eye, world, &extras)
}

fn can_stand_with(eye: Vec3, world: &Cluster, extra: &[Aabb]) -> bool {
    let crouched = player_box(eye, true);
    let rise = HEIGHT - CROUCH_HEIGHT;
    let up = clip(&crouched, 1, rise, world, extra);
    up >= rise - 1.0e-3
}

fn player_box(eye: Vec3, crouch: bool) -> Aabb {
    let feet = feet_y(eye.y);
    let height = if crouch { CROUCH_HEIGHT } else { HEIGHT };
    Aabb {
        min: Vec3::new(eye.x - HALF_WIDTH, feet, eye.z - HALF_WIDTH),
        max: Vec3::new(eye.x + HALF_WIDTH, feet + height, eye.z + HALF_WIDTH),
    }
}

fn move_horizontal(
    body: &mut Aabb,
    eye: &mut Vec3,
    axis: usize,
    delta: f32,
    rise_left: f32,
    cling: bool,
    world: &Cluster,
    extra: &[Aabb],
) -> f32 {
    let before = *body;
    let before_eye = *eye;
    let left = if let Some((climb, along)) = try_step(body, axis, delta, rise_left, world, extra)
    {
        shift(body, eye, 1, climb);
        shift(body, eye, axis, along);
        (rise_left - climb).max(0.0)
    } else {
        let moved = clip(body, axis, delta, world, extra);
        shift(body, eye, axis, moved);
        rise_left
    };
    if cling && !stands_on_platform(body, world, extra) {
        *body = before;
        *eye = before_eye;
        return rise_left;
    }
    left
}

fn shift(body: &mut Aabb, eye: &mut Vec3, axis: usize, delta: f32) {
    body.min[axis] += delta;
    body.max[axis] += delta;
    eye[axis] += delta;
}

/// Lift at most `rise_left`, finish the blocked horizontal move, then drop
/// onto a platform. A ceiling cuts the lift short, and a thin solid such as
/// the sign is not a floor, so both leave the walk blocked.
fn try_step(
    body: &Aabb,
    axis: usize,
    delta: f32,
    rise_left: f32,
    world: &Cluster,
    extra: &[Aabb],
) -> Option<(f32, f32)> {
    if rise_left < 0.02 || delta.abs() < 1.0e-8 {
        return None;
    }
    let along_floor = clip(body, axis, delta, world, extra);
    if (delta - along_floor).abs() <= 1.0e-4 {
        return None;
    }
    let up = clip(body, 1, rise_left, world, extra);
    if up < 0.02 {
        return None;
    }
    let mut raised = *body;
    raised.min.y += up;
    raised.max.y += up;
    let along = clip(&raised, axis, delta, world, extra);
    if along.abs() <= along_floor.abs() + 1.0e-4 {
        return None;
    }
    let mut moved = raised;
    moved.min[axis] += along;
    moved.max[axis] += along;
    let down = clip(&moved, 1, -up, world, extra);
    let climb = up + down;
    if !(0.02..=rise_left + 1.0e-3).contains(&climb) {
        return None;
    }
    let mut landed = moved;
    landed.min.y += down;
    landed.max.y += down;
    if !stands_on_platform(&landed, world, extra) {
        return None;
    }
    Some((climb, along))
}

fn stands_on_platform(body: &Aabb, world: &Cluster, extra: &[Aabb]) -> bool {
    let feet = body.min.y;
    let mut probe = *body;
    probe.min.y = feet - 0.08;
    probe.max.y = feet + 0.02;
    let mut found = false;
    for_solids(&probe, world, extra, |solid| {
        if found {
            return;
        }
        let gap = feet - solid.max.y;
        if gap < -1.0e-3 || gap > 0.05 {
            return;
        }
        if body.max.x <= solid.min.x + 1.0e-4 || body.min.x >= solid.max.x - 1.0e-4 {
            return;
        }
        if body.max.z <= solid.min.z + 1.0e-4 || body.min.z >= solid.max.z - 1.0e-4 {
            return;
        }
        let wide = solid.max.x - solid.min.x;
        let deep = solid.max.z - solid.min.z;
        if wide >= MIN_PLATFORM && deep >= MIN_PLATFORM {
            found = true;
        }
    });
    found
}

fn clip(body: &Aabb, axis: usize, delta: f32, world: &Cluster, extra: &[Aabb]) -> f32 {
    if delta.abs() < 1.0e-8 {
        return 0.0;
    }
    let mut swept = *body;
    if delta > 0.0 {
        swept.max[axis] += delta;
    } else {
        swept.min[axis] += delta;
    }
    let mut allowed = delta;
    for_solids(&swept, world, extra, |solid| {
        if !overlaps_other(body, &solid, axis) {
            return;
        }
        // A carpet is one pixel tall. It is not a wall and not a step.
        if axis != 1 && solid.max.y - solid.min.y <= 1.0 / 16.0 + 1.0e-4 {
            return;
        }
        if delta > 0.0 && body.max[axis] <= solid.min[axis] + 1.0e-5 {
            let gap = solid.min[axis] - body.max[axis] - SKIN;
            if gap < allowed {
                allowed = gap.max(0.0);
            }
        } else if delta < 0.0 && body.min[axis] >= solid.max[axis] - 1.0e-5 {
            let gap = solid.max[axis] - body.min[axis] + SKIN;
            if gap > allowed {
                allowed = gap.min(0.0);
            }
        }
    });
    if axis == 0 || axis == 2 {
        let (min_x, max_x, min_z, max_z) = world.bounds();
        let (edge_min, edge_max) = if axis == 0 {
            (min_x as f32, max_x as f32)
        } else {
            (min_z as f32, max_z as f32)
        };
        if delta > 0.0 {
            let gap = edge_max - body.max[axis] - SKIN;
            if gap < allowed {
                allowed = gap.max(0.0);
            }
        } else if delta < 0.0 {
            let limit = edge_min + SKIN - body.min[axis];
            if limit > allowed {
                allowed = limit.min(0.0);
            }
        }
    }
    allowed
}

fn overlaps_other(body: &Aabb, solid: &Aabb, axis: usize) -> bool {
    for other in 0..3 {
        if other == axis {
            continue;
        }
        // Feet rest on a top face. Lifting the sample by the skin keeps a
        // rounding error from catching the vertical side of that same block.
        let body_min = if other == 1 {
            body.min[other] + SKIN
        } else {
            body.min[other]
        };
        if body.max[other] <= solid.min[other] || body_min >= solid.max[other] {
            return false;
        }
    }
    true
}

fn for_solids(bounds: &Aabb, world: &Cluster, extra: &[Aabb], mut visit: impl FnMut(Aabb)) {
    let (x0, x1) = cell_span(bounds.min.x, bounds.max.x);
    let (y0, y1) = cell_span(bounds.min.y, bounds.max.y);
    let (z0, z1) = cell_span(bounds.min.z, bounds.max.z);
    for y in y0..=y1 {
        if !(0..256).contains(&y) {
            continue;
        }
        for z in z0..=z1 {
            for x in x0..=x1 {
                let block = world.block_at(x, y, z);
                if block.id == "water"
                    || block.id == "flower"
                    || block.id == "button"
                    || block.id == "lever"
                    || block.id == "tripwire"
                    || block.id == "painting"
                    || block.id == "item_frame"
                    || block.id == "flower_pot"
                    || block.id == "cobweb"
                    || block.id == "sugar_cane"
                    || block.id == "wheat_crop"
                    || block.id == "chorus_plant"
                    || block.id == "chorus_flower"
                    || block.id == "copper_grate"
                {
                    continue;
                }
                let shape = shape_of(block);
                if shape.is_empty() {
                    continue;
                }
                for_each_collider(block, |min, max| {
                    visit(Aabb {
                        min: Vec3::new(
                            x as f32 + min[0] / 16.0,
                            y as f32 + min[1] / 16.0,
                            z as f32 + min[2] / 16.0,
                        ),
                        max: Vec3::new(
                            x as f32 + max[0] / 16.0,
                            y as f32 + max[1] / 16.0,
                            z as f32 + max[2] / 16.0,
                        ),
                    });
                });
            }
        }
    }
    for solid in extra {
        visit(*solid);
    }
}

fn feet_on_soul_sand(eye: Vec3, world: &Cluster) -> bool {
    let feet = eye.y - EYE_HEIGHT;
    let x = eye.x.floor() as i32;
    let y = (feet - 0.05).floor() as i32;
    let z = eye.z.floor() as i32;
    (0..256).contains(&y) && world.block_at(x, y, z).id == "soul_sand"
}

/// True when the block under the feet is magma. The burn uses `cactus_poke`.
pub fn stands_on_magma(eye: Vec3, world: &Cluster) -> bool {
    let feet = eye.y - EYE_HEIGHT;
    let x = eye.x.floor() as i32;
    let y = (feet - 0.05).floor() as i32;
    let z = eye.z.floor() as i32;
    (0..256).contains(&y) && world.block_at(x, y, z).id == "magma"
}

fn feet_on_ice(eye: Vec3, world: &Cluster) -> bool {
    let feet = eye.y - EYE_HEIGHT;
    let x = eye.x.floor() as i32;
    let y = (feet - 0.05).floor() as i32;
    let z = eye.z.floor() as i32;
    (0..256).contains(&y) && world.block_at(x, y, z).id == "ice"
}

/// Ease toward a held direction. With no keys, keep the leftover and let it fade.
fn coast_on_ice(slide: &mut Vec3, wish: Vec3, dt: f32) -> Vec3 {
    if wish.length_squared() < 1.0e-8 {
        *slide *= (-ICE_DRAG * dt).exp();
        if slide.length_squared() < 1.0e-4 {
            *slide = Vec3::ZERO;
        }
    } else {
        let delta = wish - *slide;
        let cap = ICE_ACCEL * dt;
        if delta.length() > cap {
            *slide += delta.normalize() * cap;
        } else {
            *slide = wish;
        }
    }
    *slide
}

fn inside_cobweb(body: &Aabb, world: &Cluster) -> bool {
    let (x0, x1) = cell_span(body.min.x, body.max.x);
    let (y0, y1) = cell_span(body.min.y, body.max.y);
    let (z0, z1) = cell_span(body.min.z, body.max.z);
    for y in y0..=y1 {
        if !(0..256).contains(&y) {
            continue;
        }
        for z in z0..=z1 {
            for x in x0..=x1 {
                if world.block_at(x, y, z).id == "cobweb" {
                    return true;
                }
            }
        }
    }
    false
}

fn touches_climb(body: &Aabb, world: &Cluster) -> bool {
    let (x0, x1) = cell_span(body.min.x, body.max.x);
    let (y0, y1) = cell_span(body.min.y, body.max.y);
    let (z0, z1) = cell_span(body.min.z, body.max.z);
    for y in y0..=y1 {
        if !(0..256).contains(&y) {
            continue;
        }
        for z in z0..=z1 {
            for x in x0..=x1 {
                let id = world.block_at(x, y, z).id;
                if id == "ladder" || id == "vine" {
                    return true;
                }
            }
        }
    }
    false
}

fn overlaps_water(body: &Aabb, world: &Cluster) -> bool {
    let (x0, x1) = cell_span(body.min.x, body.max.x);
    let (y0, y1) = cell_span(body.min.y, body.max.y);
    let (z0, z1) = cell_span(body.min.z, body.max.z);
    for y in y0..=y1 {
        if !(0..256).contains(&y) {
            continue;
        }
        for z in z0..=z1 {
            for x in x0..=x1 {
                if world.block_at(x, y, z).id == "water" {
                    return true;
                }
            }
        }
    }
    false
}

/// Two water blocks in a column around the body. A one-block sheet is not a pool.
fn deep_water(body: &Aabb, world: &Cluster) -> bool {
    let x = ((body.min.x + body.max.x) * 0.5).floor() as i32;
    let z = ((body.min.z + body.max.z) * 0.5).floor() as i32;
    let y0 = body.min.y.floor() as i32 - 1;
    let y1 = (body.max.y - 1.0e-3).floor() as i32 + 1;
    let mut run = 0i32;
    for y in y0..=y1 {
        if world.block_at(x, y, z).id == "water" {
            run += 1;
            if run >= 2 {
                return true;
            }
        } else {
            run = 0;
        }
    }
    false
}

fn cell_span(min: f32, max: f32) -> (i32, i32) {
    let start = min.floor() as i32;
    let end = (max - 1.0e-4).floor() as i32;
    (start, end.max(start))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rebuild_world::{Block, Chunk, Cluster, OAK_STAIRS, shape_of};

    use crate::camera::Camera;

    fn idle() -> MoveInput {
        MoveInput {
            forward: false,
            back: false,
            left: false,
            right: false,
            jump: false,
            fast: false,
            crouch: false,
        }
    }

    fn cube_floor() -> Cluster {
        let chunk = Chunk::from_blocks(0, 0, [(0, 0, 0, Block::new("stone", 0))]).unwrap();
        Cluster::try_from_chunks(0, 0, [chunk]).unwrap()
    }

    #[test]
    fn the_feet_read_dirt_stone_wood_or_water() {
        let blocks = vec![
            (2u8, 1, 2, Block::new("dirt", 0)),
            (4, 1, 2, Block::new("stone", 0)),
            (6, 1, 2, Block::new("oak_log", 0)),
            (8, 1, 2, Block::new("water", 0)),
            (8, 2, 2, Block::new("water", 0)),
        ];
        let world =
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, blocks).unwrap()]).unwrap();
        let on = |x: f32| Vec3::new(x, 2.0 + EYE_HEIGHT, 2.5);
        assert_eq!(ground_id(&world, on(2.5)), "dirt");
        assert_eq!(ground_id(&world, on(4.5)), "stone");
        assert_eq!(ground_id(&world, on(6.5)), "oak_log");
        assert_eq!(ground_id(&world, on(8.5)), "water");
        assert!(!in_water(on(2.5), &world));
        assert!(in_water(on(8.5), &world));
    }

    #[test]
    fn hunger_veil_ramps_only_when_food_is_critical() {
        let mut hunger = Hunger::full();
        assert_eq!(hunger.hunger_veil(), 0.0);
        hunger.set_food(3);
        assert_eq!(hunger.hunger_veil(), 0.0);
        hunger.set_food(2);
        assert!((hunger.hunger_veil() - 1.0 / 3.0).abs() < 1.0e-5);
        hunger.set_food(0);
        assert_eq!(hunger.hunger_veil(), 1.0);
    }

    #[test]
    fn eye_reads_submerged_only_inside_a_water_cell() {
        let mut blocks = Vec::new();
        for z in 0..4u8 {
            for x in 0..4u8 {
                blocks.push((x, 0, z, Block::new("stone", 0)));
                for y in 1..4u16 {
                    blocks.push((x, y, z, Block::new("water", 0)));
                }
            }
        }
        let world =
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, blocks).unwrap()]).unwrap();
        assert!(eye_in_water(Vec3::new(1.5, 2.4, 1.5), &world));
        assert!(!eye_in_water(Vec3::new(1.5, 4.2, 1.5), &world));
        assert!(!eye_in_water(Vec3::new(1.5, 0.4, 1.5), &world));
    }

    #[test]
    fn water_is_not_a_floor_and_a_pool_lifts() {
        let mut blocks = Vec::new();
        for z in 0..4u8 {
            for x in 0..4u8 {
                blocks.push((x, 0, z, Block::new("stone", 0)));
                blocks.push((x, 2, z, Block::new("water", 0)));
            }
        }
        let sheet =
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, blocks).unwrap()]).unwrap();
        let mut camera = Camera::looking_at(
            Vec3::new(1.5, 2.2 + EYE_HEIGHT, 1.5),
            Vec3::new(1.5, 1.0, 0.5),
        );
        for _ in 0..90 {
            step(&mut camera, 1.0 / 60.0, &idle(), &sheet);
        }
        let feet = camera.eye.y - EYE_HEIGHT;
        assert!(
            feet < 1.2,
            "a one-block sheet should be fallen through onto the stone, feet {feet}"
        );

        let mut pool = Vec::new();
        for z in 0..4u8 {
            for x in 0..4u8 {
                pool.push((x, 0, z, Block::new("stone", 0)));
                for y in 1..4u16 {
                    pool.push((x, y, z, Block::new("water", 0)));
                }
            }
        }
        let pool =
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, pool).unwrap()]).unwrap();
        let mut swimmer = Camera::looking_at(
            Vec3::new(1.5, 2.2 + EYE_HEIGHT, 1.5),
            Vec3::new(1.5, 1.0, 0.5),
        );
        let start = swimmer.eye.y;
        for _ in 0..120 {
            step(&mut swimmer, 1.0 / 60.0, &idle(), &pool);
        }
        assert!(
            swimmer.eye.y > start + 0.4,
            "two blocks of water under the feet should lift the swimmer, eye {}",
            swimmer.eye.y
        );
        let surface_eye = 4.0 + EYE_HEIGHT;
        assert!(
            swimmer.eye.y < surface_eye,
            "buoyancy should stop at the surface, eye {}",
            swimmer.eye.y
        );

        let mut pad = Vec::new();
        for z in 0..4u8 {
            for x in 0..8u8 {
                pad.push((x, 0, z, Block::new("stone", 0)));
            }
        }
        let pad = Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, pad).unwrap()]).unwrap();
        let mut dry = Camera::looking_at(
            Vec3::new(1.5, 1.0 + EYE_HEIGHT, 1.5),
            Vec3::new(2.5, 1.0, 1.5),
        );
        let mut wade = Camera::looking_at(
            Vec3::new(1.5, 1.05 + EYE_HEIGHT, 1.5),
            Vec3::new(2.5, 1.0, 1.5),
        );
        let mut walk = idle();
        walk.forward = true;
        for _ in 0..30 {
            step(&mut dry, 1.0 / 60.0, &walk, &pad);
            step(&mut wade, 1.0 / 60.0, &walk, &pool);
        }
        assert!(
            (wade.eye.x - 1.5).abs() < (dry.eye.x - 8.5).abs() * 0.5,
            "water drag should cut the stride"
        );
    }

    #[test]
    fn a_short_drop_does_not_hurt_and_a_long_one_does() {
        assert_eq!(fall_hurt(1.3), 0);
        assert_eq!(fall_hurt(3.9), 0);
        assert_eq!(fall_hurt(4.0), 1);
        assert_eq!(fall_hurt(6.2), 3);
        assert_eq!(survive_fall(10, 1.3), (10, false));
        assert_eq!(survive_fall(10, 4.0), (9, false));
        assert_eq!(survive_fall(2, 6.2), (HEARTS, true));
        assert_eq!(soaked(1, 0), 1);
        assert_eq!(soaked(1, 1), 0);
        assert_eq!(soaked(3, 1), 2);
        assert_eq!(mace_hearts(false, 3), 1);
        assert_eq!(mace_hearts(true, 3), 3);
        assert_eq!(mace_hearts(true, 5), 5);
        assert_eq!(mace_ready(0.0), Some(MACE_WAIT));
        assert_eq!(mace_ready(0.2), None);
        assert_eq!(bite_hearts(true, 1, 3), 0);
        assert_eq!(bite_hearts(false, 1, 1), 0);
        assert_eq!(bite_hearts(false, 1, 0), 1);
        let mut worn = Armor::new();
        assert!(worn.equip(0, "leather_cap").is_none());
        assert_eq!(worn.equip(0, "leather_cap"), Some("leather_cap"));
        assert_eq!(worn.worn()[0], Some("leather_cap"));
        assert_eq!(worn.clear(0), Some("leather_cap"));
        assert!(worn.worn()[0].is_none());
    }

    #[test]
    fn walking_and_mining_spend_hunger_and_an_apple_fills_it() {
        let mut hunger = Hunger::full();
        assert_eq!(hunger.food(), HUNGER);
        assert_eq!(hunger.pace(), 1.0);
        assert!(!hunger.eat(4));
        assert_eq!(
            hunger.tick(WALK_HUNGER - 0.1, true, false, false),
            HungerTick::None
        );
        assert_eq!(hunger.food(), HUNGER);
        assert_eq!(hunger.tick(0.1, true, false, false), HungerTick::None);
        assert_eq!(hunger.food(), HUNGER - 1);
        assert_eq!(hunger.tick(20.0, false, false, false), HungerTick::None);
        assert_eq!(hunger.food(), HUNGER - 1);
        hunger.tick(WORK_HUNGER, false, true, false);
        assert_eq!(hunger.food(), HUNGER - 2);
        assert!(hunger.eat(4));
        assert_eq!(hunger.food(), HUNGER);

        let mut empty = Hunger::full();
        empty.tick(WALK_HUNGER * HUNGER as f32, true, false, false);
        assert_eq!(empty.food(), 0);
        assert_eq!(empty.pace(), STARVE_PACE);
        assert_eq!(
            empty.tick(STARVE_EVERY - 0.1, false, false, false),
            HungerTick::None
        );
        assert_eq!(empty.tick(0.1, false, false, false), HungerTick::Hurt);
        assert!(empty.eat(4));
        assert_eq!(empty.food(), 4);
        assert_eq!(empty.tick(STARVE_EVERY, false, false, true), HungerTick::None);
    }

    #[test]
    fn well_fed_idle_regen_returns_a_heart_and_spends_food() {
        let mut hunger = Hunger::full();
        assert_eq!(
            hunger.tick(REGEN_EVERY - 0.1, false, false, true),
            HungerTick::None
        );
        assert_eq!(hunger.food(), HUNGER);
        assert_eq!(hunger.tick(0.1, false, false, true), HungerTick::Heal);
        assert_eq!(hunger.food(), HUNGER - 1);
        // Full health does not keep spending food.
        assert_eq!(hunger.tick(REGEN_EVERY, false, false, false), HungerTick::None);
        assert_eq!(hunger.food(), HUNGER - 1);
        // Moving cancels the regen timer.
        assert_eq!(hunger.tick(REGEN_EVERY, true, false, true), HungerTick::None);
        // Below the threshold, idle time does not heal.
        let mut low = Hunger::full();
        while low.food() >= REGEN_FOOD {
            low.tick(WALK_HUNGER, true, false, false);
        }
        assert!(low.food() < REGEN_FOOD);
        assert_eq!(
            low.tick(REGEN_EVERY * 2.0, false, false, true),
            HungerTick::None
        );
    }

    #[test]
    fn chorus_fruit_hops_onto_a_nearby_floor() {
        let mut blocks = Vec::new();
        for x in 0..8u8 {
            for z in 0..8u8 {
                blocks.push((x, 0, z, Block::new("stone", 0)));
            }
        }
        let world =
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, blocks).unwrap()]).unwrap();
        let eye = Vec3::new(2.5, 1.0 + EYE_HEIGHT, 2.5);
        let landed = super::chorus_hop(eye, &world).expect("a floor to hop to");
        let dx = (landed.x - eye.x).abs();
        let dz = (landed.z - eye.z).abs();
        assert!(
            dx + dz >= 2.0,
            "the hop should leave the cell, moved {dx} {dz}"
        );
        assert!(
            dx <= 3.5 && dz <= 3.5,
            "the hop should stay short, {dx} {dz}"
        );
        assert!((landed.y - eye.y).abs() < 0.01);
        assert_eq!(super::chorus_hop(eye, &world), Some(landed));
        let empty =
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, []).unwrap()]).unwrap();
        assert!(super::chorus_hop(eye, &empty).is_none());
    }

    #[test]
    fn a_dragon_egg_hops_to_a_nearby_floor() {
        let mut blocks = Vec::new();
        for x in 0..8u8 {
            for z in 0..8u8 {
                blocks.push((x, 0, z, Block::new("stone", 0)));
            }
        }
        let world =
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, blocks).unwrap()]).unwrap();
        let landed = super::egg_hop([2, 1, 2], &world).expect("a floor to hop to");
        assert_eq!(landed, [3, 1, 4]);
        assert_eq!(super::egg_hop([2, 1, 2], &world), Some(landed));
        let mut sealed = Vec::new();
        for x in 0..8u8 {
            for z in 0..8u8 {
                sealed.push((x, 0, z, Block::new("stone", 0)));
                if (x, z) != (2, 2) {
                    sealed.push((x, 1, z, Block::new("stone", 0)));
                }
                sealed.push((x, 2, z, Block::new("stone", 0)));
            }
        }
        let boxed =
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, sealed).unwrap()]).unwrap();
        assert!(super::egg_hop([2, 1, 2], &boxed).is_none());
        let shape = shape_of(Block::new("dragon_egg", 0));
        assert!(!shape.is_full_cube());
        assert!(shape.boxes().len() >= 3);
        let belly = shape.boxes()[1].max[0] - shape.boxes()[1].min[0];
        let tip = shape.boxes()[2].max[0] - shape.boxes()[2].min[0];
        assert!(belly > tip, "the middle of the egg is wider than the tip");
    }

    #[test]
    fn an_end_crystal_blast_reaches_four_blocks() {
        let crystal = [1, 1, 1];
        assert!(super::crystal_reaches(Vec3::new(1.5, 1.5, 1.5), crystal));
        assert!(!super::crystal_reaches(Vec3::new(1.5, 1.5, 8.5), crystal));
        let shape = shape_of(Block::new("end_crystal", 0));
        assert!(!shape.is_full_cube());
        let body = shape.boxes()[1].max[0] - shape.boxes()[1].min[0];
        let tip = shape.boxes()[2].max[0] - shape.boxes()[2].min[0];
        assert!(body > tip, "the crystal is wider through the middle");
    }

    #[test]
    fn a_wind_burst_stops_at_a_wall_and_shoves_what_is_close() {
        let mut blocks = Vec::new();
        for x in 0..8u8 {
            for z in 0..16u8 {
                blocks.push((x, 0, z, Block::new("stone", 0)));
            }
        }
        for y in 1..4u16 {
            blocks.push((2, y, 6, Block::new("stone", 0)));
        }
        let world =
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, blocks).unwrap()]).unwrap();
        let eye = Vec3::new(2.5, 1.0 + EYE_HEIGHT, 1.5);
        let spot = super::wind_spot(eye, Vec3::new(0.0, 0.0, 1.0), &world).expect("burst");
        assert!(
            spot[2] < 6.0,
            "the wall should stop the burst, z {}",
            spot[2]
        );
        let close = super::wind_push(spot, spot).expect("the center is inside");
        assert!(close.lift > 1.0);
        assert_eq!(super::rod_ready(0.0), Some(super::ROD_WAIT));
        assert_eq!(super::rod_ready(0.2), None);
        assert!(super::ROD_LIFT < 8.6);
        assert!(super::wind_push(spot, [spot[0] + 8.0, spot[1], spot[2]]).is_none());
    }

    #[test]
    fn an_ender_pearl_lands_in_front_of_a_wall() {
        let mut blocks = Vec::new();
        for x in 0..8u8 {
            for z in 0..16u8 {
                blocks.push((x, 0, z, Block::new("stone", 0)));
            }
        }
        for y in 1..4u16 {
            blocks.push((2, y, 6, Block::new("stone", 0)));
        }
        let world =
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, blocks).unwrap()]).unwrap();
        let eye = Vec3::new(2.5, 1.0 + EYE_HEIGHT, 1.5);
        let landed = super::pearl_landing(eye, Vec3::new(0.0, 0.0, 1.0), &world)
            .expect("the wall should stop the pearl");
        assert!((landed.x - 2.5).abs() < 0.01, "x {}", landed.x);
        assert!((landed.z - 5.5).abs() < 0.01, "z {}", landed.z);
        assert!(
            (landed.y - (1.0 + EYE_HEIGHT)).abs() < 0.05,
            "y {}",
            landed.y
        );

        let open = Cluster::try_from_chunks(
            0,
            0,
            [Chunk::from_blocks(
                0,
                0,
                (0..16u8)
                    .map(|z| (2, 0, z, Block::new("stone", 0)))
                    .collect::<Vec<_>>(),
            )
            .unwrap()],
        )
        .unwrap();
        let far = super::pearl_landing(eye, Vec3::new(0.0, 0.0, 1.0), &open)
            .expect("open air still lands");
        assert!(
            far.z > landed.z,
            "the open throw should go farther, z {}",
            far.z
        );
        assert!(
            far.z <= 1.5 + 12.0 + 0.6,
            "the throw stays within reach, z {}",
            far.z
        );
    }

    #[test]
    fn a_healing_potion_restores_two_hearts_and_stops_at_full() {
        assert_eq!(potion_hearts(1), 3);
        assert_eq!(potion_hearts(9), HEARTS);
        assert_eq!(potion_hearts(HEARTS), HEARTS);
    }

    #[test]
    fn an_empty_stomach_walks_slower() {
        let blocks: Vec<_> = (0..16u8)
            .map(|x| (x, 0, 0, Block::new("stone", 0)))
            .collect();
        let world =
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, blocks).unwrap()]).unwrap();
        let start = Vec3::new(0.5, 1.0 + EYE_HEIGHT, 0.5);
        let look = start + Vec3::new(1.0, 0.0, 0.0);
        let mut fed = Camera::looking_at(start, look);
        let mut starved = Camera::looking_at(start, look);
        let mut walk = idle();
        walk.forward = true;
        for _ in 0..30 {
            step_paced(&mut fed, 1.0 / 60.0, &walk, &world, &[], 1.0);
            step_paced(&mut starved, 1.0 / 60.0, &walk, &world, &[], STARVE_PACE);
        }
        let fed_step = (fed.eye.x - start.x).abs();
        let starved_step = (starved.eye.x - start.x).abs();
        assert!(
            starved_step < fed_step * 0.7,
            "empty hunger shortens the stride"
        );
    }

    #[test]
    fn lands_on_a_cube_then_jumps_and_falls_back() {
        let world = cube_floor();
        let eye = Vec3::new(0.5, 4.0 + EYE_HEIGHT, 0.5);
        let mut camera = Camera::looking_at(eye, eye + Vec3::new(0.0, 0.0, -1.0));
        for _ in 0..180 {
            step(&mut camera, 1.0 / 60.0, &idle(), &world);
        }
        let feet = feet_y(camera.eye.y);
        assert!((feet - 1.0).abs() < 0.02, "feet at {feet}");
        assert!(camera.on_ground);
        assert!(feet < 1.0 + HEIGHT);
        assert!(ground_jump(&camera, &world));

        let mut jumping = idle();
        jumping.jump = true;
        step(&mut camera, 1.0 / 60.0, &jumping, &world);
        assert!(camera.eye.y > 1.0 + EYE_HEIGHT + 0.05);
        assert!(!camera.on_ground);
        assert!(!ground_jump(&camera, &world));
        for _ in 0..180 {
            step(&mut camera, 1.0 / 60.0, &idle(), &world);
        }
        let feet = feet_y(camera.eye.y);
        assert!((feet - 1.0).abs() < 0.02, "landed at {feet}");
        assert!(camera.on_ground);
    }

    #[test]
    fn creative_flight_rises_and_holds_without_gravity() {
        let world = stone_path(&[]);
        let mut camera = Camera::looking_at(
            Vec3::new(0.5, 1.0 + EYE_HEIGHT, 0.5),
            Vec3::new(0.5, 1.0 + EYE_HEIGHT, -1.0),
        );
        camera.on_ground = true;
        let mut up = idle();
        up.jump = true;
        step_fly(&mut camera, 1.0 / 60.0, &up, &world, &[]);
        let risen = camera.eye.y;
        assert!(risen > 1.0 + EYE_HEIGHT + 0.05, "space should lift {risen}");
        assert!(!camera.on_ground);
        let held = camera.eye.y;
        for _ in 0..90 {
            step_fly(&mut camera, 1.0 / 60.0, &idle(), &world, &[]);
        }
        assert!(
            (camera.eye.y - held).abs() < 0.08,
            "no gravity while flying, y {} vs {}",
            camera.eye.y,
            held
        );
        let mut down = idle();
        down.crouch = true;
        let before = camera.eye.y;
        step_fly(&mut camera, 1.0 / 60.0, &down, &world, &[]);
        assert!(camera.eye.y < before, "shift should sink");
    }

    fn stone_path(extra: &[(u8, u16, u8)]) -> Cluster {
        let mut blocks = vec![
            (0, 0, 0, Block::new("stone", 0)),
            (0, 0, 1, Block::new("stone", 0)),
            (0, 0, 2, Block::new("stone", 0)),
        ];
        for &(x, y, z) in extra {
            blocks.push((x, y, z, Block::new("stone", 0)));
        }
        let chunk = Chunk::from_blocks(0, 0, blocks).unwrap();
        Cluster::try_from_chunks(0, 0, [chunk]).unwrap()
    }

    fn trapdoor_floor(id: &str, metadata: u8) -> Cluster {
        let mut blocks = Vec::new();
        for z in 1..5u8 {
            blocks.push((0, 0, z, Block::new("stone", 0)));
        }
        blocks.push((0, 0, 0, Block::new(id, metadata)));
        Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, blocks).unwrap()]).unwrap()
    }

    fn approach_hatch(world: &Cluster) -> Camera {
        let eye = Vec3::new(0.5, 1.0 + EYE_HEIGHT, 2.5);
        let mut camera = Camera::looking_at(eye, Vec3::new(0.5, 1.4, 0.0));
        let mut walk = idle();
        walk.forward = true;
        for _ in 0..18 {
            step(&mut camera, 1.0 / 60.0, &walk, world);
        }
        let still = idle();
        for _ in 0..40 {
            step(&mut camera, 1.0 / 60.0, &still, world);
        }
        camera
    }

    fn walk_north(world: &Cluster, frames: u32) -> Camera {
        let eye = Vec3::new(0.5, 1.0 + EYE_HEIGHT, 2.5);
        let mut camera = Camera::looking_at(eye, Vec3::new(0.5, 1.4, 0.0));
        let mut walk = idle();
        walk.forward = true;
        for _ in 0..frames {
            step(&mut camera, 1.0 / 60.0, &walk, world);
        }
        camera
    }

    #[test]
    fn one_block_rise_is_walked_up() {
        let world = stone_path(&[(0, 1, 0)]);
        let camera = walk_north(&world, 20);
        let feet = feet_y(camera.eye.y);
        assert!(
            (feet - 2.0).abs() < 0.05,
            "did not step onto the block, feet {feet}"
        );
        assert!(camera.on_ground);
        let min_z = camera.eye.z - HALF_WIDTH;
        assert!(
            min_z < 1.0,
            "stopped at the face instead of stepping up, min z {min_z}"
        );
        assert!(min_z > 0.0, "walked off the far side, min z {min_z}");
    }

    #[test]
    fn a_slab_is_half_a_step() {
        let shape = shape_of(Block::new("slab", 0));
        assert!(!shape.is_full_cube());
        assert_eq!(shape.boxes()[0].max[1], 8);
        assert!(shape.occludes(8, 0, 8));
        assert!(!shape.occludes(8, 12, 8));
        let blocks = vec![
            (0, 0, 0, Block::new("stone", 0)),
            (0, 0, 1, Block::new("stone", 0)),
            (0, 0, 2, Block::new("stone", 0)),
            (0, 1, 0, Block::new("slab", 0)),
        ];
        let world =
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, blocks).unwrap()]).unwrap();
        let camera = walk_north(&world, 20);
        let feet = feet_y(camera.eye.y);
        assert!(
            (feet - 1.5).abs() < 0.05,
            "did not step onto the slab, feet {feet}"
        );
        assert!(camera.on_ground);
        let min_z = camera.eye.z - HALF_WIDTH;
        assert!(
            min_z < 1.0,
            "stopped at the face instead of stepping up, min z {min_z}"
        );
    }

    #[test]
    fn an_end_portal_frame_is_a_low_step() {
        let shape = shape_of(Block::new("end_portal_frame", 0));
        assert!(!shape.is_full_cube());
        assert_eq!(shape.boxes()[0].max[1], 13);
        let eyed = shape_of(Block::new("end_portal_frame", 1));
        assert_eq!(eyed.boxes().len(), 2);
        let blocks = vec![
            (0, 0, 0, Block::new("stone", 0)),
            (0, 0, 1, Block::new("stone", 0)),
            (0, 0, 2, Block::new("stone", 0)),
            (0, 1, 0, Block::new("end_portal_frame", 0)),
        ];
        let world =
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, blocks).unwrap()]).unwrap();
        let camera = walk_north(&world, 20);
        let feet = feet_y(camera.eye.y);
        assert!(
            (feet - (1.0 + 13.0 / 16.0)).abs() < 0.05,
            "did not step onto the frame, feet {feet}"
        );
        assert!(camera.on_ground);
    }

    #[test]
    fn a_cake_is_a_half_step_and_six_slices_finish_it() {
        let shape = shape_of(Block::new("cake", 0));
        assert_eq!(shape.boxes()[0].max[1], 8);
        assert!(!shape.is_full_cube());
        let blocks = vec![
            (0, 0, 0, Block::new("stone", 0)),
            (0, 0, 1, Block::new("stone", 0)),
            (0, 0, 2, Block::new("stone", 0)),
            (0, 1, 0, Block::new("cake", 0)),
        ];
        let world =
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, blocks).unwrap()]).unwrap();
        let camera = walk_north(&world, 20);
        let feet = feet_y(camera.eye.y);
        assert!(
            (feet - 1.5).abs() < 0.05,
            "did not step onto the cake, feet {feet}"
        );
        assert_eq!(super::cake_bite(0), super::CakeBite::Slice(1));
        assert_eq!(super::cake_bite(4), super::CakeBite::Slice(5));
        assert_eq!(super::cake_bite(5), super::CakeBite::Finish);
        let mut hunger = Hunger::full();
        hunger.tick(super::WALK_HUNGER, true, false, false);
        assert!(hunger.eat(super::CAKE_FOOD));
        assert_eq!(hunger.food(), super::HUNGER);
    }

    #[test]
    fn a_pumpkin_is_a_solid_cube_you_can_step_onto() {
        assert!(shape_of(Block::new("pumpkin", 0)).is_full_cube());
        assert!(shape_of(Block::new("pumpkin", 2)).is_full_cube());
        let mut blocks = Vec::new();
        for z in 0..5u8 {
            blocks.push((0, 0, z, Block::new("stone", 0)));
        }
        blocks.push((0, 1, 0, Block::new("pumpkin", 0)));
        let world =
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, blocks).unwrap()]).unwrap();
        let climbed = walk_north(&world, 20);
        let feet = feet_y(climbed.eye.y);
        assert!(
            (feet - 2.0).abs() < 0.05,
            "did not step onto the pumpkin, feet {feet}"
        );
    }

    #[test]
    fn a_melon_is_a_solid_cube_you_can_step_onto() {
        assert!(shape_of(Block::new("melon", 0)).is_full_cube());
        let mut blocks = Vec::new();
        for z in 0..5u8 {
            blocks.push((0, 0, z, Block::new("stone", 0)));
        }
        blocks.push((0, 1, 0, Block::new("melon", 0)));
        let world =
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, blocks).unwrap()]).unwrap();
        let climbed = walk_north(&world, 20);
        let feet = feet_y(climbed.eye.y);
        assert!(
            (feet - 2.0).abs() < 0.05,
            "did not step onto the melon, feet {feet}"
        );
    }

    #[test]
    fn a_hay_bale_is_a_solid_cube_you_can_step_onto() {
        assert!(shape_of(Block::new("hay_bale", 0)).is_full_cube());
        let mut blocks = Vec::new();
        for z in 0..5u8 {
            blocks.push((0, 0, z, Block::new("stone", 0)));
        }
        blocks.push((0, 1, 0, Block::new("hay_bale", 0)));
        let world =
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, blocks).unwrap()]).unwrap();
        let climbed = walk_north(&world, 20);
        let feet = feet_y(climbed.eye.y);
        assert!(
            (feet - 2.0).abs() < 0.05,
            "did not step onto the hay bale, feet {feet}"
        );
    }

    #[test]
    fn a_clay_block_is_a_solid_cube_you_can_step_onto() {
        assert!(shape_of(Block::new("clay", 0)).is_full_cube());
        let mut blocks = Vec::new();
        for z in 0..5u8 {
            blocks.push((0, 0, z, Block::new("stone", 0)));
        }
        blocks.push((0, 1, 0, Block::new("clay", 0)));
        let world =
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, blocks).unwrap()]).unwrap();
        let climbed = walk_north(&world, 20);
        let feet = feet_y(climbed.eye.y);
        assert!(
            (feet - 2.0).abs() < 0.05,
            "did not step onto the clay, feet {feet}"
        );
    }

    #[test]
    fn a_bricks_block_is_a_solid_cube_you_can_step_onto() {
        assert!(shape_of(Block::new("bricks", 0)).is_full_cube());
        let mut blocks = Vec::new();
        for z in 0..5u8 {
            blocks.push((0, 0, z, Block::new("stone", 0)));
        }
        blocks.push((0, 1, 0, Block::new("bricks", 0)));
        let world =
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, blocks).unwrap()]).unwrap();
        let climbed = walk_north(&world, 20);
        let feet = feet_y(climbed.eye.y);
        assert!(
            (feet - 2.0).abs() < 0.05,
            "did not step onto the bricks, feet {feet}"
        );
    }

    #[test]
    fn a_gravel_block_is_a_solid_cube_you_can_step_onto() {
        assert!(shape_of(Block::new("gravel", 0)).is_full_cube());
        let mut blocks = Vec::new();
        for z in 0..5u8 {
            blocks.push((0, 0, z, Block::new("stone", 0)));
        }
        blocks.push((0, 1, 0, Block::new("gravel", 0)));
        let world =
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, blocks).unwrap()]).unwrap();
        let climbed = walk_north(&world, 20);
        let feet = feet_y(climbed.eye.y);
        assert!(
            (feet - 2.0).abs() < 0.05,
            "did not step onto the gravel, feet {feet}"
        );
    }

    #[test]
    fn a_mossy_cobblestone_is_a_solid_cube_you_can_step_onto() {
        assert!(shape_of(Block::new("mossy_cobblestone", 0)).is_full_cube());
        let mut blocks = Vec::new();
        for z in 0..5u8 {
            blocks.push((0, 0, z, Block::new("stone", 0)));
        }
        blocks.push((0, 1, 0, Block::new("mossy_cobblestone", 0)));
        let world =
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, blocks).unwrap()]).unwrap();
        let climbed = walk_north(&world, 20);
        let feet = feet_y(climbed.eye.y);
        assert!(
            (feet - 2.0).abs() < 0.05,
            "did not step onto the mossy cobblestone, feet {feet}"
        );
    }

    #[test]
    fn a_stone_bricks_block_is_a_solid_cube_you_can_step_onto() {
        assert!(shape_of(Block::new("stone_bricks", 0)).is_full_cube());
        let mut blocks = Vec::new();
        for z in 0..5u8 {
            blocks.push((0, 0, z, Block::new("stone", 0)));
        }
        blocks.push((0, 1, 0, Block::new("stone_bricks", 0)));
        let world =
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, blocks).unwrap()]).unwrap();
        let climbed = walk_north(&world, 20);
        let feet = feet_y(climbed.eye.y);
        assert!(
            (feet - 2.0).abs() < 0.05,
            "did not step onto the stone bricks, feet {feet}"
        );
    }

    #[test]
    fn a_sandstone_block_is_a_solid_cube_you_can_step_onto() {
        assert!(shape_of(Block::new("sandstone", 0)).is_full_cube());
        let mut blocks = Vec::new();
        for z in 0..5u8 {
            blocks.push((0, 0, z, Block::new("stone", 0)));
        }
        blocks.push((0, 1, 0, Block::new("sandstone", 0)));
        let world =
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, blocks).unwrap()]).unwrap();
        let climbed = walk_north(&world, 20);
        let feet = feet_y(climbed.eye.y);
        assert!(
            (feet - 2.0).abs() < 0.05,
            "did not step onto the sandstone, feet {feet}"
        );
    }

    #[test]
    fn a_red_sandstone_block_is_a_solid_cube_you_can_step_onto() {
        assert!(shape_of(Block::new("red_sandstone", 0)).is_full_cube());
        let mut blocks = Vec::new();
        for z in 0..5u8 {
            blocks.push((0, 0, z, Block::new("stone", 0)));
        }
        blocks.push((0, 1, 0, Block::new("red_sandstone", 0)));
        let world =
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, blocks).unwrap()]).unwrap();
        let climbed = walk_north(&world, 20);
        let feet = feet_y(climbed.eye.y);
        assert!(
            (feet - 2.0).abs() < 0.05,
            "did not step onto the red sandstone, feet {feet}"
        );
    }

    #[test]
    fn a_smooth_stone_block_is_a_solid_cube_you_can_step_onto() {
        assert!(shape_of(Block::new("smooth_stone", 0)).is_full_cube());
        let mut blocks = Vec::new();
        for z in 0..5u8 {
            blocks.push((0, 0, z, Block::new("stone", 0)));
        }
        blocks.push((0, 1, 0, Block::new("smooth_stone", 0)));
        let world =
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, blocks).unwrap()]).unwrap();
        let climbed = walk_north(&world, 20);
        let feet = feet_y(climbed.eye.y);
        assert!(
            (feet - 2.0).abs() < 0.05,
            "did not step onto the smooth stone, feet {feet}"
        );
    }

    #[test]
    fn a_quartz_block_is_a_solid_cube_you_can_step_onto() {
        assert!(shape_of(Block::new("quartz", 0)).is_full_cube());
        let mut blocks = Vec::new();
        for z in 0..5u8 {
            blocks.push((0, 0, z, Block::new("stone", 0)));
        }
        blocks.push((0, 1, 0, Block::new("quartz", 0)));
        let world =
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, blocks).unwrap()]).unwrap();
        let climbed = walk_north(&world, 20);
        let feet = feet_y(climbed.eye.y);
        assert!(
            (feet - 2.0).abs() < 0.05,
            "did not step onto the quartz, feet {feet}"
        );
    }

    #[test]
    fn a_glowstone_block_is_a_solid_cube_you_can_step_onto() {
        assert!(shape_of(Block::new("glowstone", 0)).is_full_cube());
        let mut blocks = Vec::new();
        for z in 0..5u8 {
            blocks.push((0, 0, z, Block::new("stone", 0)));
        }
        blocks.push((0, 1, 0, Block::new("glowstone", 0)));
        let world =
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, blocks).unwrap()]).unwrap();
        let climbed = walk_north(&world, 20);
        let feet = feet_y(climbed.eye.y);
        assert!(
            (feet - 2.0).abs() < 0.05,
            "did not step onto the glowstone, feet {feet}"
        );
    }

    #[test]
    fn a_sea_lantern_is_a_solid_cube_you_can_step_onto() {
        assert!(shape_of(Block::new("sea_lantern", 0)).is_full_cube());
        let mut blocks = Vec::new();
        for z in 0..5u8 {
            blocks.push((0, 0, z, Block::new("stone", 0)));
        }
        blocks.push((0, 1, 0, Block::new("sea_lantern", 0)));
        let world =
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, blocks).unwrap()]).unwrap();
        let climbed = walk_north(&world, 20);
        let feet = feet_y(climbed.eye.y);
        assert!(
            (feet - 2.0).abs() < 0.05,
            "did not step onto the sea lantern, feet {feet}"
        );
    }

    #[test]
    fn a_prismarine_block_is_a_solid_cube_you_can_step_onto() {
        assert!(shape_of(Block::new("prismarine", 0)).is_full_cube());
        let mut blocks = Vec::new();
        for z in 0..5u8 {
            blocks.push((0, 0, z, Block::new("stone", 0)));
        }
        blocks.push((0, 1, 0, Block::new("prismarine", 0)));
        let world =
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, blocks).unwrap()]).unwrap();
        let climbed = walk_north(&world, 20);
        let feet = feet_y(climbed.eye.y);
        assert!(
            (feet - 2.0).abs() < 0.05,
            "did not step onto the prismarine, feet {feet}"
        );
    }

    #[test]
    fn a_dark_prismarine_block_is_a_solid_cube_you_can_step_onto() {
        assert!(shape_of(Block::new("dark_prismarine", 0)).is_full_cube());
        let mut blocks = Vec::new();
        for z in 0..5u8 {
            blocks.push((0, 0, z, Block::new("stone", 0)));
        }
        blocks.push((0, 1, 0, Block::new("dark_prismarine", 0)));
        let world =
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, blocks).unwrap()]).unwrap();
        let climbed = walk_north(&world, 20);
        let feet = feet_y(climbed.eye.y);
        assert!(
            (feet - 2.0).abs() < 0.05,
            "did not step onto the dark prismarine, feet {feet}"
        );
    }

    #[test]
    fn a_prismarine_bricks_block_is_a_solid_cube_you_can_step_onto() {
        assert!(shape_of(Block::new("prismarine_bricks", 0)).is_full_cube());
        let mut blocks = Vec::new();
        for z in 0..5u8 {
            blocks.push((0, 0, z, Block::new("stone", 0)));
        }
        blocks.push((0, 1, 0, Block::new("prismarine_bricks", 0)));
        let world =
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, blocks).unwrap()]).unwrap();
        let climbed = walk_north(&world, 20);
        let feet = feet_y(climbed.eye.y);
        assert!(
            (feet - 2.0).abs() < 0.05,
            "did not step onto the prismarine bricks, feet {feet}"
        );
    }

    #[test]
    fn a_cracked_stone_bricks_block_is_a_solid_cube_you_can_step_onto() {
        assert!(shape_of(Block::new("cracked_stone_bricks", 0)).is_full_cube());
        let mut blocks = Vec::new();
        for z in 0..5u8 {
            blocks.push((0, 0, z, Block::new("stone", 0)));
        }
        blocks.push((0, 1, 0, Block::new("cracked_stone_bricks", 0)));
        let world =
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, blocks).unwrap()]).unwrap();
        let climbed = walk_north(&world, 20);
        let feet = feet_y(climbed.eye.y);
        assert!(
            (feet - 2.0).abs() < 0.05,
            "did not step onto the cracked stone bricks, feet {feet}"
        );
    }

    #[test]
    fn a_mossy_stone_bricks_block_is_a_solid_cube_you_can_step_onto() {
        assert!(shape_of(Block::new("mossy_stone_bricks", 0)).is_full_cube());
        let mut blocks = Vec::new();
        for z in 0..5u8 {
            blocks.push((0, 0, z, Block::new("stone", 0)));
        }
        blocks.push((0, 1, 0, Block::new("mossy_stone_bricks", 0)));
        let world =
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, blocks).unwrap()]).unwrap();
        let climbed = walk_north(&world, 20);
        let feet = feet_y(climbed.eye.y);
        assert!(
            (feet - 2.0).abs() < 0.05,
            "did not step onto the mossy stone bricks, feet {feet}"
        );
    }

    #[test]
    fn a_chiseled_stone_bricks_block_is_a_solid_cube_you_can_step_onto() {
        assert!(shape_of(Block::new("chiseled_stone_bricks", 0)).is_full_cube());
        let mut blocks = Vec::new();
        for z in 0..5u8 {
            blocks.push((0, 0, z, Block::new("stone", 0)));
        }
        blocks.push((0, 1, 0, Block::new("chiseled_stone_bricks", 0)));
        let world =
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, blocks).unwrap()]).unwrap();
        let climbed = walk_north(&world, 20);
        let feet = feet_y(climbed.eye.y);
        assert!(
            (feet - 2.0).abs() < 0.05,
            "did not step onto the chiseled stone bricks, feet {feet}"
        );
    }

    #[test]
    fn a_deepslate_bricks_block_is_a_solid_cube_you_can_step_onto() {
        assert!(shape_of(Block::new("deepslate_bricks", 0)).is_full_cube());
        let mut blocks = Vec::new();
        for z in 0..5u8 {
            blocks.push((0, 0, z, Block::new("stone", 0)));
        }
        blocks.push((0, 1, 0, Block::new("deepslate_bricks", 0)));
        let world =
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, blocks).unwrap()]).unwrap();
        let climbed = walk_north(&world, 20);
        let feet = feet_y(climbed.eye.y);
        assert!(
            (feet - 2.0).abs() < 0.05,
            "did not step onto the deepslate bricks, feet {feet}"
        );
    }

    #[test]
    fn a_cobbled_deepslate_block_is_a_solid_cube_you_can_step_onto() {
        assert!(shape_of(Block::new("cobbled_deepslate", 0)).is_full_cube());
        let mut blocks = Vec::new();
        for z in 0..5u8 {
            blocks.push((0, 0, z, Block::new("stone", 0)));
        }
        blocks.push((0, 1, 0, Block::new("cobbled_deepslate", 0)));
        let world =
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, blocks).unwrap()]).unwrap();
        let climbed = walk_north(&world, 20);
        let feet = feet_y(climbed.eye.y);
        assert!(
            (feet - 2.0).abs() < 0.05,
            "did not step onto the cobbled deepslate, feet {feet}"
        );
    }

    #[test]
    fn a_polished_deepslate_block_is_a_solid_cube_you_can_step_onto() {
        assert!(shape_of(Block::new("polished_deepslate", 0)).is_full_cube());
        let mut blocks = Vec::new();
        for z in 0..5u8 {
            blocks.push((0, 0, z, Block::new("stone", 0)));
        }
        blocks.push((0, 1, 0, Block::new("polished_deepslate", 0)));
        let world =
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, blocks).unwrap()]).unwrap();
        let climbed = walk_north(&world, 20);
        let feet = feet_y(climbed.eye.y);
        assert!(
            (feet - 2.0).abs() < 0.05,
            "did not step onto the polished deepslate, feet {feet}"
        );
    }

    #[test]
    fn a_deepslate_tiles_block_is_a_solid_cube_you_can_step_onto() {
        assert!(shape_of(Block::new("deepslate_tiles", 0)).is_full_cube());
        let mut blocks = Vec::new();
        for z in 0..5u8 {
            blocks.push((0, 0, z, Block::new("stone", 0)));
        }
        blocks.push((0, 1, 0, Block::new("deepslate_tiles", 0)));
        let world =
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, blocks).unwrap()]).unwrap();
        let climbed = walk_north(&world, 20);
        let feet = feet_y(climbed.eye.y);
        assert!(
            (feet - 2.0).abs() < 0.05,
            "did not step onto the deepslate tiles, feet {feet}"
        );
    }

    #[test]
    fn a_cracked_deepslate_tiles_block_is_a_solid_cube_you_can_step_onto() {
        assert!(shape_of(Block::new("cracked_deepslate_tiles", 0)).is_full_cube());
        let mut blocks = Vec::new();
        for z in 0..5u8 {
            blocks.push((0, 0, z, Block::new("stone", 0)));
        }
        blocks.push((0, 1, 0, Block::new("cracked_deepslate_tiles", 0)));
        let world =
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, blocks).unwrap()]).unwrap();
        let climbed = walk_north(&world, 20);
        let feet = feet_y(climbed.eye.y);
        assert!(
            (feet - 2.0).abs() < 0.05,
            "did not step onto the cracked deepslate tiles, feet {feet}"
        );
    }

    #[test]
    fn a_blackstone_block_is_a_solid_cube_you_can_step_onto() {
        assert!(shape_of(Block::new("blackstone", 0)).is_full_cube());
        let mut blocks = Vec::new();
        for z in 0..5u8 {
            blocks.push((0, 0, z, Block::new("stone", 0)));
        }
        blocks.push((0, 1, 0, Block::new("blackstone", 0)));
        let world =
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, blocks).unwrap()]).unwrap();
        let climbed = walk_north(&world, 20);
        let feet = feet_y(climbed.eye.y);
        assert!(
            (feet - 2.0).abs() < 0.05,
            "did not step onto the blackstone, feet {feet}"
        );
    }

    #[test]
    fn a_polished_blackstone_block_is_a_solid_cube_you_can_step_onto() {
        assert!(shape_of(Block::new("polished_blackstone", 0)).is_full_cube());
        let mut blocks = Vec::new();
        for z in 0..5u8 {
            blocks.push((0, 0, z, Block::new("stone", 0)));
        }
        blocks.push((0, 1, 0, Block::new("polished_blackstone", 0)));
        let world =
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, blocks).unwrap()]).unwrap();
        let climbed = walk_north(&world, 20);
        let feet = feet_y(climbed.eye.y);
        assert!(
            (feet - 2.0).abs() < 0.05,
            "did not step onto the polished blackstone, feet {feet}"
        );
    }

    #[test]
    fn a_blackstone_bricks_block_is_a_solid_cube_you_can_step_onto() {
        assert!(shape_of(Block::new("blackstone_bricks", 0)).is_full_cube());
        let mut blocks = Vec::new();
        for z in 0..5u8 {
            blocks.push((0, 0, z, Block::new("stone", 0)));
        }
        blocks.push((0, 1, 0, Block::new("blackstone_bricks", 0)));
        let world =
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, blocks).unwrap()]).unwrap();
        let climbed = walk_north(&world, 20);
        let feet = feet_y(climbed.eye.y);
        assert!(
            (feet - 2.0).abs() < 0.05,
            "did not step onto the blackstone bricks, feet {feet}"
        );
    }

    #[test]
    fn a_cracked_blackstone_bricks_block_is_a_solid_cube_you_can_step_onto() {
        assert!(shape_of(Block::new("cracked_blackstone_bricks", 0)).is_full_cube());
        let mut blocks = Vec::new();
        for z in 0..5u8 {
            blocks.push((0, 0, z, Block::new("stone", 0)));
        }
        blocks.push((0, 1, 0, Block::new("cracked_blackstone_bricks", 0)));
        let world =
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, blocks).unwrap()]).unwrap();
        let climbed = walk_north(&world, 20);
        let feet = feet_y(climbed.eye.y);
        assert!(
            (feet - 2.0).abs() < 0.05,
            "did not step onto the cracked blackstone bricks, feet {feet}"
        );
    }

    #[test]
    fn a_chiseled_blackstone_block_is_a_solid_cube_you_can_step_onto() {
        assert!(shape_of(Block::new("chiseled_blackstone", 0)).is_full_cube());
        let mut blocks = Vec::new();
        for z in 0..5u8 {
            blocks.push((0, 0, z, Block::new("stone", 0)));
        }
        blocks.push((0, 1, 0, Block::new("chiseled_blackstone", 0)));
        let world =
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, blocks).unwrap()]).unwrap();
        let climbed = walk_north(&world, 20);
        let feet = feet_y(climbed.eye.y);
        assert!(
            (feet - 2.0).abs() < 0.05,
            "did not step onto the chiseled blackstone, feet {feet}"
        );
    }

    #[test]
    fn a_gilded_blackstone_block_is_a_solid_cube_you_can_step_onto() {
        assert!(shape_of(Block::new("gilded_blackstone", 0)).is_full_cube());
        let mut blocks = Vec::new();
        for z in 0..5u8 {
            blocks.push((0, 0, z, Block::new("stone", 0)));
        }
        blocks.push((0, 1, 0, Block::new("gilded_blackstone", 0)));
        let world =
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, blocks).unwrap()]).unwrap();
        let climbed = walk_north(&world, 20);
        let feet = feet_y(climbed.eye.y);
        assert!(
            (feet - 2.0).abs() < 0.05,
            "did not step onto the gilded blackstone, feet {feet}"
        );
    }

    #[test]
    fn a_basalt_block_is_a_solid_cube_you_can_step_onto() {
        assert!(shape_of(Block::new("basalt", 0)).is_full_cube());
        let mut blocks = Vec::new();
        for z in 0..5u8 {
            blocks.push((0, 0, z, Block::new("stone", 0)));
        }
        blocks.push((0, 1, 0, Block::new("basalt", 0)));
        let world =
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, blocks).unwrap()]).unwrap();
        let climbed = walk_north(&world, 20);
        let feet = feet_y(climbed.eye.y);
        assert!(
            (feet - 2.0).abs() < 0.05,
            "did not step onto the basalt, feet {feet}"
        );
    }

    #[test]
    fn a_polished_basalt_block_is_a_solid_cube_you_can_step_onto() {
        assert!(shape_of(Block::new("polished_basalt", 0)).is_full_cube());
        let mut blocks = Vec::new();
        for z in 0..5u8 {
            blocks.push((0, 0, z, Block::new("stone", 0)));
        }
        blocks.push((0, 1, 0, Block::new("polished_basalt", 0)));
        let world =
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, blocks).unwrap()]).unwrap();
        let climbed = walk_north(&world, 20);
        let feet = feet_y(climbed.eye.y);
        assert!(
            (feet - 2.0).abs() < 0.05,
            "did not step onto the polished basalt, feet {feet}"
        );
    }

    #[test]
    fn a_smooth_basalt_block_is_a_solid_cube_you_can_step_onto() {
        assert!(shape_of(Block::new("smooth_basalt", 0)).is_full_cube());
        let mut blocks = Vec::new();
        for z in 0..5u8 {
            blocks.push((0, 0, z, Block::new("stone", 0)));
        }
        blocks.push((0, 1, 0, Block::new("smooth_basalt", 0)));
        let world =
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, blocks).unwrap()]).unwrap();
        let climbed = walk_north(&world, 20);
        let feet = feet_y(climbed.eye.y);
        assert!(
            (feet - 2.0).abs() < 0.05,
            "did not step onto the smooth basalt, feet {feet}"
        );
    }

    #[test]
    fn a_nether_bricks_block_is_a_solid_cube_you_can_step_onto() {
        assert!(shape_of(Block::new("nether_bricks", 0)).is_full_cube());
        let mut blocks = Vec::new();
        for z in 0..5u8 {
            blocks.push((0, 0, z, Block::new("stone", 0)));
        }
        blocks.push((0, 1, 0, Block::new("nether_bricks", 0)));
        let world =
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, blocks).unwrap()]).unwrap();
        let climbed = walk_north(&world, 20);
        let feet = feet_y(climbed.eye.y);
        assert!(
            (feet - 2.0).abs() < 0.05,
            "did not step onto the nether bricks, feet {feet}"
        );
    }

    #[test]
    fn a_cracked_nether_bricks_block_is_a_solid_cube_you_can_step_onto() {
        assert!(shape_of(Block::new("cracked_nether_bricks", 0)).is_full_cube());
        let mut blocks = Vec::new();
        for z in 0..5u8 {
            blocks.push((0, 0, z, Block::new("stone", 0)));
        }
        blocks.push((0, 1, 0, Block::new("cracked_nether_bricks", 0)));
        let world =
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, blocks).unwrap()]).unwrap();
        let climbed = walk_north(&world, 20);
        let feet = feet_y(climbed.eye.y);
        assert!(
            (feet - 2.0).abs() < 0.05,
            "did not step onto the cracked nether bricks, feet {feet}"
        );
    }

    #[test]
    fn a_red_nether_bricks_block_is_a_solid_cube_you_can_step_onto() {
        assert!(shape_of(Block::new("red_nether_bricks", 0)).is_full_cube());
        let mut blocks = Vec::new();
        for z in 0..5u8 {
            blocks.push((0, 0, z, Block::new("stone", 0)));
        }
        blocks.push((0, 1, 0, Block::new("red_nether_bricks", 0)));
        let world =
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, blocks).unwrap()]).unwrap();
        let climbed = walk_north(&world, 20);
        let feet = feet_y(climbed.eye.y);
        assert!(
            (feet - 2.0).abs() < 0.05,
            "did not step onto the red nether bricks, feet {feet}"
        );
    }

    #[test]
    fn a_chiseled_nether_bricks_block_is_a_solid_cube_you_can_step_onto() {
        assert!(shape_of(Block::new("chiseled_nether_bricks", 0)).is_full_cube());
        let mut blocks = Vec::new();
        for z in 0..5u8 {
            blocks.push((0, 0, z, Block::new("stone", 0)));
        }
        blocks.push((0, 1, 0, Block::new("chiseled_nether_bricks", 0)));
        let world =
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, blocks).unwrap()]).unwrap();
        let climbed = walk_north(&world, 20);
        let feet = feet_y(climbed.eye.y);
        assert!(
            (feet - 2.0).abs() < 0.05,
            "did not step onto the chiseled nether bricks, feet {feet}"
        );
    }

    #[test]
    fn a_nether_wart_block_is_a_solid_cube_you_can_step_onto() {
        assert!(shape_of(Block::new("nether_wart_block", 0)).is_full_cube());
        let mut blocks = Vec::new();
        for z in 0..5u8 {
            blocks.push((0, 0, z, Block::new("stone", 0)));
        }
        blocks.push((0, 1, 0, Block::new("nether_wart_block", 0)));
        let world =
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, blocks).unwrap()]).unwrap();
        let climbed = walk_north(&world, 20);
        let feet = feet_y(climbed.eye.y);
        assert!(
            (feet - 2.0).abs() < 0.05,
            "did not step onto the nether wart block, feet {feet}"
        );
    }

    #[test]
    fn a_soul_sand_block_is_a_solid_cube_you_can_step_onto() {
        assert!(shape_of(Block::new("soul_sand", 0)).is_full_cube());
        let mut blocks = Vec::new();
        for z in 0..5u8 {
            blocks.push((0, 0, z, Block::new("stone", 0)));
        }
        blocks.push((0, 1, 0, Block::new("soul_sand", 0)));
        let world =
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, blocks).unwrap()]).unwrap();
        let climbed = walk_north(&world, 20);
        let feet = feet_y(climbed.eye.y);
        assert!(
            (feet - 2.0).abs() < 0.05,
            "did not step onto the soul sand, feet {feet}"
        );
    }

    #[test]
    fn soul_sand_slows_a_walk_less_than_a_cobweb() {
        let floor = |id: &str| {
            let mut blocks = Vec::new();
            for z in 0..8u8 {
                blocks.push((0, 0, z, Block::new(id, 0)));
            }
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, blocks).unwrap()]).unwrap()
        };
        let travel = |world: &Cluster| {
            let eye = Vec3::new(0.5, 1.0 + EYE_HEIGHT, 6.5);
            let mut camera = Camera::looking_at(eye, Vec3::new(0.5, 1.4, 0.0));
            let mut walk = idle();
            walk.forward = true;
            for _ in 0..40 {
                step(&mut camera, 1.0 / 60.0, &walk, world);
            }
            eye.z - camera.eye.z
        };
        let open = travel(&floor("stone"));
        let slow = travel(&floor("soul_sand"));
        assert!(open > 2.0, "an open walk should cover ground, moved {open}");
        assert!(
            slow > open * (super::COBWEB_DRAG + 0.05),
            "soul sand should be lighter than a cobweb, open {open} slow {slow}"
        );
        assert!(
            slow < open * (super::SOUL_SAND_DRAG + 0.05),
            "soul sand should be about half a walk, open {open} slow {slow}"
        );
    }

    #[test]
    fn a_crouch_covers_less_ground_than_a_walk() {
        let mut blocks = Vec::new();
        for z in 0..8u8 {
            blocks.push((0, 0, z, Block::new("stone", 0)));
        }
        let world =
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, blocks).unwrap()]).unwrap();
        let travel = |crouch: bool| {
            let eye = Vec3::new(0.5, 1.0 + EYE_HEIGHT, 6.5);
            let mut camera = Camera::looking_at(eye, Vec3::new(0.5, 1.4, 0.0));
            let mut walk = idle();
            walk.forward = true;
            walk.crouch = crouch;
            walk.fast = true;
            for _ in 0..40 {
                step(&mut camera, 1.0 / 60.0, &walk, &world);
            }
            eye.z - camera.eye.z
        };
        let open = travel(false);
        let low = travel(true);
        assert!(open > 2.0, "a sprint should cover ground, moved {open}");
        assert!(
            low < open * (super::CROUCH_DRAG + 0.08),
            "crouch stays slow even with sprint held, open {open} low {low}"
        );
        assert!(low > 0.25, "a crouch still moves, moved {low}");
    }

    #[test]
    fn a_crouch_fits_under_a_low_ceiling_and_stands_when_clear() {
        let mut blocks = Vec::new();
        for z in 0..6u8 {
            blocks.push((0, 0, z, Block::new("stone", 0)));
        }
        // Crawlspace: floor top at y=1, top slab at y=2 → ceiling at 2.5 (1.5 clearance).
        for z in 0..3u8 {
            blocks.push((0, 2, z, Block::new("slab", 1)));
        }
        let world =
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, blocks).unwrap()]).unwrap();
        let eye = Vec3::new(0.5, 1.0 + EYE_HEIGHT, 1.5);
        assert!(
            !super::can_stand(eye, &world, &[]),
            "standing body should hit the slab ceiling"
        );
        let (_, stand_max) = super::player_box_bounds_at(eye, false);
        let (_, crouch_max) = super::player_box_bounds_at(eye, true);
        assert!(stand_max[1] > 2.5, "standing reaches into the slab");
        assert!(
            (crouch_max[1] - 2.5).abs() < 1.0e-3,
            "crouch fills the 1.5 crawlspace, top {}",
            crouch_max[1]
        );
        let open = Vec3::new(0.5, 1.0 + EYE_HEIGHT, 4.5);
        assert!(super::can_stand(open, &world, &[]));
    }

    #[test]
    fn a_crouch_clings_to_a_block_edge() {
        let mut blocks = Vec::new();
        // Platform only covers z=2..4; air beyond toward -Z.
        for z in 2..5u8 {
            blocks.push((0, 0, z, Block::new("stone", 0)));
        }
        let world =
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, blocks).unwrap()]).unwrap();
        let eye = Vec3::new(0.5, 1.0 + EYE_HEIGHT, 2.4);
        let mut camera = Camera::looking_at(eye, Vec3::new(0.5, 1.4, 0.0));
        // Settle onto the platform.
        for _ in 0..20 {
            step(&mut camera, 1.0 / 60.0, &idle(), &world);
        }
        assert!(camera.on_ground);
        let start_z = camera.eye.z;
        let mut sneak = idle();
        sneak.forward = true;
        sneak.crouch = true;
        for _ in 0..90 {
            step(&mut camera, 1.0 / 60.0, &sneak, &world);
        }
        assert!(
            camera.on_ground,
            "sneak should stay on the platform, z {}",
            camera.eye.z
        );
        assert!(
            camera.eye.z >= 2.0 - HALF_WIDTH - 0.05,
            "sneak should cling near the north edge, z {} started {start_z}",
            camera.eye.z
        );
        assert!(
            camera.eye.z < start_z + 0.01,
            "sneak should edge toward the drop, z {} started {start_z}",
            camera.eye.z
        );
        // Without sneak, the same walk falls off.
        let mut camera = Camera::looking_at(eye, Vec3::new(0.5, 1.4, 0.0));
        for _ in 0..20 {
            step(&mut camera, 1.0 / 60.0, &idle(), &world);
        }
        let mut walk = idle();
        walk.forward = true;
        for _ in 0..90 {
            step(&mut camera, 1.0 / 60.0, &walk, &world);
        }
        assert!(
            camera.eye.z < 1.5 || !camera.on_ground,
            "a walk should leave the platform, z {} ground {}",
            camera.eye.z,
            camera.on_ground
        );
    }

    #[test]
    fn a_magma_block_is_a_solid_cube_you_can_step_onto() {
        assert!(shape_of(Block::new("magma", 0)).is_full_cube());
        let mut blocks = Vec::new();
        for z in 0..5u8 {
            blocks.push((0, 0, z, Block::new("stone", 0)));
        }
        blocks.push((0, 1, 0, Block::new("magma", 0)));
        let world =
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, blocks).unwrap()]).unwrap();
        let climbed = walk_north(&world, 20);
        let feet = feet_y(climbed.eye.y);
        assert!(
            (feet - 2.0).abs() < 0.05,
            "did not step onto the magma, feet {feet}"
        );
    }

    #[test]
    fn a_netherrack_block_is_a_solid_cube_you_can_step_onto() {
        assert!(shape_of(Block::new("netherrack", 0)).is_full_cube());
        let mut blocks = Vec::new();
        for z in 0..5u8 {
            blocks.push((0, 0, z, Block::new("stone", 0)));
        }
        blocks.push((0, 1, 0, Block::new("netherrack", 0)));
        let world =
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, blocks).unwrap()]).unwrap();
        let climbed = walk_north(&world, 20);
        let feet = feet_y(climbed.eye.y);
        assert!(
            (feet - 2.0).abs() < 0.05,
            "did not step onto the netherrack, feet {feet}"
        );
    }

    #[test]
    fn an_end_stone_block_is_a_solid_cube_you_can_step_onto() {
        assert!(shape_of(Block::new("end_stone", 0)).is_full_cube());
        let mut blocks = Vec::new();
        for z in 0..5u8 {
            blocks.push((0, 0, z, Block::new("stone", 0)));
        }
        blocks.push((0, 1, 0, Block::new("end_stone", 0)));
        let world =
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, blocks).unwrap()]).unwrap();
        let climbed = walk_north(&world, 20);
        let feet = feet_y(climbed.eye.y);
        assert!(
            (feet - 2.0).abs() < 0.05,
            "did not step onto the end stone, feet {feet}"
        );
    }

    #[test]
    fn an_end_stone_bricks_block_is_a_solid_cube_you_can_step_onto() {
        assert!(shape_of(Block::new("end_stone_bricks", 0)).is_full_cube());
        let mut blocks = Vec::new();
        for z in 0..5u8 {
            blocks.push((0, 0, z, Block::new("stone", 0)));
        }
        blocks.push((0, 1, 0, Block::new("end_stone_bricks", 0)));
        let world =
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, blocks).unwrap()]).unwrap();
        let climbed = walk_north(&world, 20);
        let feet = feet_y(climbed.eye.y);
        assert!(
            (feet - 2.0).abs() < 0.05,
            "did not step onto the end stone bricks, feet {feet}"
        );
    }

    #[test]
    fn a_purpur_block_is_a_solid_cube_you_can_step_onto() {
        assert!(shape_of(Block::new("purpur", 0)).is_full_cube());
        let mut blocks = Vec::new();
        for z in 0..5u8 {
            blocks.push((0, 0, z, Block::new("stone", 0)));
        }
        blocks.push((0, 1, 0, Block::new("purpur", 0)));
        let world =
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, blocks).unwrap()]).unwrap();
        let climbed = walk_north(&world, 20);
        let feet = feet_y(climbed.eye.y);
        assert!(
            (feet - 2.0).abs() < 0.05,
            "did not step onto the purpur, feet {feet}"
        );
    }

    #[test]
    fn a_purpur_pillar_block_is_a_solid_cube_you_can_step_onto() {
        assert!(shape_of(Block::new("purpur_pillar", 0)).is_full_cube());
        let mut blocks = Vec::new();
        for z in 0..5u8 {
            blocks.push((0, 0, z, Block::new("stone", 0)));
        }
        blocks.push((0, 1, 0, Block::new("purpur_pillar", 0)));
        let world =
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, blocks).unwrap()]).unwrap();
        let climbed = walk_north(&world, 20);
        let feet = feet_y(climbed.eye.y);
        assert!(
            (feet - 2.0).abs() < 0.05,
            "did not step onto the purpur pillar, feet {feet}"
        );
    }

    #[test]
    fn standing_on_magma_burns_on_the_cactus_timer() {
        let mut blocks = Vec::new();
        for z in 0..4u8 {
            blocks.push((0, 0, z, Block::new("stone", 0)));
        }
        blocks.push((0, 0, 0, Block::new("magma", 0)));
        let world =
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, blocks).unwrap()]).unwrap();
        let on = Vec3::new(0.5, 1.0 + EYE_HEIGHT, 0.5);
        let off = Vec3::new(0.5, 1.0 + EYE_HEIGHT, 2.5);
        assert!(super::stands_on_magma(on, &world));
        assert!(!super::stands_on_magma(off, &world));
        let mut left = 0.0;
        let mut burns = 0;
        for _ in 0..6 {
            let poke;
            (left, poke) = super::cactus_poke(super::stands_on_magma(on, &world), left, 0.2);
            if poke {
                burns += 1;
            }
        }
        assert_eq!(burns, 2, "a burn every 0.6 seconds, got {burns}");
        assert_eq!(super::cactus_poke(false, left, 0.2).0, 0.0);
    }

    #[test]
    fn a_cobweb_is_a_cross_you_walk_through_slowly() {
        let shape = shape_of(Block::new("cobweb", 0));
        assert!(shape.boxes().is_empty());
        assert_eq!(shape.planes().len(), 2);
        assert!(!shape.is_full_cube());
        let floor = |web: bool| {
            let mut blocks = Vec::new();
            for z in 0..6u8 {
                blocks.push((0, 0, z, Block::new("stone", 0)));
                if web {
                    blocks.push((0, 1, z, Block::new("cobweb", 0)));
                }
            }
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, blocks).unwrap()]).unwrap()
        };
        let travel = |world: &Cluster| {
            let eye = Vec3::new(0.5, 1.0 + EYE_HEIGHT, 4.5);
            let mut camera = Camera::looking_at(eye, Vec3::new(0.5, 1.4, 0.0));
            let mut walk = idle();
            walk.forward = true;
            for _ in 0..40 {
                step(&mut camera, 1.0 / 60.0, &walk, world);
            }
            (eye.z - camera.eye.z, feet_y(camera.eye.y))
        };
        let (open, open_feet) = travel(&floor(false));
        let (slow, slow_feet) = travel(&floor(true));
        assert!(
            (open_feet - 1.0).abs() < 0.05 && (slow_feet - 1.0).abs() < 0.05,
            "the web should not lift you, open {open_feet} slow {slow_feet}"
        );
        assert!(open > 2.0, "an open walk should cover ground, moved {open}");
        assert!(
            slow > 0.3,
            "the web should still let you through, moved {slow}"
        );
        assert!(
            slow < open * (super::COBWEB_DRAG + 0.05),
            "the web should be a quarter of a walk, open {open} slow {slow}"
        );

        let sheet = Cluster::try_from_chunks(
            0,
            0,
            [Chunk::from_blocks(
                0,
                0,
                [
                    (0, 0, 0, Block::new("stone", 0)),
                    (0, 3, 0, Block::new("cobweb", 0)),
                ],
            )
            .unwrap()],
        )
        .unwrap();
        let mut falling = Camera::looking_at(
            Vec3::new(0.5, 3.2 + EYE_HEIGHT, 0.5),
            Vec3::new(0.5, 1.0, 1.5),
        );
        let still = idle();
        for _ in 0..20 {
            step(&mut falling, 1.0 / 60.0, &still, &sheet);
        }
        let landed = feet_y(falling.eye.y);
        assert!(
            landed > 2.4,
            "a fall through the web should stay slow, feet {landed}"
        );
    }

    #[test]
    fn a_carpet_is_crossed_without_a_step() {
        let shape = shape_of(Block::new("carpet", 0));
        assert_eq!(shape.boxes()[0].max[1], 1);
        assert!(shape.occludes(8, 0, 8));
        assert!(!shape.occludes(8, 8, 8));
        let mut blocks = Vec::new();
        for z in 0..5u8 {
            blocks.push((0, 0, z, Block::new("stone", 0)));
        }
        blocks.push((0, 1, 1, Block::new("carpet", 0)));
        let world =
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, blocks).unwrap()]).unwrap();
        let crossed = walk_north(&world, 20);
        let feet = feet_y(crossed.eye.y);
        assert!(
            (feet - 1.0).abs() < 0.02,
            "the carpet should not step you up, feet {feet}"
        );
        let min_z = crossed.eye.z - HALF_WIDTH;
        assert!(
            min_z < 1.0,
            "stopped at the carpet instead of walking across, min z {min_z}"
        );

        let sheet = Cluster::try_from_chunks(
            0,
            0,
            [Chunk::from_blocks(0, 0, [(0, 1, 0, Block::new("carpet", 0))]).unwrap()],
        )
        .unwrap();
        let mut dropped = Camera::looking_at(
            Vec3::new(0.5, 1.0625 + 0.5 + EYE_HEIGHT, 0.5),
            Vec3::new(0.5, 1.0, 1.5),
        );
        let still = idle();
        for _ in 0..40 {
            step(&mut dropped, 1.0 / 60.0, &still, &sheet);
        }
        let landed = feet_y(dropped.eye.y);
        assert!(
            (landed - (1.0 + 1.0 / 16.0)).abs() < 0.05,
            "a fall should land on the carpet, feet {landed}"
        );
    }

    #[test]
    fn a_snow_layer_is_crossed_without_a_step() {
        let shape = shape_of(Block::new("snow", 0));
        assert_eq!(shape.boxes()[0].max[1], 1);
        assert!(!shape.is_full_cube());
        let mut blocks = Vec::new();
        for z in 0..5u8 {
            blocks.push((0, 0, z, Block::new("stone", 0)));
        }
        blocks.push((0, 1, 1, Block::new("snow", 0)));
        let world =
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, blocks).unwrap()]).unwrap();
        let crossed = walk_north(&world, 20);
        let feet = feet_y(crossed.eye.y);
        assert!(
            (feet - 1.0).abs() < 0.02,
            "the snow should not step you up, feet {feet}"
        );
        let min_z = crossed.eye.z - HALF_WIDTH;
        assert!(
            min_z < 1.0,
            "stopped at the snow instead of walking across, min z {min_z}"
        );

        let sheet = Cluster::try_from_chunks(
            0,
            0,
            [Chunk::from_blocks(0, 0, [(0, 1, 0, Block::new("snow", 0))]).unwrap()],
        )
        .unwrap();
        let mut dropped = Camera::looking_at(
            Vec3::new(0.5, 1.0625 + 0.5 + EYE_HEIGHT, 0.5),
            Vec3::new(0.5, 1.0, 1.5),
        );
        let still = idle();
        for _ in 0..40 {
            step(&mut dropped, 1.0 / 60.0, &still, &sheet);
        }
        let landed = feet_y(dropped.eye.y);
        assert!(
            (landed - (1.0 + 1.0 / 16.0)).abs() < 0.05,
            "a fall should land on the snow, feet {landed}"
        );
    }

    #[test]
    fn a_flower_does_not_stop_the_walk() {
        let mut blocks = Vec::new();
        for z in 0..5u8 {
            blocks.push((0, 0, z, Block::new("grass", 0)));
        }
        blocks.push((0, 1, 1, Block::new("flower", 0)));
        let world =
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, blocks).unwrap()]).unwrap();
        let crossed = walk_north(&world, 20);
        let feet = feet_y(crossed.eye.y);
        assert!(
            (feet - 1.0).abs() < 0.02,
            "the flower should not lift you, feet {feet}"
        );
        let min_z = crossed.eye.z - HALF_WIDTH;
        assert!(
            min_z < 1.0,
            "stopped on the flower instead of walking through, min z {min_z}"
        );
    }

    #[test]
    fn a_copper_grate_is_a_lattice_you_walk_through() {
        let shape = shape_of(Block::new("copper_grate", 0));
        assert!(!shape.is_full_cube());
        assert_eq!(shape.boxes().len(), 6);
        let mut blocks = Vec::new();
        for z in 0..5u8 {
            blocks.push((0, 0, z, Block::new("grass", 0)));
        }
        blocks.push((0, 1, 1, Block::new("copper_grate", 0)));
        let world =
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, blocks).unwrap()]).unwrap();
        let crossed = walk_north(&world, 20);
        let feet = feet_y(crossed.eye.y);
        assert!(
            (feet - 1.0).abs() < 0.02,
            "the grate should not lift you, feet {feet}"
        );
        let min_z = crossed.eye.z - HALF_WIDTH;
        assert!(
            min_z < 1.0,
            "stopped on the grate instead of walking through, min z {min_z}"
        );
    }

    #[test]
    fn a_sugar_cane_is_a_cross_you_walk_through() {
        let shape = shape_of(Block::new("sugar_cane", 0));
        assert!(shape.boxes().is_empty());
        assert_eq!(shape.planes().len(), 2);
        let mut blocks = Vec::new();
        for z in 0..5u8 {
            blocks.push((0, 0, z, Block::new("sand", 0)));
        }
        blocks.push((0, 1, 1, Block::new("sugar_cane", 0)));
        let world =
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, blocks).unwrap()]).unwrap();
        let crossed = walk_north(&world, 20);
        let feet = feet_y(crossed.eye.y);
        assert!(
            (feet - 1.0).abs() < 0.02,
            "the cane should not lift you, feet {feet}"
        );
        let min_z = crossed.eye.z - HALF_WIDTH;
        assert!(
            min_z < 1.0,
            "stopped on the cane instead of walking through, min z {min_z}"
        );
    }

    #[test]
    fn a_chorus_plant_is_a_cross_you_walk_through() {
        let shape = shape_of(Block::new("chorus_plant", 0));
        assert!(shape.boxes().is_empty());
        assert_eq!(shape.planes().len(), 2);
        let mut blocks = Vec::new();
        for z in 0..5u8 {
            blocks.push((0, 0, z, Block::new("end_stone", 0)));
        }
        blocks.push((0, 1, 1, Block::new("chorus_plant", 0)));
        let world =
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, blocks).unwrap()]).unwrap();
        let crossed = walk_north(&world, 20);
        let feet = feet_y(crossed.eye.y);
        assert!(
            (feet - 1.0).abs() < 0.02,
            "the chorus plant should not lift you, feet {feet}"
        );
        let min_z = crossed.eye.z - HALF_WIDTH;
        assert!(
            min_z < 1.0,
            "stopped on the chorus plant instead of walking through, min z {min_z}"
        );
    }

    #[test]
    fn a_chorus_flower_is_a_bloom_you_walk_through() {
        let shape = shape_of(Block::new("chorus_flower", 0));
        assert!(shape.boxes().is_empty());
        assert_eq!(shape.planes().len(), 2);
        let mut blocks = Vec::new();
        for z in 0..5u8 {
            blocks.push((0, 0, z, Block::new("end_stone", 0)));
        }
        blocks.push((0, 1, 1, Block::new("chorus_flower", 0)));
        let world =
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, blocks).unwrap()]).unwrap();
        let crossed = walk_north(&world, 20);
        let feet = feet_y(crossed.eye.y);
        assert!(
            (feet - 1.0).abs() < 0.02,
            "the chorus flower should not lift you, feet {feet}"
        );
        let min_z = crossed.eye.z - HALF_WIDTH;
        assert!(
            min_z < 1.0,
            "stopped on the chorus flower instead of walking through, min z {min_z}"
        );
    }

    #[test]
    fn ice_keeps_the_walk_going_after_you_let_go() {
        let coast = |id: &str| {
            let mut blocks = Vec::new();
            for z in 0..16u8 {
                blocks.push((0, 0, z, Block::new(id, 0)));
            }
            let world = Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, blocks).unwrap()])
                .unwrap();
            let mut camera = Camera::looking_at(
                Vec3::new(0.5, 1.0 + EYE_HEIGHT, 12.5),
                Vec3::new(0.5, 1.4, 0.0),
            );
            let mut walk = idle();
            walk.forward = true;
            for _ in 0..30 {
                step(&mut camera, 1.0 / 60.0, &walk, &world);
            }
            let released = camera.eye.z;
            let still = idle();
            for _ in 0..45 {
                step(&mut camera, 1.0 / 60.0, &still, &world);
            }
            released - camera.eye.z
        };
        let slipped = coast("ice");
        let stopped = coast("stone");
        assert!(
            slipped > 1.0,
            "letting go on ice should still carry you, moved {slipped}"
        );
        assert!(
            stopped < 0.05,
            "letting go on stone should stop, moved {stopped}"
        );
    }

    #[test]
    fn a_button_is_a_short_nub_you_walk_through() {
        let shape = shape_of(Block::new("button", 0));
        assert_eq!(shape.boxes()[0].max[1], 2);
        assert!(!shape.is_full_cube());
        let mut blocks = Vec::new();
        for z in 0..5u8 {
            blocks.push((0, 0, z, Block::new("stone", 0)));
        }
        blocks.push((0, 1, 1, Block::new("button", 0)));
        let world =
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, blocks).unwrap()]).unwrap();
        let crossed = walk_north(&world, 20);
        let min_z = crossed.eye.z - HALF_WIDTH;
        assert!(
            min_z < 1.0,
            "stopped on the button instead of walking past, min z {min_z}"
        );
    }

    #[test]
    fn a_lever_is_a_stick_you_walk_past() {
        let off = shape_of(Block::new("lever", 0));
        assert_eq!(off.boxes()[0].max[1], 12);
        assert!(!off.is_full_cube());
        let on = shape_of(Block::new("lever", 1));
        assert_eq!(on.boxes()[0].max[1], 6);
        let mut blocks = Vec::new();
        for z in 0..5u8 {
            blocks.push((0, 0, z, Block::new("stone", 0)));
        }
        blocks.push((0, 1, 1, Block::new("lever", 0)));
        let world =
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, blocks).unwrap()]).unwrap();
        let crossed = walk_north(&world, 20);
        let min_z = crossed.eye.z - HALF_WIDTH;
        assert!(
            min_z < 1.0,
            "stopped on the lever instead of walking past, min z {min_z}"
        );
    }

    #[test]
    fn an_end_rod_is_a_thin_post_that_stops_a_centered_walk() {
        let shape = shape_of(Block::new("end_rod", 0));
        assert!(!shape.is_full_cube());
        assert!(
            shape
                .boxes()
                .iter()
                .all(|box_| box_.max[0] - box_.min[0] <= 6)
        );
        let mut blocks = Vec::new();
        for z in 0..5u8 {
            blocks.push((0, 0, z, Block::new("stone", 0)));
        }
        blocks.push((0, 1, 1, Block::new("end_rod", 0)));
        let world =
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, blocks).unwrap()]).unwrap();
        let stopped = walk_north(&world, 20);
        let feet = feet_y(stopped.eye.y);
        assert!(
            (feet - 1.0).abs() < 0.05,
            "stepped onto the end rod, feet {feet}"
        );
        let min_z = stopped.eye.z - HALF_WIDTH;
        assert!(min_z > 1.0, "walked through the end rod, min z {min_z}");
    }

    #[test]
    fn a_note_block_is_a_solid_cube() {
        assert!(shape_of(Block::new("note_block", 0)).is_full_cube());
        assert!(shape_of(Block::new("note_block", 15)).is_full_cube());
    }

    #[test]
    fn a_copper_chest_is_a_solid_cube_you_can_step_onto() {
        assert!(shape_of(Block::new("copper_chest", 0)).is_full_cube());
        let mut blocks = Vec::new();
        for z in 0..5u8 {
            blocks.push((0, 0, z, Block::new("stone", 0)));
        }
        blocks.push((0, 1, 0, Block::new("copper_chest", 0)));
        let world =
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, blocks).unwrap()]).unwrap();
        let stood = walk_north(&world, 20);
        let feet = feet_y(stood.eye.y);
        assert!(
            (feet - 2.0).abs() < 0.05,
            "did not step onto the copper chest, feet {feet}"
        );
        assert!(stood.on_ground);
    }

    #[test]
    fn a_bookshelf_is_a_solid_cube_you_can_step_onto() {
        assert!(shape_of(Block::new("bookshelf", 0)).is_full_cube());
        let mut blocks = Vec::new();
        for z in 0..5u8 {
            blocks.push((0, 0, z, Block::new("stone", 0)));
        }
        blocks.push((0, 1, 0, Block::new("bookshelf", 0)));
        let world =
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, blocks).unwrap()]).unwrap();
        let stood = walk_north(&world, 20);
        let feet = feet_y(stood.eye.y);
        assert!(
            (feet - 2.0).abs() < 0.05,
            "did not step onto the bookshelf, feet {feet}"
        );
        assert!(stood.on_ground);
    }

    #[test]
    fn a_tripwire_is_a_string_you_walk_through() {
        let shape = shape_of(Block::new("tripwire", 0));
        assert!(!shape.is_full_cube());
        assert_eq!(shape.boxes()[0].min[1], 6);
        assert_eq!(shape.boxes()[0].max[1], 8);
        let mut blocks = Vec::new();
        for z in 0..5u8 {
            blocks.push((0, 0, z, Block::new("stone", 0)));
        }
        blocks.push((0, 1, 1, Block::new("tripwire", 0)));
        let world =
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, blocks).unwrap()]).unwrap();
        let crossed = walk_north(&world, 20);
        let feet = feet_y(crossed.eye.y);
        assert!(
            (feet - 1.0).abs() < 0.05,
            "the string should not lift you, feet {feet}"
        );
        let min_z = crossed.eye.z - HALF_WIDTH;
        assert!(
            min_z < 1.0,
            "stopped on the tripwire instead of walking through, min z {min_z}"
        );
    }

    #[test]
    fn an_item_frame_is_a_thin_board_you_walk_through() {
        let empty = shape_of(Block::new("item_frame", 4));
        let shown = shape_of(Block::new("item_frame", 4 | 8));
        assert_eq!(empty.boxes().len(), 1);
        assert_eq!(shown.boxes().len(), 2);
        let mut blocks = Vec::new();
        for z in 0..5u8 {
            blocks.push((0, 0, z, Block::new("stone", 0)));
        }
        blocks.push((0, 1, 1, Block::new("item_frame", 4)));
        let world =
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, blocks).unwrap()]).unwrap();
        let passed = walk_north(&world, 40);
        assert!(
            passed.eye.z - HALF_WIDTH < 0.5,
            "the frame should let the walk through"
        );
    }

    #[test]
    fn a_painting_is_a_thin_canvas_you_walk_through() {
        let shape = shape_of(Block::new("painting", 4));
        assert_eq!(shape.boxes()[0].min[2], 15);
        assert_eq!(shape.boxes()[0].max[2], 16);
        assert!(!shape.is_full_cube());
        let mut blocks = Vec::new();
        for z in 0..5u8 {
            blocks.push((0, 0, z, Block::new("stone", 0)));
        }
        blocks.push((0, 1, 1, Block::new("painting", 4)));
        let world =
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, blocks).unwrap()]).unwrap();
        let crossed = walk_north(&world, 20);
        let feet = feet_y(crossed.eye.y);
        assert!(
            (feet - 1.0).abs() < 0.05,
            "the painting should not lift you, feet {feet}"
        );
        let min_z = crossed.eye.z - HALF_WIDTH;
        assert!(
            min_z < 1.0,
            "stopped on the painting instead of walking through, min z {min_z}"
        );
    }

    #[test]
    fn a_flower_pot_does_not_stop_the_walk() {
        let shape = shape_of(Block::new("flower_pot", 0));
        assert_eq!(shape.boxes()[0].max[1], 6);
        assert!(!shape.is_full_cube());
        let mut blocks = Vec::new();
        for z in 0..5u8 {
            blocks.push((0, 0, z, Block::new("stone", 0)));
        }
        blocks.push((0, 1, 1, Block::new("flower_pot", 1)));
        let world =
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, blocks).unwrap()]).unwrap();
        let crossed = walk_north(&world, 20);
        let feet = feet_y(crossed.eye.y);
        assert!(
            (feet - 1.0).abs() < 0.05,
            "the pot should not lift you, feet {feet}"
        );
        let min_z = crossed.eye.z - HALF_WIDTH;
        assert!(
            min_z < 1.0,
            "stopped on the pot instead of walking past, min z {min_z}"
        );
    }

    #[test]
    fn a_cactus_column_stops_the_walk_and_pricks_beside_it() {
        let mut blocks = Vec::new();
        for z in 0..5u8 {
            blocks.push((0, 0, z, Block::new("sand", 0)));
        }
        blocks.push((0, 1, 1, Block::new("cactus", 0)));
        blocks.push((0, 2, 1, Block::new("cactus", 0)));
        let world =
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, blocks).unwrap()]).unwrap();
        let stopped = walk_north(&world, 20);
        let min_z = stopped.eye.z - HALF_WIDTH;
        assert!(min_z > 1.0, "walked through the cactus, min z {min_z}");
        let beside = Vec3::new(0.5, 1.0 + EYE_HEIGHT, 1.25);
        assert!(super::touches_cactus(beside, &world));
        let clear = Vec3::new(0.5, 1.0 + EYE_HEIGHT, 3.5);
        assert!(!super::touches_cactus(clear, &world));
        let (left, poke) = super::cactus_poke(true, 0.0, 0.016);
        assert!(poke);
        assert!((left - super::CACTUS_POKE).abs() < 1.0e-4);
        let (_, again) = super::cactus_poke(true, left, 0.016);
        assert!(!again);
        assert_eq!(super::cactus_poke(false, left, 0.016).0, 0.0);
    }

    #[test]
    fn a_plate_is_two_pixels_and_you_stand_on_it() {
        let shape = shape_of(Block::new("plate", 0));
        assert_eq!(shape.boxes()[0].max[1], 2);
        assert!(!shape.is_full_cube());
        let mut blocks = Vec::new();
        for z in 0..5u8 {
            blocks.push((0, 0, z, Block::new("stone", 0)));
        }
        blocks.push((0, 1, 1, Block::new("plate", 0)));
        let world =
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, blocks).unwrap()]).unwrap();
        let stood = walk_north(&world, 10);
        let feet = feet_y(stood.eye.y);
        assert!(
            (feet - (1.0 + 2.0 / 16.0)).abs() < 0.05,
            "standing on the plate should lift you two pixels, feet {feet}"
        );
        assert!(
            (stood.eye.z - 1.5).abs() < 0.35,
            "should be on the plate cell, eye z {}",
            stood.eye.z
        );
    }

    #[test]
    fn a_lily_pad_is_a_thin_floor_on_water() {
        let shape = shape_of(Block::new("lily_pad", 0));
        assert_eq!(shape.boxes()[0].max[1], 2);
        assert!(!shape.is_full_cube());
        let mut blocks = Vec::new();
        for z in 0..5u8 {
            blocks.push((0, 0, z, Block::new("stone", 0)));
            if z < 2 {
                blocks.push((0, 1, z, Block::new("water", 0)));
                blocks.push((0, 2, z, Block::new("lily_pad", 0)));
            } else {
                blocks.push((0, 1, z, Block::new("stone", 0)));
            }
        }
        let world =
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, blocks.clone()).unwrap()])
                .unwrap();
        let eye = Vec3::new(0.5, 2.0 + EYE_HEIGHT, 2.5);
        let mut camera = Camera::looking_at(eye, Vec3::new(0.5, 2.0, 0.0));
        let mut walk = idle();
        walk.forward = true;
        for _ in 0..20 {
            step(&mut camera, 1.0 / 60.0, &walk, &world);
        }
        let feet = feet_y(camera.eye.y);
        assert!(
            (feet - (2.0 + 2.0 / 16.0)).abs() < 0.05,
            "the pad should hold you two pixels above the water, feet {feet}"
        );
        assert!(
            camera.eye.z < 2.0,
            "did not walk onto the pad, z {}",
            camera.eye.z
        );
        assert!(camera.on_ground);

        let mut open = blocks.clone();
        open.retain(|block| block.3.id != "lily_pad");
        let sunk =
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, open).unwrap()]).unwrap();
        let mut falling = Camera::looking_at(eye, Vec3::new(0.5, 2.0, 0.0));
        for _ in 0..80 {
            step(&mut falling, 1.0 / 60.0, &walk, &sunk);
        }
        let dropped = feet_y(falling.eye.y);
        assert!(
            dropped < 1.6,
            "without the pad the walk should fall into the water, feet {dropped}"
        );
    }

    #[test]
    fn an_upper_slab_blocks_the_head() {
        let shape = shape_of(Block::new("slab", 1));
        assert_eq!(shape.boxes()[0].min[1], 8);
        assert_eq!(shape.boxes()[0].max[1], 16);
        assert!(!shape.occludes(8, 0, 8), "the lower half stays open");
        assert!(shape.occludes(8, 12, 8), "the upper half is solid");
        let mut blocks = Vec::new();
        for z in 0..5u8 {
            blocks.push((0, 0, z, Block::new("stone", 0)));
        }
        blocks.push((0, 2, 1, Block::new("slab", 1)));
        let world =
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, blocks).unwrap()]).unwrap();
        let stopped = walk_north(&world, 40);
        let face = 2.0;
        let min_z = stopped.eye.z - HALF_WIDTH;
        assert!(
            min_z >= face - 1.0e-3,
            "walked through the upper slab, min z {min_z}"
        );
        assert!(
            min_z < face + 0.08,
            "stopped short of the upper slab, min z {min_z}"
        );
        assert!(
            (feet_y(stopped.eye.y) - 1.0).abs() < 0.05,
            "left the floor, feet {}",
            feet_y(stopped.eye.y)
        );
    }

    #[test]
    fn two_block_rise_stops_the_walk_and_jump_still_launches() {
        let world = stone_path(&[(0, 1, 0), (0, 2, 0)]);
        let mut camera = walk_north(&world, 40);
        let feet = feet_y(camera.eye.y);
        assert!(
            (feet - 1.0).abs() < 0.05,
            "walked up a two-block wall, feet {feet}"
        );
        let min_z = camera.eye.z - HALF_WIDTH;
        assert!(
            min_z >= 1.0 - 1.0e-3,
            "walked through the two-block wall, min z {min_z}"
        );
        assert!(min_z < 1.05, "stopped short of the wall, min z {min_z}");

        let before = camera.eye.y;
        let mut jump = idle();
        jump.forward = true;
        jump.jump = true;
        step(&mut camera, 1.0 / 60.0, &jump, &world);
        assert!(
            camera.eye.y > before + 0.05,
            "jump did not leave the ground"
        );
        assert!(!camera.on_ground);
        let min_z = camera.eye.z - HALF_WIDTH;
        assert!(
            min_z >= 1.0 - 1.0e-3,
            "jump stepped through the wall, min z {min_z}"
        );
    }

    #[test]
    fn a_ceiling_blocks_the_step() {
        // The rise is one block, but the cell above the head is solid.
        // Standing fits. The raised body does not.
        let world = stone_path(&[(0, 1, 0), (0, 3, 0), (0, 3, 1), (0, 3, 2)]);
        let camera = walk_north(&world, 40);
        let feet = feet_y(camera.eye.y);
        assert!(
            (feet - 1.0).abs() < 0.05,
            "stepped up through the ceiling, feet {feet}"
        );
        let min_z = camera.eye.z - HALF_WIDTH;
        assert!(
            min_z >= 1.0 - 1.0e-3,
            "walked through the rise under the ceiling, min z {min_z}"
        );
        assert!(min_z < 1.05, "stopped short of the rise, min z {min_z}");
    }

    #[test]
    fn the_player_walks_up_the_stair() {
        // Floor under the path, and oak stairs in the cell to the north.
        // The lower tread is half a block; the upper tread finishes the block.
        let chunk = Chunk::from_blocks(
            0,
            0,
            [
                (0, 0, 0, Block::new("stone", 0)),
                (0, 0, 1, Block::new("stone", 0)),
                (0, 0, 2, Block::new("stone", 0)),
                (0, 1, 0, Block::new(OAK_STAIRS, 0)),
            ],
        )
        .unwrap();
        let world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let camera = walk_north(&world, 20);
        let feet = feet_y(camera.eye.y);
        assert!(
            feet > 1.4,
            "stopped at the bottom of the stair, feet {feet}"
        );
        assert!(feet < 2.15, "climbed past the stair, feet {feet}");
        assert!(camera.on_ground);
        let min_z = camera.eye.z - HALF_WIDTH;
        assert!(min_z < 1.0, "did not move onto the stair, min z {min_z}");
        assert!(min_z > -0.05, "walked off the stair, min z {min_z}");
    }

    fn sign_on_a_floor() -> Cluster {
        let chunk = Chunk::from_blocks(
            0,
            0,
            [
                (0, 0, 0, Block::new("stone", 0)),
                (0, 0, 1, Block::new("stone", 0)),
                (0, 0, 2, Block::new("stone", 0)),
                (0, 1, 0, Block::new("sign", 0)),
            ],
        )
        .unwrap();
        Cluster::try_from_chunks(0, 0, [chunk]).unwrap()
    }

    #[test]
    fn sign_post_box_blocks_the_player() {
        let world = sign_on_a_floor();
        let post = shape_of(Block::new("sign", 0)).boxes()[0];
        let face = f32::from(post.max[2]) / 16.0;
        let eye = Vec3::new(0.5, 1.0 + EYE_HEIGHT, 2.5);
        let mut camera = Camera::looking_at(eye, Vec3::new(0.5, 1.4, 0.0));
        let mut walk = idle();
        walk.forward = true;
        for _ in 0..120 {
            step(&mut camera, 1.0 / 60.0, &walk, &world);
        }
        let min_z = camera.eye.z - HALF_WIDTH;
        assert!(
            min_z >= face - 1.0e-3,
            "walked through the post, min z {min_z}"
        );
        assert!(
            min_z < face + 0.05,
            "stopped short of the post, min z {min_z}"
        );
    }

    #[test]
    fn a_closed_door_stops_the_walk_and_an_open_one_does_not() {
        let closed = shape_of(Block::new("door", 0));
        let open = shape_of(Block::new("door", 1));
        assert_eq!(closed.boxes()[0].max[2], 3);
        assert_eq!(open.boxes()[0].max[0], 3);
        let mut blocks = Vec::new();
        for z in 0..5u8 {
            blocks.push((0, 0, z, Block::new("stone", 0)));
        }
        blocks.push((0, 1, 1, Block::new("door", 0)));
        let shut =
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, blocks).unwrap()]).unwrap();
        let stopped = walk_north(&shut, 90);
        let face = 1.0 + 3.0 / 16.0;
        let min_z = stopped.eye.z - HALF_WIDTH;
        assert!(
            min_z >= face - 1.0e-3,
            "walked through the door, min z {min_z}"
        );
        assert!(
            min_z < face + 0.08,
            "stopped short of the door, min z {min_z}"
        );

        let mut open_blocks = Vec::new();
        for z in 0..5u8 {
            open_blocks.push((0, 0, z, Block::new("stone", 0)));
        }
        open_blocks.push((0, 1, 1, Block::new("door", 1)));
        let swung =
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, open_blocks).unwrap()])
                .unwrap();
        let passed = walk_north(&swung, 90);
        let passed_z = passed.eye.z - HALF_WIDTH;
        assert!(
            passed_z < 0.5,
            "an open door should let the walk through, min z {passed_z}"
        );
    }

    #[test]
    fn a_closed_copper_door_stops_the_walk_and_an_open_one_does_not() {
        let closed = shape_of(Block::new("copper_door", 0));
        let open = shape_of(Block::new("copper_door", 1));
        assert_eq!(closed.boxes()[0].max[2], 3);
        assert_eq!(open.boxes()[0].max[0], 3);
        let mut blocks = Vec::new();
        for z in 0..5u8 {
            blocks.push((0, 0, z, Block::new("stone", 0)));
        }
        blocks.push((0, 1, 1, Block::new("copper_door", 0)));
        let shut =
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, blocks).unwrap()]).unwrap();
        let stopped = walk_north(&shut, 90);
        let face = 1.0 + 3.0 / 16.0;
        let min_z = stopped.eye.z - HALF_WIDTH;
        assert!(
            min_z >= face - 1.0e-3,
            "walked through the copper door, min z {min_z}"
        );
        let mut open_blocks = Vec::new();
        for z in 0..5u8 {
            open_blocks.push((0, 0, z, Block::new("stone", 0)));
        }
        open_blocks.push((0, 1, 1, Block::new("copper_door", 1)));
        let swung =
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, open_blocks).unwrap()])
                .unwrap();
        let passed = walk_north(&swung, 90);
        assert!(
            passed.eye.z - HALF_WIDTH < 0.5,
            "an open copper door should let the walk through"
        );
    }

    #[test]
    fn a_closed_trapdoor_holds_the_walk_and_an_open_one_drops() {
        let closed = shape_of(Block::new("trapdoor", 0));
        let open = shape_of(Block::new("trapdoor", 1));
        assert_eq!(closed.boxes()[0].min[1], 13);
        assert_eq!(closed.boxes()[0].max[1], 16);
        assert_eq!(open.boxes()[0].max[0], 3);
        assert!(!open.occludes(8, 8, 8));
        let held = approach_hatch(&trapdoor_floor("trapdoor", 0));
        assert!(
            feet_y(held.eye.y) > 0.9,
            "a closed trapdoor should cover the hole, feet {}",
            feet_y(held.eye.y)
        );
        assert!(
            held.eye.z < 1.0,
            "the walk should reach the hatch, eye z {}",
            held.eye.z
        );
        let dropped = approach_hatch(&trapdoor_floor("trapdoor", 1));
        assert!(
            feet_y(dropped.eye.y) < 0.0,
            "an open trapdoor should drop the walk, feet {}",
            feet_y(dropped.eye.y)
        );
    }

    #[test]
    fn a_closed_copper_trapdoor_holds_the_walk_and_an_open_one_drops() {
        let closed = shape_of(Block::new("copper_trapdoor", 0));
        let open = shape_of(Block::new("copper_trapdoor", 1));
        assert_eq!(closed.boxes()[0].min[1], 13);
        assert_eq!(open.boxes()[0].max[0], 3);
        let held = approach_hatch(&trapdoor_floor("copper_trapdoor", 0));
        assert!(
            feet_y(held.eye.y) > 0.9,
            "a closed copper trapdoor should cover the hole, feet {}",
            feet_y(held.eye.y)
        );
        let dropped = approach_hatch(&trapdoor_floor("copper_trapdoor", 1));
        assert!(
            feet_y(dropped.eye.y) < 0.0,
            "an open copper trapdoor should drop the walk, feet {}",
            feet_y(dropped.eye.y)
        );
    }

    #[test]
    fn glass_is_open_in_the_middle_and_stops_the_walk() {
        let shape = shape_of(Block::new("glass", 0));
        assert!(!shape.is_full_cube());
        assert!(!shape.occludes(8, 8, 8), "the middle is open");
        assert!(!shape.occludes(8, 8, 0), "the face center is open");
        assert!(shape.occludes(0, 0, 0), "the rim is there");
        let mut blocks = Vec::new();
        for z in 0..5u8 {
            blocks.push((0, 0, z, Block::new("stone", 0)));
        }
        blocks.push((0, 1, 1, Block::new("glass", 0)));
        blocks.push((0, 2, 1, Block::new("glass", 0)));
        let world =
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, blocks).unwrap()]).unwrap();
        let stopped = walk_north(&world, 40);
        let face = 2.0;
        let min_z = stopped.eye.z - HALF_WIDTH;
        assert!(
            min_z >= face - 1.0e-3,
            "walked through the glass, min z {min_z}"
        );
        assert!(
            min_z < face + 0.08,
            "stopped short of the glass, min z {min_z}"
        );
    }

    #[test]
    fn a_fence_stops_the_walk_and_leaves_the_corners_open() {
        let shape = shape_of(Block::new("fence", 0));
        assert!(!shape.is_full_cube());
        assert!(shape.occludes(8, 8, 8), "the cross is solid");
        assert!(
            !shape.occludes(1, 8, 1),
            "a corner stays open to see through"
        );
        assert!(!shape.covers_cell_face(2, false));
        let mut blocks = Vec::new();
        for z in 0..5u8 {
            blocks.push((0, 0, z, Block::new("stone", 0)));
        }
        blocks.push((0, 1, 1, Block::new("fence", 0)));
        let world =
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, blocks).unwrap()]).unwrap();
        let stopped = walk_north(&world, 90);
        let face = 2.0;
        let min_z = stopped.eye.z - HALF_WIDTH;
        assert!(
            min_z >= face - 1.0e-3,
            "walked through the fence, min z {min_z}"
        );
        assert!(
            min_z < face + 0.08,
            "stopped short of the fence, min z {min_z}"
        );
    }

    #[test]
    fn a_closed_fence_gate_stops_the_walk_and_an_open_one_does_not() {
        let closed = shape_of(Block::new("fence_gate", 0));
        let open = shape_of(Block::new("fence_gate", 1));
        assert_eq!(closed.boxes()[0].min[2], 6);
        assert_eq!(closed.boxes()[0].max[2], 10);
        assert_eq!(open.boxes()[0].max[0], 3);
        assert!(
            !closed.occludes(1, 8, 1),
            "a corner stays open like the fence"
        );
        let mut blocks = Vec::new();
        for z in 0..5u8 {
            blocks.push((0, 0, z, Block::new("stone", 0)));
        }
        blocks.push((0, 1, 1, Block::new("fence_gate", 0)));
        let shut =
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, blocks).unwrap()]).unwrap();
        let stopped = walk_north(&shut, 90);
        let face = 1.0 + 10.0 / 16.0;
        let min_z = stopped.eye.z - HALF_WIDTH;
        assert!(
            min_z >= face - 1.0e-3,
            "walked through the gate, min z {min_z}"
        );
        assert!(
            min_z < face + 0.08,
            "stopped short of the gate, min z {min_z}"
        );

        let mut open_blocks = Vec::new();
        for z in 0..5u8 {
            open_blocks.push((0, 0, z, Block::new("stone", 0)));
        }
        open_blocks.push((0, 1, 1, Block::new("fence_gate", 1)));
        let swung =
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, open_blocks).unwrap()])
                .unwrap();
        let passed = walk_north(&swung, 90);
        let passed_z = passed.eye.z - HALF_WIDTH;
        assert!(
            passed_z < 0.5,
            "an open gate should let the walk through, min z {passed_z}"
        );
    }

    #[test]
    fn a_ladder_holds_you_and_a_forward_step_climbs() {
        let shape = shape_of(Block::new("ladder", 0));
        assert!(!shape.is_full_cube());
        assert_eq!(shape.boxes()[0].max[2], 2);
        assert!(shape.occludes(8, 8, 0));
        assert!(!shape.occludes(8, 8, 8));
        let mut blocks = Vec::new();
        for z in 0..3u8 {
            blocks.push((0, 0, z, Block::new("stone", 0)));
        }
        for y in 1..6u16 {
            blocks.push((0, y, 1, Block::new("ladder", 0)));
        }
        let world =
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, blocks).unwrap()]).unwrap();
        let start = Vec3::new(0.5, 1.0 + EYE_HEIGHT, 1.5);
        let mut camera = Camera::looking_at(start, Vec3::new(0.5, 1.4, 0.0));
        let hold = idle();
        for _ in 0..30 {
            step(&mut camera, 1.0 / 60.0, &hold, &world);
        }
        assert!(
            (camera.eye.y - start.y).abs() < 0.05,
            "the ladder should hold you, eye {}",
            camera.eye.y
        );
        let mut climb = idle();
        climb.forward = true;
        let before = camera.eye.y;
        for _ in 0..30 {
            step(&mut camera, 1.0 / 60.0, &climb, &world);
        }
        let risen = camera.eye.y - before;
        assert!(
            (risen - CLIMB_SPEED * 0.5).abs() < 0.15,
            "forward should climb, rose {risen}"
        );
        let high = camera.eye;
        camera = Camera::looking_at(high, high + Vec3::new(0.0, 0.0, 5.0));
        let mut down = idle();
        down.back = true;
        for _ in 0..15 {
            step(&mut camera, 1.0 / 60.0, &down, &world);
        }
        let dropped = high.y - camera.eye.y;
        assert!(
            (dropped - CLIMB_SPEED * 0.25).abs() < 0.15,
            "back should climb down, dropped {dropped}"
        );
    }

    #[test]
    fn a_vine_holds_you_and_a_forward_step_climbs() {
        let shape = shape_of(Block::new("vine", 3));
        assert!(!shape.is_full_cube());
        assert_eq!(shape.boxes()[0].max[2], 2);
        let mut blocks = Vec::new();
        for z in 0..3u8 {
            blocks.push((0, 0, z, Block::new("stone", 0)));
        }
        for y in 1..6u16 {
            blocks.push((0, y, 0, Block::new("stone", 0)));
            blocks.push((0, y, 1, Block::new("vine", 3)));
        }
        let world =
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, blocks).unwrap()]).unwrap();
        let start = Vec3::new(0.5, 1.0 + EYE_HEIGHT, 1.5);
        let mut camera = Camera::looking_at(start, Vec3::new(0.5, 1.4, 0.0));
        let hold = idle();
        for _ in 0..30 {
            step(&mut camera, 1.0 / 60.0, &hold, &world);
        }
        assert!(
            (camera.eye.y - start.y).abs() < 0.05,
            "the vine should hold you, eye {}",
            camera.eye.y
        );
        let mut climb = idle();
        climb.forward = true;
        let before = camera.eye.y;
        for _ in 0..30 {
            step(&mut camera, 1.0 / 60.0, &climb, &world);
        }
        let risen = camera.eye.y - before;
        assert!(
            (risen - CLIMB_SPEED * 0.5).abs() < 0.15,
            "forward should climb the vine, rose {risen}"
        );
    }

    #[test]
    fn sign_plane_blocks_the_player_beside_the_post() {
        let world = sign_on_a_floor();
        let shape = shape_of(Block::new("sign", 0));
        let post = shape.boxes()[0];
        let plane = shape.planes()[0];
        assert_eq!(plane.axis, 2);
        // Past the post on X, still on the board.
        let eye_x = f32::from(post.max[0]) / 16.0 + HALF_WIDTH + 0.05;
        let face = (f32::from(plane.at) + 0.5).min(16.0) / 16.0;
        let eye = Vec3::new(eye_x, 1.0 + EYE_HEIGHT, 2.5);
        let mut camera = Camera::looking_at(eye, Vec3::new(eye_x, 1.4, 0.0));
        let mut walk = idle();
        walk.forward = true;
        for _ in 0..180 {
            step(&mut camera, 1.0 / 60.0, &walk, &world);
        }
        let min_z = camera.eye.z - HALF_WIDTH;
        let min_x = camera.eye.x - HALF_WIDTH;
        assert!(
            min_x >= f32::from(post.max[0]) / 16.0,
            "drifted into the post, min x {min_x}"
        );
        assert!(
            min_z >= face - 1.0e-3,
            "walked through the board, min z {min_z}"
        );
        assert!(
            min_z < face + 0.05,
            "stopped short of the board, min z {min_z}"
        );
    }

    #[test]
    fn the_player_stops_at_the_edge_of_the_loaded_world() {
        let world = rebuild_world::synthetic_cluster();
        assert_eq!(world.size(), rebuild_world::WorldSize::Small);
        let (min_x, max_x, min_z, max_z) = world.bounds();
        let radius = rebuild_world::WorldSize::Small.chunk_radius();
        assert_eq!(
            (min_x, max_x, min_z, max_z),
            (
                -radius * 16,
                (radius + 1) * 16,
                -radius * 16,
                (radius + 1) * 16
            )
        );
        let mut walk = idle();
        walk.forward = true;

        let east = Vec3::new(max_x as f32 - 1.5, 1.0 + EYE_HEIGHT, 8.5);
        let mut camera = Camera::looking_at(east, east + Vec3::X);
        for _ in 0..240 {
            step(&mut camera, 1.0 / 60.0, &walk, &world);
        }
        let body_max_x = camera.eye.x + HALF_WIDTH;
        assert!(
            body_max_x <= max_x as f32,
            "walked past the east edge, x {body_max_x}"
        );
        assert!(
            body_max_x > max_x as f32 - 0.05,
            "stopped short of the east edge, x {body_max_x}"
        );

        let north = Vec3::new(8.5, 1.0 + EYE_HEIGHT, min_z as f32 + 1.5);
        let mut camera = Camera::looking_at(north, north - Vec3::Z);
        for _ in 0..240 {
            step(&mut camera, 1.0 / 60.0, &walk, &world);
        }
        let body_min_z = camera.eye.z - HALF_WIDTH;
        assert!(
            body_min_z >= min_z as f32,
            "walked past the north edge, z {body_min_z}"
        );
        assert!(
            body_min_z < min_z as f32 + 0.05,
            "stopped short of the north edge, z {body_min_z}"
        );
    }

    #[test]
    fn a_solid_box_stops_the_walk() {
        let mut blocks = Vec::new();
        for x in 0..8u8 {
            blocks.push((x, 0, 0, Block::new("stone", 0)));
        }
        let world =
            Cluster::try_from_chunks(0, 0, [Chunk::from_blocks(0, 0, blocks).unwrap()]).unwrap();
        let sheep = ([2.2, 1.0, -0.3], [3.2, 2.2, 0.3]);
        let eye = Vec3::new(1.2, 1.0 + EYE_HEIGHT, 0.0);
        let mut camera = Camera::looking_at(eye, eye + Vec3::X);
        let mut walk = idle();
        walk.forward = true;
        for _ in 0..90 {
            step_past(&mut camera, 1.0 / 60.0, &walk, &world, &[sheep]);
        }
        let body_max = camera.eye.x + HALF_WIDTH;
        assert!(
            body_max <= sheep.0[0] + 0.02,
            "walked through the sheep, body max x {body_max}"
        );
        let blocked = camera.eye.x;
        for _ in 0..30 {
            step(&mut camera, 1.0 / 60.0, &walk, &world);
        }
        assert!(camera.eye.x > blocked + 1.0);
    }
}
