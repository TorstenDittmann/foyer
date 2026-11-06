//! Keeps `crates/foyer-api/openapi.json` in sync with the code, so API
//! changes are visible in review and clients can be generated from the file.
//!
//! After changing an endpoint or a type, regenerate it with:
//!
//! ```sh
//! FOYER_UPDATE_OPENAPI=1 cargo test -p foyer-api --test openapi
//! ```

use std::path::Path;

#[test]
fn checked_in_openapi_matches_the_code() {
    let spec = foyer_api::openapi().to_pretty_json().unwrap() + "\n";
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("openapi.json");
    if std::env::var_os("FOYER_UPDATE_OPENAPI").is_some() {
        std::fs::write(&path, &spec).unwrap();
        return;
    }
    let checked_in = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(
        checked_in == spec,
        "openapi.json is out of date. Run: FOYER_UPDATE_OPENAPI=1 cargo test -p foyer-api --test openapi"
    );
}

#[test]
fn every_route_is_versioned_and_documented() {
    let spec = foyer_api::openapi();
    let paths: Vec<&String> = spec.paths.paths.keys().collect();
    assert!(paths.len() >= 25, "expected the full API, got {paths:?}");
    assert!(paths.iter().all(|p| p.starts_with("/v1/")), "{paths:?}");
}
