//! Powder snow: sink and freeze without leather boots; walk on top with them.
//!
//! Empty buckets scoop a cell into [`POWDER_SNOW_BUCKET`]; the filled bucket
//! pours [`POWDER_SNOW`] back. Leather boots make nearby powder snow solid underfoot.

use glam::Vec3;
use rebuild_world::Cluster;

use crate::body::{EYE_HEIGHT, HALF_WIDTH, HEIGHT};

pub const POWDER_SNOW: &str = "powder_snow";
pub const POWDER_SNOW_BUCKET: &str = "powder_snow_bucket";
pub const LEATHER_BOOTS: &str = "leather_boots";

/// Soft seconds between freeze ticks while sunk in powder snow.
pub const FREEZE_PERIOD: f32 = 0.8;

pub fn is_powder_snow(id: &str) -> bool {
    id == POWDER_SNOW
}

pub fn is_leather_boots(id: &str) -> bool {
    id == LEATHER_BOOTS
}

pub fn tint(id: &str) -> [f32; 3] {
    match id {
        POWDER_SNOW => [0.92, 0.96, 1.0],
        POWDER_SNOW_BUCKET => [0.85, 0.92, 1.0],
        _ => [0.92, 0.96, 1.0],
    }
}

/// True when any part of the standing body overlaps powder snow.
pub fn inside(eye: Vec3, world: &Cluster) -> bool {
    let feet = eye.y - EYE_HEIGHT;
    let min = [eye.x - HALF_WIDTH, feet, eye.z - HALF_WIDTH];
    let max = [eye.x + HALF_WIDTH, feet + HEIGHT, eye.z + HALF_WIDTH];
    let x0 = min[0].floor() as i32;
    let x1 = (max[0] - 1.0e-4).floor() as i32;
    let y0 = min[1].floor() as i32;
    let y1 = (max[1] - 1.0e-4).floor() as i32;
    let z0 = min[2].floor() as i32;
    let z1 = (max[2] - 1.0e-4).floor() as i32;
    for y in y0..=y1 {
        if !(0..256).contains(&y) {
            continue;
        }
        for z in z0..=z1 {
            for x in x0..=x1 {
                if is_powder_snow(world.block_at(x, y, z).id) {
                    return true;
                }
            }
        }
    }
    false
}

/// Full-cube solids for powder snow near the player (leather boots walk-on-top).
pub fn push_solids(eye: Vec3, world: &Cluster, out: &mut Vec<([f32; 3], [f32; 3])>) {
    let feet = eye.y - EYE_HEIGHT;
    let x0 = (eye.x - 1.5).floor() as i32;
    let x1 = (eye.x + 1.5).floor() as i32;
    let y0 = (feet - 1.0).floor() as i32;
    let y1 = (feet + HEIGHT + 0.5).floor() as i32;
    let z0 = (eye.z - 1.5).floor() as i32;
    let z1 = (eye.z + 1.5).floor() as i32;
    for y in y0..=y1 {
        if !(0..256).contains(&y) {
            continue;
        }
        for z in z0..=z1 {
            for x in x0..=x1 {
                if !is_powder_snow(world.block_at(x, y, z).id) {
                    continue;
                }
                out.push((
                    [x as f32, y as f32, z as f32],
                    [x as f32 + 1.0, y as f32 + 1.0, z as f32 + 1.0],
                ));
            }
        }
    }
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
    fn detects_body_inside_powder_snow() {
        let cluster = world(vec![
            (1, 0, 1, Block::new("stone", 0)),
            (1, 1, 1, Block::new(POWDER_SNOW, 0)),
            (1, 2, 1, Block::new(POWDER_SNOW, 0)),
        ]);
        let eye = Vec3::new(1.5, 1.0 + EYE_HEIGHT, 1.5);
        assert!(inside(eye, &cluster));
        assert!(!inside(Vec3::new(4.5, 1.0 + EYE_HEIGHT, 4.5), &cluster));
    }

    #[test]
    fn leather_boots_collect_powder_solids() {
        let cluster = world(vec![(2, 1, 2, Block::new(POWDER_SNOW, 0))]);
        let eye = Vec3::new(2.5, 2.0 + EYE_HEIGHT, 2.5);
        let mut solids = Vec::new();
        push_solids(eye, &cluster, &mut solids);
        assert_eq!(solids.len(), 1);
        assert_eq!(solids[0].0, [2.0, 1.0, 2.0]);
        assert_eq!(solids[0].1, [3.0, 2.0, 3.0]);
    }
}
