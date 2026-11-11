//! Dirt + water bottle → mud; drip / craft packs it; craft mud bricks.
//!
//! Use a [`WATER_BOTTLE`] on dirt or grass to make [`MUD`]. A watered
//! dripstone column packing a mud floor yields [`PACKED_MUD`]. Crafting:
//! mud + wheat → packed mud; four packed mud → [`MUD_BRICKS`].

use rebuild_world::{Block, Cluster};

pub const MUD: &str = "mud";
pub const PACKED_MUD: &str = "packed_mud";
pub const MUD_BRICKS: &str = "mud_bricks";
pub const WATER_BOTTLE: &str = "water_bottle";

pub fn is_mud(id: &str) -> bool {
    id == MUD
}

pub fn is_packed_mud(id: &str) -> bool {
    id == PACKED_MUD
}

pub fn is_mud_bricks(id: &str) -> bool {
    id == MUD_BRICKS
}

pub fn is_water_bottle(id: &str) -> bool {
    id == WATER_BOTTLE
}

pub fn tint(id: &str) -> [f32; 3] {
    match id {
        MUD => [0.42, 0.32, 0.24],
        PACKED_MUD => [0.55, 0.42, 0.32],
        MUD_BRICKS => [0.62, 0.48, 0.36],
        WATER_BOTTLE => [0.35, 0.55, 0.85],
        _ => [0.42, 0.32, 0.24],
    }
}

/// Dirt / grass under a water bottle becomes mud.
pub fn plan_bottle(world: &Cluster, hit: [i32; 3]) -> Option<[i32; 3]> {
    let id = world.block_at(hit[0], hit[1], hit[2]).id;
    if id == "dirt" || id == "grass" {
        Some(hit)
    } else {
        None
    }
}

pub fn mud_block() -> Block {
    Block::new(MUD, 0)
}

pub fn packed_mud_block() -> Block {
    Block::new(PACKED_MUD, 0)
}

pub fn mud_bricks_block() -> Block {
    Block::new(MUD_BRICKS, 0)
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
    fn bottle_turns_dirt_and_grass_to_mud() {
        let dirt = world(vec![(2, 1, 2, Block::new("dirt", 0))]);
        assert_eq!(plan_bottle(&dirt, [2, 1, 2]), Some([2, 1, 2]));
        let grass = world(vec![(2, 1, 2, Block::new("grass", 0))]);
        assert_eq!(plan_bottle(&grass, [2, 1, 2]), Some([2, 1, 2]));
        let stone = world(vec![(2, 1, 2, Block::new("stone", 0))]);
        assert!(plan_bottle(&stone, [2, 1, 2]).is_none());
    }
}
