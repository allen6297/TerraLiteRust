//! Anvil region files (`.mca`), the 1.8 container for 32×32 chunks.
//!
//! The first 4096 bytes are chunk locations (3-byte sector offset, 1-byte
//! sector count). The next 4096 bytes are timestamps, which this loader does
//! not use. A chunk payload is a big-endian length, a compression byte
//! (1 = gzip, 2 = zlib), then the compressed NBT.

use std::fs;
use std::io::{Read, Write};
use std::path::Path;

use flate2::Compression;
use flate2::read::{GzDecoder, ZlibDecoder};
use flate2::write::{GzEncoder, ZlibEncoder};

use crate::chunk::{Chunk, chunk_from_root};
use crate::error::Error;
use crate::nbt;

const SECTOR: usize = 4096;
const HEADER: usize = SECTOR * 2;
const MAX_DECOMPRESSED: usize = 8 * 1024 * 1024;

/// Load the one chunk at `(chunk_x, chunk_z)` from an already-read region file.
pub fn load_chunk_from_region(region: &[u8], chunk_x: i32, chunk_z: i32) -> Result<Chunk, Error> {
    if region.len() < HEADER {
        return Err(Error::Truncated("header"));
    }
    let index = location_index(chunk_x, chunk_z);
    let (offset, count) = read_location(region, index);
    if offset == 0 && count == 0 {
        return Err(Error::ChunkNotFound { chunk_x, chunk_z });
    }
    if offset < 2 || count == 0 {
        return Err(Error::Truncated("chunk location"));
    }

    let start = (offset as usize)
        .checked_mul(SECTOR)
        .ok_or(Error::Truncated("sector offset"))?;
    let sector_bytes = (count as usize)
        .checked_mul(SECTOR)
        .ok_or(Error::Truncated("sector count"))?;
    let sector_end = start
        .checked_add(sector_bytes)
        .ok_or(Error::Truncated("sector range"))?;
    if sector_end > region.len() || start + 5 > region.len() {
        return Err(Error::Truncated("chunk payload"));
    }

    let mut len_bytes = [0u8; 4];
    len_bytes.copy_from_slice(&region[start..start + 4]);
    let len = u32::from_be_bytes(len_bytes) as usize;
    if len < 1 {
        return Err(Error::Truncated("chunk length"));
    }
    let payload_end = start
        .checked_add(4 + len)
        .ok_or(Error::Truncated("chunk length"))?;
    if payload_end > region.len() || payload_end > sector_end {
        return Err(Error::Truncated("chunk payload"));
    }

    let compression = region[start + 4];
    let compressed = &region[start + 5..payload_end];
    let nbt_bytes = decompress(compression, compressed)?;
    let (_name, root) = nbt::read_root(&nbt_bytes)?;
    chunk_from_root(&root, (chunk_x, chunk_z))
}

/// Read a region file from disk and load one chunk.
pub fn load_chunk_from_path(path: &Path, chunk_x: i32, chunk_z: i32) -> Result<Chunk, Error> {
    let bytes = fs::read(path)?;
    load_chunk_from_region(&bytes, chunk_x, chunk_z)
}

pub(crate) fn encode_region(chunk_x: i32, chunk_z: i32, nbt: &[u8], compression: u8) -> Vec<u8> {
    encode_region_many(&[(chunk_x, chunk_z, nbt)], compression)
}

pub(crate) fn encode_region_many(chunks: &[(i32, i32, &[u8])], compression: u8) -> Vec<u8> {
    struct Piece {
        index: usize,
        sectors: usize,
        payload_len: usize,
        compressed: Vec<u8>,
    }
    let mut pieces = Vec::with_capacity(chunks.len());
    for &(chunk_x, chunk_z, nbt) in chunks {
        let compressed = match compression {
            1 => gzip_compress(nbt),
            2 => zlib_compress(nbt),
            other => panic!("test encoder only writes gzip or zlib, not {other}"),
        };
        // Length counts the compression byte plus the compressed bytes.
        let payload_len = 1 + compressed.len();
        let sectors = (4 + payload_len).div_ceil(SECTOR);
        if sectors > 255 {
            panic!("chunk occupies more than 255 sectors");
        }
        pieces.push(Piece {
            index: location_index(chunk_x, chunk_z),
            sectors,
            payload_len,
            compressed,
        });
    }
    let total_sectors: usize = pieces.iter().map(|piece| piece.sectors).sum();
    let mut file = vec![0u8; HEADER + total_sectors * SECTOR];
    let mut sector = 2usize;
    for piece in pieces {
        let entry = ((sector as u32) << 8) | (piece.sectors as u32);
        let header = piece.index * 4;
        file[header..header + 4].copy_from_slice(&entry.to_be_bytes());
        let start = sector * SECTOR;
        file[start..start + 4].copy_from_slice(&(piece.payload_len as u32).to_be_bytes());
        file[start + 4] = compression;
        let body = start + 5;
        file[body..body + piece.compressed.len()].copy_from_slice(&piece.compressed);
        sector += piece.sectors;
    }
    file
}

fn location_index(chunk_x: i32, chunk_z: i32) -> usize {
    let local_x = chunk_x.rem_euclid(32) as usize;
    let local_z = chunk_z.rem_euclid(32) as usize;
    local_x + local_z * 32
}

fn read_location(region: &[u8], index: usize) -> (u32, u32) {
    let start = index * 4;
    let mut bytes = [0u8; 4];
    bytes.copy_from_slice(&region[start..start + 4]);
    let entry = u32::from_be_bytes(bytes);
    (entry >> 8, entry & 0xFF)
}

fn decompress(compression: u8, data: &[u8]) -> Result<Vec<u8>, Error> {
    match compression {
        1 => read_limited(GzDecoder::new(data)),
        2 => read_limited(ZlibDecoder::new(data)),
        other => Err(Error::BadCompression(other)),
    }
}

fn read_limited(mut reader: impl Read) -> Result<Vec<u8>, Error> {
    let mut out = Vec::new();
    let mut buf = [0u8; 8192];
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        if out.len().saturating_add(n) > MAX_DECOMPRESSED {
            return Err(Error::DecompressedTooLarge);
        }
        out.extend_from_slice(&buf[..n]);
    }
    Ok(out)
}

fn zlib_compress(data: &[u8]) -> Vec<u8> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(data).expect("zlib compress");
    encoder.finish().expect("zlib compress")
}

fn gzip_compress(data: &[u8]) -> Vec<u8> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(data).expect("gzip compress");
    encoder.finish().expect("gzip compress")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunk::{Block, Chunk, chunk_to_nbt};
    use crate::nbt;

    fn region_for(chunk: &Chunk, compression: u8) -> Vec<u8> {
        let nbt = nbt::write_root("", &chunk_to_nbt(chunk));
        encode_region(chunk.x(), chunk.z(), &nbt, compression)
    }

    #[test]
    fn missing_slot_and_empty_header() {
        let empty = vec![0u8; HEADER];
        let err = load_chunk_from_region(&empty, 0, 0).unwrap_err();
        assert!(matches!(
            err,
            Error::ChunkNotFound {
                chunk_x: 0,
                chunk_z: 0
            }
        ));

        let short = vec![0u8; 100];
        assert!(matches!(
            load_chunk_from_region(&short, 0, 0),
            Err(Error::Truncated("header"))
        ));
    }

    #[test]
    fn negative_chunk_uses_positive_local_slot() {
        let chunk = Chunk::from_blocks(-1, -33, [(3, 5, 7, Block::new("stone", 2))]).unwrap();
        let region = region_for(&chunk, 2);
        let loaded = load_chunk_from_region(&region, -1, -33).unwrap();
        assert_eq!(loaded.block(3, 5, 7), Block::new("stone", 2));
        // (-1, -33) lands in local slot (31, 31). Slot (0, 0) stays empty.
        let slot = location_index(-1, -33) * 4;
        let occupied = u32::from_be_bytes(region[slot..slot + 4].try_into().unwrap());
        assert_ne!(occupied, 0);
        let empty = u32::from_be_bytes(region[0..4].try_into().unwrap());
        assert_eq!(empty, 0);
    }

    #[test]
    fn gzip_chunk_roundtrips() {
        let chunk = Chunk::from_blocks(2, 3, [(1, 2, 3, Block::new("cobblestone", 5))]).unwrap();
        let region = region_for(&chunk, 1);
        let loaded = load_chunk_from_region(&region, 2, 3).unwrap();
        assert_eq!(loaded.block(1, 2, 3), Block::new("cobblestone", 5));
    }

    #[test]
    fn wrong_nbt_coordinates_are_rejected() {
        let chunk = Chunk::from_blocks(5, 6, [(0, 0, 0, Block::new("stone", 0))]).unwrap();
        let nbt = nbt::write_root("", &chunk_to_nbt(&chunk));
        // Header slot says (0, 0); the NBT still says (5, 6).
        let region = encode_region(0, 0, &nbt, 2);
        assert!(matches!(
            load_chunk_from_region(&region, 0, 0),
            Err(Error::ChunkCoordMismatch { .. })
        ));
    }

    #[test]
    fn truncated_payload_and_bad_compression() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 0, 0, Block::new("stone", 0))]).unwrap();
        let mut region = region_for(&chunk, 2);
        region.truncate(100);
        assert!(matches!(
            load_chunk_from_region(&region, 0, 0),
            Err(Error::Truncated(_))
        ));

        let mut region = region_for(&chunk, 2);
        region[2 * SECTOR + 4] = 9;
        assert!(matches!(
            load_chunk_from_region(&region, 0, 0),
            Err(Error::BadCompression(9))
        ));
    }

    #[test]
    fn load_chunk_from_path_reads_a_file() {
        let chunk = Chunk::from_blocks(4, -2, [(1, 2, 3, Block::new("planks", 6))]).unwrap();
        let region = region_for(&chunk, 2);
        let path =
            std::env::temp_dir().join(format!("rebuild-anvil-{}-4_-2.mca", std::process::id()));
        std::fs::write(&path, &region).unwrap();
        let loaded = crate::load_chunk_from_path(&path, 4, -2).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(loaded.block(1, 2, 3), Block::new("planks", 6));
        assert_eq!((loaded.x(), loaded.z()), (4, -2));
    }

    #[test]
    fn two_chunks_in_one_region_roundtrip() {
        let first = Chunk::from_blocks(0, 0, [(1, 0, 2, Block::new("dirt", 4))]).unwrap();
        let second =
            Chunk::from_blocks(-1, 1, [(15, 4, 0, Block::new("flowing_water", 1))]).unwrap();
        let first_nbt = nbt::write_root("", &chunk_to_nbt(&first));
        let second_nbt = nbt::write_root("", &chunk_to_nbt(&second));
        let region = encode_region_many(&[(0, 0, &first_nbt), (-1, 1, &second_nbt)], 2);
        let loaded_first = load_chunk_from_region(&region, 0, 0).unwrap();
        let loaded_second = load_chunk_from_region(&region, -1, 1).unwrap();
        assert_eq!(loaded_first.block(1, 0, 2), Block::new("dirt", 4));
        assert_eq!(
            loaded_second.block(15, 4, 0),
            Block::new("flowing_water", 1)
        );
        assert!(matches!(
            load_chunk_from_region(&region, 1, 0),
            Err(Error::ChunkNotFound { .. })
        ));
    }

    #[test]
    fn asking_for_a_different_chunk_is_not_found() {
        let chunk = Chunk::from_blocks(0, 0, [(0, 0, 0, Block::new("stone", 0))]).unwrap();
        let region = region_for(&chunk, 2);
        assert!(matches!(
            load_chunk_from_region(&region, 1, 0),
            Err(Error::ChunkNotFound {
                chunk_x: 1,
                chunk_z: 0
            })
        ));
    }
}
