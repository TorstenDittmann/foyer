mod common;

use std::fs;
use std::path::Path;
use std::sync::atomic::Ordering;

use common::{Mock, TOKEN, noise};

const MIB: usize = 1024 * 1024;

fn stdout(out: &std::process::Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &std::process::Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

#[track_caller]
fn assert_ok(out: &std::process::Output) {
    assert!(
        out.status.success(),
        "foyer failed\nstdout:\n{}\nstderr:\n{}",
        stdout(out),
        stderr(out)
    );
}

fn login(mock: &Mock, config: &Path) {
    assert_ok(&mock.foyer(config, &["login", "--token", TOKEN]));
}

/// A build with one big multi-chunk asset and a couple of small files.
fn write_build(dir: &Path, assets: &[u8]) {
    fs::create_dir_all(dir.join("data")).unwrap();
    fs::write(dir.join("lantern.exe"), noise(1, 300_000)).unwrap();
    fs::write(dir.join("data/assets.pak"), assets).unwrap();
    fs::write(dir.join("data/readme.txt"), b"thanks for playing").unwrap();
}

fn push(mock: &Mock, config: &Path, dir: &Path, extra: &[&str]) -> std::process::Output {
    let dir = dir.to_str().unwrap();
    let mut args = vec!["push", dir, "--game", "lantern", "--platform", "linux"];
    args.extend_from_slice(extra);
    mock.foyer(config, &args)
}

#[test]
fn login_checks_and_saves_the_token() {
    let mock = common::start();
    let config = tempfile::tempdir().unwrap();

    let bad = mock.foyer(config.path(), &["login", "--token", "nope"]);
    assert!(!bad.status.success());
    assert!(stderr(&bad).contains("not authorized"), "{}", stderr(&bad));
    assert!(!config.path().join("credentials.json").exists());

    let good = mock.foyer(config.path(), &["login", "--token", TOKEN]);
    assert_ok(&good);
    assert!(stdout(&good).contains("Logged in as torsten"));
    let saved = fs::read_to_string(config.path().join("credentials.json")).unwrap();
    assert!(saved.contains(TOKEN));

    let who = mock.foyer(config.path(), &["whoami"]);
    assert_ok(&who);
    assert!(stdout(&who).starts_with("torsten"));

    assert_ok(&mock.foyer(config.path(), &["logout"]));
    let after = mock.foyer(config.path(), &["whoami"]);
    assert!(!after.status.success());
    assert!(stderr(&after).contains("not logged in"));
}

#[test]
fn login_reads_the_token_from_stdin() {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let mock = common::start();
    let config = tempfile::tempdir().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_foyer"))
        .arg("login")
        .env("FOYER_CONFIG_DIR", config.path())
        .env("FOYER_API_URL", &mock.url)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    writeln!(child.stdin.take().unwrap(), "{TOKEN}").unwrap();
    let out = child.wait_with_output().unwrap();
    assert_ok(&out);
    assert!(stdout(&out).contains("Logged in as torsten"));
}

#[test]
fn push_requires_a_login() {
    let mock = common::start();
    let config = tempfile::tempdir().unwrap();
    let build = tempfile::tempdir().unwrap();
    write_build(build.path(), b"small");

    let out = push(&mock, config.path(), build.path(), &[]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("not logged in"), "{}", stderr(&out));
}

#[test]
fn push_uses_foyer_token_from_the_environment() {
    let mock = common::start();
    let config = tempfile::tempdir().unwrap();
    let build = tempfile::tempdir().unwrap();
    write_build(build.path(), b"small");

    let out = std::process::Command::new(env!("CARGO_BIN_EXE_foyer"))
        .args([
            "push",
            build.path().to_str().unwrap(),
            "-g",
            "lantern",
            "-p",
            "web",
        ])
        .env("FOYER_CONFIG_DIR", config.path())
        .env("FOYER_API_URL", &mock.url)
        .env("FOYER_TOKEN", TOKEN)
        .output()
        .unwrap();
    assert_ok(&out);
}

#[test]
fn first_push_uploads_everything_and_second_push_only_the_delta() {
    let mock = common::start();
    let config = tempfile::tempdir().unwrap();
    login(&mock, config.path());

    let v1 = tempfile::tempdir().unwrap();
    let mut assets = noise(2, 12 * MIB);
    write_build(v1.path(), &assets);

    let out = push(
        &mock,
        config.path(),
        v1.path(),
        &["-c", "beta", "-v", "1.0.0"],
    );
    assert_ok(&out);
    assert!(
        stdout(&out).contains("build bld_1 is ready"),
        "{}",
        stdout(&out)
    );

    let manifest = foyer_manifest(&mock, config.path(), v1.path());
    let all_chunks = chunk_set(&manifest);
    assert!(all_chunks.len() > 5, "expected a multi-chunk build");
    {
        let store = mock.store();
        assert_eq!(
            store.puts.len(),
            all_chunks.len(),
            "every chunk uploaded once"
        );
        assert_eq!(store.leaked_auth, 0, "API token was sent to storage");
        for hash in &all_chunks {
            assert!(store.chunks.contains_key(hash), "missing {hash}");
        }
    }

    // Patch: insert a few bytes in the middle of the 12 MiB asset.
    let v2 = tempfile::tempdir().unwrap();
    assets.splice(6 * MIB..6 * MIB, *b"balance patch");
    write_build(v2.path(), &assets);

    let before = mock.store().puts.len();
    let out = push(
        &mock,
        config.path(),
        v2.path(),
        &["-c", "beta", "-v", "1.0.1", "--json"],
    );
    assert_ok(&out);
    let build: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    assert_eq!(build["id"], "bld_2");
    assert_eq!(build["state"], "ready");
    assert_eq!(build["version"], "1.0.1");

    let uploaded = mock.store().puts.len() - before;
    assert!(
        (1..=3).contains(&uploaded),
        "patch re-uploaded {uploaded} chunks"
    );

    // Pushing the same build again uploads nothing.
    let before = mock.store().puts.len();
    assert_ok(&push(&mock, config.path(), v2.path(), &["-c", "beta"]));
    assert_eq!(mock.store().puts.len(), before);
}

#[test]
fn push_retries_failed_uploads() {
    let mock = common::start();
    let config = tempfile::tempdir().unwrap();
    login(&mock, config.path());
    let build = tempfile::tempdir().unwrap();
    write_build(build.path(), &noise(3, 4 * MIB));

    mock.faults.fail_puts.store(3, Ordering::SeqCst);
    let out = push(&mock, config.path(), build.path(), &[]);
    assert_ok(&out);
    assert_eq!(mock.faults.fail_puts.load(Ordering::SeqCst), 0);
}

#[test]
fn push_retries_finalize_on_server_errors() {
    let mock = common::start();
    let config = tempfile::tempdir().unwrap();
    login(&mock, config.path());
    let build = tempfile::tempdir().unwrap();
    write_build(build.path(), b"assets");

    mock.faults.fail_finalize.store(2, Ordering::SeqCst);
    let out = push(&mock, config.path(), build.path(), &[]);
    assert_ok(&out);
    assert!(stdout(&out).contains("bld_1 is ready"), "{}", stdout(&out));
    assert_eq!(mock.store().builds.len(), 1, "no duplicate build");
}

#[test]
fn push_refreshes_expired_upload_urls() {
    let mock = common::start();
    let config = tempfile::tempdir().unwrap();
    login(&mock, config.path());
    let build = tempfile::tempdir().unwrap();
    write_build(build.path(), &noise(4, 4 * MIB));

    mock.faults.expire_puts.store(2, Ordering::SeqCst);
    let out = push(&mock, config.path(), build.path(), &[]);
    assert_ok(&out);
    assert_eq!(mock.store().refreshes, 1);
}

#[test]
fn push_reuploads_chunks_finalize_reports_missing() {
    let mock = common::start();
    let config = tempfile::tempdir().unwrap();
    login(&mock, config.path());
    let build = tempfile::tempdir().unwrap();
    write_build(build.path(), &noise(5, 4 * MIB));

    // Storage "accepts" one chunk but loses it; finalize answers 409.
    mock.faults.drop_puts.store(1, Ordering::SeqCst);
    let out = push(&mock, config.path(), build.path(), &[]);
    assert_ok(&out);
    assert!(stdout(&out).contains("is ready"));
    assert_eq!(mock.store().refreshes, 1);
}

#[test]
fn push_resumes_after_an_interrupted_run() {
    let mock = common::start();
    let config = tempfile::tempdir().unwrap();
    login(&mock, config.path());
    let build = tempfile::tempdir().unwrap();
    write_build(build.path(), &noise(6, 8 * MIB));

    // Storage keeps failing: the push gives up after its retries.
    mock.faults.fail_puts.store(1_000, Ordering::SeqCst);
    let out = push(&mock, config.path(), build.path(), &["--concurrency", "1"]);
    assert!(!out.status.success());
    assert!(
        stderr(&out).contains("failed to upload chunk"),
        "{}",
        stderr(&out)
    );

    mock.faults.fail_puts.store(0, Ordering::SeqCst);
    let manifest = foyer_manifest(&mock, config.path(), build.path());
    let total = chunk_set(&manifest).len();
    {
        // Simulate a partial earlier upload by seeding half the chunks.
        let mut store = mock.store();
        for (i, (hash, bytes)) in chunk_bytes(&manifest, build.path()).into_iter().enumerate() {
            if i % 2 == 0 {
                store.chunks.insert(hash, bytes);
            }
        }
    }
    let seeded = mock.store().chunks.len();
    let before = mock.store().puts.len();
    let out = push(&mock, config.path(), build.path(), &[]);
    assert_ok(&out);
    assert_eq!(mock.store().puts.len() - before, total - seeded);
}

#[test]
fn rejects_bad_input_before_contacting_the_api() {
    let mock = common::start();
    let config = tempfile::tempdir().unwrap();
    login(&mock, config.path());
    let build = tempfile::tempdir().unwrap();

    let empty = push(&mock, config.path(), build.path(), &[]);
    assert!(
        stderr(&empty).contains("has no files"),
        "{}",
        stderr(&empty)
    );

    write_build(build.path(), b"x");
    let channel = push(&mock, config.path(), build.path(), &["-c", "Beta Test"]);
    assert!(stderr(&channel).contains("invalid channel"));

    let platform = mock.foyer(
        config.path(),
        &[
            "push",
            build.path().to_str().unwrap(),
            "-g",
            "lantern",
            "-p",
            "amiga",
        ],
    );
    assert!(stderr(&platform).contains("unknown platform"));

    let game = mock.foyer(
        config.path(),
        &[
            "push",
            build.path().to_str().unwrap(),
            "-g",
            "nope",
            "-p",
            "linux",
        ],
    );
    assert!(stderr(&game).contains("no such game"), "{}", stderr(&game));

    assert!(mock.store().builds.is_empty());
}

#[test]
fn dry_run_needs_no_login_and_uploads_nothing() {
    let mock = common::start();
    let config = tempfile::tempdir().unwrap();
    let build = tempfile::tempdir().unwrap();
    write_build(build.path(), &noise(7, 2 * MIB));

    let out = push(&mock, config.path(), build.path(), &["--dry-run"]);
    assert_ok(&out);
    assert!(stderr(&out).contains("Scanned 3 files"), "{}", stderr(&out));
    assert!(mock.store().builds.is_empty());
}

#[test]
fn status_lists_channels_and_builds() {
    let mock = common::start();
    let config = tempfile::tempdir().unwrap();
    login(&mock, config.path());

    let empty = mock.foyer(config.path(), &["status", "lantern"]);
    assert_ok(&empty);
    assert!(stdout(&empty).contains("no builds yet"));

    let build = tempfile::tempdir().unwrap();
    write_build(build.path(), b"assets");
    assert_ok(&push(&mock, config.path(), build.path(), &["-v", "0.9.0"]));
    assert_ok(&push(
        &mock,
        config.path(),
        build.path(),
        &["-c", "beta", "-v", "1.0.0-rc1"],
    ));

    let out = mock.foyer(config.path(), &["status", "lantern"]);
    assert_ok(&out);
    let table = stdout(&out);
    let lines: Vec<&str> = table.lines().collect();
    assert_eq!(lines.len(), 3, "{table}");
    assert!(lines[0].starts_with("CHANNEL"));
    assert!(
        lines[1].starts_with("beta")
            && lines[1].contains("1.0.0-rc1")
            && lines[1].contains("bld_2")
    );
    assert!(
        lines[2].starts_with("stable") && lines[2].contains("0.9.0") && lines[2].contains("ready")
    );

    let one = mock.foyer(
        config.path(),
        &["status", "lantern", "--build", "bld_1", "--json"],
    );
    assert_ok(&one);
    let build: serde_json::Value = serde_json::from_str(&stdout(&one)).unwrap();
    assert_eq!(build["channel"], "stable");
    assert_eq!(build["platform"], "linux");
}

fn foyer_manifest(mock: &Mock, config: &Path, dir: &Path) -> serde_json::Value {
    let out = mock.foyer(config, &["manifest", dir.to_str().unwrap()]);
    assert_ok(&out);
    serde_json::from_str(&stdout(&out)).unwrap()
}

fn chunk_set(manifest: &serde_json::Value) -> std::collections::BTreeSet<String> {
    manifest["files"]
        .as_object()
        .unwrap()
        .values()
        .flat_map(|f| f["chunks"].as_array().unwrap())
        .map(|c| c["hash"].as_str().unwrap().to_string())
        .collect()
}

/// Every distinct chunk of a build with its bytes, cut by the manifest.
fn chunk_bytes(manifest: &serde_json::Value, dir: &Path) -> Vec<(String, Vec<u8>)> {
    let mut seen = std::collections::BTreeMap::new();
    for (path, file) in manifest["files"].as_object().unwrap() {
        let data = fs::read(dir.join(path)).unwrap();
        let mut offset = 0;
        for chunk in file["chunks"].as_array().unwrap() {
            let size = chunk["size"].as_u64().unwrap() as usize;
            seen.entry(chunk["hash"].as_str().unwrap().to_string())
                .or_insert_with(|| data[offset..offset + size].to_vec());
            offset += size;
        }
    }
    seen.into_iter().collect()
}
