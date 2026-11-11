//! Sculk catalyst: mob deaths nearby bloom sculk, veins, and sensors on stone.
//!
//! When a mob dies within [`CATALYST_RADIUS`] of a `sculk_catalyst`, nearby
//! stone-like cells become `sculk`, with `sculk_vein` and occasional
//! `sculk_sensor` cover.

use rebuild_world::{Block, Cluster};

/// How far a death looks for a catalyst.
pub const CATALYST_RADIUS: i32 = 8;

/// How far bloom spreads from the death cell.
pub const SPREAD_RADIUS: i32 = 3;

pub const SCULK: &str = "sculk";
pub const SCULK_VEIN: &str = "sculk_vein";
pub const SCULK_SENSOR: &str = "sculk_sensor";
pub const SCULK_CATALYST: &str = "sculk_catalyst";
pub const SCULK_SHRIEKER: &str = crate::warden::SCULK_SHRIEKER;

pub fn is_sculk(id: &str) -> bool {
    matches!(
        id,
        SCULK | SCULK_VEIN | SCULK_SENSOR | SCULK_CATALYST | SCULK_SHRIEKER
    )
}

pub fn is_catalyst(id: &str) -> bool {
    id == SCULK_CATALYST
}

pub fn tint(id: &str) -> [f32; 3] {
    match id {
        SCULK => [0.08, 0.22, 0.24],
        SCULK_VEIN => [0.12, 0.38, 0.40],
        SCULK_SENSOR => [0.18, 0.55, 0.58],
        SCULK_CATALYST => [0.15, 0.42, 0.40],
        SCULK_SHRIEKER => crate::warden::tint(SCULK_SHRIEKER),
        _ => [0.08, 0.22, 0.24],
    }
}

fn can_sculkify(id: &str) -> bool {
    matches!(
        id,
        "stone"
            | "cobblestone"
            | "deepslate"
            | "cobbled_deepslate"
            | "tuff"
            | "andesite"
            | "diorite"
            | "granite"
            | "dirt"
            | "gravel"
    )
}

fn hash3(x: i32, y: i32, z: i32) -> u32 {
    let mut n = x.wrapping_mul(374761393) as u32
        ^ (y.wrapping_mul(668265263) as u32)
        ^ (z.wrapping_mul(2147483647) as u32);
    n = (n ^ (n >> 13)).wrapping_mul(1274126177);
    n ^ (n >> 16)
}

/// Nearest catalyst within range of `death`, if any.
pub fn find_catalyst(world: &Cluster, death: [i32; 3], radius: i32) -> Option<[i32; 3]> {
    let mut best: Option<([i32; 3], i32)> = None;
    for dx in -radius..=radius {
        for dy in -radius..=radius {
            for dz in -radius..=radius {
                let at = [death[0] + dx, death[1] + dy, death[2] + dz];
                if !is_catalyst(world.block_at(at[0], at[1], at[2]).id) {
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

/// Bloom sculk around `death` when a catalyst is in range.
pub fn plan_death_spread(world: &Cluster, death: [i32; 3]) -> Vec<([i32; 3], Block)> {
    let Some(_catalyst) = find_catalyst(world, death, CATALYST_RADIUS) else {
        return Vec::new();
    };
    let r = SPREAD_RADIUS;
    let mut sculkify = Vec::new();
    let mut covers = Vec::new();
    for dx in -r..=r {
        for dy in -1..=1 {
            for dz in -r..=r {
                let at = [death[0] + dx, death[1] + dy, death[2] + dz];
                let block = world.block_at(at[0], at[1], at[2]);
                if is_catalyst(block.id) {
                    continue;
                }
                if can_sculkify(block.id) {
                    if hash3(at[0], at[1], at[2]) % 4 == 0 {
                        continue;
                    }
                    sculkify.push((at, Block::new(SCULK, 0)));
                }
            }
        }
    }
    let mut floors: Vec<[i32; 3]> = sculkify.iter().map(|(at, _)| *at).collect();
    // Also plant on existing sculk near the death.
    for dx in -r..=r {
        for dy in -1..=1 {
            for dz in -r..=r {
                let at = [death[0] + dx, death[1] + dy, death[2] + dz];
                if world.block_at(at[0], at[1], at[2]).id == SCULK {
                    floors.push(at);
                }
            }
        }
    }
    floors.sort_by_key(|a| (a[0], a[1], a[2]));
    floors.dedup();
    for at in floors {
        let above = [at[0], at[1] + 1, at[2]];
        if !world.block_at(above[0], above[1], above[2]).is_air() {
            continue;
        }
        // Skip if we already planned a sculk replace into this air cell.
        if sculkify.iter().any(|(p, _)| *p == above) {
            continue;
        }
        match hash3(at[0], at[1], at[2]) % 7 {
            0 | 1 | 2 => covers.push((above, Block::new(SCULK_VEIN, 0))),
            3 => covers.push((above, Block::new(SCULK_SENSOR, 0))),
            _ => {}
        }
    }
    sculkify.append(&mut covers);
    sculkify
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
    fn finds_nearby_catalyst() {
        let cluster = world(vec![
            (2, 2, 2, Block::new(SCULK_CATALYST, 0)),
            (4, 2, 2, Block::new("stone", 0)),
        ]);
        assert_eq!(find_catalyst(&cluster, [4, 2, 2], 8), Some([2, 2, 2]));
        assert!(find_catalyst(&cluster, [14, 2, 2], 4).is_none());
    }

    #[test]
    fn death_spreads_sculk_near_catalyst() {
        let mut blocks = vec![(3, 2, 3, Block::new(SCULK_CATALYST, 0))];
        for x in 1..=5u8 {
            for z in 1..=5u8 {
                blocks.push((x, 1, z, Block::new("stone", 0)));
            }
        }
        let cluster = world(blocks);
        let planned = plan_death_spread(&cluster, [3, 2, 3]);
        assert!(
            planned.iter().any(|(_, b)| b.id == SCULK),
            "expected sculk bloom, got {planned:?}"
        );
        assert!(
            planned
                .iter()
                .any(|(_, b)| b.id == SCULK_VEIN || b.id == SCULK_SENSOR),
            "expected vein or sensor cover"
        );
    }

    #[test]
    fn no_spread_without_catalyst() {
        let cluster = world(vec![(2, 1, 2, Block::new("stone", 0))]);
        assert!(plan_death_spread(&cluster, [2, 2, 2]).is_empty());
    }
}
