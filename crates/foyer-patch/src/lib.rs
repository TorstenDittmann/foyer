//! Foyer patch engine.
//!
//! A game build is described by a [`Manifest`]: every file in the build
//! directory, its size, its BLAKE3 hash, and its content-defined chunks
//! (FastCDC). Chunks are addressed by hash, so storage keeps each one once and
//! an update moves only the chunks that changed.
//!
//! - [`diff`] compares two manifests file by file ([`PatchPlan`]).
//! - [`chunk_delta`] and [`missing_chunks`] list the chunks that have to be
//!   transferred, for a client update or a push.
//! - [`BuildSource`] reads verified chunk bytes from a build directory.
//!
//! This crate is shared by the API (which validates pushed manifests), the CLI
//! (which creates manifests and uploads chunks) and the desktop client (which
//! downloads chunks and assembles files).

mod chunk;
mod manifest;
mod plan;
mod source;

pub use chunk::{Chunk, ChunkerConfig, hash_bytes};
pub use manifest::{ChunkLocation, FileEntry, MAX_CHUNK_SIZE, Manifest, ManifestError};
pub use plan::{PatchPlan, chunk_delta, diff, missing_chunks};
pub use source::{BuildSource, SourceError};

#[cfg(test)]
pub(crate) mod test_support {
    use crate::ChunkerConfig;

    /// Small chunk sizes so tests exercise chunking without megabytes of data.
    pub fn small_chunks() -> ChunkerConfig {
        ChunkerConfig {
            min_size: 2 * 1024,
            avg_size: 8 * 1024,
            max_size: 32 * 1024,
        }
    }

    /// Deterministic incompressible bytes (xorshift).
    pub fn noise(seed: u64, len: usize) -> Vec<u8> {
        let mut state = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
        (0..len)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                (state >> 24) as u8
            })
            .collect()
    }
}
