use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::{Chunk, Manifest};

/// What a client has to do to turn an installed build into a newer one.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PatchPlan {
    /// Files that are new in the target build.
    pub added: Vec<String>,
    /// Files whose contents changed.
    pub changed: Vec<String>,
    /// Files that no longer exist in the target build.
    pub removed: Vec<String>,
    /// Files that are identical in both builds.
    pub unchanged: Vec<String>,
}

impl PatchPlan {
    /// True when the installed build already matches the target.
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.changed.is_empty() && self.removed.is_empty()
    }

    /// Bytes the client has to download to apply this plan.
    pub fn download_size(&self, target: &Manifest) -> u64 {
        self.added
            .iter()
            .chain(&self.changed)
            .filter_map(|path| target.files.get(path))
            .map(|f| f.size)
            .sum()
    }
}

/// Compare an installed manifest with a target manifest.
///
/// All lists in the returned plan are sorted by path.
pub fn diff(installed: &Manifest, target: &Manifest) -> PatchPlan {
    let mut plan = PatchPlan::default();

    for (path, entry) in &target.files {
        match installed.files.get(path) {
            None => plan.added.push(path.clone()),
            // A mode change alone also counts, so clients re-apply `+x`.
            Some(old) if old.hash != entry.hash || old.executable != entry.executable => {
                plan.changed.push(path.clone())
            }
            Some(_) => plan.unchanged.push(path.clone()),
        }
    }
    for path in installed.files.keys() {
        if !target.files.contains_key(path) {
            plan.removed.push(path.clone());
        }
    }

    plan
}

/// Chunks a target build needs that are not in a set of known chunks.
///
/// With `have` = the chunks of the installed build, this is what a client has
/// to download; with `have` = the chunks already in storage, it is what a push
/// has to upload. Sorted by hash, each chunk listed once.
pub fn missing_chunks<'a>(
    target: &Manifest,
    have: impl IntoIterator<Item = &'a str>,
) -> Vec<Chunk> {
    let have: BTreeSet<&str> = have.into_iter().collect();
    target
        .unique_chunks()
        .into_iter()
        .filter(|(hash, _)| !have.contains(hash.as_str()))
        .map(|(hash, size)| Chunk { hash, size })
        .collect()
}

/// Chunks needed to turn `installed` into `target`.
pub fn chunk_delta(installed: &Manifest, target: &Manifest) -> Vec<Chunk> {
    let installed = installed.unique_chunks();
    missing_chunks(target, installed.keys().map(String::as_str))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::FileEntry;
    use crate::test_support::{noise, small_chunks};

    fn manifest(files: &[(&str, &str, u64)]) -> Manifest {
        Manifest {
            chunker: Default::default(),
            files: files
                .iter()
                .map(|(path, hash, size)| {
                    (
                        path.to_string(),
                        FileEntry {
                            size: *size,
                            hash: hash.to_string(),
                            chunks: Vec::new(),
                            executable: false,
                        },
                    )
                })
                .collect(),
        }
    }

    #[test]
    fn classifies_every_file() {
        let installed = manifest(&[("keep", "1", 1), ("edit", "2", 2), ("gone", "3", 3)]);
        let target = manifest(&[("keep", "1", 1), ("edit", "9", 20), ("new", "4", 40)]);

        let plan = diff(&installed, &target);

        assert_eq!(plan.added, ["new"]);
        assert_eq!(plan.changed, ["edit"]);
        assert_eq!(plan.removed, ["gone"]);
        assert_eq!(plan.unchanged, ["keep"]);
        assert_eq!(plan.download_size(&target), 60);
        assert!(!plan.is_empty());
    }

    #[test]
    fn identical_builds_need_nothing() {
        let m = manifest(&[("a", "1", 1)]);
        assert!(diff(&m, &m).is_empty());
    }

    #[test]
    fn small_edit_to_large_file_only_needs_a_few_chunks() {
        let old_dir = tempfile::tempdir().unwrap();
        let new_dir = tempfile::tempdir().unwrap();
        let mut data = noise(7, 400_000);
        std::fs::write(old_dir.path().join("assets.pak"), &data).unwrap();
        // Insert bytes in the middle: fixed-size blocks would shift every
        // later block, content-defined chunks resynchronise.
        data.splice(200_000..200_000, *b"patched!");
        std::fs::write(new_dir.path().join("assets.pak"), &data).unwrap();

        let old = Manifest::from_dir_with(old_dir.path(), small_chunks()).unwrap();
        let new = Manifest::from_dir_with(new_dir.path(), small_chunks()).unwrap();
        let delta = chunk_delta(&old, &new);
        let total = new.unique_chunks().len();

        assert_eq!(diff(&old, &new).changed, ["assets.pak"]);
        assert!(!delta.is_empty());
        assert!(
            delta.len() <= 3,
            "{} of {total} chunks changed",
            delta.len()
        );
        assert!(delta.iter().map(|c| c.size).sum::<u64>() < new.total_size() / 4);
    }

    #[test]
    fn missing_chunks_skips_known_hashes() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a"), noise(8, 100_000)).unwrap();
        let manifest = Manifest::from_dir_with(dir.path(), small_chunks()).unwrap();
        let all = manifest.unique_chunks();
        let first = all.keys().next().unwrap().as_str();

        let missing = missing_chunks(&manifest, [first]);

        assert_eq!(missing.len(), all.len() - 1);
        assert!(missing.iter().all(|c| c.hash != first));
        assert!(missing_chunks(&manifest, all.keys().map(String::as_str)).is_empty());
    }
}
