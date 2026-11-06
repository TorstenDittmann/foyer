//! Foyer's "Midnight" design tokens for the desktop client.
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

/// Navy neutrals, darkest to lightest.
pub fn canvas() -> Rgba {
    rgb(0x0b0f1a)
}
pub fn surface() -> Rgba {
    rgb(0x121829)
}
pub fn raised() -> Rgba {
    rgb(0x1a2238)
}
pub fn line() -> Rgba {
    rgb(0x222b42)
}
pub fn ink() -> Rgba {
    rgb(0xe8ecf6)
}
pub fn muted() -> Rgba {
    rgb(0x8590a8)
}
pub fn faint() -> Rgba {
    rgb(0x6b7690)
}

/// Primary actions (Play, Download) are plain light buttons.
pub fn accent() -> Rgba {
    rgb(0xe8ecf6)
}
pub fn accent_ink() -> Rgba {
    rgb(0x0b0f1a)
}

/// Coral is kept for supporting developers and for progress.
pub fn support() -> Rgba {
    rgb(0xff6b5a)
}
pub fn support_ink() -> Rgba {
    rgb(0x1d0805)
}

pub fn good() -> Rgba {
    rgb(0x3ecf8e)
}
pub fn warn() -> Rgba {
    rgb(0xf5b942)
}
pub fn bad() -> Rgba {
    rgb(0xef4565)
}
pub fn info() -> Rgba {
    rgb(0x6ea8ff)
}

pub const FONT_SANS: &str = "Onest";
pub const FONT_MONO: &str = "IBM Plex Mono";

pub const RADIUS_CARD: Pixels = px(6.);
pub const RADIUS_CONTROL: Pixels = px(4.);

/// Registers the bundled fonts. Call once at startup, before opening windows.
pub fn load_fonts(cx: &App) {
    let fonts: Vec<Cow<'static, [u8]>> = vec![
        Cow::Borrowed(include_bytes!("../assets/fonts/Onest-Regular.ttf")),
        Cow::Borrowed(include_bytes!("../assets/fonts/Onest-Medium.ttf")),
        Cow::Borrowed(include_bytes!("../assets/fonts/Onest-SemiBold.ttf")),
        Cow::Borrowed(include_bytes!("../assets/fonts/IBMPlexMono-Regular.ttf")),
        Cow::Borrowed(include_bytes!("../assets/fonts/IBMPlexMono-Medium.ttf")),
    ];
    if let Err(err) = cx.text_system().add_fonts(fonts) {
        eprintln!("foyer: could not load bundled fonts, using system fonts: {err}");
    }
}
