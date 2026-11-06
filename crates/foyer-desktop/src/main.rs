// Release builds are GUI programs on Windows: no console window.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Foyer desktop client: browse games, install them, keep them updated and
//! start them. No account needed.
//!
//! The install and update engine lives in `foyer-install`; this crate is
//! the GPUI front end. Network and disk work runs on a Tokio runtime
//! (`runtime`) and reports back to the views.

mod app;
mod images;
mod runtime;
mod self_update;
mod theme;
mod ui;
mod views;

use gpui::{
    App, Application, Bounds, TitlebarOptions, WindowBounds, WindowOptions, prelude::*, px, size,
};

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "foyer_desktop=info,foyer_install=info".into()),
        )
        .init();

    Application::new()
        .with_assets(ui::Assets)
        .run(|cx: &mut App| {
            theme::load_fonts(cx);
            let bounds = Bounds::centered(None, size(px(1180.), px(760.)), cx);
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    titlebar: Some(TitlebarOptions {
                        title: Some("Foyer".into()),
                        ..Default::default()
                    }),
                    window_min_size: Some(size(px(860.), px(540.))),
                    // Matches the .desktop file so Linux docks show our icon.
                    app_id: Some("lol.foyer.Foyer".into()),
                    ..Default::default()
                },
                |_, cx| cx.new(app::Foyer::new),
            )
            .expect("failed to open window");
            cx.activate(true);
        });
}
