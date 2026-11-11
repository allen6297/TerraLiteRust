//! Copper block oxidation, wax, and axe scrape.
//!
//! Unwaxed copper ages `copper_block` → `exposed_copper` → `weathered_copper`
//! → `oxidized_copper`. Honeycomb sets metadata bit 0 (waxed) and freezes the
//! stage. An axe dewaxes first, then scrapes one oxidation stage.

use rebuild_world::{Block, Cluster};

/// Soft seconds between nearby oxidation rolls.
pub const OXIDE_PERIOD: f32 = 8.0;

/// How far from the player copper may age.
pub const OXIDE_RADIUS: i32 = 14;

/// Oxidation ladder (fresh → fully green).
pub const STAGES: [&str; 4] = [
    "copper_block",
    "exposed_copper",
    "weathered_copper",
    "oxidized_copper",
];

pub fn is_copper_block(id: &str) -> bool {
    STAGES.iter().any(|s| *s == id)
}

pub fn is_axe(id: &str) -> bool {
    matches!(
        id,
        "wooden_axe"
            | "stone_axe"
            | "iron_axe"
            | "golden_axe"
            | "diamond_axe"
            | "netherite_axe"
    ) || (id.ends_with("_axe") && !id.contains("pickaxe"))
}

pub fn is_honeycomb(id: &str) -> bool {
    id == "honeycomb"
}

pub fn stage_index(id: &str) -> Option<usize> {
    STAGES.iter().position(|s| *s == id)
}

pub fn is_waxed(metadata: u8) -> bool {
    metadata & 1 != 0
}

pub fn with_waxed(metadata: u8, waxed: bool) -> u8 {
    if waxed {
        metadata | 1
    } else {
        metadata & !1
    }
}

pub fn make_copper(stage: usize, waxed: bool) -> Block {
    let id = STAGES[stage.min(3)];
    Block::new(id, with_waxed(0, waxed))
}

pub fn next_stage(id: &str) -> Option<&'static str> {
    let i = stage_index(id)?;
    STAGES.get(i + 1).copied()
}

pub fn prev_stage(id: &str) -> Option<&'static str> {
    let i = stage_index(id)?;
    if i == 0 {
        None
    } else {
        Some(STAGES[i - 1])
    }
}

/// Palette tint by stage; waxed reads slightly glossier / cooler.
pub fn tint(id: &str, metadata: u8) -> [f32; 3] {
    let mut base = match stage_index(id).unwrap_or(0) {
        0 => [0.78, 0.48, 0.32],
        1 => [0.62, 0.52, 0.38],
        2 => [0.42, 0.62, 0.48],
        _ => [0.32, 0.58, 0.52],
    };
    if is_waxed(metadata) {
        base[0] = (base[0] * 0.92 + 0.06_f32).min(1.0);
        base[1] = (base[1] * 0.95 + 0.05_f32).min(1.0);
        base[2] = (base[2] * 0.90 + 0.10_f32).min(1.0);
    }
    base
}

/// Apply honeycomb: lock the current stage.
pub fn wax(block: Block) -> Option<Block> {
    if !is_copper_block(block.id) || is_waxed(block.metadata) {
        return None;
    }
    Some(Block::new(block.id, with_waxed(block.metadata, true)))
}

/// Axe use: dewax, else scrape one stage. Tip word is `dewax` or `scrape`.
pub fn scrape(block: Block) -> Option<(Block, &'static str)> {
    if !is_copper_block(block.id) {
        return None;
    }
    if is_waxed(block.metadata) {
        return Some((
            Block::new(block.id, with_waxed(block.metadata, false)),
            "dewax",
        ));
    }
    let prev = prev_stage(block.id)?;
    Some((Block::new(prev, 0), "scrape"))
}

/// Age one unwaxed copper cell near `focus`, if any.
pub fn plan_age(world: &Cluster, focus: [i32; 3], radius: i32) -> Option<([i32; 3], Block)> {
    let mut candidates = Vec::new();
    for dx in -radius..=radius {
        for dy in -radius..=radius {
            for dz in -radius..=radius {
                let at = [focus[0] + dx, focus[1] + dy, focus[2] + dz];
                let block = world.block_at(at[0], at[1], at[2]);
                if !is_copper_block(block.id) || is_waxed(block.metadata) {
                    continue;
                }
                let Some(next) = next_stage(block.id) else {
                    continue;
                };
                candidates.push((at, Block::new(next, block.metadata)));
            }
        }
    }
    if candidates.is_empty() {
        return None;
    }
    // Deterministic soft pick from focus so demo/play reads stable.
    let pick = ((focus[0].unsigned_abs()
        + focus[1].unsigned_abs()
        + focus[2].unsigned_abs()
        + candidates.len() as u32)
        as usize)
        % candidates.len();
    Some(candidates[pick])
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
    fn ladder_advances_and_scrapes() {
        assert_eq!(next_stage("copper_block"), Some("exposed_copper"));
        assert_eq!(next_stage("oxidized_copper"), None);
        assert_eq!(prev_stage("weathered_copper"), Some("exposed_copper"));
        let scraped = scrape(Block::new("weathered_copper", 0)).unwrap();
        assert_eq!(scraped.0.id, "exposed_copper");
        assert_eq!(scraped.1, "scrape");
    }

    #[test]
    fn wax_locks_and_axe_dewaxes_first() {
        let waxed = wax(Block::new("exposed_copper", 0)).unwrap();
        assert!(is_waxed(waxed.metadata));
        assert!(wax(waxed).is_none());
        let (dewaxed, tip) = scrape(waxed).unwrap();
        assert_eq!(tip, "dewax");
        assert!(!is_waxed(dewaxed.metadata));
        assert_eq!(dewaxed.id, "exposed_copper");
    }

    #[test]
    fn plan_age_skips_waxed_and_max() {
        let cluster = world(vec![
            (1, 1, 1, Block::new("copper_block", 1)), // waxed
            (2, 1, 1, Block::new("oxidized_copper", 0)),
            (3, 1, 1, Block::new("exposed_copper", 0)),
        ]);
        let aged = plan_age(&cluster, [2, 1, 1], 4).unwrap();
        assert_eq!(aged.0, [3, 1, 1]);
        assert_eq!(aged.1.id, "weathered_copper");
    }

    #[test]
    fn axe_detects_real_axes_not_pickaxes() {
        assert!(is_axe("iron_axe"));
        assert!(is_axe("netherite_axe"));
        assert!(!is_axe("iron_pickaxe"));
        assert!(!is_axe("honeycomb"));
    }
}
