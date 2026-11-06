use std::collections::HashMap;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use crate::{ChunkLocation, Manifest, hash_bytes};

/// Reads chunk bytes out of a build directory described by a manifest.
///
/// Used by the CLI to upload the chunks the server is missing. Every read is
/// re-hashed, so a file that changed after the manifest was made is reported
/// instead of being uploaded under the wrong hash.
#[derive(Debug, Clone)]
pub struct BuildSource {
    root: PathBuf,
    locations: HashMap<String, ChunkLocation>,
}

#[derive(Debug, thiserror::Error)]
pub enum SourceError {
    #[error("chunk {0} is not part of this build")]
    UnknownChunk(String),
    #[error("failed to read {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("{path} changed since the manifest was created; run the push again")]
    Changed { path: PathBuf },
}

impl BuildSource {
    pub fn new(root: impl AsRef<Path>, manifest: &Manifest) -> Self {
        Self {
            root: root.as_ref().to_path_buf(),
            locations: manifest.chunk_locations(),
        }
    }

    /// Read and verify one chunk.
    pub fn read_chunk(&self, hash: &str) -> Result<Vec<u8>, SourceError> {
        let location = self
            .locations
            .get(hash)
            .ok_or_else(|| SourceError::UnknownChunk(hash.to_string()))?;
        let path = self.root.join(&location.path);
        let io_err = |source| SourceError::Io {
            path: path.clone(),
            source,
        };

        let mut file = File::open(&path).map_err(io_err)?;
        file.seek(SeekFrom::Start(location.offset))
            .map_err(io_err)?;
        let mut buf = Vec::with_capacity(location.size as usize);
        file.take(location.size)
            .read_to_end(&mut buf)
            .map_err(io_err)?;

        if buf.len() as u64 != location.size || hash_bytes(&buf) != hash {
            return Err(SourceError::Changed { path });
        }
        Ok(buf)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{noise, small_chunks};
    use std::fs;

    #[test]
    fn reads_every_chunk_back() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a"), noise(4, 120_000)).unwrap();
        fs::write(dir.path().join("b"), b"tiny").unwrap();
        let manifest = Manifest::from_dir_with(dir.path(), small_chunks()).unwrap();
        let source = BuildSource::new(dir.path(), &manifest);

        for (hash, size) in manifest.unique_chunks() {
            let bytes = source.read_chunk(&hash).unwrap();
            assert_eq!(bytes.len() as u64, size);
        }
        assert!(matches!(
            source.read_chunk(&"0".repeat(64)),
            Err(SourceError::UnknownChunk(_))
        ));
    }

    #[test]
    fn detects_files_changed_after_manifest() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a"), b"before").unwrap();
        let manifest = Manifest::from_dir(dir.path()).unwrap();
        fs::write(dir.path().join("a"), b"after!").unwrap();

        let source = BuildSource::new(dir.path(), &manifest);
        let hash = &manifest.files["a"].chunks[0].hash;
        assert!(matches!(
            source.read_chunk(hash),
            Err(SourceError::Changed { .. })
        ));
    }
}
