//! Redstone dust wire.
//!
//! Placeable dust carries lever/button power up to [`MAX_POWER`] blocks.
//! Metadata `0..=15` is signal strength (0 = unlit). Powered dust (and the
//! source itself) can drive pistons, dispensers/droppers, and lamps.

use rebuild_world::{Block, Cluster};
use std::collections::{HashMap, HashSet, VecDeque};

/// Strongest dust signal (Minecraft-style).
pub const MAX_POWER: u8 = 15;

/// Seconds a button keeps feeding the wire before signal drops.
pub const BUTTON_PULSE: f32 = 0.45;

const STEPS: [[i32; 3]; 6] = [
    [1, 0, 0],
    [-1, 0, 0],
    [0, 1, 0],
    [0, -1, 0],
    [0, 0, 1],
    [0, 0, -1],
];

pub fn is_dust(id: &str) -> bool {
    id == "redstone_dust"
}

pub fn power_of(metadata: u8) -> u8 {
    metadata.min(MAX_POWER)
}

pub fn with_power(power: u8) -> u8 {
    power.min(MAX_POWER)
}

pub fn is_lit(metadata: u8) -> bool {
    power_of(metadata) > 0
}

/// Palette tint: dull red when off, bright when fully powered.
pub fn tint(metadata: u8) -> [f32; 3] {
    let t = power_of(metadata) as f32 / MAX_POWER as f32;
    [
        0.32 + 0.68 * t,
        0.05 + 0.10 * t,
        0.05 + 0.03 * t,
    ]
}

/// Lever is a steady source when flipped on (bit 0).
pub fn lever_powered(metadata: u8) -> bool {
    metadata & 1 != 0
}

/// True when this cell can inject signal into neighboring dust / devices.
pub fn is_source(id: &str, metadata: u8, button_live: bool) -> bool {
    if id == "lever" {
        return lever_powered(metadata);
    }
    if id == "button" {
        return button_live;
    }
    false
}

/// Devices redstone can drive.
pub fn is_device(id: &str) -> bool {
    crate::piston::is_piston(id) || crate::dispenser::is_spitter(id) || id == "lamp"
}

/// Scan dust near `focus` and return desired power per dust cell.
pub fn plan_dust_power(
    world: &Cluster,
    focus: [i32; 3],
    radius: i32,
    button_sources: &[[i32; 3]],
) -> HashMap<[i32; 3], u8> {
    let mut dust = HashSet::new();
    let mut sources = Vec::new();
    for dx in -radius..=radius {
        for dy in -radius..=radius {
            for dz in -radius..=radius {
                let at = [focus[0] + dx, focus[1] + dy, focus[2] + dz];
                let block = world.block_at(at[0], at[1], at[2]);
                if is_dust(block.id) {
                    dust.insert(at);
                }
                let button_live = button_sources.iter().any(|c| *c == at);
                if is_source(block.id, block.metadata, button_live) {
                    sources.push(at);
                }
            }
        }
    }

    let mut powers: HashMap<[i32; 3], u8> = HashMap::new();
    let mut queue = VecDeque::new();
    for src in sources {
        for step in STEPS {
            let next = [src[0] + step[0], src[1] + step[1], src[2] + step[2]];
            if dust.contains(&next) {
                queue.push_back((next, MAX_POWER));
            }
        }
    }

    while let Some((at, power)) = queue.pop_front() {
        let cur = powers.get(&at).copied().unwrap_or(0);
        if power <= cur {
            continue;
        }
        powers.insert(at, power);
        if power <= 1 {
            continue;
        }
        let next_power = power - 1;
        for step in STEPS {
            let next = [at[0] + step[0], at[1] + step[1], at[2] + step[2]];
            if dust.contains(&next) {
                queue.push_back((next, next_power));
            }
        }
    }

    // Explicit zeros for dust that lost power.
    for at in dust {
        powers.entry(at).or_insert(0);
    }
    powers
}

/// Dust metadata updates from a power plan.
pub fn dust_changes(
    world: &Cluster,
    powers: &HashMap<[i32; 3], u8>,
) -> Vec<([i32; 3], Block)> {
    let mut out = Vec::new();
    for (&at, &want) in powers {
        let block = world.block_at(at[0], at[1], at[2]);
        if !is_dust(block.id) {
            continue;
        }
        if power_of(block.metadata) != want {
            out.push((at, Block::new("redstone_dust", with_power(want))));
        }
    }
    out
}

/// Every device cell that should be considered powered near `focus`.
pub fn powered_devices(
    world: &Cluster,
    focus: [i32; 3],
    radius: i32,
    button_sources: &[[i32; 3]],
    powers: &HashMap<[i32; 3], u8>,
) -> HashSet<[i32; 3]> {
    let mut out = HashSet::new();
    for dx in -radius..=radius {
        for dy in -radius..=radius {
            for dz in -radius..=radius {
                let at = [focus[0] + dx, focus[1] + dy, focus[2] + dz];
                let block = world.block_at(at[0], at[1], at[2]);
                if !is_device(block.id) {
                    continue;
                }
                if device_has_power(world, at, button_sources, powers) {
                    out.insert(at);
                }
            }
        }
    }
    out
}

fn device_has_power(
    world: &Cluster,
    at: [i32; 3],
    button_sources: &[[i32; 3]],
    powers: &HashMap<[i32; 3], u8>,
) -> bool {
    for step in STEPS {
        let n = [at[0] + step[0], at[1] + step[1], at[2] + step[2]];
        if let Some(&p) = powers.get(&n) {
            if p > 0 {
                return true;
            }
        }
        let block = world.block_at(n[0], n[1], n[2]);
        if is_dust(block.id) && power_of(block.metadata) > 0 {
            return true;
        }
        let button_live = button_sources.iter().any(|c| *c == n);
        if is_source(block.id, block.metadata, button_live) {
            return true;
        }
    }
    false
}

/// True when a lamp should stay lit from redstone (neighbor dust/source).
pub fn lamp_redstone_powered(
    world: &Cluster,
    at: [i32; 3],
    button_sources: &[[i32; 3]],
) -> bool {
    for step in STEPS {
        let n = [at[0] + step[0], at[1] + step[1], at[2] + step[2]];
        let block = world.block_at(n[0], n[1], n[2]);
        if is_dust(block.id) && power_of(block.metadata) > 0 {
            return true;
        }
        let button_live = button_sources.iter().any(|c| *c == n);
        if is_source(block.id, block.metadata, button_live) {
            return true;
        }
    }
    false
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
    fn dust_decays_along_a_line() {
        let mut blocks = vec![(1u8, 1u16, 1u8, Block::new("lever", 1))];
        for x in 2u8..=6u8 {
            blocks.push((x, 1, 1, Block::new("redstone_dust", 0)));
        }
        let cluster = world(blocks);
        let powers = plan_dust_power(&cluster, [3, 1, 1], 8, &[]);
        assert_eq!(powers.get(&[2, 1, 1]), Some(&15));
        assert_eq!(powers.get(&[3, 1, 1]), Some(&14));
        assert_eq!(powers.get(&[6, 1, 1]), Some(&11));
    }

    #[test]
    fn unpowered_lever_leaves_dust_dark() {
        let cluster = world(vec![
            (1, 1, 1, Block::new("lever", 0)),
            (2, 1, 1, Block::new("redstone_dust", 15)),
        ]);
        let powers = plan_dust_power(&cluster, [2, 1, 1], 4, &[]);
        assert_eq!(powers.get(&[2, 1, 1]), Some(&0));
    }

    #[test]
    fn dust_powers_a_neighbor_device() {
        let cluster = world(vec![
            (1, 1, 1, Block::new("lever", 1)),
            (2, 1, 1, Block::new("redstone_dust", 0)),
            (3, 1, 1, Block::new("piston", 0)),
        ]);
        let powers = plan_dust_power(&cluster, [2, 1, 1], 4, &[]);
        let devices = powered_devices(&cluster, [2, 1, 1], 4, &[], &powers);
        assert!(devices.contains(&[3, 1, 1]));
    }

    #[test]
    fn tint_brightens_with_power() {
        let off = tint(0);
        let on = tint(15);
        assert!(on[0] > off[0]);
        assert!(is_lit(7));
        assert!(!is_lit(0));
    }
}
