//! Bamboo grows upward; scaffolding builds climbable towers and breaks in a cascade.
//!
//! Bamboo stacks like sugar cane (max [`BAMBOO_MAX`]). Scaffolding is climbable;
//! breaking one clears every scaffolding cell above it in the same column.

use rebuild_world::{Block, Cluster};

/// Soft seconds between nearby bamboo growth rolls.
pub const GROW_PERIOD: f32 = 5.0;

/// How far from the player bamboo may grow.
pub const GROW_RADIUS: i32 = 14;

/// Tallest bamboo column (blocks).
pub const BAMBOO_MAX: i32 = 12;

pub fn is_bamboo(id: &str) -> bool {
    id == "bamboo"
}

pub fn is_scaffolding(id: &str) -> bool {
    id == "scaffolding"
}

pub fn tint(id: &str) -> [f32; 3] {
    match id {
        "bamboo" => [0.42, 0.72, 0.28],
        "scaffolding" => [0.78, 0.68, 0.32],
        _ => [0.42, 0.72, 0.28],
    }
}

pub fn can_plant_on(id: &str) -> bool {
    matches!(
        id,
        "dirt" | "grass" | "sand" | "moss_block" | "podzol" | "gravel" | "clay"
    )
}

fn column_height(world: &Cluster, x: i32, y: i32, z: i32) -> i32 {
    let mut base = y;
    while base > 0 && is_bamboo(world.block_at(x, base - 1, z).id) {
        base -= 1;
    }
    let mut top = y;
    while top < 255 && is_bamboo(world.block_at(x, top + 1, z).id) {
        top += 1;
    }
    top - base + 1
}

/// Grow one bamboo cell upward near `focus`, if any stalk has room.
pub fn plan_grow(world: &Cluster, focus: [i32; 3], radius: i32) -> Option<([i32; 3], Block)> {
    let mut candidates = Vec::new();
    for dx in -radius..=radius {
        for dy in -radius..=radius {
            for dz in -radius..=radius {
                let at = [focus[0] + dx, focus[1] + dy, focus[2] + dz];
                if !is_bamboo(world.block_at(at[0], at[1], at[2]).id) {
                    continue;
                }
                let above = [at[0], at[1] + 1, at[2]];
                if !world.block_at(above[0], above[1], above[2]).is_air() {
                    continue;
                }
                if column_height(world, at[0], at[1], at[2]) >= BAMBOO_MAX {
                    continue;
                }
                // Prefer the top of each stalk.
                if is_bamboo(world.block_at(at[0], at[1] + 1, at[2]).id) {
                    continue;
                }
                candidates.push(above);
            }
        }
    }
    if candidates.is_empty() {
        return None;
    }
    let pick = ((focus[0].unsigned_abs()
        + focus[1].unsigned_abs()
        + focus[2].unsigned_abs()
        + candidates.len() as u32) as usize)
        % candidates.len();
    Some((candidates[pick], Block::new("bamboo", 0)))
}

/// Bone-meal / manual grow: extend the stalk at `hit` by one if possible.
pub fn plan_fertilize(world: &Cluster, hit: [i32; 3]) -> Option<([i32; 3], Block)> {
    if !is_bamboo(world.block_at(hit[0], hit[1], hit[2]).id) {
        return None;
    }
    let mut top = hit[1];
    while top < 255 && is_bamboo(world.block_at(hit[0], top + 1, hit[2]).id) {
        top += 1;
    }
    if column_height(world, hit[0], hit[1], hit[2]) >= BAMBOO_MAX {
        return None;
    }
    let above = [hit[0], top + 1, hit[2]];
    if !world.block_at(above[0], above[1], above[2]).is_air() {
        return None;
    }
    Some((above, Block::new("bamboo", 0)))
}

/// Y of the top scaffolding cell in this column (inclusive), starting from `y`.
pub fn scaffolding_top(world: &Cluster, x: i32, y: i32, z: i32) -> i32 {
    let mut top = y;
    while top < 255 && is_scaffolding(world.block_at(x, top + 1, z).id) {
        top += 1;
    }
    top
}

/// Place target when holding scaffolding and looking at scaffolding: one above the tower.
pub fn plan_scaffold_place(world: &Cluster, hit: [i32; 3]) -> Option<[i32; 3]> {
    if !is_scaffolding(world.block_at(hit[0], hit[1], hit[2]).id) {
        return None;
    }
    let top = scaffolding_top(world, hit[0], hit[1], hit[2]);
    let at = [hit[0], top + 1, hit[2]];
    if at[1] > 255 || !world.block_at(at[0], at[1], at[2]).is_air() {
        return None;
    }
    Some(at)
}

/// Scaffolding cells above `broken` in the same column (not including `broken`).
pub fn plan_cascade(world: &Cluster, broken: [i32; 3]) -> Vec<[i32; 3]> {
    let mut out = Vec::new();
    let mut y = broken[1] + 1;
    while y <= 255 && is_scaffolding(world.block_at(broken[0], y, broken[2]).id) {
        out.push([broken[0], y, broken[2]]);
        y += 1;
    }
    out
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
    fn bamboo_grows_upward() {
        let cluster = world(vec![
            (2, 1, 2, Block::new("dirt", 0)),
            (2, 2, 2, Block::new("bamboo", 0)),
        ]);
        let grown = plan_grow(&cluster, [2, 2, 2], 4).unwrap();
        assert_eq!(grown.0, [2, 3, 2]);
        assert!(is_bamboo(grown.1.id));
    }

    #[test]
    fn bamboo_stops_at_max() {
        let mut blocks = vec![(2, 1, 2, Block::new("dirt", 0))];
        for y in 2..(2 + BAMBOO_MAX) {
            blocks.push((2, y as u16, 2, Block::new("bamboo", 0)));
        }
        let cluster = world(blocks);
        assert!(plan_grow(&cluster, [2, 5, 2], 6).is_none());
    }

    #[test]
    fn scaffold_places_atop_tower() {
        let cluster = world(vec![
            (3, 2, 3, Block::new("scaffolding", 0)),
            (3, 3, 3, Block::new("scaffolding", 0)),
        ]);
        assert_eq!(plan_scaffold_place(&cluster, [3, 2, 3]), Some([3, 4, 3]));
    }

    #[test]
    fn cascade_clears_column_above() {
        let cluster = world(vec![
            (4, 2, 4, Block::new("scaffolding", 0)),
            (4, 3, 4, Block::new("scaffolding", 0)),
            (4, 4, 4, Block::new("scaffolding", 0)),
        ]);
        let cleared = plan_cascade(&cluster, [4, 2, 4]);
        assert_eq!(cleared, vec![[4, 3, 4], [4, 4, 4]]);
    }
}
