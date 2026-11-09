//! A square of chunks around one center.
//!
//! The square's width is a [`WorldSize`]. Small, which the synthetic demo
//! uses, is 31×31 chunks, 496 blocks on a side. An empty slot becomes an
//! empty chunk, so a neighbor lookup still has an answer. One `.mca` only
//! covers its own 32×32. A byte slice is that one file. A path named
//! `r.X.Z.mca` also opens the neighboring region files in the same folder
//! when the square crosses the boundary. Loaders still read the small square.
//! Chunks outside the center demo pad can be filled with biome terrain.

use std::fs;
use std::path::Path;

use crate::chunk::{Block, Chunk};
use crate::error::Error;
use crate::region::load_chunk_from_region;
use crate::size::WorldSize;

#[derive(Debug)]
pub struct Cluster {
    center_x: i32,
    center_z: i32,
    size: WorldSize,
    chunks: Vec<Chunk>,
}

impl Cluster {
    pub fn center(&self) -> (i32, i32) {
        (self.center_x, self.center_z)
    }

    pub fn size(&self) -> WorldSize {
        self.size
    }

    pub fn chunk_count(&self) -> usize {
        self.chunks.len()
    }

    pub fn chunks(&self) -> &[Chunk] {
        &self.chunks
    }

    pub fn non_air_count(&self) -> u32 {
        self.chunks.iter().map(Chunk::non_air_count).sum()
    }

    pub fn get(&self, x: i32, z: i32) -> Option<&Chunk> {
        self.slot(x, z).map(|index| &self.chunks[index])
    }

    /// World block coordinates. Outside the cluster, and outside the world
    /// height, the block is air.
    pub fn block_at(&self, x: i32, y: i32, z: i32) -> Block {
        if !(0..256).contains(&y) {
            return Block::AIR;
        }
        let Some(chunk) = self.get(x.div_euclid(16), z.div_euclid(16)) else {
            return Block::AIR;
        };
        chunk.block(x.rem_euclid(16) as u8, y as u16, z.rem_euclid(16) as u8)
    }

    pub(crate) fn in_bounds(&self, x: i32, y: i32, z: i32) -> bool {
        (0..256).contains(&y) && self.slot(x.div_euclid(16), z.div_euclid(16)).is_some()
    }

    pub(crate) fn set_block(&mut self, x: i32, y: i32, z: i32, block: Block) -> bool {
        if !self.in_bounds(x, y, z) {
            return false;
        }
        let index = self
            .slot(x.div_euclid(16), z.div_euclid(16))
            .expect("in_bounds just accepted this cell");
        self.chunks[index].set_block(
            x.rem_euclid(16) as u8,
            y as u16,
            z.rem_euclid(16) as u8,
            block,
        );
        true
    }

    /// Inclusive minimum and exclusive maximum block coordinates.
    pub fn bounds(&self) -> (i32, i32, i32, i32) {
        let radius = self.size.chunk_radius();
        let min_x = (self.center_x - radius) * 16;
        let max_x = (self.center_x + radius + 1) * 16;
        let min_z = (self.center_z - radius) * 16;
        let max_z = (self.center_z + radius + 1) * 16;
        (min_x, max_x, min_z, max_z)
    }

    pub fn summary(&self, source: &str) -> String {
        format!(
            "{} chunks around ({}, {}): {} non-air blocks [{source}]",
            self.chunk_count(),
            self.center_x,
            self.center_z,
            self.non_air_count(),
        )
    }

    /// Build a small cluster, then replace slots with `chunks`. A chunk outside
    /// the square is an error. Slots that are not replaced stay empty.
    pub fn try_from_chunks(
        center_x: i32,
        center_z: i32,
        chunks: impl IntoIterator<Item = Chunk>,
    ) -> Result<Self, Error> {
        Self::try_from_chunks_sized(WorldSize::Small, center_x, center_z, chunks)
    }

    /// Same as [`try_from_chunks`], for a chosen finite size.
    pub fn try_from_chunks_sized(
        size: WorldSize,
        center_x: i32,
        center_z: i32,
        chunks: impl IntoIterator<Item = Chunk>,
    ) -> Result<Self, Error> {
        let mut cluster = Self::empty(size, center_x, center_z);
        for chunk in chunks {
            cluster.insert(chunk)?;
        }
        Ok(cluster)
    }

    fn empty(size: WorldSize, center_x: i32, center_z: i32) -> Self {
        let radius = size.chunk_radius();
        let mut chunks = Vec::with_capacity((size.side() * size.side()) as usize);
        for dz in -radius..=radius {
            for dx in -radius..=radius {
                chunks.push(
                    Chunk::from_blocks(center_x + dx, center_z + dz, std::iter::empty())
                        .expect("an empty chunk has no bad coordinates"),
                );
            }
        }
        Self {
            center_x,
            center_z,
            size,
            chunks,
        }
    }

    fn insert(&mut self, chunk: Chunk) -> Result<(), Error> {
        let index = self
            .slot(chunk.x(), chunk.z())
            .ok_or(Error::ChunkOutsideCluster {
                chunk_x: chunk.x(),
                chunk_z: chunk.z(),
            })?;
        self.chunks[index] = chunk;
        Ok(())
    }

    /// Replace one slot. A chunk outside the finite square is an error.
    pub fn replace_chunk(&mut self, chunk: Chunk) -> Result<(), Error> {
        self.insert(chunk)
    }

    fn slot(&self, x: i32, z: i32) -> Option<usize> {
        let radius = self.size.chunk_radius();
        let dx = x - self.center_x;
        let dz = z - self.center_z;
        if !(-radius..=radius).contains(&dx) || !(-radius..=radius).contains(&dz) {
            return None;
        }
        let side = self.size.side();
        Some(((dz + radius) * side + (dx + radius)) as usize)
    }
}

/// Load the small square around `(center_x, center_z)` from one region file.
///
/// Chunks that belong in a different region stay empty. Their local slot in
/// this file is a different chunk, so it is not read.
pub fn load_cluster_from_region(
    region: &[u8],
    center_x: i32,
    center_z: i32,
) -> Result<Cluster, Error> {
    let mut cluster = Cluster::empty(WorldSize::Small, center_x, center_z);
    let radius = cluster.size.chunk_radius();
    for dz in -radius..=radius {
        for dx in -radius..=radius {
            let x = center_x + dx;
            let z = center_z + dz;
            if !same_region(center_x, x) || !same_region(center_z, z) {
                continue;
            }
            match load_chunk_from_region(region, x, z) {
                Ok(chunk) => cluster.insert(chunk)?,
                Err(Error::ChunkNotFound { .. }) => {}
                Err(err) => return Err(err),
            }
        }
    }
    Ok(cluster)
}

/// Load the small square from `path`.
///
/// When `path` is named `r.X.Z.mca`, a chunk in another region is read from
/// `r.{region x}.{region z}.mca` beside it. A missing neighbor file is air.
/// Any other file name loads only the chunks that share the center's region.
pub fn load_cluster_from_path(path: &Path, center_x: i32, center_z: i32) -> Result<Cluster, Error> {
    let named = region_file_coords(path);
    let mut cluster = Cluster::empty(WorldSize::Small, center_x, center_z);
    let radius = cluster.size.chunk_radius();
    let mut opened: Vec<((i32, i32), Vec<u8>)> = Vec::new();
    for dz in -radius..=radius {
        for dx in -radius..=radius {
            let x = center_x + dx;
            let z = center_z + dz;
            let region_x = x.div_euclid(32);
            let region_z = z.div_euclid(32);
            let source = match &named {
                Some((_, file_x, file_z)) if region_x == *file_x && region_z == *file_z => {
                    path.to_path_buf()
                }
                Some((dir, _, _)) => {
                    let sibling = dir.join(format!("r.{region_x}.{region_z}.mca"));
                    if !sibling.is_file() {
                        continue;
                    }
                    sibling
                }
                None => {
                    if !same_region(center_x, x) || !same_region(center_z, z) {
                        continue;
                    }
                    path.to_path_buf()
                }
            };
            if !opened.iter().any(|(key, _)| *key == (region_x, region_z)) {
                opened.push(((region_x, region_z), fs::read(&source)?));
            }
            let bytes = &opened
                .iter()
                .find(|(key, _)| *key == (region_x, region_z))
                .expect("the region file was just read")
                .1;
            match load_chunk_from_region(bytes, x, z) {
                Ok(chunk) => cluster.insert(chunk)?,
                Err(Error::ChunkNotFound { .. }) => {}
                Err(err) => return Err(err),
            }
        }
    }
    Ok(cluster)
}

fn region_file_coords(path: &Path) -> Option<(std::path::PathBuf, i32, i32)> {
    let name = path.file_name()?.to_str()?;
    let (stem, ext) = name.rsplit_once('.')?;
    if !ext.eq_ignore_ascii_case("mca") {
        return None;
    }
    let mut parts = stem.split('.');
    if parts.next()? != "r" {
        return None;
    }
    let region_x: i32 = parts.next()?.parse().ok()?;
    let region_z: i32 = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some((path.parent()?.to_path_buf(), region_x, region_z))
}

fn same_region(a: i32, b: i32) -> bool {
    a.div_euclid(32) == b.div_euclid(32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunk::chunk_to_nbt;
    use crate::nbt;
    use crate::region::{encode_region, encode_region_many};

    fn region_bytes(chunks: &[Chunk]) -> Vec<u8> {
        let encoded: Vec<(i32, i32, Vec<u8>)> = chunks
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
        encode_region_many(&parts, 2)
    }

    #[test]
    fn outside_chunk_is_rejected() {
        let outside = WorldSize::Small.chunk_radius() + 1;
        let err = Cluster::try_from_chunks(
            0,
            0,
            [Chunk::from_blocks(outside, 0, [(0, 0, 0, Block::new("stone", 0))]).unwrap()],
        )
        .unwrap_err();
        assert!(matches!(
            err,
            Error::ChunkOutsideCluster {
                chunk_x: 16,
                chunk_z: 0
            }
        ));
    }

    #[test]
    fn block_at_reads_across_a_negative_border() {
        let chunk =
            Chunk::from_blocks(-1, -1, [(15, 3, 15, Block::new("cobblestone", 2))]).unwrap();
        let cluster = Cluster::try_from_chunks(0, 0, [chunk]).unwrap();
        let radius = WorldSize::Small.chunk_radius();
        assert_eq!(cluster.block_at(-1, 3, -1), Block::new("cobblestone", 2));
        assert!(cluster.block_at(-radius * 16 - 1, 3, -1).is_air());
        assert!(cluster.block_at(-1, -1, -1).is_air());
        assert!(cluster.block_at(-1, 256, -1).is_air());
        assert_eq!(
            cluster.bounds(),
            (
                -radius * 16,
                (radius + 1) * 16,
                -radius * 16,
                (radius + 1) * 16
            )
        );
    }

    #[test]
    fn missing_slot_is_an_empty_chunk() {
        let chunk = Chunk::from_blocks(0, 0, [(1, 2, 3, Block::new("cobblestone", 5))]).unwrap();
        let region = encode_region(0, 0, &nbt::write_root("", &chunk_to_nbt(&chunk)), 2);
        let cluster = load_cluster_from_region(&region, 0, 0).unwrap();
        let side = WorldSize::Small.side();
        assert_eq!(cluster.chunk_count(), (side * side) as usize);
        assert_eq!(cluster.block_at(1, 2, 3), Block::new("cobblestone", 5));
        assert_eq!(cluster.get(1, 0).unwrap().non_air_count(), 0);
        assert!(
            cluster
                .get(WorldSize::Small.chunk_radius() + 1, 0)
                .is_none()
        );
    }

    #[test]
    fn neighbor_in_another_region_file_stays_empty() {
        // Chunk 0 occupies local slot 0. Chunk 32 would alias that slot if
        // the loader ignored the region boundary.
        let decoy = Chunk::from_blocks(0, 0, [(0, 0, 0, Block::new("water", 1))]).unwrap();
        let keep = Chunk::from_blocks(31, 0, [(1, 2, 3, Block::new("cobblestone", 5))]).unwrap();
        let region = region_bytes(&[decoy, keep]);
        let cluster = load_cluster_from_region(&region, 31, 0).unwrap();
        assert_eq!(
            cluster.block_at(31 * 16 + 1, 2, 3),
            Block::new("cobblestone", 5)
        );
        assert_eq!(cluster.get(32, 0).unwrap().non_air_count(), 0);
        assert_eq!(cluster.get(30, 0).unwrap().non_air_count(), 0);
        assert!(cluster.block_at(0, 0, 0).is_air());
    }

    #[test]
    fn load_cluster_from_path_reads_a_file() {
        let chunk = Chunk::from_blocks(0, 0, [(1, 2, 3, Block::new("cobblestone", 5))]).unwrap();
        let region = encode_region(0, 0, &nbt::write_root("", &chunk_to_nbt(&chunk)), 2);
        let path =
            std::env::temp_dir().join(format!("rebuild-cluster-{}-0_0.mca", std::process::id()));
        std::fs::write(&path, &region).unwrap();
        let loaded = load_cluster_from_path(&path, 0, 0).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(loaded.block_at(1, 2, 3), Block::new("cobblestone", 5));
        let side = WorldSize::Small.side();
        assert_eq!(loaded.chunk_count(), (side * side) as usize);
    }

    #[test]
    fn named_region_does_not_alias_the_previous_region() {
        // Slot 31 of r.0.0.mca is chunk 31, not chunk -1.
        let chunk = Chunk::from_blocks(31, 0, [(0, 0, 0, Block::new("water", 1))]).unwrap();
        let region = encode_region(31, 0, &nbt::write_root("", &chunk_to_nbt(&chunk)), 2);
        let dir =
            std::env::temp_dir().join(format!("rebuild-regions-{}-alias", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("r.0.0.mca");
        std::fs::write(&path, &region).unwrap();
        let loaded = load_cluster_from_path(&path, 0, 0).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert!(loaded.block_at(-16, 0, 0).is_air());
        assert_eq!(loaded.get(-1, 0).unwrap().non_air_count(), 0);
        assert_eq!(loaded.non_air_count(), 0);
    }

    #[test]
    fn medium_reaches_past_the_small_square() {
        let past_small = WorldSize::Small.chunk_radius() + 1;
        let far = Chunk::from_blocks(past_small, 0, [(0, 1, 0, Block::new("stone", 0))]).unwrap();
        let medium = Cluster::try_from_chunks_sized(WorldSize::Medium, 0, 0, [far]).unwrap();
        let side = WorldSize::Medium.side();
        let radius = WorldSize::Medium.chunk_radius();
        assert_eq!(medium.size(), WorldSize::Medium);
        assert_eq!(medium.chunk_count(), (side * side) as usize);
        assert_eq!(
            medium.block_at(past_small * 16, 1, 0),
            Block::new("stone", 0)
        );
        assert_eq!(
            medium.bounds(),
            (
                -radius * 16,
                (radius + 1) * 16,
                -radius * 16,
                (radius + 1) * 16
            )
        );
        assert!(medium.get(radius + 1, 0).is_none());
        assert!(
            Cluster::try_from_chunks(
                0,
                0,
                [Chunk::from_blocks(past_small, 0, [(0, 1, 0, Block::new("stone", 0))]).unwrap()],
            )
            .is_err()
        );
    }
}
