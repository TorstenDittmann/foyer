//! Foyer desktop client.
//!
//! For now this opens a single window with an empty library. Installing and
//! updating games will use `foyer-patch` to apply build manifests.

use gpui::{
    App, Application, Bounds, Context, SharedString, TitlebarOptions, Window, WindowBounds,
    WindowOptions, div, prelude::*, px, rgb, size,
};

struct Library {
    games: Vec<SharedString>,
}

impl Render for Library {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let body = if self.games.is_empty() {
            div()
                .text_color(rgb(0x9a9aa5))
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
            .bg(rgb(0x16161a))
            .text_color(rgb(0xf4f4f5))
            .child(div().text_2xl().child("Foyer"))
            .child(body)
    }
}

fn main() {
    Application::new().run(|cx: &mut App| {
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
