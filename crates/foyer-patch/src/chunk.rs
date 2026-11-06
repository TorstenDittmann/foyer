use std::fs::File;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// A content-defined slice of a file, addressed by its BLAKE3 hash.
///
/// Chunks are stored once per hash in object storage, so identical chunks in
/// different files, builds or games are uploaded and downloaded only once.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Chunk {
    /// Lowercase hex BLAKE3 hash of the chunk bytes.
    pub hash: String,
    /// Size in bytes.
    pub size: u64,
}

/// Size limits for the FastCDC chunker.
///
/// The values are stored in every manifest so a build can always be
/// re-chunked the same way. Changing the defaults only affects new builds, but
/// it also means their chunks will not deduplicate against older builds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChunkerConfig {
    pub min_size: u32,
    pub avg_size: u32,
    pub max_size: u32,
}

impl Default for ChunkerConfig {
    /// 256 KiB / 1 MiB / 4 MiB: large enough that a multi-GB build has a
    /// manageable number of objects, small enough that a patch to one asset
    /// only re-uploads a few MiB.
    fn default() -> Self {
        Self {
            min_size: 256 * 1024,
            avg_size: 1024 * 1024,
            max_size: 4 * 1024 * 1024,
        }
    }
}

/// Lowercase hex BLAKE3 hash of `data`.
pub fn hash_bytes(data: &[u8]) -> String {
    blake3::hash(data).to_hex().to_string()
}

/// Result of chunking one file.
pub(crate) struct ChunkedFile {
    pub size: u64,
    pub hash: String,
    pub chunks: Vec<Chunk>,
}

/// Split a file into content-defined chunks and hash the whole file and
/// every chunk in one pass.
pub(crate) fn chunk_file(path: &Path, config: ChunkerConfig) -> io::Result<ChunkedFile> {
    let file = File::open(path)?;
    let mut file_hasher = blake3::Hasher::new();
    let mut chunks = Vec::new();
    let mut size = 0;

    let chunker = fastcdc::v2020::StreamCDC::new(
        file,
        config.min_size as usize,
        config.avg_size as usize,
        config.max_size as usize,
    );
    for chunk in chunker {
        let chunk = chunk.map_err(|err| match err {
            fastcdc::v2020::Error::IoError(err) => err,
            other => io::Error::other(other.to_string()),
        })?;
        file_hasher.update(&chunk.data);
        size += chunk.length as u64;
        chunks.push(Chunk {
            hash: hash_bytes(&chunk.data),
            size: chunk.length as u64,
        });
    }

    Ok(ChunkedFile {
        size,
        hash: file_hasher.finalize().to_hex().to_string(),
        chunks,
    })
}
