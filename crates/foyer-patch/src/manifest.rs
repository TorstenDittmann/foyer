use std::collections::{BTreeMap, HashMap};
use std::io;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::chunk::{Chunk, ChunkerConfig, chunk_file};

/// One file inside a build.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileEntry {
    /// Size in bytes.
    pub size: u64,
    /// Lowercase hex BLAKE3 hash of the file contents.
    pub hash: String,
    /// The file's content-defined chunks, in order. Concatenating them gives
    /// the file. Empty for an empty file.
    #[serde(default)]
    pub chunks: Vec<Chunk>,
    /// The file should be executable after install (Unix `+x`). Taken from
    /// the file mode on Unix; always false for builds pushed from Windows.
    /// Not part of the hash, so it does not affect chunk deduplication.
    #[serde(default, skip_serializing_if = "is_false")]
    pub executable: bool,
}

fn is_false(value: &bool) -> bool {
    !*value
}

/// Every file in a build, keyed by its path relative to the build root.
///
/// Paths always use `/` as separator so manifests created on Windows and
/// Unix are identical.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    /// Chunker settings the build was split with.
    #[serde(default)]
    pub chunker: ChunkerConfig,
    pub files: BTreeMap<String, FileEntry>,
}

/// Where a chunk's bytes can be read from inside a build directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChunkLocation {
    /// Manifest path of the file (with `/` separators).
    pub path: String,
    /// Byte offset of the chunk inside the file.
    pub offset: u64,
    /// Chunk size in bytes.
    pub size: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum ManifestError {
    #[error("failed to walk {path}: {source}")]
    Walk {
        path: PathBuf,
        #[source]
        source: walkdir::Error,
    },
    #[error("failed to read {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("path {0} is not valid UTF-8")]
    NonUtf8Path(PathBuf),
    #[error("invalid manifest: {0}")]
    Invalid(String),
}

impl Manifest {
    /// Hash and chunk every regular file below `root` with the default
    /// chunker settings.
    ///
    /// Symlinks are not followed and empty directories are not recorded.
    pub fn from_dir(root: impl AsRef<Path>) -> Result<Self, ManifestError> {
        Self::from_dir_with(root, ChunkerConfig::default())
    }

    /// Like [`Manifest::from_dir`] with explicit chunker settings.
    pub fn from_dir_with(
        root: impl AsRef<Path>,
        chunker: ChunkerConfig,
    ) -> Result<Self, ManifestError> {
        let root = root.as_ref();
        let mut files = BTreeMap::new();

        for entry in walkdir::WalkDir::new(root).follow_links(false) {
            let entry = entry.map_err(|source| ManifestError::Walk {
                path: root.to_path_buf(),
                source,
            })?;
            if !entry.file_type().is_file() {
                continue;
            }

            let path = entry.path();
            let relative = path
                .strip_prefix(root)
                .expect("walkdir yields paths below root");
            let key = relative_key(relative)?;
            let chunked = chunk_file(path, chunker).map_err(|source| ManifestError::Io {
                path: path.to_path_buf(),
                source,
            })?;
            files.insert(
                key,
                FileEntry {
                    size: chunked.size,
                    hash: chunked.hash,
                    chunks: chunked.chunks,
                    executable: is_executable(&entry),
                },
            );
        }

        Ok(Self { chunker, files })
    }

    /// Total size of all files in bytes.
    pub fn total_size(&self) -> u64 {
        self.files.values().map(|f| f.size).sum()
    }

    /// Every distinct chunk in the build, keyed by hash, with its size.
    pub fn unique_chunks(&self) -> BTreeMap<String, u64> {
        self.files
            .values()
            .flat_map(|f| &f.chunks)
            .map(|c| (c.hash.clone(), c.size))
            .collect()
    }

    /// Where to read each distinct chunk from (its first occurrence).
    pub fn chunk_locations(&self) -> HashMap<String, ChunkLocation> {
        let mut locations = HashMap::new();
        for (path, file) in &self.files {
            let mut offset = 0;
            for chunk in &file.chunks {
                locations
                    .entry(chunk.hash.clone())
                    .or_insert_with(|| ChunkLocation {
                        path: path.clone(),
                        offset,
                        size: chunk.size,
                    });
                offset += chunk.size;
            }
        }
        locations
    }

    /// Check that a manifest from an untrusted source is safe to act on.
    ///
    /// Rejects paths that could escape the install directory, malformed
    /// hashes, and chunk lists that do not add up to the file size. The API
    /// should call this on every pushed manifest and the client before
    /// applying one.
    pub fn validate(&self) -> Result<(), ManifestError> {
        let invalid = |msg: String| Err(ManifestError::Invalid(msg));
        for (path, file) in &self.files {
            if !is_safe_relative_path(path) {
                return invalid(format!("unsafe path {path:?}"));
            }
            if !is_hex_hash(&file.hash) {
                return invalid(format!("{path}: bad file hash"));
            }
            if let Some(chunk) = file.chunks.iter().find(|c| !is_hex_hash(&c.hash)) {
                return invalid(format!("{path}: bad chunk hash {:?}", chunk.hash));
            }
            if file.chunks.iter().any(|c| c.size == 0) {
                return invalid(format!("{path}: empty chunk"));
            }
            let chunk_total: u64 = file.chunks.iter().map(|c| c.size).sum();
            if chunk_total != file.size {
                return invalid(format!(
                    "{path}: chunks add up to {chunk_total} bytes, file is {}",
                    file.size
                ));
            }
        }
        Ok(())
    }
}

#[cfg(unix)]
fn is_executable(entry: &walkdir::DirEntry) -> bool {
    use std::os::unix::fs::PermissionsExt;
    entry
        .metadata()
        .is_ok_and(|m| m.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(_entry: &walkdir::DirEntry) -> bool {
    false
}

fn is_hex_hash(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

fn is_safe_relative_path(path: &str) -> bool {
    !path.is_empty()
        && !path.contains('\\')
        && !path.contains(':')
        && path
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
        && Path::new(path)
            .components()
            .all(|c| matches!(c, Component::Normal(_)))
}

fn relative_key(relative: &Path) -> Result<String, ManifestError> {
    let parts = relative
        .components()
        .map(|c| {
            c.as_os_str()
                .to_str()
                .ok_or_else(|| ManifestError::NonUtf8Path(relative.to_path_buf()))
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(parts.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{noise, small_chunks};
    use std::fs;

    #[test]
    fn hashes_nested_files_with_forward_slashes() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("data/levels")).unwrap();
        fs::write(dir.path().join("game.exe"), b"binary").unwrap();
        fs::write(dir.path().join("data/levels/1.lvl"), b"level one").unwrap();

        let manifest = Manifest::from_dir(dir.path()).unwrap();

        assert_eq!(
            manifest.files.keys().collect::<Vec<_>>(),
            ["data/levels/1.lvl", "game.exe"]
        );
        let exe = &manifest.files["game.exe"];
        assert_eq!(exe.size, 6);
        assert_eq!(exe.hash, blake3::hash(b"binary").to_hex().to_string());
        // A small file is a single chunk equal to the whole file.
        assert_eq!(
            exe.chunks,
            [Chunk {
                hash: exe.hash.clone(),
                size: 6
            }]
        );
        assert_eq!(manifest.total_size(), 15);
        manifest.validate().unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn records_the_executable_bit() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("game"), b"#!/bin/sh").unwrap();
        fs::write(dir.path().join("data.pak"), b"data").unwrap();
        fs::set_permissions(dir.path().join("game"), fs::Permissions::from_mode(0o755)).unwrap();
        fs::set_permissions(
            dir.path().join("data.pak"),
            fs::Permissions::from_mode(0o644),
        )
        .unwrap();
        let manifest = Manifest::from_dir(dir.path()).unwrap();
        assert!(manifest.files["game"].executable);
        assert!(!manifest.files["data.pak"].executable);

        // Only set flags are serialized, so older manifests stay identical.
        let json = serde_json::to_value(&manifest).unwrap();
        assert_eq!(json["files"]["game"]["executable"], true);
        assert!(json["files"]["data.pak"].get("executable").is_none());
    }

    #[test]
    fn empty_files_have_no_chunks() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("empty"), b"").unwrap();
        let manifest = Manifest::from_dir(dir.path()).unwrap();
        assert!(manifest.files["empty"].chunks.is_empty());
        manifest.validate().unwrap();
    }

    #[test]
    fn large_files_split_into_chunks_that_add_up() {
        let dir = tempfile::tempdir().unwrap();
        let data = noise(1, 200_000);
        fs::write(dir.path().join("big.pak"), &data).unwrap();

        let manifest = Manifest::from_dir_with(dir.path(), small_chunks()).unwrap();
        let file = &manifest.files["big.pak"];

        assert!(file.chunks.len() > 5, "got {} chunks", file.chunks.len());
        let mut offset = 0;
        for chunk in &file.chunks {
            let end = offset + chunk.size as usize;
            assert_eq!(chunk.hash, crate::hash_bytes(&data[offset..end]));
            offset = end;
        }
        assert_eq!(offset, data.len());
        manifest.validate().unwrap();
    }

    #[test]
    fn chunking_is_deterministic() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a"), noise(2, 100_000)).unwrap();
        let first = Manifest::from_dir_with(dir.path(), small_chunks()).unwrap();
        let second = Manifest::from_dir_with(dir.path(), small_chunks()).unwrap();
        assert_eq!(first, second);
    }

    #[test]
    fn duplicate_content_shares_chunks() {
        let dir = tempfile::tempdir().unwrap();
        let data = noise(3, 50_000);
        fs::write(dir.path().join("a"), &data).unwrap();
        fs::write(dir.path().join("b"), &data).unwrap();

        let manifest = Manifest::from_dir_with(dir.path(), small_chunks()).unwrap();
        let locations = manifest.chunk_locations();

        assert_eq!(
            manifest.unique_chunks().len(),
            manifest.files["a"].chunks.len()
        );
        assert!(locations.values().all(|l| l.path == "a"));
    }

    #[test]
    fn round_trips_through_json() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a.txt"), b"a").unwrap();
        let manifest = Manifest::from_dir(dir.path()).unwrap();

        let json = serde_json::to_string(&manifest).unwrap();
        assert_eq!(serde_json::from_str::<Manifest>(&json).unwrap(), manifest);
    }

    #[test]
    fn reads_manifests_without_chunk_fields() {
        let hash = "a".repeat(64);
        let json = format!(r#"{{"files":{{"x":{{"size":0,"hash":"{hash}"}}}}}}"#);
        let manifest: Manifest = serde_json::from_str(&json).unwrap();
        assert_eq!(manifest.chunker, ChunkerConfig::default());
        assert!(manifest.files["x"].chunks.is_empty());
    }

    #[test]
    fn validate_rejects_unsafe_manifests() {
        let hash = "a".repeat(64);
        let entry = |size, chunks: Vec<u64>| FileEntry {
            executable: false,
            size,
            hash: hash.clone(),
            chunks: chunks
                .into_iter()
                .map(|size| Chunk {
                    hash: hash.clone(),
                    size,
                })
                .collect(),
        };
        let with = |path: &str, file: FileEntry| Manifest {
            chunker: ChunkerConfig::default(),
            files: [(path.to_string(), file)].into(),
        };

        for path in [
            "../evil",
            "a/../../evil",
            "/etc/passwd",
            "C:/x",
            "a\\b",
            "a//b",
            "./a",
            "",
        ] {
            assert!(
                with(path, entry(1, vec![1])).validate().is_err(),
                "{path:?} accepted"
            );
        }
        assert!(with("ok", entry(3, vec![1, 1])).validate().is_err());
        assert!(with("ok", entry(0, vec![0])).validate().is_err());
        let mut bad_hash = entry(1, vec![1]);
        bad_hash.chunks[0].hash = "XYZ".into();
        assert!(with("ok", bad_hash).validate().is_err());
        with("dir/ok.bin", entry(3, vec![1, 2])).validate().unwrap();
    }
}
