//! Foyer desktop client.
//!
//! For now this opens a single window with an empty library. Installing and
//! updating games will use `foyer-patch` to apply build manifests.

mod theme;

use gpui::{
    App, Application, Bounds, Context, FontWeight, SharedString, TitlebarOptions, Window,
    WindowBounds, WindowOptions, div, prelude::*, px, size,
};

struct Library {
    games: Vec<SharedString>,
}

impl Render for Library {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let body = if self.games.is_empty() {
            div()
                .text_color(theme::muted())
                .child("Your library is empty. Games you install will show up here.")
        } else {
            div()
                .flex()
                .flex_col()
                .gap_2()
                .children(self.games.iter().cloned())
        };

        div()
            .flex()
            .flex_col()
            .size_full()
            .p_6()
            .gap_4()
            .bg(theme::canvas())
            .text_color(theme::ink())
            .font_family(theme::FONT_SANS)
            .text_sm()
            .child(
                div()
                    .text_xl()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Foyer"),
            )
            .child(body)
    }
}

fn main() {
    Application::new().run(|cx: &mut App| {
        theme::load_fonts(cx);
        let bounds = Bounds::centered(None, size(px(960.), px(640.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some("Foyer".into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            |_, cx| cx.new(|_| Library { games: Vec::new() }),
        )
        .expect("failed to open window");
        cx.activate(true);
    });
}
