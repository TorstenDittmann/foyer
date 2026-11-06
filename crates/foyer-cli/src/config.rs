//! Where the CLI keeps its login.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::{Deserialize, Serialize};

pub const DEFAULT_API_URL: &str = "https://api.foyer.lol";

/// Saved by `foyer login`, in `<config dir>/foyer/credentials.json`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Credentials {
    pub api_url: Option<String>,
    pub token: Option<String>,
}

/// Config directory: `$FOYER_CONFIG_DIR`, else the OS config dir.
pub fn config_dir() -> anyhow::Result<PathBuf> {
    if let Some(dir) = std::env::var_os("FOYER_CONFIG_DIR") {
        return Ok(PathBuf::from(dir));
    }
    dirs::config_dir()
        .map(|d| d.join("foyer"))
        .context("could not find a config directory; set FOYER_CONFIG_DIR")
}

fn credentials_path(dir: &Path) -> PathBuf {
    dir.join("credentials.json")
}

impl Credentials {
    pub fn load(dir: &Path) -> anyhow::Result<Self> {
        let path = credentials_path(dir);
        match fs::read_to_string(&path) {
            Ok(json) => serde_json::from_str(&json)
                .with_context(|| format!("{} is not valid JSON", path.display())),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(err) => Err(err).with_context(|| format!("failed to read {}", path.display())),
        }
    }

    pub fn save(&self, dir: &Path) -> anyhow::Result<PathBuf> {
        fs::create_dir_all(dir).with_context(|| format!("failed to create {}", dir.display()))?;
        let path = credentials_path(dir);
        let json = serde_json::to_string_pretty(self)?;
        write_private(&path, json.as_bytes())
            .with_context(|| format!("failed to write {}", path.display()))?;
        Ok(path)
    }

    pub fn remove(dir: &Path) -> anyhow::Result<bool> {
        match fs::remove_file(credentials_path(dir)) {
            Ok(()) => Ok(true),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(err) => Err(err.into()),
        }
    }
}

/// Write a file only the current user can read (the token is a secret).
#[cfg(unix)]
fn write_private(path: &Path, data: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    file.set_permissions(fs::Permissions::from_mode(0o600))?;
    file.write_all(data)
}

#[cfg(not(unix))]
fn write_private(path: &Path, data: &[u8]) -> std::io::Result<()> {
    fs::write(path, data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_load_remove() {
        let dir = tempfile::tempdir().unwrap();
        let dir = dir.path().join("nested");
        assert_eq!(Credentials::load(&dir).unwrap(), Credentials::default());

        let creds = Credentials {
            api_url: Some("http://localhost".into()),
            token: Some("secret".into()),
        };
        let path = creds.save(&dir).unwrap();
        assert_eq!(Credentials::load(&dir).unwrap(), creds);

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        let _ = path;

        assert!(Credentials::remove(&dir).unwrap());
        assert!(!Credentials::remove(&dir).unwrap());
    }
}
