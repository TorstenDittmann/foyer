use std::collections::BTreeMap;
use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// One file inside a build.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileEntry {
    /// Size in bytes.
    pub size: u64,
    /// Lowercase hex BLAKE3 hash of the file contents.
    pub hash: String,
}

/// Every file in a build, keyed by its path relative to the build root.
///
/// Paths always use `/` as separator so manifests created on Windows and
/// Unix are identical.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub files: BTreeMap<String, FileEntry>,
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
}

impl Manifest {
    /// Hash every regular file below `root`.
    ///
    /// Symlinks are not followed and empty directories are not recorded.
    pub fn from_dir(root: impl AsRef<Path>) -> Result<Self, ManifestError> {
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
            let file_entry = hash_file(path)?;
            files.insert(key, file_entry);
        }

        Ok(Self { files })
    }

    /// Total size of all files in bytes.
    pub fn total_size(&self) -> u64 {
        self.files.values().map(|f| f.size).sum()
    }
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

fn hash_file(path: &Path) -> Result<FileEntry, ManifestError> {
    let io_err = |source| ManifestError::Io {
        path: path.to_path_buf(),
        source,
    };
    let mut file = File::open(path).map_err(io_err)?;
    let mut hasher = blake3::Hasher::new();
    let size = io::copy(&mut file, &mut hasher).map_err(io_err)?;
    Ok(FileEntry {
        size,
        hash: hasher.finalize().to_hex().to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
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
        assert_eq!(manifest.total_size(), 15);
    }

    #[test]
    fn round_trips_through_json() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a.txt"), b"a").unwrap();
        let manifest = Manifest::from_dir(dir.path()).unwrap();

        let json = serde_json::to_string(&manifest).unwrap();
        assert_eq!(serde_json::from_str::<Manifest>(&json).unwrap(), manifest);
    }
}
