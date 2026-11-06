//! Updates for the Foyer client itself (not the games it installs).
//!
//! Every desktop release publishes `foyer-desktop.json` next to its files on
//! GitHub Releases. The client reads it, downloads the file for its platform
//! and checks it three ways before touching anything: the size, the SHA-256
//! and a [minisign](https://jedisct1.github.io/minisign/) signature made with
//! the release key. The signature's trusted comment names the version and
//! platform, so an old (but genuinely signed) file can't be passed off as a
//! newer one. A build without the public key compiled in still learns about
//! new versions, but only offers the download page.
//!
//! How the new version goes in depends on how Foyer was installed
//! ([`InstallKind`]):
//!
//! - **Windows installer**: run the new installer silently; it closes Foyer,
//!   replaces the files and starts it again.
//! - **AppImage**: write the new AppImage next to the old one and rename it
//!   over the top, then start it.
//! - **macOS app bundle**: unpack the new `Foyer.app` next to the old one,
//!   swap the two folders, then start it.
//! - Anything else (a `.deb`, a portable copy, `cargo run`) is left to the
//!   player or their package manager.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use minisign_verify::{PublicKey, Signature};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

/// Where released clients look for the newest version.
pub const DEFAULT_MANIFEST_URL: &str =
    "https://github.com/TorstenDittmann/foyer/releases/latest/download/foyer-desktop.json";

/// The website's download page, for installs that can't update themselves.
pub const DOWNLOAD_PAGE: &str = "https://foyer.lol/app";

/// `foyer-desktop.json`: the newest release and one file per platform.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Release {
    pub version: String,
    #[serde(default)]
    pub notes_url: Option<String>,
    #[serde(default)]
    pub published_at: Option<String>,
    /// Keyed by [`target`], e.g. `windows-x86_64`.
    #[serde(default)]
    pub assets: BTreeMap<String, Asset>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Asset {
    pub url: String,
    pub size: u64,
    /// Lowercase hex.
    pub sha256: String,
    /// The `.minisig` file's text. Missing when the release wasn't signed.
    #[serde(default)]
    pub signature: Option<String>,
}

impl Release {
    /// Whether this release is newer than `current`. Unparseable versions
    /// are never newer, so a broken manifest can't loop updates.
    pub fn is_newer_than(&self, current: &str) -> bool {
        match (
            semver::Version::parse(self.version.trim_start_matches('v')),
            semver::Version::parse(current.trim_start_matches('v')),
        ) {
            (Ok(new), Ok(current)) => new > current,
            _ => false,
        }
    }

    /// The file for `target`.
    pub fn asset(&self, target: &str) -> Option<&Asset> {
        self.assets.get(target)
    }
}

/// This build's key in [`Release::assets`]: `<os>-<arch>`, e.g.
/// `windows-x86_64` or `macos-aarch64`.
pub fn target() -> String {
    format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)
}

/// The trusted comment a release signs into every file's signature.
pub fn trusted_comment(version: &str, target: &str) -> String {
    format!("foyer-desktop {version} {target}")
}

#[derive(Debug, thiserror::Error)]
pub enum UpdateError {
    #[error("could not reach the update server: {0}")]
    Http(#[from] reqwest::Error),
    #[error("{0}")]
    Io(#[from] io::Error),
    #[error("the update file is {got} bytes, expected {expected}")]
    Size { expected: u64, got: u64 },
    #[error("the update file's checksum does not match")]
    Checksum,
    #[error("the update file's signature is not valid: {0}")]
    Signature(String),
    #[error("this release is not signed")]
    Unsigned,
    #[error("there is no update file for this platform ({0})")]
    NoAsset(String),
    #[error("this copy of Foyer can't update itself")]
    Unsupported,
}

/// Checks a downloaded file against its manifest entry and, when this build
/// carries a public key, its signature.
pub fn verify(
    bytes: &[u8],
    asset: &Asset,
    version: &str,
    target: &str,
    public_key: Option<&str>,
) -> Result<(), UpdateError> {
    if bytes.len() as u64 != asset.size {
        return Err(UpdateError::Size {
            expected: asset.size,
            got: bytes.len() as u64,
        });
    }
    if !hex(&Sha256::digest(bytes)).eq_ignore_ascii_case(asset.sha256.trim()) {
        return Err(UpdateError::Checksum);
    }
    let Some(key) = public_key else {
        return Ok(());
    };
    let signature = asset.signature.as_deref().ok_or(UpdateError::Unsigned)?;
    verify_signature(bytes, signature, key, &trusted_comment(version, target))
}

/// Checks a minisign signature and its trusted comment.
pub fn verify_signature(
    bytes: &[u8],
    signature: &str,
    public_key: &str,
    expected_comment: &str,
) -> Result<(), UpdateError> {
    let bad = |e: minisign_verify::Error| UpdateError::Signature(e.to_string());
    let key = PublicKey::from_base64(public_key.trim()).map_err(bad)?;
    let signature = Signature::decode(signature).map_err(bad)?;
    key.verify(bytes, &signature, false).map_err(bad)?;
    // Only checked after `verify`, which covers the trusted comment too.
    if signature.trusted_comment() != expected_comment {
        return Err(UpdateError::Signature(format!(
            "it was made for \"{}\"",
            signature.trusted_comment()
        )));
    }
    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

/// How this copy of Foyer was installed, which decides how it updates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallKind {
    /// Installed by the Windows setup program.
    WindowsInstaller,
    /// Running from an AppImage at this path.
    AppImage(PathBuf),
    /// Running from a macOS app bundle at this path, in a folder Foyer can
    /// write to.
    MacBundle(PathBuf),
    /// Anything else: a package manager, a portable copy or a dev build.
    Unmanaged,
}

impl InstallKind {
    /// Looks at the running executable (and `$APPIMAGE`).
    pub fn detect() -> Self {
        let Ok(exe) = std::env::current_exe() else {
            return Self::Unmanaged;
        };
        let appimage = std::env::var_os("APPIMAGE").map(PathBuf::from);
        Self::detect_from(&exe, appimage.as_deref())
    }

    fn detect_from(exe: &Path, appimage: Option<&Path>) -> Self {
        if cfg!(target_os = "windows") {
            // Inno Setup puts its uninstaller next to the program.
            return match exe.parent() {
                Some(dir) if dir.join("unins000.exe").is_file() => Self::WindowsInstaller,
                _ => Self::Unmanaged,
            };
        }
        if cfg!(target_os = "linux") {
            return match appimage {
                Some(path) if path.is_file() && writable(path.parent()) => {
                    Self::AppImage(path.to_path_buf())
                }
                _ => Self::Unmanaged,
            };
        }
        if cfg!(target_os = "macos") {
            // <bundle>.app/Contents/MacOS/<exe>
            let bundle = exe
                .parent()
                .filter(|p| p.ends_with("Contents/MacOS"))
                .and_then(Path::parent)
                .and_then(Path::parent)
                .filter(|p| p.extension().is_some_and(|e| e == "app"));
            return match bundle {
                Some(bundle) if writable(bundle.parent()) => Self::MacBundle(bundle.to_path_buf()),
                _ => Self::Unmanaged,
            };
        }
        Self::Unmanaged
    }

    /// Whether this install can replace itself.
    pub fn can_self_update(&self) -> bool {
        !matches!(self, Self::Unmanaged)
    }
}

/// Whether we may create files in `dir`, tested by making a folder there.
fn writable(dir: Option<&Path>) -> bool {
    let Some(dir) = dir else { return false };
    let probe = dir.join(format!(".foyer-write-test-{}", std::process::id()));
    match std::fs::create_dir(&probe) {
        Ok(()) => {
            let _ = std::fs::remove_dir(&probe);
            true
        }
        Err(_) => false,
    }
}

/// Checks for, downloads and installs new versions of the client.
#[derive(Clone)]
pub struct Updater {
    http: reqwest::Client,
    manifest_url: String,
    current: String,
    public_key: Option<String>,
}

impl Updater {
    /// `current` is the running version; `public_key` the release key in
    /// minisign's base64 form, if this build was given one.
    pub fn new(
        manifest_url: &str,
        current: &str,
        public_key: Option<&str>,
    ) -> anyhow::Result<Self> {
        let http = reqwest::Client::builder()
            .user_agent(format!("foyer-desktop/{current}"))
            .connect_timeout(Duration::from_secs(15))
            .build()?;
        Ok(Self {
            http,
            manifest_url: manifest_url.to_string(),
            current: current.to_string(),
            public_key: public_key
                .map(str::trim)
                .filter(|k| !k.is_empty())
                .map(String::from),
        })
    }

    /// Whether downloads are checked against a release key. Without one the
    /// client only points players at the download page.
    pub fn verifies_signatures(&self) -> bool {
        self.public_key.is_some()
    }

    /// The newest release, if it is newer than this one.
    pub async fn check(&self) -> Result<Option<Release>, UpdateError> {
        let release: Release = self
            .http
            .get(&self.manifest_url)
            .timeout(Duration::from_secs(30))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        Ok(release.is_newer_than(&self.current).then_some(release))
    }

    /// Downloads this platform's file into `dir`, verifies it and returns
    /// its path. `progress` gets (bytes so far, total).
    pub async fn download(
        &self,
        release: &Release,
        dir: &Path,
        progress: impl Fn(u64, u64),
    ) -> Result<PathBuf, UpdateError> {
        if self.public_key.is_none() {
            return Err(UpdateError::Unsupported);
        }
        let target = target();
        let asset = release
            .asset(&target)
            .ok_or_else(|| UpdateError::NoAsset(target.clone()))?;
        let name = asset
            .url
            .rsplit('/')
            .next()
            .filter(|n| !n.is_empty() && !n.contains(['\\', ':']) && *n != "..")
            .unwrap_or("foyer-update");
        tokio::fs::create_dir_all(dir).await?;
        let path = dir.join(name);
        let partial = dir.join(format!("{name}.part"));

        let mut response = self.http.get(&asset.url).send().await?.error_for_status()?;
        let mut file = tokio::fs::File::create(&partial).await?;
        let mut bytes = Vec::with_capacity(asset.size.min(512 << 20) as usize);
        while let Some(chunk) = response.chunk().await? {
            bytes.extend_from_slice(&chunk);
            if bytes.len() as u64 > asset.size {
                break;
            }
            file.write_all(&chunk).await?;
            progress(bytes.len() as u64, asset.size);
        }
        file.flush().await?;
        drop(file);

        let key = self.public_key.clone();
        let (asset, version) = (asset.clone(), release.version.clone());
        let checked = tokio::task::spawn_blocking(move || {
            verify(&bytes, &asset, &version, &target, key.as_deref())
        })
        .await
        .map_err(io::Error::other)?;
        if let Err(err) = checked {
            let _ = tokio::fs::remove_file(&partial).await;
            return Err(err);
        }
        tokio::fs::rename(&partial, &path).await?;
        Ok(path)
    }
}

/// Removes downloads left over from earlier updates.
pub fn clean_downloads(dir: &Path) {
    let _ = std::fs::remove_dir_all(dir);
}

/// Puts a downloaded update in place and starts the new version. The caller
/// should quit straight after.
pub fn apply(kind: &InstallKind, file: &Path) -> Result<(), UpdateError> {
    match kind {
        InstallKind::WindowsInstaller => {
            // `/relaunch=1` makes our setup script start Foyer when done.
            Command::new(file)
                .args([
                    "/VERYSILENT",
                    "/SUPPRESSMSGBOXES",
                    "/NORESTART",
                    "/CLOSEAPPLICATIONS",
                    "/relaunch=1",
                ])
                .spawn()?;
            Ok(())
        }
        InstallKind::AppImage(path) => {
            let staged = PathBuf::from(format!("{}.new", path.display()));
            std::fs::copy(file, &staged)?;
            make_executable(&staged)?;
            std::fs::rename(&staged, path)?;
            Command::new(path).env_remove("APPIMAGE").spawn()?;
            Ok(())
        }
        InstallKind::MacBundle(bundle) => {
            swap_bundle(bundle, file)?;
            Command::new("/usr/bin/open")
                .arg("-n")
                .arg(bundle)
                .spawn()?;
            Ok(())
        }
        InstallKind::Unmanaged => Err(UpdateError::Unsupported),
    }
}

/// Unpacks `archive` (a `.tar.gz` holding one `*.app`) next to `bundle` and
/// swaps it in, putting the old bundle back if anything fails.
fn swap_bundle(bundle: &Path, archive: &Path) -> Result<(), UpdateError> {
    let parent = bundle.parent().ok_or(UpdateError::Unsupported)?;
    let staging = parent.join(format!(".foyer-update-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir(&staging)?;
    let result = (|| {
        let status = Command::new("tar")
            .arg("-xzf")
            .arg(archive)
            .arg("-C")
            .arg(&staging)
            .status()?;
        if !status.success() {
            return Err(io::Error::other("could not unpack the update").into());
        }
        let new = std::fs::read_dir(&staging)?
            .filter_map(Result::ok)
            .map(|e| e.path())
            .find(|p| p.extension().is_some_and(|e| e == "app"))
            .ok_or_else(|| io::Error::other("the update has no app bundle"))?;
        let old = staging.join("previous.app");
        std::fs::rename(bundle, &old)?;
        if let Err(err) = std::fs::rename(&new, bundle) {
            let _ = std::fs::rename(&old, bundle);
            return Err(err.into());
        }
        Ok(())
    })();
    // The old bundle's files stay readable for this process until it exits.
    let _ = std::fs::remove_dir_all(&staging);
    result
}

#[cfg(unix)]
fn make_executable(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
}

#[cfg(not(unix))]
fn make_executable(_: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // A throwaway key pair made with `minisign -G -W`, and a signature of
    // b"hello foyer" with the trusted comment "foyer-desktop 1.2.3 linux-x86_64".
    const PUBLIC_KEY: &str = include_str!("../tests/fixtures/update.pub");
    const SIGNATURE: &str = include_str!("../tests/fixtures/update.minisig");
    const SIGNED: &[u8] = b"hello foyer";

    fn release(version: &str) -> Release {
        Release {
            version: version.into(),
            notes_url: None,
            published_at: None,
            assets: BTreeMap::from([("linux-x86_64".into(), asset(SIGNED))]),
        }
    }

    fn asset(bytes: &[u8]) -> Asset {
        Asset {
            url: "https://example.com/Foyer.AppImage".into(),
            size: bytes.len() as u64,
            sha256: hex(&Sha256::digest(bytes)),
            signature: Some(SIGNATURE.into()),
        }
    }

    fn key() -> &'static str {
        PUBLIC_KEY.lines().last().unwrap()
    }

    #[test]
    fn compares_versions() {
        assert!(release("0.2.0").is_newer_than("0.1.9"));
        assert!(release("v1.0.0").is_newer_than("0.9.0"));
        assert!(!release("0.1.0").is_newer_than("0.1.0"));
        assert!(!release("0.1.0").is_newer_than("0.2.0"));
        assert!(!release("0.2.0-beta.1").is_newer_than("0.2.0"));
        assert!(!release("latest").is_newer_than("0.1.0"));
    }

    #[test]
    fn parses_manifest() {
        let json = r#"{
            "version": "0.2.0",
            "notes_url": "https://github.com/TorstenDittmann/foyer/releases/tag/desktop-v0.2.0",
            "assets": {
                "windows-x86_64": {"url": "https://x/Foyer-Setup.exe", "size": 3, "sha256": "ab"}
            }
        }"#;
        let release: Release = serde_json::from_str(json).unwrap();
        assert_eq!(release.version, "0.2.0");
        let asset = release.asset("windows-x86_64").unwrap();
        assert_eq!(asset.size, 3);
        assert!(asset.signature.is_none());
        assert!(release.asset("linux-x86_64").is_none());
    }

    #[test]
    fn target_is_os_and_arch() {
        let target = target();
        let (os, arch) = target.split_once('-').unwrap();
        assert_eq!(os, std::env::consts::OS);
        assert_eq!(arch, std::env::consts::ARCH);
    }

    #[test]
    fn accepts_a_signed_file() {
        verify(SIGNED, &asset(SIGNED), "1.2.3", "linux-x86_64", Some(key())).unwrap();
    }

    #[test]
    fn checks_size_and_hash_without_a_key() {
        verify(SIGNED, &asset(SIGNED), "1.2.3", "linux-x86_64", None).unwrap();
        let mut wrong = asset(SIGNED);
        wrong.size += 1;
        assert!(matches!(
            verify(SIGNED, &wrong, "1.2.3", "linux-x86_64", None),
            Err(UpdateError::Size { .. })
        ));
        let mut wrong = asset(SIGNED);
        wrong.sha256 = "00".repeat(32);
        assert!(matches!(
            verify(SIGNED, &wrong, "1.2.3", "linux-x86_64", None),
            Err(UpdateError::Checksum)
        ));
    }

    #[test]
    fn rejects_tampered_files() {
        let tampered = b"hello fayer";
        let mut entry = asset(SIGNED);
        // Matching size and hash, but not what was signed.
        entry.sha256 = hex(&Sha256::digest(tampered));
        assert!(matches!(
            verify(tampered, &entry, "1.2.3", "linux-x86_64", Some(key())),
            Err(UpdateError::Signature(_))
        ));
    }

    #[test]
    fn rejects_a_signature_for_another_version_or_platform() {
        assert!(matches!(
            verify(SIGNED, &asset(SIGNED), "9.9.9", "linux-x86_64", Some(key())),
            Err(UpdateError::Signature(_))
        ));
        assert!(matches!(
            verify(
                SIGNED,
                &asset(SIGNED),
                "1.2.3",
                "windows-x86_64",
                Some(key())
            ),
            Err(UpdateError::Signature(_))
        ));
    }

    #[test]
    fn rejects_unsigned_files_when_a_key_is_set() {
        let mut entry = asset(SIGNED);
        entry.signature = None;
        assert!(matches!(
            verify(SIGNED, &entry, "1.2.3", "linux-x86_64", Some(key())),
            Err(UpdateError::Unsigned)
        ));
    }

    #[test]
    fn plain_binaries_are_unmanaged() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("foyer-desktop");
        assert_eq!(InstallKind::detect_from(&exe, None), InstallKind::Unmanaged);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn detects_and_replaces_an_appimage() {
        let dir = tempfile::tempdir().unwrap();
        let image = dir.path().join("Foyer.AppImage");
        std::fs::write(&image, b"old").unwrap();
        let kind = InstallKind::detect_from(Path::new("/tmp/.mount_x/usr/bin/foyer"), Some(&image));
        assert_eq!(kind, InstallKind::AppImage(image.clone()));

        // A script stands in for the new AppImage so starting it works.
        let new = dir.path().join("download");
        std::fs::write(&new, b"#!/bin/sh\nexit 0\n").unwrap();
        apply(&kind, &new).unwrap();
        assert_eq!(std::fs::read(&image).unwrap(), b"#!/bin/sh\nexit 0\n");
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&image).unwrap().permissions().mode();
        assert_eq!(mode & 0o111, 0o111);
    }

    #[cfg(windows)]
    #[test]
    fn detects_the_windows_installer() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("Foyer.exe");
        assert_eq!(InstallKind::detect_from(&exe, None), InstallKind::Unmanaged);
        std::fs::write(dir.path().join("unins000.exe"), b"").unwrap();
        assert_eq!(
            InstallKind::detect_from(&exe, None),
            InstallKind::WindowsInstaller
        );
    }
}
