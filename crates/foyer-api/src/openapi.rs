//! The OpenAPI document. Paths are collected from the `#[utoipa::path]`
//! annotations by `utoipa-axum`; this module adds the metadata and the bearer
//! security scheme.
//!
//! `crates/foyer-api/openapi.json` is a checked-in copy that tests keep in
//! sync, so API changes show up in review and other crates can generate
//! clients without running the server.

use utoipa::openapi::security::{ApiKey, ApiKeyValue, Http, HttpAuthScheme, SecurityScheme};
use utoipa::{Modify, OpenApi};

#[derive(OpenApi)]
#[openapi(
    info(
        title = "Foyer API",
        description = "Indie game store and platform: accounts, developer profiles, game pages, \
                       builds, the review queue and moderation. Send `Authorization: Bearer <token>` with \
                       a session token from login or an API token. Player endpoints also \
                       accept tester keys in `X-Foyer-Key` (comma separated), which unlock \
                       unpublished games and private channels.",
    ),
    modifiers(&BearerAuth),
    tags(
        (name = "auth", description = "Accounts, sessions and API tokens"),
        (name = "developers", description = "Developer profiles"),
        (name = "games", description = "Game pages"),
        (name = "builds", description = "Build uploads and chunk downloads"),
        (name = "downloads", description = "Anonymous downloads, update checks and download counts"),
        (name = "review", description = "Submissions and the review queue"),
        (name = "moderation", description = "Player reports, developer appeals and strikes"),
        (name = "testers", description = "Tester keys for unpublished games and private channels"),
        (name = "admin", description = "User administration"),
    ),
)]
struct ApiDoc;

struct BearerAuth;

impl Modify for BearerAuth {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        let components = openapi.components.get_or_insert_with(Default::default);
        components.add_security_scheme(
            "bearer",
            SecurityScheme::Http(Http::new(HttpAuthScheme::Bearer)),
        );
        components.add_security_scheme(
            "tester_key",
            SecurityScheme::ApiKey(ApiKey::Header(ApiKeyValue::new(
                foyer_types::TESTER_KEY_HEADER,
            ))),
        );
    }
}

pub(crate) fn base() -> utoipa::openapi::OpenApi {
    ApiDoc::openapi()
}

/// The full OpenAPI document for every `/v1` route.
pub fn openapi() -> utoipa::openapi::OpenApi {
    crate::routes::api_routes().into_openapi()
}
