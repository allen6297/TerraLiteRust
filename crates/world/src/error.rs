use std::fmt;
use std::io;

/// Failure while reading 1.8 Anvil chunks.
#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    /// The region header has no chunk at this coordinate.
    ChunkNotFound {
        chunk_x: i32,
        chunk_z: i32,
    },
    /// The file ended before a header or payload that it claimed to contain.
    Truncated(&'static str),
    /// Anvil compression type is not gzip (1) or zlib (2).
    BadCompression(u8),
    DecompressedTooLarge,
    Nbt(&'static str),
    ChunkCoordMismatch {
        expected_x: i32,
        expected_z: i32,
        found_x: i32,
        found_z: i32,
    },
    BadSection(&'static str),
    BadBlockCoord,
    /// The chunk coordinate is outside the loaded square.
    ChunkOutsideCluster {
        chunk_x: i32,
        chunk_z: i32,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(err) => write!(f, "{err}"),
            Error::ChunkNotFound { chunk_x, chunk_z } => {
                write!(f, "chunk ({chunk_x}, {chunk_z}) is not in this region")
            }
            Error::Truncated(what) => write!(f, "truncated region file ({what})"),
            Error::BadCompression(kind) => write!(
                f,
                "unsupported chunk compression type {kind} (expected gzip or zlib)"
            ),
            Error::DecompressedTooLarge => {
                write!(f, "decompressed chunk exceeds the safety limit")
            }
            Error::Nbt(what) => write!(f, "invalid chunk NBT ({what})"),
            Error::ChunkCoordMismatch {
                expected_x,
                expected_z,
                found_x,
                found_z,
            } => write!(
                f,
                "chunk NBT says ({found_x}, {found_z}), asked for ({expected_x}, {expected_z})"
            ),
            Error::BadSection(what) => write!(f, "invalid chunk section ({what})"),
            Error::BadBlockCoord => write!(f, "block coordinate is outside the chunk"),
            Error::ChunkOutsideCluster { chunk_x, chunk_z } => {
                write!(f, "chunk ({chunk_x}, {chunk_z}) is outside this cluster")
            }
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io(err) => Some(err),
            _ => None,
        }
    }
}

impl From<io::Error> for Error {
    fn from(err: io::Error) -> Self {
        Error::Io(err)
    }
}
