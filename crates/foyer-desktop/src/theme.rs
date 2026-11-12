//! Foyer's "Store" design tokens for the desktop client.
//!
//! These mirror the `@theme` block in `crates/foyer-web/style/tailwind.css`
//! one to one, so the website and the client look like the same product.
//! Change both together. See `docs/design-language.md` for how each token is
//! meant to be used.

// The full token set is defined up front; screens will pick tokens up as
// they are built.
#![allow(dead_code)]

use std::borrow::Cow;

use gpui::{App, Pixels, Rgba, px, rgb};

/// Neutral greys, darkest to lightest. Game art supplies the colour.
pub fn canvas() -> Rgba {
    rgb(0x121214)
}
/// Sidebar and top bar.
pub fn chrome() -> Rgba {
    rgb(0x18181b)
}
pub fn surface() -> Rgba {
    rgb(0x1e1e22)
}
pub fn raised() -> Rgba {
    rgb(0x2a2a2e)
}
pub fn line() -> Rgba {
    rgb(0x2e2e33)
}
pub fn ink() -> Rgba {
    rgb(0xf0f0f2)
}
pub fn muted() -> Rgba {
    rgb(0xa6a6ad)
}
pub fn faint() -> Rgba {
    rgb(0x8a8a92)
}

/// Green means "get it": Download, Install, Play, and download progress.
pub fn accent() -> Rgba {
    rgb(0x38c172)
}
pub fn accent_ink() -> Rgba {
    rgb(0x0c1f13)
}

/// Coral is kept for supporting developers (tips, Ko-fi, Patreon).
pub fn support() -> Rgba {
    rgb(0xff6b5a)
}
pub fn support_ink() -> Rgba {
    rgb(0x1d0805)
}

pub fn good() -> Rgba {
    rgb(0x38c172)
}
pub fn warn() -> Rgba {
    rgb(0xf5b942)
}
pub fn bad() -> Rgba {
    rgb(0xef4565)
}
/// Blue: review summaries and links to developers.
pub fn info() -> Rgba {
    rgb(0x6fb8ff)
}

pub const FONT_SANS: &str = "Barlow";
pub const FONT_DISPLAY: &str = "Barlow Semi Condensed";
pub const FONT_MONO: &str = "IBM Plex Mono";

pub const RADIUS_CARD: Pixels = px(10.);
pub const RADIUS_CONTROL: Pixels = px(6.);

/// Registers the bundled fonts. Call once at startup, before opening windows.
pub fn load_fonts(cx: &App) {
    let fonts: Vec<Cow<'static, [u8]>> = vec![
        Cow::Borrowed(include_bytes!("../assets/fonts/Barlow-Regular.ttf")),
        Cow::Borrowed(include_bytes!("../assets/fonts/Barlow-Medium.ttf")),
        Cow::Borrowed(include_bytes!("../assets/fonts/Barlow-SemiBold.ttf")),
        Cow::Borrowed(include_bytes!("../assets/fonts/Barlow-Bold.ttf")),
        Cow::Borrowed(include_bytes!(
            "../assets/fonts/BarlowSemiCondensed-SemiBold.ttf"
        )),
        Cow::Borrowed(include_bytes!(
            "../assets/fonts/BarlowSemiCondensed-Bold.ttf"
        )),
        Cow::Borrowed(include_bytes!("../assets/fonts/IBMPlexMono-Regular.ttf")),
        Cow::Borrowed(include_bytes!("../assets/fonts/IBMPlexMono-Medium.ttf")),
    ];
    if let Err(err) = cx.text_system().add_fonts(fonts) {
        eprintln!("foyer: could not load bundled fonts, using system fonts: {err}");
    }
}
