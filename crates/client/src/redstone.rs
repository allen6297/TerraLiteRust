//! Redstone dust, repeaters, and comparators.
//!
//! Placeable dust carries lever/button power up to [`MAX_POWER`] blocks.
//! Metadata `0..=15` is signal strength (0 = unlit). Powered dust (and the
//! source itself) can drive pistons, dispensers/droppers, and lamps.
//!
//! Repeaters are one-way diodes with a 1–4 tick delay; facing + delay fit in
//! the 4-bit metadata and powered state lives in [`Block::fill`].
//!
//! Comparators compare or subtract (click toggles mode), can read a chest's
//! fill behind them, and inject the resulting strength into the front dust.

use rebuild_world::{Block, CarriedSlot, Cluster, FILL_EMBER, FILL_NONE};
use std::collections::{HashMap, HashSet, VecDeque};

/// Strongest dust signal (Minecraft-style).
pub const MAX_POWER: u8 = 15;

/// Seconds a button keeps feeding the wire before signal drops.
pub const BUTTON_PULSE: f32 = 0.45;

/// One redstone tick in seconds (Minecraft ≈ 0.1s).
pub const TICK: f32 = 0.1;

/// Inventory slots TerraLite containers use (3×3).
pub const CONTAINER_SLOTS: u8 = 9;

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

pub fn is_comparator(id: &str) -> bool {
    id == "redstone_comparator"
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

/// Comparator tint: subtract mode reads warmer/torchier.
pub fn comparator_tint(metadata: u8) -> [f32; 3] {
    if subtract_mode(metadata) {
        [0.72, 0.28, 0.14]
    } else {
        [0.48, 0.40, 0.36]
    }
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

pub fn is_container_block(id: &str) -> bool {
    matches!(
        id,
        "chest"
            | "copper_chest"
            | "ender_chest"
            | "barrel"
            | "hopper"
            | "dispenser"
            | "dropper"
            | "shulker_box"
    )
}

// --- Shared facing (repeater + comparator) ---

pub fn facing_index(metadata: u8) -> u8 {
    metadata & 3
}

pub fn facing_dir(metadata: u8) -> [i32; 3] {
    FACE_DIRS[facing_index(metadata) as usize]
}

pub fn with_facing(facing: u8) -> u8 {
    facing & 3
}

/// Look → horizontal facing (output points along look).
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

pub fn back_cell(at: [i32; 3], metadata: u8) -> [i32; 3] {
    let d = facing_dir(metadata);
    [at[0] - d[0], at[1] - d[1], at[2] - d[2]]
}

pub fn front_cell(at: [i32; 3], metadata: u8) -> [i32; 3] {
    let d = facing_dir(metadata);
    [at[0] + d[0], at[1] + d[1], at[2] + d[2]]
}

/// Left / right cells relative to facing (for comparator sides).
pub fn side_cells(at: [i32; 3], metadata: u8) -> ([i32; 3], [i32; 3]) {
    let (lx, lz) = match facing_index(metadata) {
        0 => (0, 1),  // +X face → left +Z
        1 => (0, -1), // -X → left -Z
        2 => (-1, 0), // +Z → left -X
        _ => (1, 0),  // -Z → left +X
    };
    (
        [at[0] + lx, at[1], at[2] + lz],
        [at[0] - lx, at[1], at[2] - lz],
    )
}

// --- Repeater: facing bits 0..=1, delay bits 2..=3, powered via fill ---

pub fn delay_ticks(metadata: u8) -> u8 {
    ((metadata >> 2) & 3) + 1
}

pub fn repeater_powered(block: Block) -> bool {
    is_repeater(block.id) && block.fill != FILL_NONE
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

/// True when the cell behind a repeater is feeding it.
pub fn repeater_input_on(
    world: &Cluster,
    at: [i32; 3],
    metadata: u8,
    button_sources: &[[i32; 3]],
    powers: &HashMap<[i32; 3], u8>,
) -> bool {
    signal_at(
        world,
        back_cell(at, metadata),
        button_sources,
        powers,
        &HashMap::new(),
    ) > 0
}

// --- Comparator: facing bits 0..=1, subtract bit 2, lit fill when output > 0 ---

pub fn subtract_mode(metadata: u8) -> bool {
    metadata & 4 != 0
}

pub fn with_subtract(metadata: u8, on: bool) -> u8 {
    (metadata & !4) | if on { 4 } else { 0 }
}

pub fn toggle_subtract(metadata: u8) -> u8 {
    with_subtract(metadata, !subtract_mode(metadata))
}

pub fn make_comparator(facing: u8, subtract: bool, lit: bool) -> Block {
    let meta = with_subtract(with_facing(facing), subtract);
    let block = Block::new("redstone_comparator", meta);
    if lit {
        block.with_fill(FILL_EMBER)
    } else {
        block.with_fill(FILL_NONE)
    }
}

pub fn with_comparator_lit(block: Block, lit: bool) -> Block {
    Block::new("redstone_comparator", block.metadata).with_fill(if lit {
        FILL_EMBER
    } else {
        FILL_NONE
    })
}

pub fn comparator_lit(block: Block) -> bool {
    is_comparator(block.id) && block.fill != FILL_NONE
}

/// Minecraft-style container comparator signal from inventory slots.
pub fn container_signal(slots: &[CarriedSlot], stack_cap: impl Fn(&str) -> u16) -> u8 {
    let mut fullness = 0.0f32;
    let mut any = false;
    for slot in slots {
        if slot.count == 0 || slot.id.is_empty() || slot.id == "air" {
            continue;
        }
        any = true;
        let cap = stack_cap(&slot.id).max(1) as f32;
        fullness += (slot.count as f32 / cap).min(1.0);
    }
    if !any {
        return 0;
    }
    let avg = fullness / CONTAINER_SLOTS as f32;
    ((1.0 + avg * 14.0).floor() as u8).min(MAX_POWER)
}

/// Signal strength present at a cell (dust / source / diode front).
pub fn signal_at(
    world: &Cluster,
    at: [i32; 3],
    button_sources: &[[i32; 3]],
    powers: &HashMap<[i32; 3], u8>,
    comparator_out: &HashMap<[i32; 3], u8>,
) -> u8 {
    if let Some(&p) = powers.get(&at) {
        if p > 0 {
            return p;
        }
    }
    let block = world.block_at(at[0], at[1], at[2]);
    if is_dust(block.id) {
        return power_of(block.metadata);
    }
    let button_live = button_sources.iter().any(|c| *c == at);
    if is_source(block.id, block.metadata, button_live) {
        return MAX_POWER;
    }
    if repeater_powered(block) {
        // A powered repeater only feeds its front; being ON the repeater cell
        // itself is not a side/back wire signal.
        return 0;
    }
    if let Some(&p) = comparator_out.get(&at) {
        return p;
    }
    0
}

/// Side-wire strength into a comparator (dust / sources pointing in).
fn side_signal(
    world: &Cluster,
    side: [i32; 3],
    button_sources: &[[i32; 3]],
    powers: &HashMap<[i32; 3], u8>,
    comparator_out: &HashMap<[i32; 3], u8>,
) -> u8 {
    let mut best = signal_at(world, side, button_sources, powers, comparator_out);
    let block = world.block_at(side[0], side[1], side[2]);
    if repeater_powered(block) && front_cell(side, block.metadata) != side {
        // Powered repeater feeds only its front; if that front is our comparator
        // the side cell itself is the repeater — no side signal from sitting on it.
    }
    // Soft diode into this side cell from a neighbor repeater/comparator front.
    for step in STEPS {
        let n = [side[0] + step[0], side[1] + step[1], side[2] + step[2]];
        let nb = world.block_at(n[0], n[1], n[2]);
        if repeater_powered(nb) && front_cell(n, nb.metadata) == side {
            best = best.max(MAX_POWER);
        }
        if let Some(&p) = comparator_out.get(&n) {
            if p > 0 && is_comparator(nb.id) && front_cell(n, nb.metadata) == side {
                best = best.max(p);
            }
        }
    }
    best
}

/// Compute comparator output strength.
///
/// `back_signal` is the rear input (dust/source or container fill).
/// Compare: output = back if back >= max(sides), else 0.
/// Subtract: output = back.saturating_sub(max(sides)).
pub fn comparator_output(back: u8, side: u8, subtract: bool) -> u8 {
    if subtract {
        back.saturating_sub(side)
    } else if back >= side {
        back
    } else {
        0
    }
}

/// Plan outputs for every comparator near `focus`.
///
/// `back_override` supplies container fill signals keyed by comparator cell
/// (already resolved by the caller from the save).
pub fn plan_comparator_outputs(
    world: &Cluster,
    focus: [i32; 3],
    radius: i32,
    button_sources: &[[i32; 3]],
    powers: &HashMap<[i32; 3], u8>,
    back_override: &HashMap<[i32; 3], u8>,
) -> HashMap<[i32; 3], u8> {
    let mut out = HashMap::new();
    // Seed empty so side reads of chained comparators see prior outputs on a
    // second pass; one pass is enough for sides that are plain dust.
    let empty = HashMap::new();
    for dx in -radius..=radius {
        for dy in -radius..=radius {
            for dz in -radius..=radius {
                let at = [focus[0] + dx, focus[1] + dy, focus[2] + dz];
                let block = world.block_at(at[0], at[1], at[2]);
                if !is_comparator(block.id) {
                    continue;
                }
                let back = back_cell(at, block.metadata);
                let back_sig = back_override.get(&at).copied().unwrap_or_else(|| {
                    signal_at(world, back, button_sources, powers, &empty)
                });
                let (left, right) = side_cells(at, block.metadata);
                let side = side_signal(world, left, button_sources, powers, &empty)
                    .max(side_signal(world, right, button_sources, powers, &empty));
                let strength =
                    comparator_output(back_sig, side, subtract_mode(block.metadata));
                out.insert(at, strength);
            }
        }
    }
    out
}

/// Scan dust near `focus` and return desired power per dust cell.
pub fn plan_dust_power(
    world: &Cluster,
    focus: [i32; 3],
    radius: i32,
    button_sources: &[[i32; 3]],
) -> HashMap<[i32; 3], u8> {
    plan_dust_power_with(world, focus, radius, button_sources, &HashMap::new())
}

/// Like [`plan_dust_power`], also injecting comparator front strengths.
pub fn plan_dust_power_with(
    world: &Cluster,
    focus: [i32; 3],
    radius: i32,
    button_sources: &[[i32; 3]],
    comparator_out: &HashMap<[i32; 3], u8>,
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
    for (&at, &strength) in comparator_out {
        if strength == 0 {
            continue;
        }
        let block = world.block_at(at[0], at[1], at[2]);
        if !is_comparator(block.id) {
            continue;
        }
        let front = front_cell(at, block.metadata);
        if dust.contains(&front) {
            queue.push_back((front, strength));
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
    powered_devices_with(world, focus, radius, button_sources, powers, &HashMap::new())
}

pub fn powered_devices_with(
    world: &Cluster,
    focus: [i32; 3],
    radius: i32,
    button_sources: &[[i32; 3]],
    powers: &HashMap<[i32; 3], u8>,
    comparator_out: &HashMap<[i32; 3], u8>,
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
                if device_has_power(world, at, button_sources, powers, comparator_out) {
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
    comparator_out: &HashMap<[i32; 3], u8>,
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
        if let Some(&p) = comparator_out.get(&n) {
            if p > 0 && is_comparator(block.id) && front_cell(n, block.metadata) == at {
                return true;
            }
        }
    }
    false
}

/// True when a lamp should stay lit from redstone.
pub fn lamp_redstone_powered(
    world: &Cluster,
    at: [i32; 3],
    button_sources: &[[i32; 3]],
) -> bool {
    lamp_redstone_powered_with(world, at, button_sources, &HashMap::new())
}

pub fn lamp_redstone_powered_with(
    world: &Cluster,
    at: [i32; 3],
    button_sources: &[[i32; 3]],
    comparator_out: &HashMap<[i32; 3], u8>,
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
        if let Some(&p) = comparator_out.get(&n) {
            if p > 0 && is_comparator(block.id) && front_cell(n, block.metadata) == at {
                return true;
            }
        }
        // Lit comparator fill also counts when facing the lamp.
        if comparator_lit(block) && front_cell(n, block.metadata) == at {
            return true;
        }
    }
    false
}

/// Repeater cells whose powered bit should flip after their delay.
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

    #[test]
    fn compare_passes_when_back_beats_side() {
        assert_eq!(comparator_output(10, 4, false), 10);
        assert_eq!(comparator_output(3, 5, false), 0);
        assert_eq!(comparator_output(7, 7, false), 7);
    }

    #[test]
    fn subtract_mode_cuts_side_from_back() {
        assert_eq!(comparator_output(10, 4, true), 6);
        assert_eq!(comparator_output(3, 5, true), 0);
    }

    #[test]
    fn toggle_subtract_flips_mode() {
        let meta = with_facing(1);
        assert!(!subtract_mode(meta));
        let sub = toggle_subtract(meta);
        assert!(subtract_mode(sub));
        assert_eq!(facing_index(sub), 1);
    }

    #[test]
    fn comparator_injects_front_strength() {
        let comp = make_comparator(0, false, true);
        let cluster = world(vec![
            (2, 1, 1, comp),
            (3, 1, 1, Block::new("redstone_dust", 0)),
            (4, 1, 1, Block::new("redstone_dust", 0)),
        ]);
        let mut outs = HashMap::new();
        outs.insert([2, 1, 1], 9u8);
        let powers = plan_dust_power_with(&cluster, [3, 1, 1], 6, &[], &outs);
        assert_eq!(powers.get(&[3, 1, 1]), Some(&9));
        assert_eq!(powers.get(&[4, 1, 1]), Some(&8));
    }

    #[test]
    fn container_signal_scales_with_fill() {
        let empty = container_signal(&[], |_| 64);
        assert_eq!(empty, 0);
        let one = container_signal(
            &[CarriedSlot {
                slot: 0,
                id: "dirt".into(),
                count: 64,
            }],
            |_| 64,
        );
        assert!(one >= 1 && one <= 3, "one full stack of nine → small signal {one}");
        let full: Vec<_> = (0..9)
            .map(|slot| CarriedSlot {
                slot,
                id: "dirt".into(),
                count: 64,
            })
            .collect();
        assert_eq!(container_signal(&full, |_| 64), 15);
    }

    #[test]
    fn comparator_reads_override_back_from_chest() {
        let comp = make_comparator(0, false, false);
        let cluster = world(vec![
            (1, 1, 1, Block::new("chest", 0)),
            (2, 1, 1, comp),
            (3, 1, 1, Block::new("redstone_dust", 0)),
        ]);
        let powers = HashMap::new();
        let mut backs = HashMap::new();
        backs.insert([2, 1, 1], 11u8);
        let outs = plan_comparator_outputs(&cluster, [2, 1, 1], 4, &[], &powers, &backs);
        assert_eq!(outs.get(&[2, 1, 1]), Some(&11));
        let dust = plan_dust_power_with(&cluster, [2, 1, 1], 4, &[], &outs);
        assert_eq!(dust.get(&[3, 1, 1]), Some(&11));
    }
}
