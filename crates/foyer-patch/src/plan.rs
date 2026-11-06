use serde::{Deserialize, Serialize};

use crate::Manifest;

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
            Some(old) if old.hash != entry.hash => plan.changed.push(path.clone()),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::FileEntry;

    fn manifest(files: &[(&str, &str, u64)]) -> Manifest {
        Manifest {
            files: files
                .iter()
                .map(|(path, hash, size)| {
                    (
                        path.to_string(),
                        FileEntry {
                            size: *size,
                            hash: hash.to_string(),
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
}
