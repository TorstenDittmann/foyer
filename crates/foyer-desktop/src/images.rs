//! Cover art and screenshots, downloaded once into a cache folder and shown
//! from disk.

use std::collections::HashMap;
use std::path::PathBuf;

use foyer_install::Api;

#[derive(Debug, Clone)]
enum Slot {
    Loading,
    Ready(PathBuf),
    Failed,
}

#[derive(Debug)]
pub struct Images {
    dir: PathBuf,
    slots: HashMap<String, Slot>,
}

impl Images {
    pub fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            slots: HashMap::new(),
        }
    }

    /// The image file for `url` if it is on disk. Returns `Err(true)` when
    /// the caller should start a download.
    pub fn get(&mut self, url: &str) -> Result<PathBuf, bool> {
        match self.slots.get(url) {
            Some(Slot::Ready(path)) => Ok(path.clone()),
            Some(Slot::Loading | Slot::Failed) => Err(false),
            None => {
                let path = self.path_for(url);
                if path.is_file() {
                    self.slots
                        .insert(url.to_string(), Slot::Ready(path.clone()));
                    Ok(path)
                } else {
                    self.slots.insert(url.to_string(), Slot::Loading);
                    Err(true)
                }
            }
        }
    }

    pub fn finish(&mut self, url: &str, result: Option<PathBuf>) {
        let slot = result.map(Slot::Ready).unwrap_or(Slot::Failed);
        self.slots.insert(url.to_string(), slot);
    }

    pub fn path_for(&self, url: &str) -> PathBuf {
        let ext = url
            .rsplit('/')
            .next()
            .and_then(|name| name.split('?').next())
            .and_then(|name| name.rsplit_once('.').map(|(_, ext)| ext.to_lowercase()))
            .filter(|ext| matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "webp" | "gif"))
            .unwrap_or_else(|| "img".into());
        let key = blake3::hash(url.as_bytes()).to_hex();
        self.dir.join(format!("{}.{ext}", &key[..32]))
    }
}

/// Download `url` to `path`. Returns the path on success.
pub async fn download(api: Api, url: String, path: PathBuf) -> Option<PathBuf> {
    let bytes = api.fetch(&url).await.ok()?;
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await.ok()?;
    }
    let part = path.with_extension("part");
    tokio::fs::write(&part, bytes).await.ok()?;
    tokio::fs::rename(&part, &path).await.ok()?;
    Some(path)
}
