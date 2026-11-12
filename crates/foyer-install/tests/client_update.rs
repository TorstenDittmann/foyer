//! Checking for and downloading a client update from a stand-in for GitHub
//! Releases. The fixture signature was made for "foyer-desktop 1.2.3
//! linux-x86_64", so the signed downloads only run on that platform.

use std::sync::{Arc, Mutex};

use axum::Router;
use axum::extract::State;
use axum::routing::get;
use foyer_install::client_update::{Release, UpdateError, Updater, target};
use serde_json::json;
use sha2::{Digest, Sha256};

const PUBLIC_KEY: &str = include_str!("fixtures/update.pub");
const SIGNATURE: &str = include_str!("fixtures/update.minisig");
const SIGNED: &[u8] = b"hello foyer";

#[derive(Clone, Default)]
struct Served {
    manifest: Arc<Mutex<String>>,
    file: Arc<Mutex<Vec<u8>>>,
}

fn key() -> &'static str {
    PUBLIC_KEY.lines().last().unwrap()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

async fn start() -> (String, Served) {
    let served = Served::default();
    let app = Router::new()
        .route(
            "/foyer-desktop.json",
            get(|State(s): State<Served>| async move { s.manifest.lock().unwrap().clone() }),
        )
        .route(
            "/Foyer.AppImage",
            get(|State(s): State<Served>| async move { s.file.lock().unwrap().clone() }),
        )
        .with_state(served.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (url, served)
}

fn manifest(url: &str, version: &str, file: &[u8]) -> String {
    json!({
        "version": version,
        "notes_url": null,
        "assets": {
            target(): {
                "url": format!("{url}/Foyer.AppImage"),
                "size": file.len(),
                "sha256": hex(&Sha256::digest(file)),
                "signature": SIGNATURE,
            }
        }
    })
    .to_string()
}

#[tokio::test]
async fn reports_only_newer_versions() {
    let (url, served) = start().await;
    let updater = Updater::new(&format!("{url}/foyer-desktop.json"), "1.2.3", None).unwrap();

    *served.manifest.lock().unwrap() = manifest(&url, "1.2.3", SIGNED);
    assert_eq!(updater.check().await.unwrap(), None);

    *served.manifest.lock().unwrap() = manifest(&url, "1.3.0", SIGNED);
    let release: Release = updater.check().await.unwrap().unwrap();
    assert_eq!(release.version, "1.3.0");
}

#[tokio::test]
async fn needs_a_key_to_download() {
    let (url, served) = start().await;
    *served.manifest.lock().unwrap() = manifest(&url, "1.2.3", SIGNED);
    let updater = Updater::new(&format!("{url}/foyer-desktop.json"), "1.0.0", None).unwrap();
    assert!(!updater.verifies_signatures());
    let release = updater.check().await.unwrap().unwrap();
    let dir = tempfile::tempdir().unwrap();
    assert!(matches!(
        updater.download(&release, dir.path(), |_, _| {}).await,
        Err(UpdateError::Unsupported)
    ));
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[tokio::test]
async fn downloads_and_verifies() {
    let (url, served) = start().await;
    *served.manifest.lock().unwrap() = manifest(&url, "1.2.3", SIGNED);
    *served.file.lock().unwrap() = SIGNED.to_vec();
    let updater = Updater::new(&format!("{url}/foyer-desktop.json"), "1.0.0", Some(key())).unwrap();
    let release = updater.check().await.unwrap().unwrap();

    let dir = tempfile::tempdir().unwrap();
    let seen = Arc::new(Mutex::new(0));
    let progress = seen.clone();
    let path = updater
        .download(&release, dir.path(), move |n, _| {
            *progress.lock().unwrap() = n
        })
        .await
        .unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), SIGNED);
    assert_eq!(path.file_name().unwrap(), "Foyer.AppImage");
    assert_eq!(*seen.lock().unwrap(), SIGNED.len() as u64);
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[tokio::test]
async fn refuses_a_swapped_file() {
    let (url, served) = start().await;
    // The manifest matches the file, but the signature doesn't.
    let evil = b"hello fayer";
    *served.manifest.lock().unwrap() = manifest(&url, "1.2.3", evil);
    *served.file.lock().unwrap() = evil.to_vec();
    let updater = Updater::new(&format!("{url}/foyer-desktop.json"), "1.0.0", Some(key())).unwrap();
    let release = updater.check().await.unwrap().unwrap();
    let dir = tempfile::tempdir().unwrap();
    assert!(matches!(
        updater.download(&release, dir.path(), |_, _| {}).await,
        Err(UpdateError::Signature(_))
    ));
    // Nothing is left behind to install by mistake.
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[tokio::test]
async fn refuses_a_replayed_release() {
    let (url, served) = start().await;
    // A genuine 1.2.3 file announced as 9.0.0.
    *served.manifest.lock().unwrap() = manifest(&url, "9.0.0", SIGNED);
    *served.file.lock().unwrap() = SIGNED.to_vec();
    let updater = Updater::new(&format!("{url}/foyer-desktop.json"), "1.0.0", Some(key())).unwrap();
    let release = updater.check().await.unwrap().unwrap();
    let dir = tempfile::tempdir().unwrap();
    assert!(matches!(
        updater.download(&release, dir.path(), |_, _| {}).await,
        Err(UpdateError::Signature(_))
    ));
}
