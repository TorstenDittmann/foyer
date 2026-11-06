//! Foyer website.
//!
//! One crate, two builds: `ssr` compiles the Axum server that renders pages,
//! `hydrate` compiles the same components to WebAssembly so they become
//! interactive in the browser. `cargo leptos` drives both.
//!
//! Pages get data only through the server functions in [`api`]. Until the
//! Foyer API is live those read an in-memory mock (`mock`, server only).

pub mod api;
pub mod app;
pub mod components;
#[cfg(any(feature = "ssr", test))]
pub mod mock;
pub mod model;
pub mod pages;

/// The sample account the sign-in page offers, while data is mocked.
pub const DEMO_EMAIL: &str = "demo@foyer.play";
pub const DEMO_PASSWORD: &str = "foyer-demo";

#[cfg(feature = "hydrate")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn hydrate() {
    console_error_panic_hook::set_once();
    leptos::mount::hydrate_body(app::App);
}
