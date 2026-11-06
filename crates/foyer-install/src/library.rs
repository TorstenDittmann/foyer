//! The local library: settings and the games installed on this computer.
//!
//! Stored as one JSON file (`library.json`) in the client's data directory
//! and written atomically. Each install also keeps its own state inside the
//! game folder (see [`crate::install`]), so the library can be rebuilt from
//! the folders if this file is lost.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use foyer_types::{Build, Game, MediaKind, Platform};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    /// Where new games are installed, one folder per game.
    pub install_root: PathBuf,
    /// API base URL; `FOYER_API_URL` wins when set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_url: Option<String>,
    /// Check for updates at startup and every half hour.
    #[serde(default = "yes")]
    pub check_updates: bool,
    /// Apply updates as soon as they are found.
    #[serde(default)]
    pub auto_update: bool,
}

fn yes() -> bool {
    true
}

impl Settings {
    pub fn with_root(install_root: PathBuf) -> Self {
        Self {
            install_root,
            api_url: None,
            check_updates: true,
            auto_update: false,
        }
    }
}

/// The build an install points at.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildRef {
    pub id: Uuid,
    pub version: Option<String>,
    /// Sum of all file sizes in bytes.
    pub size: u64,
}

impl From<&Build> for BuildRef {
    fn from(build: &Build) -> Self {
        Self {
            id: build.id,
            version: build.version.clone(),
            size: build.total_size,
        }
    }
}

/// A game in the local library: installed, or with an install in progress.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InstalledGame {
    pub game_id: Uuid,
    pub slug: String,
    pub title: String,
    pub developer: String,
    /// Capsule image URL, for the library grid.
    #[serde(default)]
    pub capsule_url: Option<String>,
    pub channel: String,
    pub platform: Platform,
    pub install_dir: PathBuf,
    /// The build on disk. `None` until the first install finishes.
    pub installed: Option<BuildRef>,
    /// A build being installed or paused part way. Resumed on request.
    #[serde(default)]
    pub pending: Option<BuildRef>,
    /// The newest build on the channel, when it differs from `installed`.
    #[serde(default)]
    pub update: Option<BuildRef>,
    /// Bytes the update needs to download, once known.
    #[serde(default)]
    pub update_download: Option<u64>,
    /// The game is no longer public; it keeps working but gets no updates.
    #[serde(default)]
    pub unavailable: bool,
    /// Executable to start, relative to `install_dir` with `/` separators.
    /// Chosen automatically after install; the player can change it.
    #[serde(default)]
    pub launch: Option<String>,
    #[serde(default)]
    pub launch_args: Vec<String>,
    pub added_at: DateTime<Utc>,
    #[serde(default)]
    pub updated_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub last_played: Option<DateTime<Utc>>,
}

impl InstalledGame {
    pub fn new(game: &Game, channel: &str, platform: Platform, install_dir: PathBuf) -> Self {
        Self {
            game_id: game.id,
            slug: game.slug.clone(),
            title: game.title.clone(),
            developer: game.developer.display_name.clone(),
            capsule_url: capsule_url(game),
            channel: channel.to_string(),
            platform,
            install_dir,
            installed: None,
            pending: None,
            update: None,
            update_download: None,
            unavailable: false,
            launch: None,
            launch_args: Vec::new(),
            added_at: Utc::now(),
            updated_at: None,
            last_played: None,
        }
    }

    pub fn is_installed(&self) -> bool {
        self.installed.is_some()
    }
}

pub fn capsule_url(game: &Game) -> Option<String> {
    game.media
        .iter()
        .find(|m| m.kind == MediaKind::Capsule)
        .or_else(|| game.media.iter().min_by_key(|m| m.position))
        .map(|m| m.url.clone())
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LibraryData {
    pub settings: Settings,
    /// By game slug.
    #[serde(default)]
    pub games: BTreeMap<String, InstalledGame>,
}

/// The library file and its contents.
#[derive(Debug, Clone)]
pub struct Library {
    path: PathBuf,
    pub data: LibraryData,
}

impl Library {
    /// Load `library.json` from `data_dir`, or start an empty library that
    /// installs into `default_root`.
    pub fn load(data_dir: &Path, default_root: PathBuf) -> io::Result<Self> {
        let path = data_dir.join("library.json");
        let data = match fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|err| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("{} is damaged: {err}", path.display()),
                )
            })?,
            Err(err) if err.kind() == io::ErrorKind::NotFound => LibraryData {
                settings: Settings::with_root(default_root),
                games: BTreeMap::new(),
            },
            Err(err) => return Err(err),
        };
        Ok(Self { path, data })
    }

    pub fn save(&self) -> io::Result<()> {
        let json = serde_json::to_vec_pretty(&self.data).map_err(io::Error::other)?;
        write_atomic(&self.path, &json)
    }

    pub fn get(&self, slug: &str) -> Option<&InstalledGame> {
        self.data.games.get(slug)
    }

    pub fn get_mut(&mut self, slug: &str) -> Option<&mut InstalledGame> {
        self.data.games.get_mut(slug)
    }

    pub fn upsert(&mut self, game: InstalledGame) {
        self.data.games.insert(game.slug.clone(), game);
    }

    pub fn remove(&mut self, slug: &str) -> Option<InstalledGame> {
        self.data.games.remove(slug)
    }

    /// A folder for a new install of `slug` under the install root that is
    /// not used yet.
    pub fn new_install_dir(&self, slug: &str) -> PathBuf {
        let root = &self.data.settings.install_root;
        let taken = |p: &Path| p.exists() || self.data.games.values().any(|g| g.install_dir == p);
        let first = root.join(slug);
        if !taken(&first) {
            return first;
        }
        (2..)
            .map(|n| root.join(format!("{slug}-{n}")))
            .find(|p| !taken(p))
            .expect("some suffix is free")
    }
}

/// Default folders: (data dir for `library.json`, install root).
pub fn default_dirs() -> (PathBuf, PathBuf) {
    let data = std::env::var_os("FOYER_DATA_DIR")
        .map(PathBuf::from)
        .or_else(|| dirs::data_dir().map(|d| d.join("Foyer")))
        .unwrap_or_else(|| PathBuf::from(".foyer"));
    let root = dirs::home_dir()
        .map(|h| h.join("Games").join("Foyer"))
        .unwrap_or_else(|| data.join("games"));
    (data, root)
}

/// Write a file by writing a sibling and renaming it over the target.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, bytes)?;
    fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_picks_free_folders() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("games");
        let mut library = Library::load(dir.path(), root.clone()).unwrap();
        assert!(library.data.games.is_empty());
        assert!(library.data.settings.check_updates);

        assert_eq!(library.new_install_dir("moss"), root.join("moss"));
        fs::create_dir_all(root.join("moss")).unwrap();
        assert_eq!(library.new_install_dir("moss"), root.join("moss-2"));

        library.data.settings.auto_update = true;
        library.save().unwrap();
        let again = Library::load(dir.path(), PathBuf::from("elsewhere")).unwrap();
        assert_eq!(again.data, library.data);
    }

    #[test]
    fn reports_a_damaged_file() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("library.json"), b"{nope").unwrap();
        let err = Library::load(dir.path(), PathBuf::new()).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }
}
