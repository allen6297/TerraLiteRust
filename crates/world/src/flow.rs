//! Slow compound fill seep between adjacent host blocks.
//!
//! Water and ember copy into neighboring empty compound hosts (pipe / conduit
//! feel). Pumps gate and push one-way when powered. One pulse plans a small
//! number of spreads near a focus so a large world stays cheap.

use std::collections::HashSet;

use crate::changes::CellChange;
use crate::chunk::{
    BATTERY_MAX, FILL_EMBER, FILL_NONE, FILL_WATER, TANK_MAX, battery_at, battery_level,
    buffer_level, buffer_max, compound_accepts_meta, ember_conducts, generator_on, is_battery,
    is_buffer, is_drain, is_generator, is_lamp, is_pipe, is_pump, is_spout, is_tank, is_wire,
    lamp_at, lamp_on, pump_dir, pump_on, tank_at, tank_level,
};
use crate::cluster::Cluster;

/// Neighbors used when tracing ember lamp power through wire networks.
const POWER_NEIGHBORS: [[i32; 3]; 6] = [
    [1, 0, 0],
    [-1, 0, 0],
    [0, 1, 0],
    [0, -1, 0],
    [0, 0, 1],
    [0, 0, -1],
];

/// Water: down first, then horizontal, then up.
const WATER_NEIGHBORS: [[i32; 3]; 6] = [
    [0, -1, 0],
    [1, 0, 0],
    [-1, 0, 0],
    [0, 0, 1],
    [0, 0, -1],
    [0, 1, 0],
];

/// Ember: horizontal conduit first, then down, then up.
const EMBER_NEIGHBORS: [[i32; 3]; 6] = [
    [1, 0, 0],
    [-1, 0, 0],
    [0, 0, 1],
    [0, 0, -1],
    [0, -1, 0],
    [0, 1, 0],
];

fn neighbors_for(fill: u8) -> &'static [[i32; 3]; 6] {
    match fill {
        FILL_EMBER => &EMBER_NEIGHBORS,
        _ => &WATER_NEIGHBORS,
    }
}

/// Max filled hosts considered in one pulse near the focus.
const SOURCE_CAP: usize = 32;

fn accepts(block: crate::chunk::Block, fill: u8) -> bool {
    compound_accepts_meta(block.id, block.metadata, fill)
}

/// True when `dest` can take a unit of `src.fill` (empty host or underfilled buffer).
fn can_receive(dest: crate::chunk::Block, fill: u8) -> bool {
    if !accepts(dest, fill) {
        return false;
    }
    if dest.fill == FILL_NONE {
        return true;
    }
    is_buffer(dest.id) && dest.fill == fill && buffer_level(dest) < buffer_max(dest.id)
}

/// Next dest block after receiving one unit of `fill`.
fn receive_fill(dest: crate::chunk::Block, fill: u8) -> crate::chunk::Block {
    if is_battery(dest.id) {
        let level = if dest.fill == FILL_NONE {
            1
        } else {
            buffer_level(dest).saturating_add(1).min(BATTERY_MAX)
        };
        return battery_at(level);
    }
    if is_tank(dest.id) {
        let level = if dest.fill == FILL_NONE {
            1
        } else {
            buffer_level(dest).saturating_add(1).min(buffer_max(dest.id))
        };
        return tank_at(fill, level);
    }
    dest.with_fill(fill)
}

/// Drawdown after a leveled buffer sources one unit. `None` when not a buffer.
fn buffer_after_source(src: crate::chunk::Block) -> Option<crate::chunk::Block> {
    if !is_buffer(src.id) {
        return None;
    }
    let level = buffer_level(src);
    if is_battery(src.id) {
        return if level <= 1 {
            Some(crate::chunk::Block::new("battery", 0))
        } else {
            Some(battery_at(level - 1))
        };
    }
    if level <= 1 {
        Some(crate::chunk::Block::new("tank", 0))
    } else {
        Some(tank_at(src.fill, level - 1))
    }
}

/// True when fill may move from `src` into `dest` (pump one-way + tank buffer).
fn seep_allowed(
    src: crate::chunk::Block,
    dest: crate::chunk::Block,
    dx: i32,
    dy: i32,
    dz: i32,
) -> bool {
    if !can_receive(dest, src.fill) {
        return false;
    }
    if is_pump(src.id) {
        if !pump_on(src.metadata) {
            return false;
        }
        let dir = pump_dir(src.metadata);
        return [dx, dy, dz] == dir;
    }
    if is_pump(dest.id) {
        if !pump_on(dest.metadata) {
            return false;
        }
        // Enter only from the intake (opposite facing).
        let dir = pump_dir(dest.metadata);
        return [dx, dy, dz] == dir;
    }
    true
}

/// Plan fill seeps into adjacent empty compound hosts around `focus`.
///
/// Sources keep their fill (pipe pressure). Destinations that would be claimed
/// twice in the same pulse are skipped. Returns at most `max_spreads` changes.
pub fn plan_compound_seeps(
    world: &Cluster,
    focus: [i32; 3],
    radius: i32,
    max_spreads: usize,
) -> Vec<CellChange> {
    if max_spreads == 0 || radius < 0 {
        return Vec::new();
    }
    let mut sources = Vec::new();
    let min_cx = (focus[0] - radius).div_euclid(16);
    let max_cx = (focus[0] + radius).div_euclid(16);
    let min_cz = (focus[2] - radius).div_euclid(16);
    let max_cz = (focus[2] + radius).div_euclid(16);
    for cz in min_cz..=max_cz {
        for cx in min_cx..=max_cx {
            let Some(chunk) = world.get(cx, cz) else {
                continue;
            };
            chunk.for_each_block(|x, y, z, block| {
                if sources.len() >= SOURCE_CAP {
                    return;
                }
                if !accepts(block, block.fill) {
                    return;
                }
                if (x - focus[0]).abs() > radius
                    || (y - focus[1]).abs() > radius
                    || (z - focus[2]).abs() > radius
                {
                    return;
                }
                let prefer = is_pipe(block.id)
                    || is_wire(block.id)
                    || is_pump(block.id)
                    || is_buffer(block.id);
                sources.push((x, y, z, block, prefer));
            });
        }
    }
    // Buffers / pumps / conduits first, then lower cells so water drains through runs.
    sources.sort_by_key(|&(x, y, z, _, prefer)| {
        (
            !prefer,
            y,
            (x - focus[0]).abs() + (z - focus[2]).abs(),
            x,
            z,
        )
    });

    let mut claimed = HashSet::new();
    let mut out = Vec::new();
    let mut dests = 0usize;
    for (x, y, z, src, _) in sources {
        if dests >= max_spreads {
            break;
        }
        let mut candidates = Vec::new();
        for [dx, dy, dz] in neighbors_for(src.fill) {
            let nx = x + dx;
            let ny = y + dy;
            let nz = z + dz;
            if claimed.contains(&(nx, ny, nz)) {
                continue;
            }
            let dest = world.block_at(nx, ny, nz);
            if !seep_allowed(src, dest, *dx, *dy, *dz) {
                continue;
            }
            let prefer = is_pipe(dest.id)
                || is_wire(dest.id)
                || is_pump(dest.id)
                || is_buffer(dest.id);
            // Ember prefers batteries; water prefers tanks; then other conduits.
            let rank = if src.fill == FILL_EMBER && is_battery(dest.id) {
                0u8
            } else if src.fill == FILL_WATER && is_tank(dest.id) {
                0
            } else if is_buffer(dest.id) {
                1
            } else if prefer {
                2
            } else {
                3
            };
            candidates.push((rank, nx, ny, nz, dest));
        }
        candidates.sort_by_key(|&(rank, ..)| rank);
        for (_rank, nx, ny, nz, dest) in candidates {
            if dests >= max_spreads {
                break;
            }
            if !claimed.insert((nx, ny, nz)) {
                continue;
            }
            out.push(CellChange {
                x: nx,
                y: ny,
                z: nz,
                block: receive_fill(dest, src.fill),
            });
            dests += 1;
            if let Some(after) = buffer_after_source(src) {
                out.push(CellChange {
                    x,
                    y,
                    z,
                    block: after,
                });
                // One unit out of a buffer per pulse.
                break;
            }
        }
    }
    out
}

/// Powered pumps push fill from intake to outlet in one step (pressure keep).
pub fn plan_pump_pushes(
    world: &Cluster,
    focus: [i32; 3],
    radius: i32,
    max_pushes: usize,
) -> Vec<CellChange> {
    if max_pushes == 0 || radius < 0 {
        return Vec::new();
    }
    let mut pumps = Vec::new();
    let min_cx = (focus[0] - radius).div_euclid(16);
    let max_cx = (focus[0] + radius).div_euclid(16);
    let min_cz = (focus[2] - radius).div_euclid(16);
    let max_cz = (focus[2] + radius).div_euclid(16);
    for cz in min_cz..=max_cz {
        for cx in min_cx..=max_cx {
            let Some(chunk) = world.get(cx, cz) else {
                continue;
            };
            chunk.for_each_block(|x, y, z, block| {
                if !is_pump(block.id) || !pump_on(block.metadata) {
                    return;
                }
                if (x - focus[0]).abs() > radius
                    || (y - focus[1]).abs() > radius
                    || (z - focus[2]).abs() > radius
                {
                    return;
                }
                pumps.push((x, y, z, block.metadata));
            });
        }
    }
    pumps.sort_by_key(|&(x, y, z, _)| {
        (
            (x - focus[0]).abs() + (z - focus[2]).abs(),
            y,
            x,
            z,
        )
    });

    let mut claimed = HashSet::new();
    let mut out = Vec::new();
    for (x, y, z, meta) in pumps {
        if out.len() >= max_pushes {
            break;
        }
        let dir = pump_dir(meta);
        let ix = x - dir[0];
        let iy = y - dir[1];
        let iz = z - dir[2];
        let ox = x + dir[0];
        let oy = y + dir[1];
        let oz = z + dir[2];
        if !claimed.insert((ox, oy, oz)) {
            continue;
        }
        let input = world.block_at(ix, iy, iz);
        let output = world.block_at(ox, oy, oz);
        if input.fill == FILL_NONE {
            continue;
        }
        if !can_receive(output, input.fill) {
            continue;
        }
        // Closed-valve pumps never appear here; powered outlet must accept.
        out.push(CellChange {
            x: ox,
            y: oy,
            z: oz,
            block: receive_fill(output, input.fill),
        });
        if let Some(after) = buffer_after_source(input) {
            out.push(CellChange {
                x: ix,
                y: iy,
                z: iz,
                block: after,
            });
        }
    }
    out
}

/// Trace the ember power network from a lamp. Returns whether the lamp should
/// light, and an optional battery cell to draw from when `drain` is set.
fn lamp_power(
    world: &Cluster,
    x: i32,
    y: i32,
    z: i32,
) -> (bool, Option<(i32, i32, i32, crate::chunk::Block)>) {
    // Adjacent redstone dust (power > 0) or a flipped lever lights the lamp.
    for [dx, dy, dz] in POWER_NEIGHBORS {
        let block = world.block_at(x + dx, y + dy, z + dz);
        if block.id == "redstone_dust" && (block.metadata & 15) > 0 {
            return (true, None);
        }
        if block.id == "lever" && block.metadata & 1 != 0 {
            return (true, None);
        }
        if block.id == "redstone_repeater" && block.fill != 0 {
            return (true, None);
        }
        if block.id == "redstone_comparator" && block.fill != 0 {
            return (true, None);
        }
    }
    let mut seen = HashSet::new();
    let mut queue = Vec::new();
    seen.insert((x, y, z));
    queue.push((x, y, z));
    let mut powered = false;
    let mut battery: Option<(i32, i32, i32, crate::chunk::Block)> = None;
    let mut head = 0usize;
    while head < queue.len() && head < 128 {
        let (cx, cy, cz) = queue[head];
        head += 1;
        for [dx, dy, dz] in POWER_NEIGHBORS {
            let nx = cx + dx;
            let ny = cy + dy;
            let nz = cz + dz;
            if !seen.insert((nx, ny, nz)) {
                continue;
            }
            let block = world.block_at(nx, ny, nz);
            if !ember_conducts(block) {
                continue;
            }
            queue.push((nx, ny, nz));
            if is_battery(block.id) {
                let level = battery_level(block);
                if level > 0 {
                    powered = true;
                    let better = match battery {
                        Some((_, _, _, cur)) => battery_level(cur) < level,
                        None => true,
                    };
                    if better {
                        battery = Some((nx, ny, nz, block));
                    }
                }
            } else if is_generator(block.id) && generator_on(block.metadata) {
                powered = true;
            } else if block.fill == FILL_EMBER {
                powered = true;
            }
        }
    }
    (powered, battery)
}

/// Plan ember lamp on/off updates (and optional battery drain) near `focus`.
///
/// A lamp lights when a wire/battery network reaches ember fill or a charged
/// battery. When `drain` is true and any planned lamp stays lit off a battery,
/// one charge unit is drawn from the strongest connected battery.
pub fn plan_ember_lamps(
    world: &Cluster,
    focus: [i32; 3],
    radius: i32,
    max_lamps: usize,
    drain: bool,
) -> Vec<CellChange> {
    if max_lamps == 0 || radius < 0 {
        return Vec::new();
    }
    let mut lamps = Vec::new();
    let min_cx = (focus[0] - radius).div_euclid(16);
    let max_cx = (focus[0] + radius).div_euclid(16);
    let min_cz = (focus[2] - radius).div_euclid(16);
    let max_cz = (focus[2] + radius).div_euclid(16);
    for cz in min_cz..=max_cz {
        for cx in min_cx..=max_cx {
            let Some(chunk) = world.get(cx, cz) else {
                continue;
            };
            chunk.for_each_block(|x, y, z, block| {
                if !is_lamp(block.id) {
                    return;
                }
                if (x - focus[0]).abs() > radius
                    || (y - focus[1]).abs() > radius
                    || (z - focus[2]).abs() > radius
                {
                    return;
                }
                lamps.push((x, y, z, block.metadata));
            });
        }
    }
    lamps.sort_by_key(|&(x, y, z, _)| {
        (
            (x - focus[0]).abs() + (z - focus[2]).abs(),
            y,
            x,
            z,
        )
    });

    let mut out = Vec::new();
    let mut drain_battery: Option<(i32, i32, i32, crate::chunk::Block)> = None;
    let mut any_lit = false;
    for (x, y, z, meta) in lamps {
        if out.len() >= max_lamps {
            break;
        }
        let (powered, battery) = lamp_power(world, x, y, z);
        let on = lamp_on(meta);
        if powered != on {
            out.push(CellChange {
                x,
                y,
                z,
                block: lamp_at(powered),
            });
        }
        if powered {
            any_lit = true;
            if let Some((bx, by, bz, block)) = battery {
                let better = match drain_battery {
                    Some((_, _, _, cur)) => battery_level(cur) < battery_level(block),
                    None => true,
                };
                if better {
                    drain_battery = Some((bx, by, bz, block));
                }
            }
        }
    }
    if drain && any_lit {
        if let Some((bx, by, bz, block)) = drain_battery {
            let level = battery_level(block);
            let after = if level <= 1 {
                crate::chunk::Block::new("battery", 0)
            } else {
                battery_at(level - 1)
            };
            out.push(CellChange {
                x: bx,
                y: by,
                z: bz,
                block: after,
            });
        }
    }
    out
}

/// Apply [`plan_ember_lamps`] into `world`. Returns how many cells changed.
pub fn apply_ember_lamps(
    world: &mut Cluster,
    focus: [i32; 3],
    radius: i32,
    max_lamps: usize,
    drain: bool,
) -> usize {
    let planned = plan_ember_lamps(world, focus, radius, max_lamps, drain);
    let count = planned.len();
    for cell in planned {
        world.set_block(cell.x, cell.y, cell.z, cell.block);
    }
    count
}

/// BFS from a running generator: pick the emptiest connected battery under cap,
/// or the first empty wire on the network when no battery needs charge.
fn generator_charge_target(
    world: &Cluster,
    x: i32,
    y: i32,
    z: i32,
) -> Option<(i32, i32, i32, crate::chunk::Block)> {
    let mut seen = HashSet::new();
    let mut queue = Vec::new();
    seen.insert((x, y, z));
    queue.push((x, y, z));
    let mut best_battery: Option<(i32, i32, i32, crate::chunk::Block)> = None;
    let mut empty_wire: Option<(i32, i32, i32, crate::chunk::Block)> = None;
    let mut head = 0usize;
    while head < queue.len() && head < 128 {
        let (cx, cy, cz) = queue[head];
        head += 1;
        for [dx, dy, dz] in POWER_NEIGHBORS {
            let nx = cx + dx;
            let ny = cy + dy;
            let nz = cz + dz;
            if !seen.insert((nx, ny, nz)) {
                continue;
            }
            let block = world.block_at(nx, ny, nz);
            // Wires and batteries always join; other cells need ember_conducts.
            let joins = is_wire(block.id) || is_battery(block.id) || ember_conducts(block);
            if !joins {
                continue;
            }
            queue.push((nx, ny, nz));
            if is_battery(block.id) {
                let level = battery_level(block);
                if level < BATTERY_MAX {
                    let better = match best_battery {
                        Some((_, _, _, cur)) => battery_level(cur) > level,
                        None => true,
                    };
                    if better {
                        best_battery = Some((nx, ny, nz, block));
                    }
                }
            } else if is_wire(block.id) && block.fill == FILL_NONE && empty_wire.is_none() {
                empty_wire = Some((nx, ny, nz, block));
            }
        }
    }
    best_battery.or(empty_wire)
}

/// True when any face neighbor is a campfire (optional fuel boost).
fn campfire_beside(world: &Cluster, x: i32, y: i32, z: i32) -> bool {
    POWER_NEIGHBORS.iter().any(|[dx, dy, dz]| {
        world.block_at(x + dx, y + dy, z + dz).id == "campfire"
    })
}

/// Plan ember generator charges near `focus`.
///
/// Each running generator slowly adds one ember unit to the emptiest connected
/// battery (or fills one empty wire). Free tick for demo; a neighboring
/// campfire boosts the pulse by one extra charge unit.
pub fn plan_ember_charges(
    world: &Cluster,
    focus: [i32; 3],
    radius: i32,
    max_gens: usize,
) -> Vec<CellChange> {
    if max_gens == 0 || radius < 0 {
        return Vec::new();
    }
    let mut gens = Vec::new();
    let min_cx = (focus[0] - radius).div_euclid(16);
    let max_cx = (focus[0] + radius).div_euclid(16);
    let min_cz = (focus[2] - radius).div_euclid(16);
    let max_cz = (focus[2] + radius).div_euclid(16);
    for cz in min_cz..=max_cz {
        for cx in min_cx..=max_cx {
            let Some(chunk) = world.get(cx, cz) else {
                continue;
            };
            chunk.for_each_block(|x, y, z, block| {
                if !is_generator(block.id) || !generator_on(block.metadata) {
                    return;
                }
                if (x - focus[0]).abs() > radius
                    || (y - focus[1]).abs() > radius
                    || (z - focus[2]).abs() > radius
                {
                    return;
                }
                gens.push((x, y, z));
            });
        }
    }
    gens.sort_by_key(|&(x, y, z)| {
        (
            (x - focus[0]).abs() + (z - focus[2]).abs(),
            y,
            x,
            z,
        )
    });

    let mut claimed = HashSet::new();
    let mut out = Vec::new();
    for (x, y, z) in gens {
        if out.len() >= max_gens {
            break;
        }
        let Some((tx, ty, tz, block)) = generator_charge_target(world, x, y, z) else {
            continue;
        };
        if !claimed.insert((tx, ty, tz)) {
            continue;
        }
        let boost = if campfire_beside(world, x, y, z) { 2 } else { 1 };
        let after = if is_battery(block.id) {
            let level = battery_level(block);
            battery_at(level.saturating_add(boost).min(BATTERY_MAX))
        } else if is_wire(block.id) {
            block.with_fill(FILL_EMBER)
        } else {
            continue;
        };
        out.push(CellChange {
            x: tx,
            y: ty,
            z: tz,
            block: after,
        });
    }
    out
}

/// Apply [`plan_ember_charges`] into `world`. Returns how many cells changed.
pub fn apply_ember_charges(
    world: &mut Cluster,
    focus: [i32; 3],
    radius: i32,
    max_gens: usize,
) -> usize {
    let planned = plan_ember_charges(world, focus, radius, max_gens);
    let count = planned.len();
    for cell in planned {
        world.set_block(cell.x, cell.y, cell.z, cell.block);
    }
    count
}

/// True when `src` can feed a spout (water tank with level, or water-filled host).
fn spout_source(block: crate::chunk::Block) -> Option<(u8, crate::chunk::Block)> {
    if is_tank(block.id) && block.fill == FILL_WATER {
        let level = tank_level(block);
        if level == 0 {
            return None;
        }
        let after = if level <= 1 {
            crate::chunk::Block::new("tank", 0)
        } else {
            tank_at(FILL_WATER, level - 1)
        };
        return Some((level, after));
    }
    if block.fill == FILL_WATER
        && (is_pipe(block.id)
            || is_pump(block.id)
            || (compound_accepts_meta(block.id, block.metadata, FILL_WATER)
                && !is_spout(block.id)
                && !is_drain(block.id)
                && !is_lamp(block.id)
                && !is_battery(block.id)))
    {
        return Some((1, block.with_fill(FILL_NONE)));
    }
    None
}

/// Plan spout drains: open spouts dump water into air ahead of facing.
///
/// Intake is the opposite face (pipe / tank). Tank level `> 0` is pressurized —
/// the spout sprays one extra cell along facing when the first outlet is free
/// or already water. Each pulse draws one unit from the intake source.
pub fn plan_water_spouts(
    world: &Cluster,
    focus: [i32; 3],
    radius: i32,
    max_spouts: usize,
) -> Vec<CellChange> {
    if max_spouts == 0 || radius < 0 {
        return Vec::new();
    }
    let mut spouts = Vec::new();
    let min_cx = (focus[0] - radius).div_euclid(16);
    let max_cx = (focus[0] + radius).div_euclid(16);
    let min_cz = (focus[2] - radius).div_euclid(16);
    let max_cz = (focus[2] + radius).div_euclid(16);
    for cz in min_cz..=max_cz {
        for cx in min_cx..=max_cx {
            let Some(chunk) = world.get(cx, cz) else {
                continue;
            };
            chunk.for_each_block(|x, y, z, block| {
                if !is_spout(block.id) || !pump_on(block.metadata) {
                    return;
                }
                if (x - focus[0]).abs() > radius
                    || (y - focus[1]).abs() > radius
                    || (z - focus[2]).abs() > radius
                {
                    return;
                }
                spouts.push((x, y, z, block.metadata));
            });
        }
    }
    spouts.sort_by_key(|&(x, y, z, _)| {
        (
            (x - focus[0]).abs() + (z - focus[2]).abs(),
            y,
            x,
            z,
        )
    });

    let mut claimed = HashSet::new();
    let mut out = Vec::new();
    let mut drained = 0usize;
    for (x, y, z, meta) in spouts {
        if drained >= max_spouts {
            break;
        }
        let dir = pump_dir(meta);
        let ix = x - dir[0];
        let iy = y - dir[1];
        let iz = z - dir[2];
        let ox = x + dir[0];
        let oy = y + dir[1];
        let oz = z + dir[2];
        let intake = world.block_at(ix, iy, iz);
        let Some((pressure, after_src)) = spout_source(intake) else {
            continue;
        };
        let outlet = world.block_at(ox, oy, oz);
        let mut placed = false;
        if outlet.is_air() && claimed.insert((ox, oy, oz)) {
            out.push(CellChange {
                x: ox,
                y: oy,
                z: oz,
                block: crate::chunk::Block::new("water", 0),
            });
            placed = true;
        }
        // Pressurized tank spray (level > 0): one more cell along facing.
        // Pipe pressure is 1 and does not spray; only tanks do.
        if is_tank(intake.id) && pressure > 0 {
            let sx = ox + dir[0];
            let sy = oy + dir[1];
            let sz = oz + dir[2];
            let spray = world.block_at(sx, sy, sz);
            let first_ok = outlet.is_air() || outlet.id == "water";
            if first_ok && spray.is_air() && claimed.insert((sx, sy, sz)) {
                out.push(CellChange {
                    x: sx,
                    y: sy,
                    z: sz,
                    block: crate::chunk::Block::new("water", 0),
                });
                placed = true;
            }
        }
        if !placed {
            continue;
        }
        out.push(CellChange {
            x: ix,
            y: iy,
            z: iz,
            block: after_src,
        });
        drained += 1;
    }
    out
}

/// Apply [`plan_water_spouts`] into `world`. Returns how many cells changed.
pub fn apply_water_spouts(
    world: &mut Cluster,
    focus: [i32; 3],
    radius: i32,
    max_spouts: usize,
) -> usize {
    let planned = plan_water_spouts(world, focus, radius, max_spouts);
    let count = planned.len();
    for cell in planned {
        world.set_block(cell.x, cell.y, cell.z, cell.block);
    }
    count
}

/// True when `dest` can take one unit of water from a drain.
fn drain_sink(block: crate::chunk::Block) -> Option<crate::chunk::Block> {
    if is_tank(block.id) {
        if block.fill == FILL_NONE {
            return Some(tank_at(FILL_WATER, 1));
        }
        if block.fill == FILL_WATER {
            let level = tank_level(block);
            if level < TANK_MAX {
                return Some(tank_at(FILL_WATER, level + 1));
            }
        }
        return None;
    }
    if block.fill == FILL_NONE
        && (is_pipe(block.id)
            || is_pump(block.id)
            || (compound_accepts_meta(block.id, block.metadata, FILL_WATER)
                && !is_spout(block.id)
                && !is_drain(block.id)
                && !is_lamp(block.id)
                && !is_battery(block.id)))
    {
        return Some(block.with_fill(FILL_WATER));
    }
    None
}

/// Plan drain intakes: open drains pull world water ahead of facing into the
/// opposite pipe / tank. Each pulse moves one water cell.
pub fn plan_water_drains(
    world: &Cluster,
    focus: [i32; 3],
    radius: i32,
    max_drains: usize,
) -> Vec<CellChange> {
    if max_drains == 0 || radius < 0 {
        return Vec::new();
    }
    let mut drains = Vec::new();
    let min_cx = (focus[0] - radius).div_euclid(16);
    let max_cx = (focus[0] + radius).div_euclid(16);
    let min_cz = (focus[2] - radius).div_euclid(16);
    let max_cz = (focus[2] + radius).div_euclid(16);
    for cz in min_cz..=max_cz {
        for cx in min_cx..=max_cx {
            let Some(chunk) = world.get(cx, cz) else {
                continue;
            };
            chunk.for_each_block(|x, y, z, block| {
                if !is_drain(block.id) || !pump_on(block.metadata) {
                    return;
                }
                if (x - focus[0]).abs() > radius
                    || (y - focus[1]).abs() > radius
                    || (z - focus[2]).abs() > radius
                {
                    return;
                }
                drains.push((x, y, z, block.metadata));
            });
        }
    }
    drains.sort_by_key(|&(x, y, z, _)| {
        (
            (x - focus[0]).abs() + (z - focus[2]).abs(),
            y,
            x,
            z,
        )
    });

    let mut claimed = HashSet::new();
    let mut out = Vec::new();
    let mut pulled = 0usize;
    for (x, y, z, meta) in drains {
        if pulled >= max_drains {
            break;
        }
        let dir = pump_dir(meta);
        // Facing = world intake; opposite = pipe/tank sink.
        let ix = x + dir[0];
        let iy = y + dir[1];
        let iz = z + dir[2];
        let ox = x - dir[0];
        let oy = y - dir[1];
        let oz = z - dir[2];
        let intake = world.block_at(ix, iy, iz);
        if intake.id != "water" || !claimed.insert((ix, iy, iz)) {
            continue;
        }
        let sink = world.block_at(ox, oy, oz);
        let Some(after_sink) = drain_sink(sink) else {
            continue;
        };
        if !claimed.insert((ox, oy, oz)) {
            continue;
        }
        out.push(CellChange {
            x: ix,
            y: iy,
            z: iz,
            block: crate::chunk::Block::AIR,
        });
        out.push(CellChange {
            x: ox,
            y: oy,
            z: oz,
            block: after_sink,
        });
        pulled += 1;
    }
    out
}

/// Apply [`plan_water_drains`] into `world`. Returns how many cells changed.
pub fn apply_water_drains(
    world: &mut Cluster,
    focus: [i32; 3],
    radius: i32,
    max_drains: usize,
) -> usize {
    let planned = plan_water_drains(world, focus, radius, max_drains);
    let count = planned.len();
    for cell in planned {
        world.set_block(cell.x, cell.y, cell.z, cell.block);
    }
    count
}

/// Apply [`plan_compound_seeps`] into `world`. Returns how many cells filled.
pub fn apply_compound_seeps(
    world: &mut Cluster,
    focus: [i32; 3],
    radius: i32,
    max_spreads: usize,
) -> usize {
    let planned = plan_compound_seeps(world, focus, radius, max_spreads);
    let count = planned.len();
    for cell in planned {
        world.set_block(cell.x, cell.y, cell.z, cell.block);
    }
    count
}

/// Apply [`plan_pump_pushes`] into `world`. Returns how many cells filled.
pub fn apply_pump_pushes(
    world: &mut Cluster,
    focus: [i32; 3],
    radius: i32,
    max_pushes: usize,
) -> usize {
    let planned = plan_pump_pushes(world, focus, radius, max_pushes);
    let count = planned.len();
    for cell in planned {
        world.set_block(cell.x, cell.y, cell.z, cell.block);
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunk::{Block, Chunk, FILL_EMBER, FILL_WATER, pump_meta};
    use crate::cluster::Cluster;

    fn world(blocks: &[(i32, i32, i32, Block)]) -> Cluster {
        let local: Vec<(u8, u16, u8, Block)> = blocks
            .iter()
            .map(|&(x, y, z, block)| (x as u8, y as u16, z as u8, block))
            .collect();
        let chunk = Chunk::from_blocks(0, 0, local).unwrap();
        Cluster::try_from_chunks(0, 0, [chunk]).unwrap()
    }

    #[test]
    fn water_seeps_into_an_adjacent_empty_fence() {
        let mut cluster = world(&[
            (2, 1, 2, Block::new("copper_grate", 0).with_fill(FILL_WATER)),
            (3, 1, 2, Block::new("fence", 0)),
        ]);
        let planned = plan_compound_seeps(&cluster, [2, 1, 2], 8, 4);
        assert_eq!(planned.len(), 1);
        assert_eq!(planned[0].x, 3);
        assert_eq!(planned[0].block.fill, FILL_WATER);
        assert_eq!(apply_compound_seeps(&mut cluster, [2, 1, 2], 8, 4), 1);
        assert_eq!(cluster.block_at(3, 1, 2).fill, FILL_WATER);
        assert_eq!(
            cluster.block_at(2, 1, 2).fill,
            FILL_WATER,
            "source keeps pressure"
        );
    }

    #[test]
    fn ember_seeps_along_a_short_pipe() {
        let mut cluster = world(&[
            (1, 1, 1, Block::new("glass", 0).with_fill(FILL_EMBER)),
            (2, 1, 1, Block::new("fence", 0)),
            (3, 1, 1, Block::new("fence", 0)),
        ]);
        assert_eq!(apply_compound_seeps(&mut cluster, [2, 1, 1], 8, 1), 1);
        assert_eq!(cluster.block_at(2, 1, 1).fill, FILL_EMBER);
        assert_eq!(cluster.block_at(3, 1, 1).fill, FILL_NONE);
        assert_eq!(apply_compound_seeps(&mut cluster, [2, 1, 1], 8, 1), 1);
        assert_eq!(cluster.block_at(3, 1, 1).fill, FILL_EMBER);
    }

    #[test]
    fn seep_is_capped_and_skips_solid_stone() {
        let cluster = world(&[
            (2, 1, 2, Block::new("copper_grate", 0).with_fill(FILL_WATER)),
            (3, 1, 2, Block::new("stone", 0)),
            (4, 1, 2, Block::new("fence", 0)),
        ]);
        let planned = plan_compound_seeps(&cluster, [2, 1, 2], 8, 4);
        assert!(
            planned.is_empty(),
            "stone blocks the only ortho neighbor toward the fence"
        );
        let open = world(&[
            (2, 1, 2, Block::new("copper_grate", 0).with_fill(FILL_WATER)),
            (3, 1, 2, Block::new("fence", 0)),
            (2, 1, 3, Block::new("fence", 0)),
            (1, 1, 2, Block::new("leaves", 0)),
        ]);
        assert_eq!(plan_compound_seeps(&open, [2, 1, 2], 8, 1).len(), 1);
        assert_eq!(plan_compound_seeps(&open, [2, 1, 2], 8, 2).len(), 2);
    }

    #[test]
    fn water_prefers_downward_neighbor_first() {
        let cluster = world(&[
            (2, 2, 2, Block::new("fence", 0).with_fill(FILL_WATER)),
            (2, 1, 2, Block::new("fence", 0)),
            (3, 2, 2, Block::new("fence", 0)),
        ]);
        let planned = plan_compound_seeps(&cluster, [2, 2, 2], 8, 1);
        assert_eq!(planned.len(), 1);
        assert_eq!((planned[0].x, planned[0].y, planned[0].z), (2, 1, 2));
    }

    #[test]
    fn ember_prefers_horizontal_conduit_first() {
        let cluster = world(&[
            (2, 2, 2, Block::new("fence", 0).with_fill(FILL_EMBER)),
            (2, 1, 2, Block::new("fence", 0)),
            (3, 2, 2, Block::new("fence", 0)),
        ]);
        let planned = plan_compound_seeps(&cluster, [2, 2, 2], 8, 1);
        assert_eq!(planned.len(), 1);
        assert_eq!((planned[0].x, planned[0].y, planned[0].z), (3, 2, 2));
    }

    #[test]
    fn water_flows_along_pipes_but_not_wires() {
        let mut cluster = world(&[
            (1, 1, 1, Block::new("copper_grate", 0).with_fill(FILL_WATER)),
            (2, 1, 1, Block::new("pipe", 0)),
            (3, 1, 1, Block::new("pipe", 0)),
            (2, 1, 2, Block::new("wire", 0)),
        ]);
        assert_eq!(apply_compound_seeps(&mut cluster, [2, 1, 1], 8, 1), 1);
        assert_eq!(cluster.block_at(2, 1, 1).fill, FILL_WATER);
        assert_eq!(cluster.block_at(2, 1, 2).fill, FILL_NONE);
        assert_eq!(apply_compound_seeps(&mut cluster, [2, 1, 1], 8, 1), 1);
        assert_eq!(cluster.block_at(3, 1, 1).fill, FILL_WATER);
    }

    #[test]
    fn ember_flows_along_wires_but_not_pipes() {
        let mut cluster = world(&[
            (1, 1, 1, Block::new("glass", 0).with_fill(FILL_EMBER)),
            (2, 1, 1, Block::new("wire", 0)),
            (3, 1, 1, Block::new("wire", 0)),
            (2, 1, 2, Block::new("pipe", 0)),
        ]);
        assert_eq!(apply_compound_seeps(&mut cluster, [2, 1, 1], 8, 1), 1);
        assert_eq!(cluster.block_at(2, 1, 1).fill, FILL_EMBER);
        assert_eq!(cluster.block_at(2, 1, 2).fill, FILL_NONE);
        assert_eq!(apply_compound_seeps(&mut cluster, [2, 1, 1], 8, 1), 1);
        assert_eq!(cluster.block_at(3, 1, 1).fill, FILL_EMBER);
    }

    #[test]
    fn seep_prefers_pipe_destination_over_fence() {
        let cluster = world(&[
            (2, 1, 2, Block::new("copper_grate", 0).with_fill(FILL_WATER)),
            (3, 1, 2, Block::new("fence", 0)),
            (2, 1, 3, Block::new("pipe", 0)),
        ]);
        let planned = plan_compound_seeps(&cluster, [2, 1, 2], 8, 1);
        assert_eq!(planned.len(), 1);
        assert_eq!(
            (planned[0].x, planned[0].y, planned[0].z),
            (2, 1, 3),
            "pipe should win over fence when both are open"
        );
    }

    #[test]
    fn closed_pump_blocks_seep() {
        // Facing +X (0), off.
        let pump = Block::new("pump", pump_meta(0, false));
        let cluster = world(&[
            (1, 1, 1, Block::new("pipe", 0).with_fill(FILL_WATER)),
            (2, 1, 1, pump),
            (3, 1, 1, Block::new("pipe", 0)),
        ]);
        assert!(plan_compound_seeps(&cluster, [2, 1, 1], 8, 4).is_empty());
        assert!(plan_pump_pushes(&cluster, [2, 1, 1], 8, 4).is_empty());
    }

    #[test]
    fn powered_pump_pushes_one_way() {
        // Facing +X, on: intake west, outlet east.
        let pump = Block::new("pump", pump_meta(0, true));
        let mut cluster = world(&[
            (1, 1, 1, Block::new("pipe", 0).with_fill(FILL_WATER)),
            (2, 1, 1, pump),
            (3, 1, 1, Block::new("pipe", 0)),
        ]);
        let planned = plan_pump_pushes(&cluster, [2, 1, 1], 8, 4);
        assert_eq!(planned.len(), 1);
        assert_eq!((planned[0].x, planned[0].y, planned[0].z), (3, 1, 1));
        assert_eq!(planned[0].block.fill, FILL_WATER);
        assert_eq!(apply_pump_pushes(&mut cluster, [2, 1, 1], 8, 4), 1);
        assert_eq!(cluster.block_at(3, 1, 1).fill, FILL_WATER);
        assert_eq!(cluster.block_at(1, 1, 1).fill, FILL_WATER, "intake keeps pressure");
    }

    #[test]
    fn powered_pump_rejects_reverse_seep() {
        let pump = Block::new("pump", pump_meta(0, true));
        let cluster = world(&[
            (3, 1, 1, Block::new("pipe", 0).with_fill(FILL_WATER)),
            (2, 1, 1, pump),
            (1, 1, 1, Block::new("pipe", 0)),
        ]);
        // Outlet-side source must not seep backward into the pump.
        let planned = plan_compound_seeps(&cluster, [2, 1, 1], 8, 4);
        assert!(
            planned
                .iter()
                .all(|c| !(c.x == 2 && c.y == 1 && c.z == 1)),
            "pump must not accept fill from the outlet side"
        );
    }

    #[test]
    fn tank_buffers_and_drawdown_on_seep() {
        use crate::chunk::{tank_at, tank_level};
        let mut cluster = world(&[
            (2, 1, 2, tank_at(FILL_WATER, 3)),
            (3, 1, 2, Block::new("pipe", 0)),
        ]);
        let planned = plan_compound_seeps(&cluster, [2, 1, 2], 8, 1);
        assert_eq!(planned.len(), 2, "dest fill + tank drawdown");
        assert_eq!(apply_compound_seeps(&mut cluster, [2, 1, 2], 8, 1), 2);
        assert_eq!(cluster.block_at(3, 1, 2).fill, FILL_WATER);
        assert_eq!(tank_level(cluster.block_at(2, 1, 2)), 2);
    }

    #[test]
    fn tank_accepts_units_until_full() {
        use crate::chunk::{TANK_MAX, tank_at, tank_level};
        let mut cluster = world(&[
            (2, 1, 2, Block::new("pipe", 0).with_fill(FILL_WATER)),
            (3, 1, 2, tank_at(FILL_WATER, TANK_MAX - 1)),
        ]);
        assert_eq!(apply_compound_seeps(&mut cluster, [2, 1, 2], 8, 1), 1);
        assert_eq!(tank_level(cluster.block_at(3, 1, 2)), TANK_MAX);
        // Full tank rejects more.
        assert!(plan_compound_seeps(&cluster, [2, 1, 2], 8, 1)
            .iter()
            .all(|c| c.x != 3));
    }

    #[test]
    fn seep_prefers_tank_buffer_over_pipe() {
        use crate::chunk::tank_at;
        let cluster = world(&[
            (2, 1, 2, Block::new("copper_grate", 0).with_fill(FILL_WATER)),
            (3, 1, 2, Block::new("pipe", 0)),
            (2, 1, 3, Block::new("tank", 0)),
        ]);
        let planned = plan_compound_seeps(&cluster, [2, 1, 2], 8, 1);
        assert_eq!(planned.len(), 1);
        assert_eq!((planned[0].x, planned[0].z), (2, 3));
        assert_eq!(planned[0].block.fill, FILL_WATER);
        let _ = tank_at;
    }

    #[test]
    fn ember_seep_prefers_battery_over_wire() {
        use crate::chunk::{battery_at, battery_level};
        let mut cluster = world(&[
            (2, 1, 2, Block::new("glass", 0).with_fill(FILL_EMBER)),
            (3, 1, 2, Block::new("wire", 0)),
            (2, 1, 3, Block::new("battery", 0)),
        ]);
        let planned = plan_compound_seeps(&cluster, [2, 1, 2], 8, 1);
        assert_eq!(planned.len(), 1);
        assert_eq!((planned[0].x, planned[0].z), (2, 3));
        assert_eq!(planned[0].block.fill, FILL_EMBER);
        assert_eq!(apply_compound_seeps(&mut cluster, [2, 1, 2], 8, 1), 1);
        assert_eq!(battery_level(cluster.block_at(2, 1, 3)), 1);
        let _ = battery_at;
    }

    #[test]
    fn battery_drawdown_feeds_wire() {
        use crate::chunk::{battery_at, battery_level};
        let mut cluster = world(&[
            (2, 1, 2, battery_at(3)),
            (3, 1, 2, Block::new("wire", 0)),
        ]);
        assert_eq!(apply_compound_seeps(&mut cluster, [2, 1, 2], 8, 1), 2);
        assert_eq!(cluster.block_at(3, 1, 2).fill, FILL_EMBER);
        assert_eq!(battery_level(cluster.block_at(2, 1, 2)), 2);
    }

    #[test]
    fn battery_rejects_water() {
        use crate::chunk::compound_accepts;
        assert!(compound_accepts("battery", FILL_EMBER));
        assert!(!compound_accepts("battery", FILL_WATER));
        let cluster = world(&[
            (2, 1, 2, Block::new("pipe", 0).with_fill(FILL_WATER)),
            (3, 1, 2, Block::new("battery", 0)),
        ]);
        assert!(plan_compound_seeps(&cluster, [2, 1, 2], 8, 1)
            .iter()
            .all(|c| c.x != 3));
    }

    #[test]
    fn lamp_lights_from_wired_battery_and_drains() {
        use crate::chunk::{battery_at, battery_level, lamp_at, lamp_on};
        let mut cluster = world(&[
            (1, 1, 1, battery_at(3)),
            (2, 1, 1, Block::new("wire", 0)),
            (3, 1, 1, lamp_at(false)),
        ]);
        let planned = plan_ember_lamps(&cluster, [2, 1, 1], 8, 4, true);
        assert!(
            planned.iter().any(|c| c.x == 3 && lamp_on(c.block.metadata)),
            "lamp turns on"
        );
        assert!(
            planned
                .iter()
                .any(|c| c.x == 1 && is_battery(c.block.id) && battery_level(c.block) == 2),
            "battery loses one charge while lighting"
        );
        assert_eq!(apply_ember_lamps(&mut cluster, [2, 1, 1], 8, 4, true), 2);
        assert!(lamp_on(cluster.block_at(3, 1, 1).metadata));
        assert_eq!(battery_level(cluster.block_at(1, 1, 1)), 2);
    }

    #[test]
    fn lamp_lights_from_ember_wire_without_drain() {
        use crate::chunk::{lamp_at, lamp_on};
        let cluster = world(&[
            (1, 1, 1, Block::new("glass", 0).with_fill(FILL_EMBER)),
            (2, 1, 1, Block::new("wire", 0).with_fill(FILL_EMBER)),
            (3, 1, 1, lamp_at(false)),
        ]);
        let planned = plan_ember_lamps(&cluster, [2, 1, 1], 8, 4, true);
        assert_eq!(planned.len(), 1);
        assert!(lamp_on(planned[0].block.metadata));
        assert_eq!(planned[0].x, 3);
    }

    #[test]
    fn lamp_turns_off_without_power() {
        use crate::chunk::{lamp_at, lamp_on};
        let cluster = world(&[
            (2, 1, 1, Block::new("wire", 0)),
            (3, 1, 1, lamp_at(true)),
        ]);
        let planned = plan_ember_lamps(&cluster, [2, 1, 1], 8, 4, false);
        assert_eq!(planned.len(), 1);
        assert!(!lamp_on(planned[0].block.metadata));
    }

    #[test]
    fn spout_drains_pipe_water_into_air() {
        use crate::chunk::pump_meta;
        let mut cluster = world(&[
            (1, 1, 1, Block::new("pipe", 0).with_fill(FILL_WATER)),
            (2, 1, 1, Block::new("spout", pump_meta(0, true))),
        ]);
        let planned = plan_water_spouts(&cluster, [2, 1, 1], 8, 2);
        assert!(
            planned
                .iter()
                .any(|c| c.x == 3 && c.block.id == "water"),
            "water dumps ahead of spout"
        );
        assert!(
            planned
                .iter()
                .any(|c| c.x == 1 && c.block.fill == FILL_NONE),
            "pipe empties"
        );
        assert_eq!(apply_water_spouts(&mut cluster, [2, 1, 1], 8, 2), 2);
        assert_eq!(cluster.block_at(3, 1, 1).id, "water");
        assert_eq!(cluster.block_at(1, 1, 1).fill, FILL_NONE);
    }

    #[test]
    fn pressurized_tank_spout_sprays_two_cells() {
        use crate::chunk::{pump_meta, tank_at, tank_level};
        let mut cluster = world(&[
            (1, 1, 1, tank_at(FILL_WATER, 4)),
            (2, 1, 1, Block::new("spout", pump_meta(0, true))),
        ]);
        let planned = plan_water_spouts(&cluster, [2, 1, 1], 8, 2);
        assert!(planned.iter().any(|c| c.x == 3 && c.block.id == "water"));
        assert!(
            planned.iter().any(|c| c.x == 4 && c.block.id == "water"),
            "pressure sprays one cell farther"
        );
        assert!(planned.iter().any(|c| c.x == 1 && tank_level(c.block) == 3));
        assert_eq!(apply_water_spouts(&mut cluster, [2, 1, 1], 8, 2), 3);
        assert_eq!(cluster.block_at(3, 1, 1).id, "water");
        assert_eq!(cluster.block_at(4, 1, 1).id, "water");
        assert_eq!(tank_level(cluster.block_at(1, 1, 1)), 3);
    }

    #[test]
    fn tank_level_one_still_sprays() {
        use crate::chunk::{pump_meta, tank_at};
        let planned = plan_water_spouts(
            &world(&[
                (1, 1, 1, tank_at(FILL_WATER, 1)),
                (2, 1, 1, Block::new("spout", pump_meta(0, true))),
            ]),
            [2, 1, 1],
            8,
            2,
        );
        assert!(planned.iter().any(|c| c.x == 3 && c.block.id == "water"));
        assert!(
            planned.iter().any(|c| c.x == 4 && c.block.id == "water"),
            "tank level > 0 sprays"
        );
        assert!(planned.iter().any(|c| c.x == 1 && c.block.id == "tank" && c.block.fill == FILL_NONE));
    }

    #[test]
    fn closed_spout_holds_water() {
        use crate::chunk::pump_meta;
        let cluster = world(&[
            (1, 1, 1, Block::new("pipe", 0).with_fill(FILL_WATER)),
            (2, 1, 1, Block::new("spout", pump_meta(0, false))),
        ]);
        assert!(plan_water_spouts(&cluster, [2, 1, 1], 8, 2).is_empty());
    }

    #[test]
    fn drain_pulls_world_water_into_pipe() {
        use crate::chunk::pump_meta;
        let mut cluster = world(&[
            (3, 1, 1, Block::new("water", 0)),
            (2, 1, 1, Block::new("drain", pump_meta(0, true))),
            (1, 1, 1, Block::new("pipe", 0)),
        ]);
        let planned = plan_water_drains(&cluster, [2, 1, 1], 8, 2);
        assert!(
            planned
                .iter()
                .any(|c| c.x == 3 && c.block.is_air()),
            "world water removed"
        );
        assert!(
            planned
                .iter()
                .any(|c| c.x == 1 && c.block.fill == FILL_WATER),
            "pipe fills"
        );
        assert_eq!(apply_water_drains(&mut cluster, [2, 1, 1], 8, 2), 2);
        assert!(cluster.block_at(3, 1, 1).is_air());
        assert_eq!(cluster.block_at(1, 1, 1).fill, FILL_WATER);
    }

    #[test]
    fn drain_fills_tank_from_world_water() {
        use crate::chunk::{pump_meta, tank_at, tank_level};
        let mut cluster = world(&[
            (3, 1, 1, Block::new("water", 0)),
            (2, 1, 1, Block::new("drain", pump_meta(0, true))),
            (1, 1, 1, tank_at(FILL_WATER, 2)),
        ]);
        let planned = plan_water_drains(&cluster, [2, 1, 1], 8, 2);
        assert!(planned.iter().any(|c| c.x == 1 && tank_level(c.block) == 3));
        assert_eq!(apply_water_drains(&mut cluster, [2, 1, 1], 8, 2), 2);
        assert_eq!(tank_level(cluster.block_at(1, 1, 1)), 3);
    }

    #[test]
    fn closed_drain_leaves_water() {
        use crate::chunk::pump_meta;
        let cluster = world(&[
            (3, 1, 1, Block::new("water", 0)),
            (2, 1, 1, Block::new("drain", pump_meta(0, false))),
            (1, 1, 1, Block::new("pipe", 0)),
        ]);
        assert!(plan_water_drains(&cluster, [2, 1, 1], 8, 2).is_empty());
    }

    #[test]
    fn generator_charges_empty_battery_over_wire() {
        use crate::chunk::{battery_level, generator_at};
        let mut cluster = world(&[
            (1, 1, 1, generator_at(true)),
            (2, 1, 1, Block::new("wire", 0)),
            (3, 1, 1, Block::new("battery", 0)),
        ]);
        let planned = plan_ember_charges(&cluster, [2, 1, 1], 8, 2);
        assert!(
            planned
                .iter()
                .any(|c| c.x == 3 && is_battery(c.block.id) && battery_level(c.block) == 1),
            "empty battery gains one charge"
        );
        assert_eq!(apply_ember_charges(&mut cluster, [2, 1, 1], 8, 2), 1);
        assert_eq!(battery_level(cluster.block_at(3, 1, 1)), 1);
    }

    #[test]
    fn campfire_beside_generator_boosts_charge() {
        use crate::chunk::{battery_at, battery_level, generator_at};
        let planned = plan_ember_charges(
            &world(&[
                (1, 1, 1, generator_at(true)),
                (1, 1, 2, Block::new("campfire", 0)),
                (2, 1, 1, Block::new("wire", 0)),
                (3, 1, 1, battery_at(1)),
            ]),
            [2, 1, 1],
            8,
            2,
        );
        assert!(
            planned
                .iter()
                .any(|c| c.x == 3 && battery_level(c.block) == 3),
            "fueled generator adds two units"
        );
    }

    #[test]
    fn generator_fills_empty_wire_when_no_battery() {
        use crate::chunk::generator_at;
        let planned = plan_ember_charges(
            &world(&[
                (1, 1, 1, generator_at(true)),
                (2, 1, 1, Block::new("wire", 0)),
            ]),
            [2, 1, 1],
            8,
            2,
        );
        assert!(
            planned
                .iter()
                .any(|c| c.x == 2 && c.block.id == "wire" && c.block.fill == FILL_EMBER),
            "wire takes ember when no battery"
        );
    }

    #[test]
    fn generator_prefers_emptier_battery() {
        use crate::chunk::{battery_at, battery_level, generator_at};
        let planned = plan_ember_charges(
            &world(&[
                (2, 1, 1, generator_at(true)),
                (1, 1, 1, battery_at(5)),
                (3, 1, 1, battery_at(2)),
            ]),
            [2, 1, 1],
            8,
            2,
        );
        assert!(
            planned
                .iter()
                .any(|c| c.x == 3 && battery_level(c.block) == 3),
            "charges the lower battery first"
        );
    }

    #[test]
    fn off_generator_does_not_charge() {
        use crate::chunk::generator_at;
        let cluster = world(&[
            (1, 1, 1, generator_at(false)),
            (2, 1, 1, Block::new("wire", 0)),
            (3, 1, 1, Block::new("battery", 0)),
        ]);
        assert!(plan_ember_charges(&cluster, [2, 1, 1], 8, 2).is_empty());
    }

    #[test]
    fn lamp_lights_from_running_generator() {
        use crate::chunk::{generator_at, lamp_at, lamp_on};
        let planned = plan_ember_lamps(
            &world(&[
                (1, 1, 1, generator_at(true)),
                (2, 1, 1, Block::new("wire", 0)),
                (3, 1, 1, lamp_at(false)),
            ]),
            [2, 1, 1],
            8,
            4,
            false,
        );
        assert_eq!(planned.len(), 1);
        assert!(lamp_on(planned[0].block.metadata));
    }
}
