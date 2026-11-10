//! Slow compound fill seep between adjacent host blocks.
//!
//! Water and ember copy into neighboring empty compound hosts (pipe / conduit
//! feel). Pumps gate and push one-way when powered. One pulse plans a small
//! number of spreads near a focus so a large world stays cheap.

use std::collections::HashSet;

use crate::changes::CellChange;
use crate::chunk::{
    FILL_EMBER, FILL_NONE, TANK_MAX, compound_accepts_meta, is_pipe, is_pump, is_tank, is_wire,
    pump_dir, pump_on, tank_at, tank_level,
};
use crate::cluster::Cluster;

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

/// True when `dest` can take a unit of `src.fill` (empty host or underfilled tank).
fn can_receive(dest: crate::chunk::Block, fill: u8) -> bool {
    if !accepts(dest, fill) {
        return false;
    }
    if dest.fill == FILL_NONE {
        return true;
    }
    is_tank(dest.id) && dest.fill == fill && tank_level(dest) < TANK_MAX
}

/// Next dest block after receiving one unit of `fill`.
fn receive_fill(dest: crate::chunk::Block, fill: u8) -> crate::chunk::Block {
    if is_tank(dest.id) {
        let level = if dest.fill == FILL_NONE {
            1
        } else {
            tank_level(dest).saturating_add(1).min(TANK_MAX)
        };
        return tank_at(fill, level);
    }
    dest.with_fill(fill)
}

/// Drawdown after a tank sources one unit. `None` when the source is not a tank.
fn tank_after_source(src: crate::chunk::Block) -> Option<crate::chunk::Block> {
    if !is_tank(src.id) {
        return None;
    }
    let level = tank_level(src);
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
                let prefer =
                    is_pipe(block.id) || is_wire(block.id) || is_pump(block.id) || is_tank(block.id);
                sources.push((x, y, z, block, prefer));
            });
        }
    }
    // Tanks / pumps / conduits first, then lower cells so water drains through runs.
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
            let prefer =
                is_pipe(dest.id) || is_wire(dest.id) || is_pump(dest.id) || is_tank(dest.id);
            candidates.push((prefer, is_tank(dest.id), nx, ny, nz, dest));
        }
        // Prefer tanks (buffers), then other conduits.
        candidates.sort_by_key(|&(prefer, tank, ..)| (!tank, !prefer));
        for (_prefer, _tank, nx, ny, nz, dest) in candidates {
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
            if let Some(after) = tank_after_source(src) {
                out.push(CellChange {
                    x,
                    y,
                    z,
                    block: after,
                });
                // One unit out of a tank per pulse.
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
        if let Some(after) = tank_after_source(input) {
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
}
