//! Bone meal on a moss block spreads lush cover nearby.
//!
//! Converts stone-like floors to `moss_block`, then plants `moss_carpet`,
//! `azalea`, `flower`, or top-converts to `grass` in a small radius.

use rebuild_world::{Block, Cluster};

/// How far moss bone meal reaches from the fertilized block.
pub const SPREAD_RADIUS: i32 = 2;

pub fn is_moss_block(id: &str) -> bool {
    id == "moss_block"
}

pub fn is_moss_cover(id: &str) -> bool {
    matches!(id, "moss_carpet" | "azalea")
}

pub fn tint(id: &str) -> [f32; 3] {
    match id {
        "moss_block" => [0.32, 0.58, 0.28],
        "moss_carpet" => [0.38, 0.66, 0.32],
        "azalea" => [0.42, 0.72, 0.28],
        _ => [0.32, 0.58, 0.28],
    }
}

fn can_mossify(id: &str) -> bool {
    matches!(
        id,
        "stone"
            | "cobblestone"
            | "dirt"
            | "grass"
            | "deepslate"
            | "cobbled_deepslate"
            | "gravel"
            | "andesite"
            | "diorite"
            | "granite"
            | "tuff"
    )
}

fn hash3(x: i32, y: i32, z: i32) -> u32 {
    let mut n = x.wrapping_mul(374761393) as u32
        ^ (y.wrapping_mul(668265263) as u32)
        ^ (z.wrapping_mul(2147483647) as u32);
    n = (n ^ (n >> 13)).wrapping_mul(1274126177);
    n ^ (n >> 16)
}

/// Vegetation / surface pick for air above moss.
fn cover_for(x: i32, y: i32, z: i32) -> Option<&'static str> {
    match hash3(x, y, z) % 10 {
        0 | 1 | 2 | 3 => Some("moss_carpet"),
        4 | 5 => Some("azalea"),
        6 | 7 => Some("flower"),
        8 => Some("grass"), // top-convert the moss under to grass instead of planting
        _ => None,
    }
}

/// Plan bone-meal spread centered on a moss block at `origin`.
pub fn plan_spread(world: &Cluster, origin: [i32; 3]) -> Vec<([i32; 3], Block)> {
    if !is_moss_block(world.block_at(origin[0], origin[1], origin[2]).id) {
        return Vec::new();
    }
    let r = SPREAD_RADIUS;
    let mut mossify = Vec::new();
    let mut covers = Vec::new();
    for dx in -r..=r {
        for dz in -r..=r {
            for dy in -1..=1 {
                let at = [origin[0] + dx, origin[1] + dy, origin[2] + dz];
                if at == origin {
                    continue;
                }
                let block = world.block_at(at[0], at[1], at[2]);
                if can_mossify(block.id) {
                    // Skip some cells so the patch looks patchy.
                    if hash3(at[0], at[1], at[2]) % 5 == 0 {
                        continue;
                    }
                    mossify.push((at, Block::new("moss_block", 0)));
                }
            }
        }
    }
    // Plant on existing moss + newly mossified tops (air above).
    let mut moss_floor: Vec<[i32; 3]> = mossify.iter().map(|(at, _)| *at).collect();
    moss_floor.push(origin);
    for at in moss_floor {
        let above = [at[0], at[1] + 1, at[2]];
        if !world.block_at(above[0], above[1], above[2]).is_air() {
            // Still allow converting this moss cell itself to grass.
            if let Some("grass") = cover_for(at[0], at[1], at[2]) {
                if at != origin {
                    covers.push((at, Block::new("grass", 0)));
                }
            }
            continue;
        }
        match cover_for(above[0], above[1], above[2]) {
            Some("grass") => {
                if at != origin {
                    covers.push((at, Block::new("grass", 0)));
                }
            }
            Some(id) => covers.push((above, Block::new(id, 0))),
            None => {}
        }
    }
    // Covers win on the same cell (e.g. moss → grass).
    let mut map = std::collections::BTreeMap::new();
    for (at, block) in mossify {
        map.insert(at, block);
    }
    for (at, block) in covers {
        map.insert(at, block);
    }
    map.into_iter().collect()
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
    fn bone_meal_mossifies_and_plants() {
        let cluster = world(vec![
            (4, 2, 4, Block::new("moss_block", 0)),
            (5, 2, 4, Block::new("stone", 0)),
            (3, 2, 4, Block::new("dirt", 0)),
            (4, 2, 5, Block::new("cobblestone", 0)),
        ]);
        let planned = plan_spread(&cluster, [4, 2, 4]);
        assert!(!planned.is_empty(), "expected spread changes");
        assert!(
            planned.iter().any(|(_, b)| {
                matches!(
                    b.id,
                    "moss_block" | "moss_carpet" | "azalea" | "flower" | "grass"
                )
            }),
            "expected lush ids, got {planned:?}"
        );
    }

    #[test]
    fn ignores_non_moss_origin() {
        let cluster = world(vec![(4, 2, 4, Block::new("stone", 0))]);
        assert!(plan_spread(&cluster, [4, 2, 4]).is_empty());
    }

    #[test]
    fn tint_reads_lush() {
        let t = tint("moss_block");
        assert!(t[1] > t[0]);
    }
}
