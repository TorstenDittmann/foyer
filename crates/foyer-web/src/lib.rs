//! Foyer website.
//!
//! One crate, two builds: `ssr` compiles the Axum server that renders pages,
//! `hydrate` compiles the same components to WebAssembly so they become
//! interactive in the browser. `cargo leptos` drives both.
//!
//! Pages get data only through the server functions in [`api`], which call
//! the Foyer API through `backend` (server only). `FOYER_WEB_MOCK=1` swaps
//! the API for the in-memory `mock`.

pub mod api;
pub mod app;
#[cfg(feature = "ssr")]
pub mod backend;
pub mod components;
#[cfg(any(feature = "ssr", test))]
pub mod mock;
pub mod model;
pub mod pages;
#[cfg(feature = "ssr")]
pub mod routes;

/// The sample account the mock data ships with (`FOYER_WEB_MOCK=1`).
pub const DEMO_EMAIL: &str = "demo@foyer.lol";
pub const DEMO_PASSWORD: &str = "foyer-demo";

#[cfg(feature = "hydrate")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn hydrate() {
    console_error_panic_hook::set_once();
    // A server function that redirects (sign-in, saving a game, review
    // decisions) loads the target as a server-rendered page, like a link
    // does (see public/navigation.js), instead of rendering it in the
    // browser. Set before the router's own hook, which then doesn't apply.
    _ = leptos::server_fn::redirect::set_redirect_hook(|loc: &str| {
        if let Err(e) = leptos::prelude::window().location().assign(loc) {
            leptos::logging::error!("failed to redirect: {e:?}");
        }
    });
    leptos::mount::hydrate_body(app::App);
}
