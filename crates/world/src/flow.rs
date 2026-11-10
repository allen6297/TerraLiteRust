//! Slow compound fill seep between adjacent host blocks.
//!
//! Water and ember copy into neighboring empty compound hosts (pipe / conduit
//! feel). One pulse plans a small number of spreads near a focus so a large
//! world stays cheap.

use std::collections::HashSet;

use crate::changes::CellChange;
use crate::chunk::{FILL_EMBER, FILL_NONE, compound_accepts, is_pipe, is_wire};
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
                if !compound_accepts(block.id, block.fill) {
                    return;
                }
                if (x - focus[0]).abs() > radius
                    || (y - focus[1]).abs() > radius
                    || (z - focus[2]).abs() > radius
                {
                    return;
                }
                sources.push((x, y, z, block.fill, is_pipe(block.id) || is_wire(block.id)));
            });
        }
    }
    // Dedicated conduits first, then lower cells so water drains through pipe runs.
    sources.sort_by_key(|&(x, y, z, _, conduit)| {
        (
            !conduit,
            y,
            (x - focus[0]).abs() + (z - focus[2]).abs(),
            x,
            z,
        )
    });

    let mut claimed = HashSet::new();
    let mut out = Vec::new();
    for (x, y, z, fill, _) in sources {
        if out.len() >= max_spreads {
            break;
        }
        // Prefer pipe/wire destinations within this source's neighbor bias.
        let mut candidates = Vec::new();
        for [dx, dy, dz] in neighbors_for(fill) {
            let nx = x + dx;
            let ny = y + dy;
            let nz = z + dz;
            if claimed.contains(&(nx, ny, nz)) {
                continue;
            }
            let dest = world.block_at(nx, ny, nz);
            if dest.fill != FILL_NONE || !compound_accepts(dest.id, fill) {
                continue;
            }
            let conduit = is_pipe(dest.id) || is_wire(dest.id);
            candidates.push((conduit, nx, ny, nz, dest));
        }
        candidates.sort_by_key(|&(conduit, ..)| !conduit);
        for (_conduit, nx, ny, nz, dest) in candidates {
            if out.len() >= max_spreads {
                break;
            }
            if !claimed.insert((nx, ny, nz)) {
                continue;
            }
            out.push(CellChange {
                x: nx,
                y: ny,
                z: nz,
                block: dest.with_fill(fill),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunk::{Block, Chunk, FILL_EMBER, FILL_WATER};
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
}
