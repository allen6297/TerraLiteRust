//! Pointed dripstone: stalactites / stalagmites grow under water drips,
//! fill cauldrons, and spike fall landings.
//!
//! `pointed_dripstone` metadata bit 0 = hanging (stalactite). Standing tips
//! grow upward; hanging tips grow downward. A water cell above
//! `dripstone_block` drives the drip.

use rebuild_world::{Block, Cluster};
use crate::hotbar::CAULDRON_FULL;

/// Soft seconds between nearby drip rolls.
pub const DRIP_PERIOD: f32 = 5.0;

/// How far from the player drips may act.
pub const DRIP_RADIUS: i32 = 14;

/// Max air cells a drip scans below a block.
pub const DRIP_DEPTH: i32 = 12;

pub fn is_dripstone_block(id: &str) -> bool {
    id == "dripstone_block"
}

pub fn is_pointed(id: &str) -> bool {
    id == "pointed_dripstone"
}

pub fn is_hanging(metadata: u8) -> bool {
    metadata & 1 != 0
}

pub fn make_pointed(hanging: bool) -> Block {
    Block::new("pointed_dripstone", if hanging { 1 } else { 0 })
}

pub fn tint(id: &str) -> [f32; 3] {
    match id {
        "dripstone_block" => [0.55, 0.42, 0.34],
        "pointed_dripstone" => [0.62, 0.48, 0.38],
        _ => [0.55, 0.42, 0.34],
    }
}

/// Extra hearts when landing on a pointed tip. Short hops still sting.
pub fn spike_bonus(drop: f32) -> i32 {
    if drop < 0.55 {
        0
    } else if drop < 4.0 {
        2
    } else {
        3
    }
}

fn is_water(block: Block) -> bool {
    block.id == "water"
}

fn has_water_above(world: &Cluster, x: i32, y: i32, z: i32) -> bool {
    is_water(world.block_at(x, y + 1, z))
}

/// One drip outcome: grow a tip, or pour into a cauldron.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DripAct {
    Place([i32; 3], Block),
    FillCauldron([i32; 3], u8),
}

/// Prefer cauldron fill, else extend / sprout hanging then standing tips.
pub fn plan_drip(world: &Cluster, focus: [i32; 3], radius: i32) -> Option<DripAct> {
    let mut fills = Vec::new();
    let mut grows = Vec::new();
    for dx in -radius..=radius {
        for dy in -radius..=radius {
            for dz in -radius..=radius {
                let at = [focus[0] + dx, focus[1] + dy, focus[2] + dz];
                let block = world.block_at(at[0], at[1], at[2]);
                if !is_dripstone_block(block.id) || !has_water_above(world, at[0], at[1], at[2]) {
                    continue;
                }
                scan_column(world, at, &mut fills, &mut grows);
            }
        }
    }
    let pool: Vec<DripAct> = if !fills.is_empty() {
        fills
    } else {
        grows
    };
    if pool.is_empty() {
        return None;
    }
    let pick = ((focus[0].unsigned_abs()
        + focus[1].unsigned_abs()
        + focus[2].unsigned_abs()
        + pool.len() as u32) as usize)
        % pool.len();
    Some(pool[pick].clone())
}

fn scan_column(
    world: &Cluster,
    support: [i32; 3],
    fills: &mut Vec<DripAct>,
    grows: &mut Vec<DripAct>,
) {
    let (sx, sy, sz) = (support[0], support[1], support[2]);
    let mut y = sy - 1;
    let mut hanging_tip = None;
    let mut standing_top = None;
    let mut floor_y = None;
    let mut cauldron = None;
    for _ in 0..DRIP_DEPTH {
        if y < 0 {
            break;
        }
        let cell = world.block_at(sx, y, sz);
        if cell.id == "cauldron" {
            cauldron = Some((y, cell.metadata.min(CAULDRON_FULL)));
            break;
        }
        if is_pointed(cell.id) {
            if is_hanging(cell.metadata) {
                // Lowest hanging tip wins for extension (scan goes top → bottom).
                hanging_tip = Some(y);
            } else if standing_top.is_none() {
                // First standing tip from above is the top of the stalagmite.
                standing_top = Some(y);
            }
            y -= 1;
            continue;
        }
        if cell.is_air() {
            y -= 1;
            continue;
        }
        // First solid under the drip column.
        floor_y = Some(y);
        break;
    }
    if let Some((cy, level)) = cauldron {
        if level < CAULDRON_FULL {
            fills.push(DripAct::FillCauldron([sx, cy, sz], level + 1));
        }
        return;
    }
    // Grow at most one cell this drip.
    if let Some(tip) = hanging_tip {
        let below = tip - 1;
        if below >= 0 && world.block_at(sx, below, sz).is_air() {
            grows.push(DripAct::Place([sx, below, sz], make_pointed(true)));
            return;
        }
    } else if world.block_at(sx, sy - 1, sz).is_air() {
        grows.push(DripAct::Place([sx, sy - 1, sz], make_pointed(true)));
        return;
    }
    if let Some(floor) = floor_y {
        let above = floor + 1;
        if above < sy && world.block_at(sx, above, sz).is_air() {
            grows.push(DripAct::Place([sx, above, sz], make_pointed(false)));
            return;
        }
        if let Some(top) = standing_top {
            let grow = top + 1;
            if grow < sy && world.block_at(sx, grow, sz).is_air() {
                grows.push(DripAct::Place([sx, grow, sz], make_pointed(false)));
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
    fn drip_fills_cauldron_under_watered_block() {
        let cluster = world(vec![
            (2, 5, 2, Block::new("water", 0)),
            (2, 4, 2, Block::new("dripstone_block", 0)),
            (2, 1, 2, Block::new("cauldron", 1)),
        ]);
        let act = plan_drip(&cluster, [2, 4, 2], 6).unwrap();
        assert_eq!(act, DripAct::FillCauldron([2, 1, 2], 2));
    }

    #[test]
    fn drip_sprouts_hanging_tip_under_block() {
        let cluster = world(vec![
            (2, 5, 2, Block::new("water", 0)),
            (2, 4, 2, Block::new("dripstone_block", 0)),
            (2, 0, 2, Block::new("stone", 0)),
        ]);
        let act = plan_drip(&cluster, [2, 4, 2], 6).unwrap();
        match act {
            DripAct::Place(at, block) => {
                assert_eq!(at, [2, 3, 2]);
                assert!(is_pointed(block.id));
                assert!(is_hanging(block.metadata));
            }
            other => panic!("expected place, got {other:?}"),
        }
    }

    #[test]
    fn drip_extends_hanging_column() {
        let cluster = world(vec![
            (2, 5, 2, Block::new("water", 0)),
            (2, 4, 2, Block::new("dripstone_block", 0)),
            (2, 3, 2, make_pointed(true)),
            (2, 0, 2, Block::new("stone", 0)),
        ]);
        let act = plan_drip(&cluster, [2, 4, 2], 6).unwrap();
        assert_eq!(act, DripAct::Place([2, 2, 2], make_pointed(true)));
    }

    #[test]
    fn spike_bonus_stings_short_falls() {
        assert_eq!(spike_bonus(0.2), 0);
        assert_eq!(spike_bonus(1.0), 2);
        assert_eq!(spike_bonus(5.0), 3);
    }
}
