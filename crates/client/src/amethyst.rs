//! Budding amethyst grows small → medium → large clusters nearby.
//!
//! Breaking a large `amethyst_cluster` drops `amethyst_shard` (loot file).
//! Optional `tinted_glass` crafts from glass + shard.

use rebuild_world::{Block, Cluster};

/// Soft seconds between nearby growth rolls.
pub const GROW_PERIOD: f32 = 6.0;

/// How far from the player buds may grow / spawn.
pub const GROW_RADIUS: i32 = 14;

/// Cluster ladder (small → large).
pub const STAGES: [&str; 3] = [
    "small_amethyst_bud",
    "medium_amethyst_bud",
    "amethyst_cluster",
];

pub fn is_bud(id: &str) -> bool {
    STAGES.iter().any(|s| *s == id)
}

pub fn is_budding(id: &str) -> bool {
    id == "budding_amethyst"
}

pub fn is_amethyst_block(id: &str) -> bool {
    id == "amethyst_block" || is_budding(id)
}

pub fn stage_index(id: &str) -> Option<usize> {
    STAGES.iter().position(|s| *s == id)
}

pub fn next_stage(id: &str) -> Option<&'static str> {
    let i = stage_index(id)?;
    STAGES.get(i + 1).copied()
}

/// Facing like torch / end rod: 0 up, 1 west, 2 east, 3 north, 4 south, 5 down.
pub fn face_offset(facing: u8) -> [i32; 3] {
    match facing & 7 {
        1 => [-1, 0, 0],
        2 => [1, 0, 0],
        3 => [0, 0, -1],
        4 => [0, 0, 1],
        5 => [0, -1, 0],
        _ => [0, 1, 0],
    }
}

/// Facing that points from support `from` toward air `to`.
pub fn facing_toward(from: [i32; 3], to: [i32; 3]) -> u8 {
    let dx = to[0] - from[0];
    let dy = to[1] - from[1];
    let dz = to[2] - from[2];
    if dx < 0 {
        1
    } else if dx > 0 {
        2
    } else if dz < 0 {
        3
    } else if dz > 0 {
        4
    } else if dy < 0 {
        5
    } else {
        0
    }
}

pub fn make_bud(stage: usize, facing: u8) -> Block {
    let id = STAGES[stage.min(2)];
    Block::new(id, facing & 7)
}

pub fn tint(id: &str) -> [f32; 3] {
    match id {
        "budding_amethyst" => [0.55, 0.32, 0.72],
        "amethyst_block" => [0.62, 0.38, 0.82],
        "small_amethyst_bud" => [0.72, 0.48, 0.92],
        "medium_amethyst_bud" => [0.68, 0.42, 0.88],
        "amethyst_cluster" => [0.78, 0.55, 0.98],
        "tinted_glass" => [0.28, 0.18, 0.32],
        "amethyst_shard" => [0.82, 0.58, 1.0],
        _ => [0.62, 0.38, 0.82],
    }
}

/// Advance one nearby bud, or sprout a small bud beside budding amethyst.
pub fn plan_grow(world: &Cluster, focus: [i32; 3], radius: i32) -> Option<([i32; 3], Block)> {
    let mut advance = Vec::new();
    let mut sprouts = Vec::new();
    for dx in -radius..=radius {
        for dy in -radius..=radius {
            for dz in -radius..=radius {
                let at = [focus[0] + dx, focus[1] + dy, focus[2] + dz];
                let block = world.block_at(at[0], at[1], at[2]);
                if let Some(next) = next_stage(block.id) {
                    advance.push((at, Block::new(next, block.metadata & 7)));
                    continue;
                }
                if !is_budding(block.id) {
                    continue;
                }
                for facing in 0u8..=5 {
                    let off = face_offset(facing);
                    let cell = [at[0] + off[0], at[1] + off[1], at[2] + off[2]];
                    if !world.block_at(cell[0], cell[1], cell[2]).is_air() {
                        continue;
                    }
                    sprouts.push((cell, make_bud(0, facing)));
                }
            }
        }
    }
    let pool = if !advance.is_empty() {
        advance
    } else {
        sprouts
    };
    if pool.is_empty() {
        return None;
    }
    let pick = ((focus[0].unsigned_abs()
        + focus[1].unsigned_abs()
        + focus[2].unsigned_abs()
        + pool.len() as u32) as usize)
        % pool.len();
    Some(pool[pick])
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
    fn ladder_advances_to_cluster() {
        assert_eq!(next_stage("small_amethyst_bud"), Some("medium_amethyst_bud"));
        assert_eq!(next_stage("medium_amethyst_bud"), Some("amethyst_cluster"));
        assert_eq!(next_stage("amethyst_cluster"), None);
    }

    #[test]
    fn plan_grow_advances_before_sprouting() {
        let cluster = world(vec![
            (2, 2, 2, Block::new("budding_amethyst", 0)),
            (2, 3, 2, Block::new("small_amethyst_bud", 0)),
        ]);
        let grown = plan_grow(&cluster, [2, 2, 2], 4).unwrap();
        assert_eq!(grown.0, [2, 3, 2]);
        assert_eq!(grown.1.id, "medium_amethyst_bud");
    }

    #[test]
    fn plan_grow_sprouts_on_air_beside_budding() {
        let cluster = world(vec![(2, 2, 2, Block::new("budding_amethyst", 0))]);
        let grown = plan_grow(&cluster, [2, 2, 2], 4).unwrap();
        assert!(is_bud(grown.1.id));
        assert_eq!(grown.1.id, "small_amethyst_bud");
        let off = face_offset(grown.1.metadata);
        assert_eq!(
            [grown.0[0] - off[0], grown.0[1] - off[1], grown.0[2] - off[2]],
            [2, 2, 2]
        );
    }

    #[test]
    fn facing_toward_cardinals() {
        assert_eq!(facing_toward([0, 0, 0], [0, 1, 0]), 0);
        assert_eq!(facing_toward([0, 0, 0], [-1, 0, 0]), 1);
        assert_eq!(facing_toward([0, 0, 0], [0, -1, 0]), 5);
    }
}
