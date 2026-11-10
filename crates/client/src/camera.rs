//! Walking camera. Y is up. Yaw 0 looks toward +X; yaw -π/2 looks toward -Z.
//!
//! WASD moves on the horizontal plane. Space jumps, then gravity brings the
//! player back down onto the top of a solid box. Shift crouches: the view
//! drops, the walk slows, the hitbox shortens, and solid edges cling.

use std::f32::consts::FRAC_PI_2;

use glam::camera::rh::proj::directx;
use glam::camera::rh::view;
use glam::{Mat4, Vec3};
use rebuild_world::Cluster;

use crate::body;

/// Oak stairs, the point a held compass aims at.
pub const STAIRS_XZ: [f32; 2] = [8.5, 12.5];

/// Pixel offset of the compass mark. `+x` is to the right, `+y` is forward.
///
/// Standing on the stairs puts the mark in the middle of the dial.
pub fn compass_mark(eye_x: f32, eye_z: f32, yaw: f32) -> [f32; 2] {
    compass_mark_at(eye_x, eye_z, yaw, STAIRS_XZ)
}

/// Pixel offset of the compass mark toward `target` instead of the stairs.
pub fn compass_mark_at(eye_x: f32, eye_z: f32, yaw: f32, target: [f32; 2]) -> [f32; 2] {
    const REACH: f32 = 12.0;
    let dx = target[0] - eye_x;
    let dz = target[1] - eye_z;
    let dist = (dx * dx + dz * dz).sqrt();
    if dist < 1.25 {
        return [0.0, 0.0];
    }
    let (sy, cy) = yaw.sin_cos();
    let forward = dx * cy + dz * sy;
    let right = -dx * sy + dz * cy;
    let scale = REACH / dist;
    [right * scale, forward * scale]
}

/// Where a thrown eye of ender flies, and whether that throw breaks it.
///
/// It travels up to 12 blocks toward the stairs. One cell in five breaks.
pub struct EyeThrow {
    pub landing: [f32; 2],
    pub breaks: bool,
}

/// Fly toward the stairs from `(x, z)`. The same spot always breaks the same way.
pub fn eye_throw(x: f32, z: f32) -> EyeThrow {
    let dx = STAIRS_XZ[0] - x;
    let dz = STAIRS_XZ[1] - z;
    let dist = (dx * dx + dz * dz).sqrt();
    let travel = 12.0_f32.min(dist);
    let landing = if dist < 0.05 {
        [x, z]
    } else {
        [x + dx / dist * travel, z + dz / dist * travel]
    };
    let cell = x.floor() as i32;
    let row = z.floor() as i32;
    let breaks = (cell.wrapping_mul(31) ^ row.wrapping_mul(17)).unsigned_abs() % 5 == 0;
    EyeThrow { landing, breaks }
}

#[derive(Clone, Copy)]
pub struct MoveInput {
    pub forward: bool,
    pub back: bool,
    pub left: bool,
    pub right: bool,
    pub jump: bool,
    pub fast: bool,
    /// Shift. The view drops, the walk slows, and the hitbox shortens.
    pub crouch: bool,
}

pub struct Camera {
    pub eye: Vec3,
    pub velocity_y: f32,
    /// Horizontal leftover from walking on ice. Other ground clears it.
    pub slide: Vec3,
    pub on_ground: bool,
    yaw: f32,
    pitch: f32,
}

impl Camera {
    /// In the air above the grass, just south of the oak stairs, looking at them.
    ///
    /// Feet start at y = 3.5. The grass cubes top out at y = 1, and the stairs
    /// occupy block (8, 1, 12). Walking forward from the landing spot steps
    /// up those stairs.
    pub fn drop_by_the_stairs() -> Self {
        let feet_y = 3.5;
        let eye = Vec3::new(8.5, feet_y + body::EYE_HEIGHT, 14.4);
        let target = Vec3::new(8.5, 1.4, 12.4);
        Self::looking_at(eye, target)
    }

    /// Eye height standing on the block at `at`, looking north.
    pub fn anchor_eye(at: [i32; 3]) -> Self {
        let eye = Vec3::new(
            at[0] as f32 + 0.5,
            at[1] as f32 + 1.0 + body::EYE_HEIGHT,
            at[2] as f32 + 0.5,
        );
        Self::looking_at(eye, eye + Vec3::new(0.0, 0.0, -1.0))
    }

    pub fn looking_at(eye: Vec3, target: Vec3) -> Self {
        let dir = (target - eye).normalize();
        Self {
            eye,
            velocity_y: 0.0,
            slide: Vec3::ZERO,
            on_ground: false,
            yaw: dir.z.atan2(dir.x),
            pitch: dir.y.asin(),
        }
    }

    pub fn yaw_pitch(&self) -> (f32, f32) {
        (self.yaw, self.pitch)
    }

    /// Put the eye and the look back where a save left them.
    pub fn restore(&mut self, eye: Vec3, yaw: f32, pitch: f32) {
        let limit = FRAC_PI_2 - 0.01;
        self.eye = eye;
        self.yaw = yaw;
        self.pitch = pitch.clamp(-limit, limit);
        self.velocity_y = 0.0;
        self.slide = Vec3::ZERO;
        self.on_ground = false;
    }

    pub fn forward(&self) -> Vec3 {
        let (sp, cp) = self.pitch.sin_cos();
        let (sy, cy) = self.yaw.sin_cos();
        Vec3::new(cy * cp, sp, sy * cp)
    }

    /// Horizontal axes used by WASD, independent of pitch.
    pub fn walk_axes(&self) -> (Vec3, Vec3) {
        let (sy, cy) = self.yaw.sin_cos();
        let forward = Vec3::new(cy, 0.0, sy);
        let right = Vec3::new(-sy, 0.0, cy);
        (forward, right)
    }

    pub fn apply_mouse(&mut self, dx: f64, dy: f64) {
        const SENSITIVITY: f32 = 0.0022;
        self.yaw += dx as f32 * SENSITIVITY;
        self.pitch -= dy as f32 * SENSITIVITY;
        let limit = FRAC_PI_2 - 0.01;
        self.pitch = self.pitch.clamp(-limit, limit);
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn step(&mut self, dt: f32, input: &MoveInput, world: &Cluster) {
        self.step_past(dt, input, world, &[]);
    }

    pub fn step_past(
        &mut self,
        dt: f32,
        input: &MoveInput,
        world: &Cluster,
        obstacles: &[([f32; 3], [f32; 3])],
    ) {
        self.step_paced(dt, input, world, obstacles, 1.0);
    }

    pub fn step_paced(
        &mut self,
        dt: f32,
        input: &MoveInput,
        world: &Cluster,
        obstacles: &[([f32; 3], [f32; 3])],
        pace: f32,
    ) {
        body::step_paced(self, dt, input, world, obstacles, pace);
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn view_proj(&self, aspect: f32) -> Mat4 {
        self.view_proj_fov(aspect, LOOK_FOV)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn view_proj_fov(&self, aspect: f32, fov_degrees: f32) -> Mat4 {
        self.view_from(aspect, fov_degrees, self.eye)
    }

    /// The view from `eye`, still looking the way this camera looks.
    pub fn view_from(&self, aspect: f32, fov_degrees: f32, eye: Vec3) -> Mat4 {
        let proj = directx::perspective(fov_degrees.to_radians(), aspect.max(0.01), 0.1, 800.0);
        let view_matrix = view::look_to_mat4(eye, self.forward(), Vec3::Y);
        proj * view_matrix
    }

    /// A view eye offset by a walk bob. The stored eye does not move.
    pub fn bobbed_eye(&self, side: f32, rise: f32) -> Vec3 {
        let (_, right) = self.walk_axes();
        self.eye + right * side + Vec3::Y * rise
    }
}

/// How much the walk moves the view. Full is the ordinary bob.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bob {
    Off,
    Low,
    Full,
}

impl Bob {
    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Low => "low",
            Self::Full => "full",
        }
    }

    /// Full steps down to low, then off, then back to full.
    pub fn cycle(self) -> Self {
        match self {
            Self::Full => Self::Low,
            Self::Low => Self::Off,
            Self::Off => Self::Full,
        }
    }

    /// Scale for [`head_bob`]. Off holds the view still.
    pub fn scale(self) -> f32 {
        match self {
            Self::Off => 0.0,
            Self::Low => 0.4,
            Self::Full => 1.0,
        }
    }
}

/// Sideways and vertical view offset while walking, in blocks.
pub fn head_bob(phase: f32, amount: f32) -> (f32, f32) {
    let amount = amount.clamp(0.0, 1.0);
    let side = (phase * 0.5).sin() * 0.02 * amount;
    let rise = phase.sin() * 0.025 * amount;
    (side, rise)
}

/// How far the view dips while eating or drinking, in blocks.
pub fn nibble(amount: f32) -> f32 {
    amount.clamp(0.0, 1.0) * 0.06
}

/// Extra vertical chew bob while food is still in the mouth.
pub fn chew_rise(amount: f32, phase: f32) -> f32 {
    let amount = amount.clamp(0.0, 1.0);
    if amount <= 0.02 {
        return 0.0;
    }
    (phase * 12.0).sin() * 0.018 * amount
}

/// Degrees of FOV chew while eating. Negative tightens; oscillates with `phase`.
pub const CHEW_FOV: f32 = 3.0;

/// FOV punch while chewing. `sip` is the taste fade (`1` just bit → `0`).
pub fn chew_fov(sip: f32, phase: f32) -> f32 {
    let sip = sip.clamp(0.0, 1.0);
    if sip <= 0.02 {
        return 0.0;
    }
    -sip * 1.5 + (phase * 10.0).sin() * CHEW_FOV * sip
}

/// How far the view drops while crouching, in blocks. `1` is fully down.
///
/// Matches standing eye (`1.62`) down to sneak eye (`1.27`).
pub fn crouch_drop(amount: f32) -> f32 {
    amount.clamp(0.0, 1.0) * (body::EYE_HEIGHT - body::CROUCH_EYE)
}

/// Degrees the view widens at a full sprint (before view-bob scale).
pub const SPRINT_FOV: f32 = 10.0;

/// Degrees the view widens while the eye is underwater (lens feel).
pub const WATER_FOV: f32 = 8.0;

/// Degrees the view widens while rowing a boat (before view-bob scale).
pub const BOAT_FOV: f32 = 5.0;

/// Degrees the view widens at jump takeoff (before view-bob scale).
pub const JUMP_FOV: f32 = 4.0;

/// Sprint / water / boat widen the view. A hit or landing tightens it. Spyglass stays narrow.
///
/// `sprint`, `hit`, and `boat` are already scaled by the view-bob setting (off → no punch).
/// `water` is an environmental amount (`0`..`1`) and is not bob-scaled.
pub fn punched_fov(base: f32, sprint: f32, hit: f32, water: f32, boat: f32) -> f32 {
    if base < LOOK_FOV - 1.0 {
        return base;
    }
    let sprint = sprint.clamp(0.0, 1.0);
    let hit = hit.clamp(0.0, 1.0);
    let water = water.clamp(0.0, 1.0);
    let boat = boat.clamp(0.0, 1.0);
    base + sprint * SPRINT_FOV + water * WATER_FOV + boat * BOAT_FOV - hit * 4.0
}

/// The ordinary view, in degrees.
pub const LOOK_FOV: f32 = 60.0;

/// The spyglass view, in degrees. Narrower than [`LOOK_FOV`].
pub const SPYGLASS_FOV: f32 = 20.0;

/// `20` while a spyglass button is held, otherwise `60`.
pub fn look_fov(zooming: bool) -> f32 {
    if zooming { SPYGLASS_FOV } else { LOOK_FOV }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rebuild_world::{Block, Chunk};

    #[test]
    fn a_spyglass_narrows_the_view_until_it_is_released() {
        assert_eq!(look_fov(false), LOOK_FOV);
        assert_eq!(look_fov(true), SPYGLASS_FOV);
        assert!(SPYGLASS_FOV < LOOK_FOV);
    }

    #[test]
    fn a_walk_bobs_the_view_and_a_sprint_or_hit_nudges_the_fov() {
        let camera = Camera::looking_at(Vec3::new(0.0, 2.0, 0.0), Vec3::new(0.0, 2.0, -10.0));
        let (side, rise) = head_bob(std::f32::consts::FRAC_PI_2, 1.0);
        assert!(rise > 0.02 && rise < 0.03);
        assert!(side.abs() < 0.02);
        let bobbed = camera.bobbed_eye(side, rise);
        assert!((camera.eye - Vec3::new(0.0, 2.0, 0.0)).length() < 1.0e-5);
        assert!((bobbed - camera.eye).length() < 0.05);
        assert!((bobbed - camera.eye).length() > 0.01);
        let _ = camera.view_proj(1.0);
        assert_eq!(nibble(0.0), 0.0);
        assert_eq!(nibble(1.0), 0.06);
        assert_eq!(nibble(2.0), 0.06);
        assert_eq!(chew_fov(0.0, 1.0), 0.0);
        assert!(chew_fov(1.0, 0.0) < 0.0);
        assert!(
            (chew_rise(1.0, std::f32::consts::FRAC_PI_2 / 12.0) - 0.018).abs() < 1.0e-5
        );
        assert_eq!(
            punched_fov(LOOK_FOV, 1.0, 0.0, 0.0, 0.0),
            LOOK_FOV + SPRINT_FOV
        );
        assert_eq!(punched_fov(LOOK_FOV, 0.0, 1.0, 0.0, 0.0), LOOK_FOV - 4.0);
        assert_eq!(punched_fov(SPYGLASS_FOV, 1.0, 1.0, 1.0, 1.0), SPYGLASS_FOV);
        assert_eq!(
            punched_fov(LOOK_FOV, Bob::Low.scale(), 0.0, 0.0, 0.0),
            LOOK_FOV + SPRINT_FOV * 0.4
        );
        assert_eq!(
            punched_fov(LOOK_FOV, Bob::Off.scale(), 0.0, 0.0, 0.0),
            LOOK_FOV
        );
        assert_eq!(
            punched_fov(LOOK_FOV, 0.0, 0.0, 1.0, 0.0),
            LOOK_FOV + WATER_FOV
        );
        assert_eq!(
            punched_fov(LOOK_FOV, 0.0, 0.0, 0.0, 1.0),
            LOOK_FOV + BOAT_FOV
        );
        assert_eq!(JUMP_FOV, 4.0);
        assert_eq!(head_bob(0.4, 0.0), (0.0, 0.0));
        let (full_side, full_rise) = head_bob(1.2, Bob::Full.scale());
        let (low_side, low_rise) = head_bob(1.2, Bob::Low.scale());
        assert!(full_rise.abs() > low_rise.abs() && low_rise.abs() > 0.0);
        assert!((low_rise - full_rise * 0.4).abs() < 1.0e-5);
        assert!((low_side - full_side * 0.4).abs() < 1.0e-5);
        assert_eq!(head_bob(1.2, Bob::Off.scale()), (0.0, 0.0));
        assert_eq!(Bob::Full.cycle(), Bob::Low);
        assert_eq!(Bob::Low.cycle(), Bob::Off);
        assert_eq!(Bob::Off.cycle(), Bob::Full);
        assert_eq!(Bob::Low.label(), "low");
    }

    fn approx(a: Vec3, b: Vec3) {
        assert!((a - b).length() < 1e-4, "{a} != {b}");
    }

    #[test]
    fn initial_look_toward_negative_z() {
        let camera = Camera::looking_at(Vec3::ZERO, Vec3::new(0.0, 0.0, -10.0));
        approx(camera.forward(), Vec3::new(0.0, 0.0, -1.0));
        let (forward, right) = camera.walk_axes();
        approx(forward, Vec3::new(0.0, 0.0, -1.0));
        approx(right, Vec3::new(1.0, 0.0, 0.0));
    }

    #[test]
    fn player_starts_above_the_grass_by_the_stairs() {
        let camera = Camera::drop_by_the_stairs();
        approx(camera.eye, Vec3::new(8.5, 3.5 + body::EYE_HEIGHT, 14.4));
        assert!(camera.eye.z > 13.0, "south of the stair cell");
        assert!(camera.forward().z < 0.0);
        assert!(!camera.on_ground);
        let (forward, _) = camera.walk_axes();
        assert!(forward.z < 0.0);
        let (yaw, _) = camera.yaw_pitch();
        let [x, y] = compass_mark(camera.eye.x, camera.eye.z, yaw);
        assert!(y > 8.0, "the mark sits forward, at {y}");
        assert!(x.abs() < 1.0, "the stairs are ahead, not aside, at {x}");
        let [away_x, away_y] = compass_mark(camera.eye.x, camera.eye.z, yaw + std::f32::consts::PI);
        assert!(
            away_y < -8.0,
            "turning around puts the mark behind, at {away_y}"
        );
        assert!(away_x.abs() < 1.0, "{away_x}");
        assert_eq!(compass_mark(STAIRS_XZ[0], STAIRS_XZ[1], yaw), [0.0, 0.0]);
    }

    #[test]
    fn a_linked_lodestone_aims_the_dial_at_that_block() {
        let target = [20.5, 8.5];
        let [x, y] = compass_mark_at(8.5, 8.5, 0.0, target);
        assert!(y > 8.0, "the mark sits forward, at {y}");
        assert!(x.abs() < 1.0, "the lodestone is ahead, at {x}");
        assert_eq!(
            compass_mark_at(target[0], target[1], 0.0, target),
            [0.0, 0.0]
        );
        assert_eq!(compass_mark(STAIRS_XZ[0], STAIRS_XZ[1], 0.0), [0.0, 0.0]);
        assert_eq!(crouch_drop(0.0), 0.0);
        let full = body::EYE_HEIGHT - body::CROUCH_EYE;
        assert!((crouch_drop(1.0) - full).abs() < 1e-4);
        assert!((crouch_drop(0.5) - full * 0.5).abs() < 1e-4);
    }

    #[test]
    fn a_charged_anchor_stands_you_on_top_of_it() {
        let camera = Camera::anchor_eye([4, 2, 6]);
        approx(camera.eye, Vec3::new(4.5, 3.0 + body::EYE_HEIGHT, 6.5));
        assert!(camera.forward().z < 0.0);
    }

    #[test]
    fn an_eye_of_ender_flies_toward_the_stairs_and_sometimes_breaks() {
        let flight = eye_throw(0.5, 0.5);
        assert!(flight.breaks);
        let start = (0.5 - STAIRS_XZ[0]).hypot(0.5 - STAIRS_XZ[1]);
        let end = (flight.landing[0] - STAIRS_XZ[0]).hypot(flight.landing[1] - STAIRS_XZ[1]);
        assert!(
            (start - end - 12.0).abs() < 0.05,
            "it should close 12 blocks, from {start} to {end}"
        );
        let kept = eye_throw(1.5, 0.5);
        assert!(!kept.breaks);
        let kept_end = (kept.landing[0] - STAIRS_XZ[0]).hypot(kept.landing[1] - STAIRS_XZ[1]);
        let kept_start = (1.5 - STAIRS_XZ[0]).hypot(0.5 - STAIRS_XZ[1]);
        assert!(kept_end < kept_start, "the drop still aims at the stairs");
        assert_eq!(eye_throw(0.5, 0.5).breaks, flight.breaks);
    }

    #[test]
    fn wasd_moves_on_the_horizontal_plane() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 0, 0, Block::new("stone", 0))]).unwrap();
        let world = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let start = Vec3::new(0.5, 1.0 + body::EYE_HEIGHT, 0.5);
        let mut camera = Camera::looking_at(start, Vec3::new(0.5, 1.0, -10.0));
        let dt = 1.0 / 60.0;
        camera.step(
            dt,
            &MoveInput {
                forward: true,
                back: false,
                left: false,
                right: true,
                jump: false,
                fast: false,
                crouch: false,
            },
            &world,
        );
        let expected = Vec3::new(1.0, 0.0, -1.0).normalize() * 6.0 * dt;
        let moved = camera.eye - start;
        assert!((moved.x - expected.x).abs() < 1e-4, "x {}", moved.x);
        assert!((moved.z - expected.z).abs() < 1e-4, "z {}", moved.z);
        assert!((body::feet_y(camera.eye.y) - 1.0).abs() < 0.02);
    }
}
