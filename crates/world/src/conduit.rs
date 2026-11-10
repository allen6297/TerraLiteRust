//! Pipe / wire junction geometry shared by mesh and outline.
//!
//! Link masks and shell boxes match Minecraft-style connected conduits:
//! straight runs are one continuous tube; corners and tees use a core plus
//! same-thickness arms.

use crate::chunk::conduit_links;
use crate::cluster::Cluster;
use crate::shape::CellBox;

/// Bit mask order: +X -X +Y -Y +Z -Z.
pub fn link_mask_at(world: &Cluster, id: &str, x: i32, y: i32, z: i32) -> u8 {
    let dirs = [[1, 0, 0], [-1, 0, 0], [0, 1, 0], [0, -1, 0], [0, 0, 1], [0, 0, -1]];
    let mut mask = 0u8;
    for (i, [dx, dy, dz]) in dirs.into_iter().enumerate() {
        let neighbor = world.block_at(x + dx, y + dy, z + dz);
        if conduit_links(id, neighbor.id) {
            mask |= 1 << i;
        }
    }
    mask
}

pub fn span(id: &str) -> (u8, u8) {
    if id == "pipe" {
        (5, 11)
    } else {
        (7, 9)
    }
}

pub fn fill_span(id: &str) -> (u8, u8) {
    if id == "pipe" {
        (6, 10)
    } else {
        (7, 9)
    }
}

/// Shell boxes for a conduit cell.
///
/// Linked axes draw one continuous tube that runs from the far linked face
/// (or the core) through the center — corners and tees cross like MC panes.
/// `caps` adds a short collar on each open side of a linked axis.
pub fn shell_boxes(lo: u8, hi: u8, links: u8, caps: bool) -> Vec<CellBox> {
    let mut boxes = Vec::with_capacity(6);
    let px = links & (1 << 0) != 0;
    let nx = links & (1 << 1) != 0;
    let py = links & (1 << 2) != 0;
    let ny = links & (1 << 3) != 0;
    let pz = links & (1 << 4) != 0;
    let nz = links & (1 << 5) != 0;

    if links == 0 {
        boxes.push(CellBox {
            min: [lo, lo, lo],
            max: [hi, hi, hi],
        });
        return boxes;
    }

    if px || nx {
        let x0 = if nx { 0 } else { lo };
        let x1 = if px { 16 } else { hi };
        boxes.push(CellBox {
            min: [x0, lo, lo],
            max: [x1, hi, hi],
        });
        if caps {
            if !nx {
                boxes.push(CellBox {
                    min: [lo.saturating_sub(1), lo, lo],
                    max: [lo + 1, hi, hi],
                });
            }
            if !px {
                boxes.push(CellBox {
                    min: [hi.saturating_sub(1), lo, lo],
                    max: [(hi + 1).min(16), hi, hi],
                });
            }
        }
    }
    if py || ny {
        let y0 = if ny { 0 } else { lo };
        let y1 = if py { 16 } else { hi };
        boxes.push(CellBox {
            min: [lo, y0, lo],
            max: [hi, y1, hi],
        });
        if caps {
            if !ny {
                boxes.push(CellBox {
                    min: [lo, lo.saturating_sub(1), lo],
                    max: [hi, lo + 1, hi],
                });
            }
            if !py {
                boxes.push(CellBox {
                    min: [lo, hi.saturating_sub(1), lo],
                    max: [hi, (hi + 1).min(16), hi],
                });
            }
        }
    }
    if pz || nz {
        let z0 = if nz { 0 } else { lo };
        let z1 = if pz { 16 } else { hi };
        boxes.push(CellBox {
            min: [lo, lo, z0],
            max: [hi, hi, z1],
        });
        if caps {
            if !nz {
                boxes.push(CellBox {
                    min: [lo, lo, lo.saturating_sub(1)],
                    max: [hi, hi, lo + 1],
                });
            }
            if !pz {
                boxes.push(CellBox {
                    min: [lo, lo, hi.saturating_sub(1)],
                    max: [hi, hi, (hi + 1).min(16)],
                });
            }
        }
    }
    boxes
}

/// Face cull helper: tube cross-section on a cell edge.
pub fn edge_tube(id: &str, mx: u8, my: u8, mz: u8) -> bool {
    let (lo, hi) = span(id);
    if mx == 0 || mx == 15 {
        return my >= lo && my < hi && mz >= lo && mz < hi;
    }
    if my == 0 || my == 15 {
        return mx >= lo && mx < hi && mz >= lo && mz < hi;
    }
    if mz == 0 || mz == 15 {
        return mx >= lo && mx < hi && my >= lo && my < hi;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunk::{Block, Chunk};
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
    fn straight_pipe_run_is_one_tube() {
        let cluster = world(&[
            (1, 1, 1, Block::new("pipe", 0)),
            (2, 1, 1, Block::new("pipe", 0)),
            (3, 1, 1, Block::new("pipe", 0)),
        ]);
        let links = link_mask_at(&cluster, "pipe", 2, 1, 1);
        let boxes = shell_boxes(5, 11, links, true);
        assert_eq!(boxes.len(), 1, "straight run should be a single tube");
        assert_eq!(boxes[0].min, [0, 5, 5]);
        assert_eq!(boxes[0].max, [16, 11, 11]);
    }

    #[test]
    fn corner_pipe_is_two_crossing_tubes() {
        let cluster = world(&[
            (2, 1, 1, Block::new("pipe", 0)),
            (3, 1, 1, Block::new("pipe", 0)),
            (2, 1, 2, Block::new("pipe", 0)),
        ]);
        let links = link_mask_at(&cluster, "pipe", 2, 1, 1);
        let boxes = shell_boxes(5, 11, links, false);
        assert_eq!(boxes.len(), 2, "corner is two crossing tubes, got {}", boxes.len());
        // +X and +Z stubs meet through the core.
        assert!(boxes.iter().any(|b| b.min[0] == 5 && b.max[0] == 16));
        assert!(boxes.iter().any(|b| b.min[2] == 5 && b.max[2] == 16));
    }

    #[test]
    fn tee_pipe_is_through_run_plus_stem() {
        let cluster = world(&[
            (1, 1, 1, Block::new("pipe", 0)),
            (2, 1, 1, Block::new("pipe", 0)),
            (3, 1, 1, Block::new("pipe", 0)),
            (2, 1, 2, Block::new("pipe", 0)),
        ]);
        let links = link_mask_at(&cluster, "pipe", 2, 1, 1);
        let boxes = shell_boxes(5, 11, links, false);
        assert_eq!(boxes.len(), 2);
        assert!(boxes.iter().any(|b| b.min[0] == 0 && b.max[0] == 16));
        assert!(boxes.iter().any(|b| b.min[2] == 5 && b.max[2] == 16));
    }
}
