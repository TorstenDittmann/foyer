//! The worker (scan, zip, promote) and anonymous downloads.

mod common;

use std::io::Read;

use axum::http::StatusCode;
use common::{ADMIN_EMAIL, TestApp, manifest_for};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// Creates a build from `files` on the channel named after `platform`,
/// uploads its chunks and finalizes it. The worker has not run yet.
async fn push(
    app: &TestApp,
    token: &str,
    game: &str,
    platform: &str,
    version: &str,
    files: &[(&str, &[u8])],
    tweak: impl FnOnce(&mut Value),
) -> String {
    let (manifest, chunks) = manifest_for(files);
    let mut body = json!({ "channel": platform, "platform": platform, "version": version, "manifest": manifest });
    tweak(&mut body);
    let reply = app
        .post(&format!("/v1/games/{game}/builds"), Some(token), body)
        .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{}", reply.body);
    for upload in reply.body["uploads"].as_array().unwrap() {
        let hash = upload["hash"].as_str().unwrap();
        app.upload(upload["url"].as_str().unwrap(), &chunks[hash])
            .await;
    }
    let id = reply.body["build"]["id"].as_str().unwrap().to_string();
    let reply = app
        .post(
            &format!("/v1/builds/{id}/finalize"),
            Some(token),
            json!(null),
        )
        .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    id
}

/// Submits the game and has an admin approve it.
async fn publish(app: &TestApp, dev: &str, game: &str) -> String {
    let admin = app.register("root", ADMIN_EMAIL).await;
    let sub = app
        .post(
            &format!("/v1/games/{game}/submissions"),
            Some(dev),
            json!({}),
        )
        .await;
    assert_eq!(sub.status, StatusCode::CREATED, "{}", sub.body);
    let id = sub.body["id"].as_str().unwrap();
    let decided = app
        .post(
            &format!("/v1/review/submissions/{id}/decision"),
            Some(&admin),
            json!({ "outcome": "approved" }),
        )
        .await;
    assert_eq!(decided.status, StatusCode::OK, "{}", decided.body);
    admin
}

/// Deterministic incompressible bytes.
fn noise(seed: u64, len: usize) -> Vec<u8> {
    let mut state = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    (0..len)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state >> 24) as u8
        })
        .collect()
}

#[tokio::test]
async fn worker_packs_a_zip_players_download_anonymously() {
    let app = TestApp::new().await;
    let dev = app.developer_with_game("studio", "lantern").await;
    // Larger than the test part size, so the zip goes up in parts.
    let big = noise(7, 200_000);
    let build = push(
        &app,
        &dev,
        "lantern",
        "linux",
        "1.0 beta",
        &[
            ("bin/lantern", b"#!/bin/sh\necho hi\n"),
            ("data/big.pak", &big),
            ("empty.txt", b""),
        ],
        |body| {
            body["manifest"]["files"]["bin/lantern"]["executable"] = json!(true);
            body["launch"] = json!({ "executable": "bin/lantern", "args": ["--fullscreen"] });
        },
    )
    .await;
    assert_eq!(app.run_jobs().await, 1);
    let ready = app.get(&format!("/v1/builds/{build}"), Some(&dev)).await;
    assert_eq!(ready.body["state"], "ready", "{}", ready.body);
    assert_eq!(ready.body["launch"]["executable"], "bin/lantern");

    // Unpublished: hidden from players.
    assert_eq!(
        app.get("/v1/games/lantern/downloads", None).await.status,
        StatusCode::NOT_FOUND
    );
    publish(&app, &dev, "lantern").await;

    let list = app.get("/v1/games/lantern/downloads", None).await;
    assert_eq!(list.status, StatusCode::OK, "{}", list.body);
    let entry = &list.body[0];
    assert_eq!(entry["platform"], "linux");
    assert_eq!(entry["channel"], "linux");
    assert_eq!(entry["default"], true);
    assert_eq!(entry["filename"], "lantern-1.0-beta-linux.zip");
    assert_eq!(entry["build"]["id"], build);

    let link = app
        .post(
            "/v1/games/lantern/channels/linux/archive",
            None,
            json!(null),
        )
        .await;
    assert_eq!(link.status, StatusCode::OK, "{}", link.body);
    let url = link.body["url"].as_str().unwrap();
    assert!(url.contains("response-content-disposition"), "{url}");
    let bytes = reqwest::get(url).await.unwrap().bytes().await.unwrap();
    assert_eq!(bytes.len() as u64, link.body["size"].as_u64().unwrap());

    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes.to_vec())).unwrap();
    let mut names: Vec<String> = zip.file_names().map(|n| n.unwrap().to_string()).collect();
    names.sort();
    assert_eq!(
        names,
        [
            "lantern-1.0-beta-linux/bin/lantern",
            "lantern-1.0-beta-linux/data/big.pak",
            "lantern-1.0-beta-linux/empty.txt",
        ]
    );
    let mut exe = zip.by_name("lantern-1.0-beta-linux/bin/lantern").unwrap();
    assert_eq!(exe.unix_mode().unwrap() & 0o777, 0o755);
    let mut text = String::new();
    exe.read_to_string(&mut text).unwrap();
    assert_eq!(text, "#!/bin/sh\necho hi\n");
    drop(exe);
    let mut pak = Vec::new();
    zip.by_name("lantern-1.0-beta-linux/data/big.pak")
        .unwrap()
        .read_to_end(&mut pak)
        .unwrap();
    assert_eq!(pak, big);

    // The desktop client counts installs and updates when it asks for chunks.
    let chunks = app
        .post(
            &format!("/v1/builds/{build}/downloads"),
            None,
            json!({ "event": "install" }),
        )
        .await;
    assert_eq!(chunks.status, StatusCode::OK, "{}", chunks.body);

    // A newer build replaces the old one and its zip.
    let v2 = push(
        &app,
        &dev,
        "lantern",
        "linux",
        "1.1",
        &[("bin/lantern", b"#!/bin/sh\necho v2\n")],
        |_| {},
    )
    .await;
    // Until the worker ran, players still get v1.
    let check = app
        .post(
            "/v1/updates",
            None,
            json!({ "installed": [{ "game": "lantern", "channel": "linux", "build_id": build }] }),
        )
        .await;
    assert_eq!(check.body["updates"], json!([]));
    app.run_jobs().await;
    let check = app
        .post(
            "/v1/updates",
            None,
            json!({ "installed": [
                { "game": "lantern", "channel": "linux", "build_id": build },
                { "game": "lantern", "channel": "windows", "build_id": build },
                { "game": "missing", "channel": "linux", "build_id": build },
            ] }),
        )
        .await;
    assert_eq!(check.status, StatusCode::OK, "{}", check.body);
    assert_eq!(check.body["updates"][0]["latest"]["id"], v2);
    assert_eq!(check.body["updates"][0]["installed_build_id"], build);
    assert_eq!(check.body["unavailable"].as_array().unwrap().len(), 2);
    let game_id = app.get("/v1/games/lantern", None).await.body["id"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(
        app.object(&format!("games/{game_id}/archives/{build}.zip"))
            .is_none(),
        "the replaced zip is deleted"
    );
    assert!(
        app.object(&format!("games/{game_id}/archives/{v2}.zip"))
            .is_some()
    );
    app.post(
        &format!("/v1/builds/{v2}/downloads"),
        None,
        json!({ "event": "update" }),
    )
    .await;

    // Counts are for the developer and staff only.
    assert_eq!(
        app.get("/v1/games/lantern/stats", None).await.status,
        StatusCode::UNAUTHORIZED
    );
    let stats = app.get("/v1/games/lantern/stats?days=7", Some(&dev)).await;
    assert_eq!(stats.status, StatusCode::OK, "{}", stats.body);
    assert_eq!(
        stats.body["total"],
        json!({ "web": 1, "client_installs": 1, "client_updates": 1 })
    );
    assert_eq!(stats.body["platforms"][0]["platform"], "linux");
    assert_eq!(stats.body["platforms"][0]["web"], 1);
    let daily = stats.body["daily"].as_array().unwrap();
    assert_eq!(daily.len(), 7);
    assert_eq!(daily[6]["client_installs"], 1);
    assert_eq!(daily[0]["web"], 0);
}

#[tokio::test]
async fn launch_target_must_be_in_the_build() {
    let app = TestApp::new().await;
    let dev = app.developer_with_game("studio", "lantern").await;
    let (manifest, _) = manifest_for(&[("game.exe", b"x")]);
    let reply = app
        .post(
            "/v1/games/lantern/builds",
            Some(&dev),
            json!({ "channel": "windows", "platform": "windows", "manifest": manifest,
                    "launch": { "executable": "nope.exe" } }),
        )
        .await;
    assert_eq!(reply.status, StatusCode::BAD_REQUEST);
    assert!(reply.body["message"].as_str().unwrap().contains("nope.exe"));
}

#[tokio::test]
async fn tampered_chunks_fail_the_build() {
    let app = TestApp::new().await;
    let dev = app.developer_with_game("studio", "lantern").await;
    let (manifest, chunks) = manifest_for(&[("game.exe", b"real bytes")]);
    let reply = app
        .post(
            "/v1/games/lantern/builds",
            Some(&dev),
            json!({ "channel": "windows", "platform": "windows", "manifest": manifest }),
        )
        .await;
    let upload = &reply.body["uploads"][0];
    let hash = upload["hash"].as_str().unwrap();
    // Same size, different bytes: finalize only checks sizes.
    let fake = vec![b'x'; chunks[hash].len()];
    app.upload(upload["url"].as_str().unwrap(), &fake).await;
    let id = reply.body["build"]["id"].as_str().unwrap();
    app.post(
        &format!("/v1/builds/{id}/finalize"),
        Some(&dev),
        json!(null),
    )
    .await;
    app.run_jobs().await;
    let build = app.get(&format!("/v1/builds/{id}"), Some(&dev)).await;
    assert_eq!(build.body["state"], "failed");
    assert!(
        build.body["failure"]
            .as_str()
            .unwrap()
            .contains("does not match its hash"),
        "{}",
        build.body
    );
}

/// A clamd stand-in: flags streams containing `EICAR`, refuses streams
/// containing `HUGE` as over its size limit.
async fn fake_clamd() -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    tokio::spawn(async move {
        loop {
            let (mut socket, _) = listener.accept().await.unwrap();
            tokio::spawn(async move {
                let mut command = [0u8; 10];
                socket.read_exact(&mut command).await.unwrap();
                assert_eq!(&command, b"zINSTREAM\0");
                let mut data = Vec::new();
                loop {
                    let len = socket.read_u32().await.unwrap() as usize;
                    if len == 0 {
                        break;
                    }
                    let mut chunk = vec![0; len];
                    socket.read_exact(&mut chunk).await.unwrap();
                    data.extend_from_slice(&chunk);
                }
                let text = String::from_utf8_lossy(&data);
                let reply: &[u8] = if text.contains("EICAR") {
                    b"stream: Win.Test.EICAR_HDB-1 FOUND\0"
                } else if text.contains("HUGE") {
                    b"INSTREAM size limit exceeded. ERROR\0"
                } else {
                    b"stream: OK\0"
                };
                socket.write_all(reply).await.unwrap();
            });
        }
    });
    addr
}

#[tokio::test]
async fn malware_blocks_the_build_and_flags_the_submission() {
    let mut app = TestApp::new().await;
    app.worker.clamd_addr = Some(fake_clamd().await);
    let dev = app.developer_with_game("studio", "lantern").await;

    let clean = push(
        &app,
        &dev,
        "lantern",
        "windows",
        "1.0",
        &[("game.exe", b"fine")],
        |_| {},
    )
    .await;
    app.run_jobs().await;
    let build = app.get(&format!("/v1/builds/{clean}"), Some(&dev)).await;
    assert_eq!(build.body["scan"]["status"], "clean");

    let bad = push(
        &app,
        &dev,
        "lantern",
        "windows",
        "1.1",
        &[("game.exe", b"fine"), ("plugin.dll", b"X5O EICAR test")],
        |_| {},
    )
    .await;
    app.run_jobs().await;
    let build = app.get(&format!("/v1/builds/{bad}"), Some(&dev)).await;
    assert_eq!(build.body["state"], "rejected", "{}", build.body);
    assert_eq!(build.body["scan"]["status"], "infected");
    assert_eq!(build.body["scan"]["findings"][0]["path"], "plugin.dll");
    assert_eq!(
        build.body["scan"]["findings"][0]["detail"],
        "Win.Test.EICAR_HDB-1"
    );
    // The channel still serves the clean build.
    let channels = app.get("/v1/games/lantern/channels", Some(&dev)).await;
    assert_eq!(channels.body[0]["latest"]["id"], clean);

    // Too large for the scanner: served, but flagged.
    let partial = push(
        &app,
        &dev,
        "lantern",
        "linux",
        "1.0",
        &[("assets.pak", b"HUGE asset")],
        |_| {},
    )
    .await;
    app.run_jobs().await;
    let build = app.get(&format!("/v1/builds/{partial}"), Some(&dev)).await;
    assert_eq!(build.body["state"], "ready");
    assert_eq!(build.body["scan"]["status"], "incomplete");

    // Reviewers see both on the submission.
    let admin = app.register("root", ADMIN_EMAIL).await;
    let sub = app
        .post("/v1/games/lantern/submissions", Some(&dev), json!({}))
        .await;
    assert_eq!(sub.status, StatusCode::CREATED, "{}", sub.body);
    let queue = app.get("/v1/review/queue", Some(&admin)).await;
    let flagged = queue.body["items"][0]["flagged_builds"].as_array().unwrap();
    assert_eq!(flagged.len(), 2, "{}", queue.body);
    assert_eq!(flagged[0]["id"], partial);
    assert_eq!(flagged[1]["id"], bad);
}

#[tokio::test]
async fn unreachable_scanner_retries_instead_of_skipping() {
    let mut app = TestApp::new().await;
    // Nothing listens here.
    app.worker.clamd_addr = Some("127.0.0.1:1".into());
    let dev = app.developer_with_game("studio", "lantern").await;
    let id = push(
        &app,
        &dev,
        "lantern",
        "windows",
        "1.0",
        &[("game.exe", b"fine")],
        |_| {},
    )
    .await;
    // One attempt runs; the retry is scheduled for later.
    assert_eq!(app.run_jobs().await, 1);
    let build = app.get(&format!("/v1/builds/{id}"), Some(&dev)).await;
    assert_eq!(build.body["state"], "processing");
}
