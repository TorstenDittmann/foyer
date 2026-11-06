//! Foyer website.
//!
//! One crate, two builds: `ssr` compiles the Axum server that renders pages,
//! `hydrate` compiles the same components to WebAssembly so they become
//! interactive in the browser. `cargo leptos` drives both.

pub mod app;

#[cfg(feature = "hydrate")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn hydrate() {
    console_error_panic_hook::set_once();
    leptos::mount::hydrate_body(app::App);
}
