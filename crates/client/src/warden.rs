//! Sculk shrieker warnings summon a seeking warden with a darkness veil.
//!
//! Stepping on a `sculk_shrieker`, or vibrating a nearby `sculk_sensor` that
//! links to one, climbs a warning ladder then summons the warden. The warden
//! hunts the player / last vibration; killing it drops a catalyst or echo shard.

use rebuild_world::Cluster;

pub const SCULK_SHRIEKER: &str = "sculk_shrieker";
pub const ECHO_SHARD: &str = "echo_shard";

/// How far a sensor looks for a shrieker to wake.
pub const SENSOR_LINK: i32 = 8;
/// Warnings before a warden rises.
pub const WARN_MAX: u8 = 3;
/// Seconds of darkness after a shriek / while the warden hunts nearby.
pub const DARKNESS_SECS: f32 = 14.0;
/// How long a vibration lure stays hot for the warden.
pub const VIBE_SECS: f32 = 6.0;

pub fn is_shrieker(id: &str) -> bool {
    id == SCULK_SHRIEKER
}

pub fn tint(id: &str) -> [f32; 3] {
    match id {
        SCULK_SHRIEKER => [0.1, 0.36, 0.38],
        ECHO_SHARD => [0.25, 0.85, 0.9],
        "recovery_compass" => [0.2, 0.7, 0.78],
        _ => [0.1, 0.36, 0.38],
    }
}

/// Cell under the player's feet when it is a shrieker.
pub fn shrieker_under(world: &Cluster, eye: [f32; 3]) -> Option<[i32; 3]> {
    let x = eye[0].floor() as i32;
    let z = eye[2].floor() as i32;
    let feet = eye[1] - crate::body::EYE_HEIGHT;
    let sole = (feet - 0.02).floor() as i32;
    let at = [x, sole, z];
    if is_shrieker(world.block_at(at[0], at[1], at[2]).id) {
        Some(at)
    } else {
        None
    }
}

/// Cell under the feet when it is a sculk sensor.
pub fn sensor_under(world: &Cluster, eye: [f32; 3]) -> Option<[i32; 3]> {
    let x = eye[0].floor() as i32;
    let z = eye[2].floor() as i32;
    let feet = eye[1] - crate::body::EYE_HEIGHT;
    let sole = (feet - 0.02).floor() as i32;
    let at = [x, sole, z];
    if world.block_at(at[0], at[1], at[2]).id == crate::sculk::SCULK_SENSOR {
        Some(at)
    } else {
        None
    }
}

/// Nearest shrieker within [`SENSOR_LINK`] of `from`.
pub fn find_shrieker(world: &Cluster, from: [i32; 3], radius: i32) -> Option<[i32; 3]> {
    let mut best: Option<([i32; 3], i32)> = None;
    for dx in -radius..=radius {
        for dy in -radius..=radius {
            for dz in -radius..=radius {
                let at = [from[0] + dx, from[1] + dy, from[2] + dz];
                if !is_shrieker(world.block_at(at[0], at[1], at[2]).id) {
                    continue;
                }
                let dist = dx.abs() + dy.abs() + dz.abs();
                if best.is_none_or(|(_, d)| dist < d) {
                    best = Some((at, dist));
                }
            }
        }
    }
    best.map(|(at, _)| at)
}

/// Resolve the shrieker that should warn for the player's current footing.
pub fn warn_target(world: &Cluster, eye: [f32; 3]) -> Option<[i32; 3]> {
    if let Some(at) = shrieker_under(world, eye) {
        return Some(at);
    }
    if let Some(sensor) = sensor_under(world, eye) {
        return find_shrieker(world, sensor, SENSOR_LINK);
    }
    None
}

/// Advance warning count for a shrieker cell. Returns `(new_count, should_summon)`.
///
/// Once at [`WARN_MAX`], further steps do not re-summon until [`reset_warn`].
pub fn next_warn(prev: u8) -> (u8, bool) {
    if prev >= WARN_MAX {
        return (WARN_MAX, false);
    }
    let next = prev.saturating_add(1);
    (next, next >= WARN_MAX)
}

/// Clear a shrieker's warning ladder after the warden falls.
pub fn reset_warn() -> u8 {
    0
}

/// Feet spawn just above a shrieker cell.
pub fn summon_feet(at: [i32; 3]) -> [f32; 3] {
    [at[0] as f32 + 0.5, at[1] as f32 + 1.05, at[2] as f32 + 0.5]
}

/// Dark teal fog tint for the darkness veil.
pub fn darkness_fog() -> ([f32; 3], [f32; 3], f32, f32) {
    let sky = [0.02, 0.04, 0.05];
    let horizon = [0.03, 0.06, 0.07];
    (sky, horizon, 2.5, 11.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rebuild_world::{Block, Chunk, Cluster};

    fn world(blocks: Vec<(u8, u16, u8, Block)>) -> Cluster {
        let chunk = Chunk::from_blocks(0, 0, blocks).unwrap();
        Cluster::try_from_chunks(0, 0, [chunk]).unwrap()
    }

    #[test]
    fn sensor_links_to_nearby_shrieker() {
        let cluster = world(vec![
            (2, 2, 2, Block::new(crate::sculk::SCULK_SENSOR, 0)),
            (5, 2, 2, Block::new(SCULK_SHRIEKER, 0)),
        ]);
        assert_eq!(find_shrieker(&cluster, [2, 2, 2], SENSOR_LINK), Some([5, 2, 2]));
    }

    #[test]
    fn three_warns_summon() {
        assert_eq!(next_warn(0), (1, false));
        assert_eq!(next_warn(1), (2, false));
        assert_eq!(next_warn(2), (3, true));
        assert_eq!(next_warn(3), (3, false));
    }
}
