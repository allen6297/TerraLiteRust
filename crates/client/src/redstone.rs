//! Redstone dust wire and repeaters.
//!
//! Placeable dust carries lever/button power up to [`MAX_POWER`] blocks.
//! Metadata `0..=15` is signal strength (0 = unlit). Powered dust (and the
//! source itself) can drive pistons, dispensers/droppers, and lamps.
//!
//! Repeaters are one-way diodes: they read the back face, wait 1–4 redstone
//! ticks, then drive the front face at full strength. Facing + delay fit in
//! the 4-bit metadata; powered state lives in [`Block::fill`].

use rebuild_world::{Block, Cluster, FILL_EMBER, FILL_NONE};
use std::collections::{HashMap, HashSet, VecDeque};

/// Strongest dust signal (Minecraft-style).
pub const MAX_POWER: u8 = 15;

/// Seconds a button keeps feeding the wire before signal drops.
pub const BUTTON_PULSE: f32 = 0.45;

/// One redstone tick in seconds (Minecraft ≈ 0.1s).
pub const TICK: f32 = 0.1;

const STEPS: [[i32; 3]; 6] = [
    [1, 0, 0],
    [-1, 0, 0],
    [0, 1, 0],
    [0, -1, 0],
    [0, 0, 1],
    [0, 0, -1],
];

/// Horizontal facing index → step. 0=+X 1=-X 2=+Z 3=-Z.
const FACE_DIRS: [[i32; 3]; 4] = [[1, 0, 0], [-1, 0, 0], [0, 0, 1], [0, 0, -1]];

pub fn is_dust(id: &str) -> bool {
    id == "redstone_dust"
}

pub fn is_repeater(id: &str) -> bool {
    id == "redstone_repeater"
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

/// Repeater tint from delay bits (powered state is fill; mesh uses this base).
pub fn repeater_tint(metadata: u8) -> [f32; 3] {
    let delay = (delay_ticks(metadata) as f32 - 1.0) / 3.0;
    [0.55 + 0.30 * delay, 0.22 + 0.08 * delay, 0.18]
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

// --- Repeater: facing bits 0..=1, delay bits 2..=3, powered via fill ---

pub fn facing_index(metadata: u8) -> u8 {
    metadata & 3
}

pub fn facing_dir(metadata: u8) -> [i32; 3] {
    FACE_DIRS[facing_index(metadata) as usize]
}

pub fn delay_ticks(metadata: u8) -> u8 {
    ((metadata >> 2) & 3) + 1
}

pub fn repeater_powered(block: Block) -> bool {
    is_repeater(block.id) && block.fill != FILL_NONE
}

pub fn with_facing(facing: u8) -> u8 {
    facing & 3
}

pub fn with_delay(metadata: u8, ticks: u8) -> u8 {
    let ticks = ticks.clamp(1, 4) - 1;
    (metadata & !0b1100) | (ticks << 2)
}

pub fn cycle_delay(metadata: u8) -> u8 {
    let next = delay_ticks(metadata) % 4 + 1;
    with_delay(metadata, next)
}

pub fn delay_seconds(metadata: u8) -> f32 {
    delay_ticks(metadata) as f32 * TICK
}

/// Build a repeater cell (powered uses fill because metadata is only 4 bits).
pub fn make_repeater(facing: u8, ticks: u8, powered: bool) -> Block {
    let meta = with_delay(with_facing(facing), ticks);
    let block = Block::new("redstone_repeater", meta);
    if powered {
        block.with_fill(FILL_EMBER)
    } else {
        block.with_fill(FILL_NONE)
    }
}

pub fn with_repeater_powered(block: Block, on: bool) -> Block {
    Block::new("redstone_repeater", block.metadata).with_fill(if on {
        FILL_EMBER
    } else {
        FILL_NONE
    })
}

/// Look → horizontal repeater facing (output points along look).
pub fn facing_from_look(forward: [f32; 3]) -> u8 {
    if forward[0].abs() >= forward[2].abs() {
        if forward[0] >= 0.0 {
            0
        } else {
            1
        }
    } else if forward[2] >= 0.0 {
        2
    } else {
        3
    }
}

fn back_cell(at: [i32; 3], metadata: u8) -> [i32; 3] {
    let d = facing_dir(metadata);
    [at[0] - d[0], at[1] - d[1], at[2] - d[2]]
}

fn front_cell(at: [i32; 3], metadata: u8) -> [i32; 3] {
    let d = facing_dir(metadata);
    [at[0] + d[0], at[1] + d[1], at[2] + d[2]]
}

/// True when the cell behind a repeater is feeding it.
pub fn repeater_input_on(
    world: &Cluster,
    at: [i32; 3],
    metadata: u8,
    button_sources: &[[i32; 3]],
    powers: &HashMap<[i32; 3], u8>,
) -> bool {
    let back = back_cell(at, metadata);
    if let Some(&p) = powers.get(&back) {
        if p > 0 {
            return true;
        }
    }
    let block = world.block_at(back[0], back[1], back[2]);
    if is_dust(block.id) && power_of(block.metadata) > 0 {
        return true;
    }
    let button_live = button_sources.iter().any(|c| *c == back);
    if is_source(block.id, block.metadata, button_live) {
        return true;
    }
    if repeater_powered(block) && front_cell(back, block.metadata) == at {
        return true;
    }
    false
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
    let mut repeaters = Vec::new();
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
                if repeater_powered(block) {
                    repeaters.push((at, block.metadata));
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
    for (at, meta) in repeaters {
        let front = front_cell(at, meta);
        if dust.contains(&front) {
            queue.push_back((front, MAX_POWER));
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
        if repeater_powered(block) && front_cell(n, block.metadata) == at {
            return true;
        }
    }
    false
}

/// True when a lamp should stay lit from redstone (neighbor dust/source/repeater).
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
        if repeater_powered(block) && front_cell(n, block.metadata) == at {
            return true;
        }
    }
    false
}

/// Repeater cells whose powered bit should flip after their delay.
/// Returns `(cell, seconds_until_flip, want_on)`.
pub fn plan_repeater_schedules(
    world: &Cluster,
    focus: [i32; 3],
    radius: i32,
    button_sources: &[[i32; 3]],
    powers: &HashMap<[i32; 3], u8>,
) -> Vec<([i32; 3], f32, bool)> {
    let mut out = Vec::new();
    for dx in -radius..=radius {
        for dy in -radius..=radius {
            for dz in -radius..=radius {
                let at = [focus[0] + dx, focus[1] + dy, focus[2] + dz];
                let block = world.block_at(at[0], at[1], at[2]);
                if !is_repeater(block.id) {
                    continue;
                }
                let want = repeater_input_on(world, at, block.metadata, button_sources, powers);
                let on = repeater_powered(block);
                if want != on {
                    out.push((at, delay_seconds(block.metadata), want));
                }
            }
        }
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

    #[test]
    fn repeater_refreshes_front_dust_to_full() {
        let rep = make_repeater(0, 2, true);
        assert!(repeater_powered(rep));
        let cluster = world(vec![
            (2, 1, 1, rep),
            (3, 1, 1, Block::new("redstone_dust", 0)),
            (4, 1, 1, Block::new("redstone_dust", 0)),
        ]);
        let powers = plan_dust_power(&cluster, [3, 1, 1], 6, &[]);
        assert_eq!(powers.get(&[3, 1, 1]), Some(&15));
        assert_eq!(powers.get(&[4, 1, 1]), Some(&14));
    }

    #[test]
    fn repeater_is_a_diode() {
        let cluster = world(vec![
            (1, 1, 1, Block::new("redstone_dust", 0)),
            (2, 1, 1, make_repeater(0, 1, true)),
            (3, 1, 1, Block::new("redstone_dust", 0)),
        ]);
        let powers = plan_dust_power(&cluster, [2, 1, 1], 4, &[]);
        assert_eq!(powers.get(&[3, 1, 1]), Some(&15));
        assert_eq!(powers.get(&[1, 1, 1]), Some(&0));
    }

    #[test]
    fn delay_cycles_one_through_four() {
        let mut meta = with_facing(2);
        assert_eq!(delay_ticks(meta), 1);
        meta = cycle_delay(meta);
        assert_eq!(delay_ticks(meta), 2);
        meta = cycle_delay(meta);
        assert_eq!(delay_ticks(meta), 3);
        meta = cycle_delay(meta);
        assert_eq!(delay_ticks(meta), 4);
        meta = cycle_delay(meta);
        assert_eq!(delay_ticks(meta), 1);
        assert!((delay_seconds(with_delay(0, 4)) - 0.4).abs() < 0.001);
    }

    #[test]
    fn repeater_reads_back_input() {
        let rep = make_repeater(0, 1, false);
        let cluster = world(vec![
            (1, 1, 1, Block::new("lever", 1)),
            (2, 1, 1, rep),
        ]);
        let powers = HashMap::new();
        assert!(repeater_input_on(
            &cluster,
            [2, 1, 1],
            rep.metadata,
            &[],
            &powers
        ));
    }
}
