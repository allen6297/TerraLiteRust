//! Built-in scenery so tests and the demo run without a saved world.
//!
//! The cluster is the small world: 31×31 chunks, 496 blocks on a side. The
//! center 3×3 of those chunks holds the scene. Every other chunk stays empty.
//!
//! The center chunk is the old single-chunk scene, plus one cobblestone on
//! its east edge, plus oak stairs just south of the log, plus a copper grate
//! for compound fills, plus coal ore and iron ore beside those stairs, plus a
//! sign just west of the coal, plus an orange lantern just southwest of that
//! sign, plus a perch beside the chicken.
//! Public ids:
//! grass, cobblestone, coal ore, iron ore, deepslate, oak log, oak stairs, sign, lantern, wool, and
//! `marker` (stored above 255 in the region file so the Add nibble is exercised). The stairs are two boxes
//! inside the cell, not a full cube. The sign is a model: a post and a plane.
//!
//! Each other chunk is a wool floor. The metadata changes with the chunk, so
//! crossing a border changes the ground color. The east neighbor has a
//! cobblestone against the center's, and each corner has a short post.

use crate::chunk::{Block, Chunk, chunk_to_nbt};
use crate::cluster::Cluster;
use crate::nbt;
use crate::region;
use crate::shape::{COAL_ORE, IRON_ORE, LANTERN, OAK_STAIRS, SIGN};
use crate::size::WorldSize;

pub fn synthetic_chunk() -> Chunk {
    Chunk::from_blocks(0, 0, demo_blocks()).expect("synthetic chunk fits in one section")
}

pub fn synthetic_cluster() -> Cluster {
    synthetic_cluster_sized(WorldSize::Small)
}

/// The demo pad inside a square of `size`. Empty chunks fill the rest.
pub fn synthetic_cluster_sized(size: WorldSize) -> Cluster {
    let mut chunks = Vec::with_capacity(9);
    for z in -1..=1 {
        for x in -1..=1 {
            chunks.push(synthetic_chunk_at(x, z));
        }
    }
    Cluster::try_from_chunks_sized(size, 0, 0, chunks).unwrap_or_else(|err| {
        panic!(
            "the synthetic chunks fit the {} square: {err}",
            size.label()
        )
    })
}

pub fn synthetic_region() -> Vec<u8> {
    let chunk = synthetic_chunk();
    let nbt = nbt::write_root("", &chunk_to_nbt(&chunk));
    region::encode_region(chunk.x(), chunk.z(), &nbt, 2)
}

fn synthetic_chunk_at(x: i32, z: i32) -> Chunk {
    if x == 0 && z == 0 {
        let mut blocks = demo_blocks();
        // Shares a face with the cobblestone in chunk (1, 0).
        blocks.push((15, 1, 8, Block::new("cobblestone", 0)));
        // South of the log, on the line the start camera looks along.
        blocks.push((8, 1, 12, Block::new(OAK_STAIRS, 0)));
        // Compound host for water/ember fills (waterlogging-like).
        blocks.push((11, 1, 12, Block::new("copper_grate", 0)));
        // Just east of the stairs, in view from the start camera.
        blocks.push((9, 1, 13, Block::new(COAL_ORE, 0)));
        // One block further east, so a walk past the coal reaches the other ore.
        blocks.push((10, 1, 13, Block::new(IRON_ORE, 0)));
        // West of the ore, on the grass, off the line that walks into the stairs.
        blocks.push((7, 1, 13, Block::new(SIGN, 0)));
        // Southwest of the sign, on the grass, where a walk toward the sign meets it.
        blocks.push((6, 1, 14, Block::new(LANTERN, 0)));
        // Beside the chicken, west of the oak. The seat is a Blockbench model.
        blocks.push((4, 1, 9, Block::new("perch", 0)));
        return Chunk::from_blocks(0, 0, blocks).expect("center chunk");
    }
    let mut blocks = wool_floor(neighbor_wool(x, z));
    if x == 1 && z == 0 {
        blocks.push((0, 1, 8, Block::new("cobblestone", 0)));
    }
    if x != 0 && z != 0 {
        blocks.push((8, 1, 8, Block::new("cobblestone", 0)));
    }
    Chunk::from_blocks(x, z, blocks).expect("neighbor chunk")
}

fn neighbor_wool(x: i32, z: i32) -> u8 {
    let index = (z + 1) * 3 + (x + 1);
    let metadata = if index > 4 { index - 1 } else { index };
    metadata as u8
}

fn wool_floor(metadata: u8) -> Vec<(u8, u16, u8, Block)> {
    let mut blocks = Vec::with_capacity(256);
    for z in 0..16u8 {
        for x in 0..16u8 {
            blocks.push((x, 0, z, Block::new("wool", metadata)));
        }
    }
    blocks
}

fn demo_blocks() -> Vec<(u8, u16, u8, Block)> {
    let mut blocks = Vec::with_capacity(267);
    for z in 0..16u8 {
        for x in 0..16u8 {
            blocks.push((x, 0, z, Block::new("grass", 0)));
        }
    }
    blocks.push((0, 1, 0, Block::new("cobblestone", 0)));
    blocks.push((0, 2, 0, Block::new("cobblestone", 0)));
    for y in 1..4u16 {
        blocks.push((8, y, 8, Block::new("oak_log", 0)));
    }
    // Wool metadata: red, yellow, blue, white, black.
    for (i, metadata) in [14u8, 4, 11, 0, 15].into_iter().enumerate() {
        blocks.push((2 + i as u8, 1, 4, Block::new("wool", metadata)));
    }
    blocks.push((14, 1, 14, Block::new("marker", 7)));
    blocks
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::region::load_chunk_from_region;

    #[test]
    fn synthetic_region_roundtrips_the_demo() {
        let loaded = load_chunk_from_region(&synthetic_region(), 0, 0).unwrap();
        let direct = synthetic_chunk();
        assert_eq!(loaded.x(), 0);
        assert_eq!(loaded.z(), 0);
        assert_eq!(loaded.section_count(), 1);
        assert_eq!(loaded.non_air_count(), 267);
        assert_eq!(direct.non_air_count(), 267);
        for y in 0..256u16 {
            for z in 0..16u8 {
                for x in 0..16u8 {
                    assert_eq!(
                        loaded.block(x, y, z),
                        direct.block(x, y, z),
                        "block {x},{y},{z}"
                    );
                }
            }
        }
        assert_eq!(loaded.block(0, 0, 0), Block::new("grass", 0));
        assert_eq!(loaded.block(0, 2, 0), Block::new("cobblestone", 0));
        assert_eq!(loaded.block(8, 3, 8), Block::new("oak_log", 0));
        assert_eq!(loaded.block(8, 4, 8), Block::AIR);
        assert_eq!(loaded.block(2, 1, 4), Block::new("wool", 14));
        assert_eq!(loaded.block(6, 1, 4), Block::new("wool", 15));
        assert_eq!(loaded.block(14, 1, 14), Block::new("marker", 7));
        assert!(loaded.block(1, 1, 1).is_air());
    }

    #[test]
    fn synthetic_cluster_is_the_small_square() {
        let cluster = synthetic_cluster();
        let center = cluster.get(0, 0).unwrap();
        let demo = synthetic_chunk();
        // Center demo is 267 blocks, plus the east-edge cobblestone, the
        // stairs, the coal ore, the iron ore, the sign, the lantern, and the perch.
        // Eight wool floors are 256 each. The east neighbor and the four
        // corners each add one cobblestone: 274 + 8 * 256 + 5 = 2327.
        // The rest of the 31×31 square is empty.
        let side = crate::WorldSize::Small.side();
        let radius = crate::WorldSize::Small.chunk_radius();
        assert_eq!(cluster.center(), (0, 0));
        assert_eq!(cluster.size(), crate::WorldSize::Small);
        assert_eq!(cluster.chunk_count(), (side * side) as usize);
        assert_eq!(
            cluster.bounds(),
            (
                -radius * 16,
                (radius + 1) * 16,
                -radius * 16,
                (radius + 1) * 16
            )
        );
        assert_eq!(center.non_air_count(), 274);
        assert_eq!(cluster.non_air_count(), 2327);
        assert_eq!(center.block(15, 1, 8), Block::new("cobblestone", 0));
        assert_eq!(center.block(8, 1, 12), Block::new(OAK_STAIRS, 0));
        assert_eq!(center.block(9, 1, 13), Block::new(COAL_ORE, 0));
        assert_eq!(center.block(10, 1, 13), Block::new(IRON_ORE, 0));
        assert_eq!(center.block(7, 1, 13), Block::new(SIGN, 0));
        assert_eq!(center.block(6, 1, 14), Block::new(LANTERN, 0));
        assert_eq!(center.block(4, 1, 9), Block::new("perch", 0));
        assert!(demo.block(15, 1, 8).is_air());
        assert!(demo.block(8, 1, 12).is_air());
        assert!(demo.block(9, 1, 13).is_air());
        assert!(demo.block(10, 1, 13).is_air());
        assert!(demo.block(7, 1, 13).is_air());
        assert!(demo.block(6, 1, 14).is_air());
        for y in 0..256u16 {
            for z in 0..16u8 {
                for x in 0..16u8 {
                    if (x, y, z) == (15, 1, 8)
                        || (x, y, z) == (8, 1, 12)
                        || (x, y, z) == (9, 1, 13)
                        || (x, y, z) == (10, 1, 13)
                        || (x, y, z) == (7, 1, 13)
                        || (x, y, z) == (6, 1, 14)
                        || (x, y, z) == (4, 1, 9)
                    {
                        continue;
                    }
                    assert_eq!(center.block(x, y, z), demo.block(x, y, z));
                }
            }
        }
        assert_eq!(cluster.block_at(15, 0, 0), Block::new("grass", 0));
        assert_eq!(cluster.block_at(16, 0, 0), Block::new("wool", 4));
        assert_eq!(cluster.block_at(15, 1, 8), Block::new("cobblestone", 0));
        assert_eq!(cluster.block_at(16, 1, 8), Block::new("cobblestone", 0));
        assert_eq!(
            cluster.get(-1, -1).unwrap().block(0, 0, 0),
            Block::new("wool", 0)
        );
        assert_eq!(
            cluster.get(1, 1).unwrap().block(8, 1, 8),
            Block::new("cobblestone", 0)
        );
        assert!(cluster.block_at(-17, 0, 0).is_air());
        assert!(cluster.block_at(32, 0, 0).is_air());
    }

    #[test]
    fn one_region_file_roundtrips_only_its_own_chunks() {
        let direct = synthetic_cluster();
        // Chunks x >= 0, z >= 0 live in r.0.0. Negative chunks are other files.
        let owned: Vec<&Chunk> = direct
            .chunks()
            .iter()
            .filter(|chunk| chunk.x() >= 0 && chunk.z() >= 0)
            .collect();
        let span = (crate::WorldSize::Small.chunk_radius() + 1) as usize;
        assert_eq!(owned.len(), span * span);
        let encoded: Vec<(i32, i32, Vec<u8>)> = owned
            .iter()
            .map(|chunk| {
                (
                    chunk.x(),
                    chunk.z(),
                    nbt::write_root("", &chunk_to_nbt(chunk)),
                )
            })
            .collect();
        let parts: Vec<(i32, i32, &[u8])> = encoded
            .iter()
            .map(|(x, z, bytes)| (*x, *z, bytes.as_slice()))
            .collect();
        let region = region::encode_region_many(&parts, 2);
        let loaded = crate::load_cluster_from_region(&region, 0, 0).unwrap();
        let side = crate::WorldSize::Small.side();
        assert_eq!(loaded.chunk_count(), (side * side) as usize);
        for z in -1..=1 {
            for x in -1..=1 {
                let got = loaded.get(x, z).unwrap();
                if x < 0 || z < 0 {
                    assert_eq!(got.non_air_count(), 0, "chunk {x},{z} is another region");
                    continue;
                }
                let want = direct.get(x, z).unwrap();
                for y in 0..16u16 {
                    for lz in 0..16u8 {
                        for lx in 0..16u8 {
                            assert_eq!(
                                got.block(lx, y, lz),
                                want.block(lx, y, lz),
                                "chunk {x},{z} block {lx},{y},{lz}"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn neighboring_region_files_fill_the_cluster() {
        let direct = synthetic_cluster();
        let dir =
            std::env::temp_dir().join(format!("rebuild-regions-{}-cluster", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let guard = DirGuard(dir.clone());
        for region_z in [-1, 0] {
            for region_x in [-1, 0] {
                let encoded: Vec<(i32, i32, Vec<u8>)> = direct
                    .chunks()
                    .iter()
                    .filter(|chunk| {
                        chunk.x().div_euclid(32) == region_x && chunk.z().div_euclid(32) == region_z
                    })
                    .map(|chunk| {
                        (
                            chunk.x(),
                            chunk.z(),
                            nbt::write_root("", &chunk_to_nbt(chunk)),
                        )
                    })
                    .collect();
                let parts: Vec<(i32, i32, &[u8])> = encoded
                    .iter()
                    .map(|(x, z, bytes)| (*x, *z, bytes.as_slice()))
                    .collect();
                let bytes = region::encode_region_many(&parts, 2);
                let path = dir.join(format!("r.{region_x}.{region_z}.mca"));
                std::fs::write(&path, bytes).unwrap();
            }
        }
        let loaded = crate::load_cluster_from_path(&dir.join("r.0.0.mca"), 0, 0).unwrap();
        assert_eq!(loaded.non_air_count(), direct.non_air_count());
        for z in -1..=1 {
            for x in -1..=1 {
                let got = loaded.get(x, z).unwrap();
                let want = direct.get(x, z).unwrap();
                assert_eq!(got.non_air_count(), want.non_air_count());
                assert_eq!(got.block(0, 0, 0), want.block(0, 0, 0));
                assert_eq!(got.block(8, 1, 8), want.block(8, 1, 8));
            }
        }
        drop(guard);
    }
}

#[cfg(test)]
struct DirGuard(std::path::PathBuf);

#[cfg(test)]
impl Drop for DirGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
