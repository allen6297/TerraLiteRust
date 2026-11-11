//! Hanging cave vines grow downward; berried tips light and yield edible glow berries.
//!
//! Empty [`cave_vines`] hang from a ceiling and lengthen into air below. Bone meal
//! berries an empty vine or extends a tip. Berried vines ([`cave_vines_berries`])
//! emit light; picking them drops [`glow_berries`].

use rebuild_world::{Block, Cluster};

/// Soft seconds between nearby cave-vine growth rolls.
pub const GROW_PERIOD: f32 = 5.0;

/// How far from the player vines may grow.
pub const GROW_RADIUS: i32 = 14;

/// Tallest hanging vine column (blocks).
pub const VINE_MAX: i32 = 12;

pub const CAVE_VINES: &str = "cave_vines";
pub const CAVE_VINES_BERRIES: &str = "cave_vines_berries";
pub const GLOW_BERRIES: &str = "glow_berries";

pub fn is_cave_vine(id: &str) -> bool {
    id == CAVE_VINES || id == CAVE_VINES_BERRIES
}

pub fn has_berries(id: &str) -> bool {
    id == CAVE_VINES_BERRIES
}

pub fn empty_vine() -> Block {
    Block::new(CAVE_VINES, 0)
}

pub fn berried_vine() -> Block {
    Block::new(CAVE_VINES_BERRIES, 0)
}

pub fn tint(id: &str) -> [f32; 3] {
    match id {
        CAVE_VINES => [0.28, 0.55, 0.22],
        CAVE_VINES_BERRIES => [0.55, 0.72, 0.28],
        GLOW_BERRIES => [0.95, 0.82, 0.28],
        _ => [0.28, 0.55, 0.22],
    }
}

fn column_length(world: &Cluster, x: i32, y: i32, z: i32) -> i32 {
    let mut top = y;
    while top < 255 && is_cave_vine(world.block_at(x, top + 1, z).id) {
        top += 1;
    }
    let mut bottom = y;
    while bottom > 0 && is_cave_vine(world.block_at(x, bottom - 1, z).id) {
        bottom -= 1;
    }
    top - bottom + 1
}

fn is_tip(world: &Cluster, at: [i32; 3]) -> bool {
    is_cave_vine(world.block_at(at[0], at[1], at[2]).id)
        && !is_cave_vine(world.block_at(at[0], at[1] - 1, at[2]).id)
}

/// Grow nearby: berry an empty vine, or hang one cell lower from a tip.
pub fn plan_grow(world: &Cluster, focus: [i32; 3], radius: i32) -> Option<([i32; 3], Block)> {
    let mut berry = Vec::new();
    let mut extend = Vec::new();
    for dx in -radius..=radius {
        for dy in -radius..=radius {
            for dz in -radius..=radius {
                let at = [focus[0] + dx, focus[1] + dy, focus[2] + dz];
                let id = world.block_at(at[0], at[1], at[2]).id;
                if !is_cave_vine(id) {
                    continue;
                }
                if id == CAVE_VINES {
                    berry.push(at);
                }
                if !is_tip(world, at) {
                    continue;
                }
                if column_length(world, at[0], at[1], at[2]) >= VINE_MAX {
                    continue;
                }
                let below = [at[0], at[1] - 1, at[2]];
                if below[1] < 0 || !world.block_at(below[0], below[1], below[2]).is_air() {
                    continue;
                }
                extend.push(below);
            }
        }
    }
    // Prefer berries so caves light up; otherwise lengthen.
    if !berry.is_empty() {
        let pick = ((focus[0].unsigned_abs()
            + focus[1].unsigned_abs()
            + focus[2].unsigned_abs()
            + berry.len() as u32) as usize)
            % berry.len();
        return Some((berry[pick], berried_vine()));
    }
    if extend.is_empty() {
        return None;
    }
    let pick = ((focus[0].unsigned_abs()
        + focus[1].unsigned_abs()
        + focus[2].unsigned_abs()
        + extend.len() as u32) as usize)
        % extend.len();
    Some((extend[pick], empty_vine()))
}

/// Bone meal: berry an empty vine, else hang one cell lower from the tip.
pub fn plan_fertilize(world: &Cluster, hit: [i32; 3]) -> Option<([i32; 3], Block)> {
    let id = world.block_at(hit[0], hit[1], hit[2]).id;
    if !is_cave_vine(id) {
        return None;
    }
    if id == CAVE_VINES {
        return Some((hit, berried_vine()));
    }
    let mut tip_y = hit[1];
    while tip_y > 0 && is_cave_vine(world.block_at(hit[0], tip_y - 1, hit[2]).id) {
        tip_y -= 1;
    }
    if column_length(world, hit[0], hit[1], hit[2]) >= VINE_MAX {
        return None;
    }
    let below = [hit[0], tip_y - 1, hit[2]];
    if below[1] < 0 || !world.block_at(below[0], below[1], below[2]).is_air() {
        return None;
    }
    Some((below, empty_vine()))
}

/// Pick berries: berried cell becomes empty vine.
pub fn plan_harvest(world: &Cluster, hit: [i32; 3]) -> Option<[i32; 3]> {
    if !has_berries(world.block_at(hit[0], hit[1], hit[2]).id) {
        return None;
    }
    Some(hit)
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
    fn vines_grow_downward() {
        let cluster = world(vec![
            (2, 6, 2, Block::new("stone", 0)),
            (2, 5, 2, empty_vine()),
        ]);
        // Prefer berry first when empty vines exist.
        let grown = plan_grow(&cluster, [2, 5, 2], 4).unwrap();
        assert_eq!(grown.0, [2, 5, 2]);
        assert!(has_berries(grown.1.id));
    }

    #[test]
    fn berried_tip_extends_down() {
        let cluster = world(vec![
            (2, 6, 2, Block::new("stone", 0)),
            (2, 5, 2, berried_vine()),
        ]);
        let grown = plan_grow(&cluster, [2, 5, 2], 4).unwrap();
        assert_eq!(grown.0, [2, 4, 2]);
        assert_eq!(grown.1.id, CAVE_VINES);
    }

    #[test]
    fn fertilize_berries_empty_vine() {
        let cluster = world(vec![(3, 4, 3, empty_vine())]);
        let next = plan_fertilize(&cluster, [3, 4, 3]).unwrap();
        assert_eq!(next.0, [3, 4, 3]);
        assert!(has_berries(next.1.id));
    }

    #[test]
    fn harvest_clears_berries() {
        let cluster = world(vec![(4, 5, 4, berried_vine())]);
        assert_eq!(plan_harvest(&cluster, [4, 5, 4]), Some([4, 5, 4]));
        assert!(plan_harvest(&world(vec![(4, 5, 4, empty_vine())]), [4, 5, 4]).is_none());
    }

    #[test]
    fn stops_at_max_length() {
        let mut blocks = vec![(2, 20, 2, Block::new("stone", 0))];
        for i in 0..VINE_MAX {
            blocks.push((2, (19 - i) as u16, 2, berried_vine()));
        }
        let cluster = world(blocks);
        assert!(plan_grow(&cluster, [2, 15, 2], 8).is_none());
    }
}
